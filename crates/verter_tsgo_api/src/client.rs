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
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;

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
    /// Whether a first `updateSnapshot` response has passed the
    /// integer-handle rail ([`gate::require_integer_snapshot_handle`]).
    /// Shared across clones (the clones share one engine), so the validated
    /// raw first-response path runs once per client, not once per clone. This
    /// is the lock-free fast-path check; cold-start serialization rides
    /// [`Self::first_validation_lock`].
    first_snapshot_validated: Arc<AtomicBool>,
    /// Serializes the cold-start first-`updateSnapshot` validation: exactly one
    /// caller runs the rail while concurrent clones wait, then take the fast
    /// path. Shared across clones (they share one engine).
    first_validation_lock: Arc<tokio::sync::Mutex<()>>,
    /// The per-request hard deadline applied to every typed call (see
    /// [`RequestOptions::deadline`]). `None` = unbounded waits (legacy
    /// interactive behavior); batch/standalone drivers set it so a hung engine
    /// fails bounded and is torn down.
    request_deadline: Option<Duration>,
}

/// The bound on the version probe inside [`TsgoClient::connect`] (mirrors the
/// toolchain validator's probe bound).
const CONNECT_PROBE_BOUND: Duration = Duration::from_secs(5);

impl TsgoClient {
    /// Connect to a tsgo engine at `exe`, working directory `cwd`, with an
    /// initial overlay snapshot. Runs the fail-closed wire gate first: if the
    /// engine's version is not the pinned one (or its wire fingerprint diverges),
    /// returns [`TsgoApiError::UnsupportedTsgoWire`] WITHOUT spawning the actor.
    ///
    /// The version probe is END-TO-END bounded ([`probe_engine_version_bounded`]):
    /// a wedged candidate fails the connect within the bound instead of hanging
    /// the caller.
    ///
    /// `queue_depth` bounds each scheduling lane's backlog (backpressure).
    pub async fn connect(
        exe: &Path,
        cwd: &Path,
        snapshot: OverlaySnapshot,
        queue_depth: usize,
    ) -> TsgoApiResult<Self> {
        // 1. Probe version + gate (fail-closed) BEFORE spawning the session.
        let version = probe_engine_version_bounded(exe, CONNECT_PROBE_BOUND).await?;
        let clearance = gate::validate(&ObservedEngine::from_codec_wire(version))?;

        // 2. Spawn transport + actor.
        let transport = StdioPipeTransport::spawn(exe, cwd)?;
        let handle = spawn_actor(transport, snapshot, queue_depth);

        Ok(Self {
            handle,
            clearance,
            first_snapshot_validated: Arc::new(AtomicBool::new(false)),
            first_validation_lock: Arc::new(tokio::sync::Mutex::new(())),
            request_deadline: None,
        })
    }

    /// Construct a client over an already-built handle + clearance. Test-only:
    /// it bypasses the spawn + wire gate so a mock-engine `FrameStream` can drive
    /// the typed methods and their exact request-param wire shape is assertable.
    #[cfg(test)]
    pub(crate) fn from_parts(handle: ClientHandle, clearance: GateClearance) -> Self {
        Self {
            handle,
            clearance,
            first_snapshot_validated: Arc::new(AtomicBool::new(false)),
            first_validation_lock: Arc::new(tokio::sync::Mutex::new(())),
            request_deadline: None,
        }
    }

    /// Set the per-request hard deadline applied to every typed call. On
    /// expiry a call fails with [`TsgoApiError::Timeout`] and the engine is
    /// TORN DOWN (the single-flight wire cannot recover a wedged request), so
    /// a hung engine never blocks the caller forever. Batch/standalone drivers
    /// (e.g. verter-tsc) MUST set this; `None` (the default) keeps unbounded
    /// waits for interactive lanes.
    pub fn with_request_deadline(mut self, deadline: Duration) -> Self {
        self.request_deadline = Some(deadline);
        self
    }

    /// The capabilities the wire gate confirmed for the connected engine.
    pub fn clearance(&self) -> &GateClearance {
        &self.clearance
    }

