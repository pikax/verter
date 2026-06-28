//! The typed, gated tsgo `--api` client.
//!
//! [`TsgoClient`] is the crate's public TYPED surface. It wires the pieces
//! together in the correct order:
//!   1. discover (or accept) the tsgo binary,
//!   2. probe its version and run the FAIL-CLOSED wire gate ([`crate::gate`]) —
//!      a diverged/unknown engine makes `connect` return
//!      [`TsgoApiError::UnsupportedTsgoWire`] before any request can be made,
//!   3. spawn the stdio-pipe transport + the single-writer actor, and
//!   4. expose typed operations (no raw MessagePack on the public surface).
//!
//! The raw frame/codec types live behind [`crate::proto`] for the transport and
//! tests; consumers use the typed methods here.

use std::path::Path;
use std::process::Command;

use crate::actor::{spawn_actor, ClientHandle};
use crate::error::{TsgoApiError, TsgoApiResult};
use crate::gate::{self, GateClearance, ObservedEngine};
use crate::proto::types::{
    method, Diagnostic, InitializeResponse, OpaqueHandle, SymbolResponse, TypeResponse,
    UpdateSnapshotParams, UpdateSnapshotResponse,
};
use crate::snapshot::OverlaySnapshot;
use crate::transport::pipe::StdioPipeTransport;
use crate::RequestOptions;

/// A connected, wire-gated tsgo `--api` client exposing typed operations.
///
/// Clone-friendly via the inner [`ClientHandle`]; dropping the last clone tears
/// down the actor and the child process.
#[derive(Clone)]
pub struct TsgoClient {
    handle: ClientHandle,
    clearance: GateClearance,
}

impl TsgoClient {
    /// Connect to a tsgo engine at `exe`, working directory `cwd`, with an
    /// initial overlay snapshot. Runs the fail-closed wire gate first: if the
    /// engine's version is not the pinned one (or its wire fingerprint diverges),
    /// returns [`TsgoApiError::UnsupportedTsgoWire`] WITHOUT spawning the actor.
    ///
    /// `queue_depth` bounds each scheduling lane's backlog (backpressure).
    pub fn connect(
        exe: &Path,
        cwd: &Path,
        snapshot: OverlaySnapshot,
        queue_depth: usize,
    ) -> TsgoApiResult<Self> {
        // 1. Probe version + gate (fail-closed) BEFORE spawning the session.
        let version = probe_engine_version(exe)?;
        let clearance = gate::validate(&ObservedEngine::from_codec_wire(version))?;

        // 2. Spawn transport + actor.
        let transport = StdioPipeTransport::spawn(exe, cwd)?;
        let handle = spawn_actor(transport, snapshot, queue_depth);

        Ok(Self { handle, clearance })
    }

    /// The capabilities the wire gate confirmed for the connected engine.
    pub fn clearance(&self) -> &GateClearance {
        &self.clearance
    }

    /// The underlying handle (for advanced callers needing lane/cancel control).
    pub fn handle(&self) -> &ClientHandle {
        &self.handle
    }

    /// Publish a new overlay snapshot (e.g. after the VFS changed).
    pub fn publish_snapshot(&self, snapshot: OverlaySnapshot) {
        self.handle.publish_snapshot(snapshot);
    }

    /// `initialize` — the once-only startup handshake. Returns the engine's
    /// case-sensitivity + current directory.
    pub async fn initialize(&self) -> TsgoApiResult<InitializeResponse> {
        self.typed(method::INITIALIZE, &serde_json::Value::Null)
            .await
    }

    /// `updateSnapshot` — open/refresh a project snapshot.
    pub async fn update_snapshot(
        &self,
        params: &UpdateSnapshotParams,
    ) -> TsgoApiResult<UpdateSnapshotResponse> {
        self.typed(method::UPDATE_SNAPSHOT, params).await
    }

    /// `getSemanticDiagnostics` — type-check diagnostics for `file` in the given
    /// snapshot/project.
    pub async fn get_semantic_diagnostics(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
        file: &str,
    ) -> TsgoApiResult<Vec<Diagnostic>> {
        self.typed(
            method::GET_SEMANTIC_DIAGNOSTICS,
            &serde_json::json!({ "snapshot": snapshot, "project": project, "file": file }),
        )
        .await
    }

    /// `getSyntacticDiagnostics` — parse diagnostics for `file`.
    pub async fn get_syntactic_diagnostics(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
        file: &str,
    ) -> TsgoApiResult<Vec<Diagnostic>> {
        self.typed(
            method::GET_SYNTACTIC_DIAGNOSTICS,
            &serde_json::json!({ "snapshot": snapshot, "project": project, "file": file }),
        )
        .await
    }