    /// The engine version string the wire gate observed and channel-validated.
    pub fn observed_version(&self) -> &str {
        &self.clearance.observed_version
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
    ///
    /// The FIRST response is decoded raw and run through the version-lie-immune
    /// integer-handle rail ([`gate::require_integer_snapshot_handle`]): an
    /// engine whose snapshot handle is not a bare JSON integer speaks a
    /// different opaque-handle wire class and is refused with a typed
    /// [`TsgoApiError::UnsupportedTsgoWire`] BEFORE any product result.
    ///
    /// Cold start is a double-checked async init: the rail runs exactly once
    /// even under concurrent first calls — one caller holds
    /// [`Self::first_validation_lock`] and validates while the rest wait, then
    /// take the fast path. Steady state is a single atomic load, no lock. The
    /// flag flips ONLY after the rail AND the typed decode both succeed, so a
    /// refused first call never unlocks the typed fast path (fail-closed).
    pub async fn update_snapshot(
        &self,
        params: &UpdateSnapshotParams,
    ) -> TsgoApiResult<UpdateSnapshotResponse> {
        // Fast path: the first response already cleared the rail.
        if self.first_snapshot_validated.load(Ordering::Acquire) {
            return self.typed(method::UPDATE_SNAPSHOT, params).await;
        }
        // Slow path: serialize the cold-start validation.
        let _guard = self.first_validation_lock.lock().await;
        if self.first_snapshot_validated.load(Ordering::Acquire) {
            // Another caller validated while we waited for the lock.
            return self.typed(method::UPDATE_SNAPSHOT, params).await;
        }
        let raw = self.typed_value(method::UPDATE_SNAPSHOT, params).await?;
        gate::require_integer_snapshot_handle(&raw["snapshot"], self.observed_version())?;
        let resp = serde_json::from_value(raw)?;
        // Flip the warm flag ONLY after the full first response validated AND decoded.
        self.first_snapshot_validated.store(true, Ordering::Release);
        Ok(resp)
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

    /// `getSemanticDiagnostics` for the WHOLE PROGRAM — the `file` argument is
    /// OMITTED, which the engine reads as "return diagnostics for every file in
    /// the program" (the standard TS `Program.getSemanticDiagnostics()` with no
    /// argument). This surfaces type errors in NON-root imported files the
    /// per-file getter never reaches. Each returned diagnostic carries its own
    /// `file_name`; offsets are UTF-16 code units per the `--api` offset contract
    /// ([`crate::offset`]).
    pub async fn get_semantic_diagnostics_for_program(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
    ) -> TsgoApiResult<Vec<Diagnostic>> {
        self.typed(
            method::GET_SEMANTIC_DIAGNOSTICS,
            &serde_json::json!({ "snapshot": snapshot, "project": project }),
        )
        .await
    }

    /// `getSyntacticDiagnostics` for the WHOLE PROGRAM — the `file` argument is
    /// OMITTED (whole-program parse diagnostics). See
    /// [`Self::get_semantic_diagnostics_for_program`] for the file-omitted
    /// contract.
    pub async fn get_syntactic_diagnostics_for_program(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
    ) -> TsgoApiResult<Vec<Diagnostic>> {
        self.typed(
            method::GET_SYNTACTIC_DIAGNOSTICS,
            &serde_json::json!({ "snapshot": snapshot, "project": project }),
        )
        .await
    }

    /// `getConfigFileParsingDiagnostics` — the project's config-file parse /
    /// compiler-options diagnostics (e.g. an invalid `target` → TS6046). These
    /// are program-wide and NOT covered by the per-file semantic/syntactic
    /// getters. A global option diagnostic carries no `file_name`; a config-file
    /// diagnostic carries the tsconfig path. Offsets are UTF-16 code units per
    /// the `--api` offset contract ([`crate::offset`]).
    pub async fn get_config_file_parsing_diagnostics(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
    ) -> TsgoApiResult<Vec<Diagnostic>> {
        self.typed(
            method::GET_CONFIG_FILE_PARSING_DIAGNOSTICS,
            &serde_json::json!({ "snapshot": snapshot, "project": project }),
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

    /// `release` — release a snapshot handle. GA reads `{ snapshot: N }`
    /// (api.js:196); fire-and-forget (the response is ignored).
    pub async fn release(&self, snapshot: &OpaqueHandle) -> TsgoApiResult<()> {
        let payload = serde_json::to_vec(&serde_json::json!({ "snapshot": snapshot }))?;
        self.handle
            .request(method::RELEASE, payload, self.request_options())
            .await
            .map(|_| ())
    }

    /// Shut down the client + engine.
    pub async fn close(&self) -> TsgoApiResult<()> {
        self.handle.close().await
    }

    // ── internal typed request helpers ──────────────────────────────────────

    /// The request options for a typed call: the client's configured deadline,
    /// default lane, no cancellation.
    fn request_options(&self) -> RequestOptions {
        RequestOptions {
            deadline: self.request_deadline,
            ..RequestOptions::default()
        }
    }

    async fn typed<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
    ) -> TsgoApiResult<R> {
        let payload = serde_json::to_vec(params)?;
        let bytes = self
            .handle
            .request(method, payload, self.request_options())
            .await?;
        serde_json::from_slice(&bytes).map_err(Into::into)
    }

    /// A typed request decoded to a raw [`serde_json::Value`], for call sites
    /// that must inspect the response shape before committing to a DTO.
    async fn typed_value<P: serde::Serialize>(
        &self,
        method: &str,
        params: &P,
    ) -> TsgoApiResult<serde_json::Value> {
        let payload = serde_json::to_vec(params)?;
        let bytes = self
            .handle
            .request(method, payload, self.request_options())
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
            .request(method, payload, self.request_options())
            .await?;
        if bytes.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice(&bytes).map(Some).map_err(Into::into)
    }
}

/// Probe `tsgo --version` under a hard END-TO-END bound, returning the bare
/// version string (e.g. `7.0.0-dev.20260526.1`). The output is `Version <v>`.
///
/// The ENTIRE probe — spawn, `wait`, AND the stdout/stderr drain — runs under
/// ONE timeout: a candidate whose descendant inherits the pipe handles and
/// outlives it cannot wedge the drain. On timeout (or any wait/join failure)
/// the probe kills the whole process TREE ([`crate::process::TreeKill`] — a
/// descendant holding the pipes dies too) and reaps the direct child, so
/// provider failover never leaks a stuck discovery process. There is NO
/// unbounded probe variant anywhere in the crate.
pub async fn probe_engine_version_bounded(exe: &Path, bound: Duration) -> TsgoApiResult<String> {
    use crate::process::{configure_tree_spawn, reap_child_bounded, TreeKill, REAP_BOUND};

    let mut command = tokio::process::Command::new(exe);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_tree_spawn(&mut command);
    let mut child = command.spawn().map_err(|error| {
        TsgoApiError::Spawn(format!(
            "probe `tsgo --version` at {}: {error}",
            exe.display()
        ))
    })?;
    let tree = TreeKill::arm(child.id().unwrap_or(0));
    let mut stdout = child.stdout.take().ok_or_else(|| {
        TsgoApiError::Transport("version probe did not expose stdout".to_string())
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| {
        TsgoApiError::Transport("version probe did not expose stderr".to_string())
    })?;
    let mut stdout_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    let mut stderr_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).await.map(|_| bytes)
    });

    // The whole probe — process wait AND both pipe drains — under one bound.
    let work = async {
        let status = child.wait().await.map_err(|error| {
            TsgoApiError::Transport(format!(
                "wait for `tsgo --version` at {} failed: {error}",
                exe.display()
            ))
        })?;
        let stdout = (&mut stdout_task)
            .await
            .map_err(|error| TsgoApiError::Transport(format!("join version stdout: {error}")))?
            .map_err(|error| TsgoApiError::Transport(format!("read version stdout: {error}")))?;
        let stderr = (&mut stderr_task)
            .await
            .map_err(|error| TsgoApiError::Transport(format!("join version stderr: {error}")))?
            .map_err(|error| TsgoApiError::Transport(format!("read version stderr: {error}")))?;
        Ok::<_, TsgoApiError>((status, stdout, stderr))
    };

    let (status, stdout, stderr) = match tokio::time::timeout(bound, work).await {
        Ok(Ok(parts)) => parts,
        Ok(Err(error)) => {
            // A wait/join failure: kill the tree and reap before surfacing.
            tree.kill_tree();
            let _ = reap_child_bounded(&mut child, REAP_BOUND).await;
            stdout_task.abort();
            stderr_task.abort();
            return Err(error);
        }
        Err(_) => {
            // The bound fired (possibly inside the drain, e.g. a descendant
            // holding the pipes): kill the WHOLE TREE, reap, report bounded.
            tree.kill_tree();
            let reaped = reap_child_bounded(&mut child, REAP_BOUND).await;
            stdout_task.abort();
            stderr_task.abort();
            return Err(TsgoApiError::Timeout(format!(
                "`tsgo --version` at {} exceeded {} ms{}",
                exe.display(),
                bound.as_millis(),
                if reaped {
                    ""
                } else {
                    " (the process tree was killed but the child could not be reaped)"
                }
            )));
        }
    };

    if !status.success() {
        return Err(TsgoApiError::Spawn(format!(
            "`tsgo --version` exited with {:?}: {}",
            status.code(),
            String::from_utf8_lossy(&stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&stdout);
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

    // ── DISCRIMINATING: the whole-program getters OMIT the `file` field on the
    //    wire (file-omitted == "diagnostics for all files"), while the per-file
    //    getters INCLUDE it. A regression that reused the per-file param (with a
    //    `file` key) would fail here. Drives the ACTUAL client methods over a
    //    mock-engine FrameStream and inspects the captured request payload. ──────
    use crate::actor::{spawn_actor, FrameStream};
    use crate::gate::{EngineVersionWitness, GateClearance, WireCapability};
    use crate::proto::frame::{decode_frame, encode_frame, MessageType};
    use crate::proto::schema_manifest::PINNED;
    use crate::proto::types::OpaqueHandle;
    use crate::snapshot::OverlaySnapshot;
    use tokio::sync::mpsc;

    fn test_client() -> (TsgoClient, mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
        let (inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(64);
        let (outbound_tx, outbound_rx) = mpsc::channel::<Vec<u8>>(64);
        let stream = FrameStream::new(inbound_rx, outbound_tx);
        let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);
        let clearance = GateClearance {
            manifest: PINNED,
            capabilities: vec![WireCapability::SyncTupleApi],
            observed_version: PINNED.engine_version.to_string(),
            witness: EngineVersionWitness::VersionProbe,
        };
        (
            TsgoClient::from_parts(handle, clearance),
            inbound_tx,
            outbound_rx,
        )
    }

    /// Capture the JSON request payload the next client call sends, replying with
    /// an empty diagnostics array so the call resolves.
    async fn capture_request_payload(
        mut to_engine: mpsc::Receiver<Vec<u8>>,
        from_engine: mpsc::Sender<Vec<u8>>,
    ) -> tokio::task::JoinHandle<serde_json::Value> {
        tokio::spawn(async move {
            let raw = to_engine
                .recv()
                .await
                .expect("client sends a request frame");
            let (req, _) = decode_frame(&raw, 0).expect("decode request frame");
            let payload: serde_json::Value =
                serde_json::from_slice(req.payload).expect("request payload is JSON");
            let resp = encode_frame(MessageType::Response, req.name, b"[]");
            from_engine.send(resp).await.expect("engine reply");
            payload
        })
    }

    #[tokio::test]
    async fn program_semantic_getter_omits_file_and_per_file_getter_includes_it() {
        let snap = OpaqueHandle(1);

        // (1) Whole-program semantic getter: the `file` key MUST be absent.
        {
            let (client, from_engine, to_engine) = test_client();
            let engine = capture_request_payload(to_engine, from_engine).await;
            client
                .get_semantic_diagnostics_for_program(&snap, "p.x")
                .await
                .expect("program getter resolves");
            let payload = engine.await.unwrap();
            assert_eq!(payload["project"], serde_json::json!("p.x"));
            assert!(
                payload.get("file").is_none(),
                "whole-program getSemanticDiagnostics MUST omit `file` (got {payload})"
            );
        }

        // (2) Per-file semantic getter: the `file` key MUST be present. This is the
        // discriminating control — the two share a method name but differ on `file`.
        {
            let (client, from_engine, to_engine) = test_client();
            let engine = capture_request_payload(to_engine, from_engine).await;
            client
                .get_semantic_diagnostics(&snap, "p.x", "/proj/A.tsx")
                .await
                .expect("per-file getter resolves");
            let payload = engine.await.unwrap();
            assert_eq!(
                payload["file"],
                serde_json::json!("/proj/A.tsx"),
                "per-file getSemanticDiagnostics MUST carry `file` (got {payload})"
            );
        }
    }

    #[tokio::test]
    async fn program_syntactic_getter_omits_file() {
        let snap = OpaqueHandle(2);
        let (client, from_engine, to_engine) = test_client();
        let engine = capture_request_payload(to_engine, from_engine).await;
        client
            .get_syntactic_diagnostics_for_program(&snap, "p.y")
            .await
            .expect("program syntactic getter resolves");
        let payload = engine.await.unwrap();
        assert_eq!(payload["project"], serde_json::json!("p.y"));
        assert!(
            payload.get("file").is_none(),
            "whole-program getSyntacticDiagnostics MUST omit `file` (got {payload})"
        );
    }

    #[tokio::test]
    async fn config_file_parsing_getter_sends_config_method_without_file() {
        let snap = OpaqueHandle(3);
        let (client, from_engine, to_engine) = test_client();
        // Capture BOTH the method name and payload for the config getter.
        let engine = tokio::spawn(async move {
            let mut to_engine = to_engine;
            let raw = to_engine.recv().await.unwrap();
            let (req, _) = decode_frame(&raw, 0).unwrap();
            let name = String::from_utf8_lossy(req.name).into_owned();
            let payload: serde_json::Value = serde_json::from_slice(req.payload).unwrap();
            let resp = encode_frame(MessageType::Response, req.name, b"[]");
            from_engine.send(resp).await.unwrap();
            (name, payload)
        });
        client
            .get_config_file_parsing_diagnostics(&snap, "p.z")
            .await
            .expect("config getter resolves");
        let (name, payload) = engine.await.unwrap();
        assert_eq!(
            name, "getConfigFileParsingDiagnostics",
            "the config getter uses the dedicated wire method"
        );
        assert_eq!(payload["project"], serde_json::json!("p.z"));
        assert!(
            payload.get("file").is_none(),
            "getConfigFileParsingDiagnostics is program-wide and carries no `file`"
        );
    }

    // ── DISCRIMINATING: `release` sends the GA `{ snapshot: N }` param
    //    (api.js:196), NOT the pre-GA `{ handle: N }`. GA reads `snapshot`, so a
    //    `handle` payload silently no-ops (a snapshot leak). Fire-and-forget, so
    //    the divergence never surfaces through parity/codec_roundtrip — a capture
    //    test is the only rail. Fails RED against the `{ handle }` payload. ──────
    #[tokio::test]
    async fn release_sends_snapshot_param_not_handle() {
        let (client, from_engine, to_engine) = test_client();
        let engine = capture_request_payload(to_engine, from_engine).await;
        client
            .release(&OpaqueHandle(42))
            .await
            .expect("release resolves");
        let payload = engine.await.unwrap();
        assert_eq!(
            payload["snapshot"],
            serde_json::json!(42),
            "release must send the GA `snapshot` param (got {payload})"
        );
        assert!(
            payload["snapshot"].is_number(),
            "the released handle rides the wire as a JSON integer: {payload}"
        );
        assert!(
            payload.get("handle").is_none(),
            "release must NOT send the pre-GA `handle` param: {payload}"
        );
        let obj = payload.as_object().expect("release params are an object");
        assert_eq!(obj.len(), 1, "release sends `snapshot` ONLY: {payload}");
    }

    // ── DISCRIMINATING: the OWNED first-`updateSnapshot` integer-handle rail.
    //    An engine whose first snapshot handle is a STRING (the pre-integer
    //    opaque-handle wire) must be refused with the typed
    //    `UnsupportedTsgoWire` naming the observed engine version — never
    //    decoded into a product result and never a generic decode error. ──────
    #[tokio::test]
    async fn first_update_snapshot_with_string_handle_fails_closed() {
        let (client, from_engine, to_engine) = test_client();
        let engine = tokio::spawn(async move {
            let mut to_engine = to_engine;
            let raw = to_engine.recv().await.expect("client sends updateSnapshot");
            let (req, _) = decode_frame(&raw, 0).expect("decode request frame");
            let body = serde_json::json!({ "snapshot": "n0000000000000003", "projects": [] });
            let resp = encode_frame(
                MessageType::Response,
                req.name,
                &serde_json::to_vec(&body).unwrap(),
            );
            from_engine.send(resp).await.expect("engine reply");
        });
        let err = client
            .update_snapshot(&UpdateSnapshotParams::default())
            .await
            .expect_err("a string first snapshot handle must be refused");
        engine.await.unwrap();
        assert!(
            matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m) if m.contains("7.0.2")),
            "the refusal must be the typed UnsupportedTsgoWire naming the observed \
             engine version; got {err:?}"
        );
    }

    /// The genuine integer-handle wire still decodes through the validated
    /// first-call path (the rail must not break the real engine).
    #[tokio::test]
    async fn first_update_snapshot_with_integer_handle_decodes() {
        let (client, from_engine, to_engine) = test_client();
        let engine = tokio::spawn(async move {
            let mut to_engine = to_engine;
            let raw = to_engine.recv().await.expect("client sends updateSnapshot");
            let (req, _) = decode_frame(&raw, 0).expect("decode request frame");
            let body = serde_json::json!({ "snapshot": 7, "projects": [] });
            let resp = encode_frame(
                MessageType::Response,
                req.name,
                &serde_json::to_vec(&body).unwrap(),
            );
            from_engine.send(resp).await.expect("engine reply");
        });
        let resp = client
            .update_snapshot(&UpdateSnapshotParams::default())
            .await
            .expect("an integer first handle decodes");
        engine.await.unwrap();
        assert_eq!(resp.snapshot, OpaqueHandle(7));
        assert!(resp.projects.is_empty());
    }

    // ── SCHEDULE COVERAGE: bind `PINNED_REQUEST_FIELDS` to what the codec ACTUALLY
    //    sends. For every op the codec drives, capture the real serialized request
    //    payload over the mock engine and assert its JSON object key set matches
    //    that op's scheduled row — EQ for the fixed-shape ops, and ⊆ the scheduled
    //    lease superset for `updateSnapshot` (whose first open sends only
    //    `openProjects` of the {openProjects,closeProjects,openFiles,closeFiles,
    //    fileChanges} family). DISCRIMINATING: a schedule row that DROPS a key the
    //    codec really sends — or GAINS one it never sends — fails this test, so the
    //    hand-maintained schedule cannot silently drift from the send-sites. The
    //    op-NAME-only fingerprint was blind to exactly this dimension (it shipped
    //    the `openProject`→`openProjects` fork), so binding the schedule to the
    //    real sends is what closes that gap. ──────────────────────────────────────
    #[tokio::test]
    async fn pinned_request_fields_match_real_send_sites() {
        use crate::proto::schema_manifest::PINNED_REQUEST_FIELDS;
        use std::collections::{BTreeMap, BTreeSet};

        let (client, from_engine, to_engine) = test_client();
        let recorded: Arc<parking_lot::Mutex<Vec<(String, serde_json::Value)>>> =
            Arc::new(parking_lot::Mutex::new(Vec::new()));

        // The mock engine records each (method, payload) and replies with a per-op
        // canned body so the client call resolves. It processes exactly the number
        // of ops the test drives, then returns (deterministic, no hang).
        const DRIVEN_OPS: usize = 8;
        let rec = Arc::clone(&recorded);
        let engine = tokio::spawn(async move {
            let mut to_engine = to_engine;
            for _ in 0..DRIVEN_OPS {
                let raw = to_engine
                    .recv()
                    .await
                    .expect("client sends a request frame");
                let (req, _) = decode_frame(&raw, 0).expect("decode request frame");
                let name = String::from_utf8_lossy(req.name).into_owned();
                let payload: serde_json::Value =
                    serde_json::from_slice(req.payload).expect("request payload is JSON");
                rec.lock().push((name.clone(), payload));
                let body: Vec<u8> = match name.as_str() {
                    // Integer handle so the first-open rail passes.
                    "updateSnapshot" => {
                        serde_json::to_vec(&serde_json::json!({ "snapshot": 1, "projects": [] }))
                            .unwrap()
                    }
                    "typeToString" => serde_json::to_vec(&serde_json::json!("number")).unwrap(),
                    "getTypeAtPosition" => {
                        serde_json::to_vec(&serde_json::json!({ "id": 7, "flags": 1 })).unwrap()
                    }
                    "getSymbolAtPosition" => serde_json::to_vec(
                        &serde_json::json!({ "id": 1, "name": "x", "flags": 0, "checkFlags": 0 }),
                    )
                    .unwrap(),
                    // The diagnostics getters decode `Vec<Diagnostic>`; `release` is
                    // fire-and-forget — an empty array resolves them all.
                    _ => b"[]".to_vec(),
                };
                let resp = encode_frame(MessageType::Response, req.name, &body);
                from_engine.send(resp).await.expect("engine reply");
            }
        });

        // Drive every op the codec SENDS. `initialize` is intentionally excluded —
        // its params are a bare JSON null (not an object) and its scheduled row is
        // empty; the schedule tracks object-shaped request payloads.
        let snap = OpaqueHandle(1);
        client
            .update_snapshot(&UpdateSnapshotParams::single_project("/ws/tsconfig.json"))
            .await
            .expect("updateSnapshot resolves");
        client
            .get_semantic_diagnostics(&snap, "proj", "/ws/A.tsx")
            .await
            .expect("getSemanticDiagnostics resolves");
        client
            .get_syntactic_diagnostics(&snap, "proj", "/ws/A.tsx")
            .await
            .expect("getSyntacticDiagnostics resolves");
        client
            .get_config_file_parsing_diagnostics(&snap, "proj")
            .await
            .expect("getConfigFileParsingDiagnostics resolves");
        client
            .get_type_at_position(&snap, "proj", "/ws/A.tsx", 10)
            .await
            .expect("getTypeAtPosition resolves");
        client
            .get_symbol_at_position(&snap, "proj", "/ws/A.tsx", 10)
            .await
            .expect("getSymbolAtPosition resolves");
        client
            .type_to_string(&snap, "proj", &OpaqueHandle(7))
            .await
            .expect("typeToString resolves");
        client.release(&snap).await.expect("release resolves");

        engine.await.expect("mock engine did not panic");

        // Index the schedule for lookup.
        let schedule: BTreeMap<&str, BTreeSet<&str>> = PINNED_REQUEST_FIELDS
            .iter()
            .map(|(op, fields)| (*op, fields.iter().copied().collect()))
            .collect();

        let recorded = recorded.lock();
        // We drove exactly the scheduled send ops (initialize excluded).
        let driven: BTreeSet<&str> = recorded.iter().map(|(m, _)| m.as_str()).collect();
        let expected: BTreeSet<&str> = [
            "updateSnapshot",
            "getSemanticDiagnostics",
            "getSyntacticDiagnostics",
            "getConfigFileParsingDiagnostics",
            "getTypeAtPosition",
            "getSymbolAtPosition",
            "typeToString",
            "release",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            driven, expected,
            "the test must drive exactly the scheduled send ops"
        );

        for (op, payload) in recorded.iter() {
            let keys: BTreeSet<&str> = payload
                .as_object()
                .unwrap_or_else(|| panic!("`{op}` params must be a JSON object: {payload}"))
                .keys()
                .map(String::as_str)
                .collect();
            let scheduled = schedule
                .get(op.as_str())
                .unwrap_or_else(|| panic!("`{op}` has no PINNED_REQUEST_FIELDS row"));
            if op == "updateSnapshot" {
                // The first open sends only `openProjects` of the lease family; the
                // scheduled row is the full superset, so the sent keys must be ⊆ it.
                assert!(
                    keys.is_subset(scheduled),
                    "`updateSnapshot` first-open keys {keys:?} must be ⊆ the scheduled \
                     lease superset {scheduled:?}"
                );
                assert!(
                    keys.contains("openProjects"),
                    "`updateSnapshot` first open must lease openProjects (got {keys:?})"
                );
            } else {
                // Fixed-shape ops: the sent key set must EQUAL the scheduled row, so
                // a dropped-or-added schedule key fails the test.
                assert_eq!(
                    &keys, scheduled,
                    "the codec's `{op}` send keys must EQUAL its schedule row"
                );
            }
        }
    }

    #[tokio::test]
    async fn connect_to_bogus_binary_is_typed_error() {
        // No real engine: probing version fails with a typed Spawn error before
        // any actor is spawned. (TsgoClient is intentionally not Debug, so we
        // match on the result rather than using expect_err.)
        let result = TsgoClient::connect(
            Path::new("definitely-not-tsgo-xyz"),
            std::env::temp_dir().as_path(),
            OverlaySnapshot::builder().build(),
            8,
        )
        .await;
        match result {
            Ok(_) => panic!("bogus binary must not connect"),
            Err(e) => assert!(matches!(e, TsgoApiError::Spawn(_)), "got {e:?}"),
        }
    }
}