    /// `getTypeAtPosition` — the type at `position` in `file`. `None` when the
    /// engine returns no type at that position.
    pub async fn get_type_at_position(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
        file: &str,
        position: u32,
    ) -> TsgoApiResult<Option<TypeResponse>> {
        self.typed_opt(
            method::GET_TYPE_AT_POSITION,
            &serde_json::json!({ "snapshot": snapshot, "project": project, "file": file, "position": position }),
        )
        .await
    }

    /// `getSymbolAtPosition` — the symbol at `position` in `file`.
    pub async fn get_symbol_at_position(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
        file: &str,
        position: u32,
    ) -> TsgoApiResult<Option<SymbolResponse>> {
        self.typed_opt(
            method::GET_SYMBOL_AT_POSITION,
            &serde_json::json!({ "snapshot": snapshot, "project": project, "file": file, "position": position }),
        )
        .await
    }

    /// `typeToString` — the display string for a type handle. The type handle is
    /// the engine's opaque integer id (it flows from [`TypeResponse::id`]), so it
    /// rides into the request params as a bare integer.
    pub async fn type_to_string(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
        type_id: &OpaqueHandle,
    ) -> TsgoApiResult<String> {
        self.typed(
            method::TYPE_TO_STRING,
            &serde_json::json!({ "snapshot": snapshot, "project": project, "type": type_id }),
        )
        .await
    }

    /// `release` — release a snapshot handle.
    pub async fn release(&self, snapshot: &OpaqueHandle) -> TsgoApiResult<()> {
        let payload = serde_json::to_vec(&serde_json::json!({ "handle": snapshot }))?;
        self.handle
            .request(method::RELEASE, payload, RequestOptions::default())
            .await
            .map(|_| ())
    }

    /// Shut down the client + engine.
    pub async fn close(&self) -> TsgoApiResult<()> {
        self.handle.close().await
    }

    // ── internal typed request helpers ──────────────────────────────────────
    async fn typed<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
    ) -> TsgoApiResult<R> {
        let payload = serde_json::to_vec(params)?;
        let bytes = self
            .handle
            .request(method, payload, RequestOptions::default())
            .await?;
        serde_json::from_slice(&bytes).map_err(Into::into)
    }

    /// A typed request whose result is `undefined`/empty → `None` (the engine
    /// returns an empty payload when there is no value, sync/client.js:51-54).
    async fn typed_opt<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
    ) -> TsgoApiResult<Option<R>> {
        let payload = serde_json::to_vec(params)?;
        let bytes = self
            .handle
            .request(method, payload, RequestOptions::default())
            .await?;
        if bytes.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice(&bytes).map(Some).map_err(Into::into)
    }
}

/// Probe `tsgo --version`, returning the bare version string (e.g.
/// `7.0.0-dev.20260526.1`). The output is `Version <v>`.
pub fn probe_engine_version(exe: &Path) -> TsgoApiResult<String> {
    let output = Command::new(exe).arg("--version").output().map_err(|e| {
        TsgoApiError::Spawn(format!("probe `tsgo --version` at {}: {e}", exe.display()))
    })?;
    if !output.status.success() {
        return Err(TsgoApiError::Spawn(format!(
            "`tsgo --version` exited with {:?}",
            output.status.code()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_version(&text).ok_or_else(|| {
        TsgoApiError::Spawn(format!(
            "could not parse tsgo version from `{}`",
            text.trim()
        ))
    })
}

/// Parse `Version <v>` (or a bare `<v>`) into the version string.
fn parse_version(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let v = trimmed
        .strip_prefix("Version")
        .map(str::trim)
        .unwrap_or(trimmed);
    if v.is_empty() {
        None
    } else {
        // Take the first whitespace-delimited token (defensive against extra output).
        v.split_whitespace().next().map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_handles_prefixed_and_bare() {
        assert_eq!(
            parse_version("Version 7.0.0-dev.20260526.1\n").as_deref(),
            Some("7.0.0-dev.20260526.1")
        );
        assert_eq!(parse_version("7.0.1-rc").as_deref(), Some("7.0.1-rc"));
        assert_eq!(parse_version("   ").as_deref(), None);
        assert_eq!(parse_version("Version").as_deref(), None);
    }

    #[test]
    fn connect_to_bogus_binary_is_typed_error() {
        // No real engine: probing version fails with a typed Spawn error before
        // any actor is spawned. (TsgoClient is intentionally not Debug, so we
        // match on the result rather than using expect_err.)
        let result = TsgoClient::connect(
            Path::new("definitely-not-tsgo-xyz"),
            std::env::temp_dir().as_path(),
            OverlaySnapshot::builder().build(),
            8,
        );
        match result {
            Ok(_) => panic!("bogus binary must not connect"),
            Err(e) => assert!(matches!(e, TsgoApiError::Spawn(_)), "got {e:?}"),
        }
    }
}
