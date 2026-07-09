//! The typed `--api` checker client over an attached vscode-jsonrpc connection.
//!
//! This drives the tsgo `--api` Program/Checker/diagnostics ops over a
//! [`JsonRpcConnection`] connected to the server-minted pipe (the
//! `custom/initializeAPISession` attach). It REUSES the standalone path's typed
//! DTOs and method-name constants ([`crate::proto::types`]) verbatim — only the
//! framing (vscode-jsonrpc) and transport (the pipe) differ from the standalone
//! MessagePack wire.
//!
//! Method/param shapes mirror the shipped async client (`dist/api/async/api.js`):
//! every op sends its plain method name with `params` carrying `snapshot` and (for
//! project-scoped ops) `project`, plus op-specific fields. For an LSP-ATTACHED
//! session `updateSnapshot` leases the GA ref-counted `openProjects: [tsconfig]`
//! the FIRST time each DISTINCT project is opened and OMITS it on a later snapshot
//! of an already-open project — the open persists across snapshots on the engine
//! (a re-send would double-increment the refcount). One `ApiAttachClient` serves
//! MULTIPLE projects (per-carrier), so the lease is keyed PER-PROJECT, not on a
//! single global first-snapshot latch. The `tsgo --lsp` server owns
//! document/overlay state (Verter sets overlays via `--lsp` didOpen), so the
//! attached `--api` session never pushes `fileChanges`/`openFiles`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::TsgoApiResult;
use crate::gate;
use crate::jsonrpc::JsonRpcConnection;
use crate::proto::types::{
    method, Diagnostic, InitializeResponse, OpaqueHandle, ProjectResponse, TypeResponse,
};

/// A typed client for the `--api` checker surface over an attached connection.
#[derive(Debug, Clone)]
pub struct ApiAttachClient {
    conn: JsonRpcConnection,
    /// Whether a first `updateSnapshot` response has passed the integer-handle
    /// rail ([`gate::require_integer_snapshot_handle`]). Shared across clones
    /// (the clones share one engine), so the validated raw first-response path
    /// runs once per attach, not once per clone. Lock-free fast-path check;
    /// cold-start serialization rides [`Self::first_validation_lock`].
    first_snapshot_validated: Arc<AtomicBool>,
    /// Serializes the cold-start first-`updateSnapshot` validation: exactly one
    /// caller runs the rail while concurrent clones wait, then take the fast
    /// path. Shared across clones (they share one engine).
    first_validation_lock: Arc<tokio::sync::Mutex<()>>,
    /// The set of tsconfig paths whose GA `openProjects` lease has been
    /// SUCCESSFULLY opened on the engine. GA `openProjects` opens are
    /// ref-counted and ADDITIVE, so each DISTINCT project must send
    /// `openProjects` exactly once; a subsequent snapshot of an already-leased
    /// project omits it (the refcount persists). ONE `ApiAttachClient` serves
    /// MULTIPLE projects (per-carrier), so the lease is keyed on the tsconfig
    /// path — NOT the global [`Self::first_snapshot_validated`] latch, which is
    /// the orthogonal one-per-attach integer-handle rail. Shared across clones
    /// (they share one engine); a path is inserted ONLY after its open fully
    /// succeeds, so a failed open re-leases on retry.
    leased_projects: Arc<tokio::sync::Mutex<HashSet<String>>>,
}

/// The configured-project snapshot the attached `--api` session opened: the
/// snapshot handle plus the resolved projects (each with its `rootFiles`,
/// `compilerOptions`, and config path). The membership check (was the carrier
/// companion admitted as a Program root?) reads `ProjectResponse::root_files`.
#[derive(Debug, Clone)]
pub struct AttachSnapshot {
    /// The opaque snapshot handle the engine returned (`updateSnapshot.snapshot`),
    /// a bare integer; see [`OpaqueHandle`].
    pub snapshot: OpaqueHandle,
    /// The resolved projects in this snapshot.
    pub projects: Vec<ProjectResponse>,
}

impl AttachSnapshot {
    /// The project whose `configFileName` matches `tsconfig` (path-normalized by
    /// the caller), if any — the configured project the carrier should belong to.
    #[must_use]
    pub fn project_for_config(&self, predicate: impl Fn(&str) -> bool) -> Option<&ProjectResponse> {
        self.projects
            .iter()
            .find(|p| predicate(&p.config_file_name))
    }
}

impl ApiAttachClient {
    /// Wrap an already-connected `--api` pipe connection. The caller is
    /// responsible for having sent `custom/initializeAPISession` over the `--lsp`
    /// connection and connected this [`JsonRpcConnection`] to the minted pipe.
    #[must_use]
    pub fn new(conn: JsonRpcConnection) -> Self {
        Self {
            conn,
            first_snapshot_validated: Arc::new(AtomicBool::new(false)),
            first_validation_lock: Arc::new(tokio::sync::Mutex::new(())),
            leased_projects: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
        }
    }

    /// Send the `--api` `initialize` (params `null`), establishing the checker
    /// session over the pipe. Mirrors the async client's `ensureInitialized`.
    pub async fn initialize(&self) -> TsgoApiResult<InitializeResponse> {
        let value = self
            .conn
            .request(method::INITIALIZE, serde_json::Value::Null)
            .await?;
        deserialize(value)
    }

    /// Open / refresh the configured-project snapshot for `tsconfig_path`. The
    /// FIRST time each DISTINCT project is opened leases the GA ref-counted
    /// `openProjects: [tsconfig]`; a later snapshot of an already-leased project
    /// omits it (the open persists on the engine — see
    /// [`Self::request_update_snapshot`]). The `--lsp` server owns documents, so
    /// no `fileChanges` ride this call.
    ///
    /// The FIRST response (across the whole attach, ANY project) is run through
    /// the version-lie-immune integer-handle rail
    /// ([`gate::require_integer_snapshot_handle`]): an engine whose snapshot
    /// handle is not a bare JSON integer speaks a different opaque-handle wire
    /// class and is refused with a typed
    /// [`crate::error::TsgoApiError::UnsupportedTsgoWire`] naming
    /// `observed_version`, BEFORE any product result. `observed_version` is the
    /// engine version the wire gate accepted.
    ///
    /// Cold start is a double-checked async init: the rail runs exactly once
    /// even under concurrent first calls — one caller holds
    /// [`Self::first_validation_lock`] and validates while the rest wait, then
    /// take the fast path. Steady state is a single atomic load, no lock. The
    /// flag flips ONLY after the rail AND the typed decode both succeed, so a
    /// refused first call never unlocks the fast path (fail-closed).
    ///
    /// The per-project `openProjects` lease is ORTHOGONAL to that one-per-attach
    /// rail: a project's tsconfig path is recorded leased ONLY after its open
    /// fully succeeds (rail — on the first ever — plus decode), so a failed open
    /// re-leases on retry and every distinct project sends `openProjects` exactly
    /// once.
    pub async fn update_snapshot_open_project(
        &self,
        tsconfig_path: &str,
        observed_version: &str,
    ) -> TsgoApiResult<AttachSnapshot> {
        // Fast path: the cold-start rail already cleared. The per-project lease
        // still decides openProjects-vs-`{}` per tsconfig path.
        if self.first_snapshot_validated.load(Ordering::Acquire) {
            return self.warm_open_and_lease(tsconfig_path).await;
        }
        // Slow path: serialize the cold-start validation.
        let _guard = self.first_validation_lock.lock().await;
        if self.first_snapshot_validated.load(Ordering::Acquire) {
            // Another caller validated the rail while we waited for the lock.
            return self.warm_open_and_lease(tsconfig_path).await;
        }
        let value = self.request_update_snapshot(tsconfig_path).await?;
        gate::require_integer_snapshot_handle(&value["snapshot"], observed_version)?;
        let snap = decode_attach_snapshot(value)?;
        // Flip the warm flag ONLY after the full first response validated AND decoded.
        self.first_snapshot_validated.store(true, Ordering::Release);
        // Record the project leased only after the full open succeeded.
        self.mark_project_leased(tsconfig_path).await;
        Ok(snap)
    }

    /// The warm open (the cold-start rail already cleared): send the snapshot
    /// request (leasing `openProjects` iff this project is not yet leased),
    /// decode it, and record the project leased on success. Shared by the atomic
    /// fast path and the slow-path double-check so both lease identically.
    async fn warm_open_and_lease(&self, tsconfig_path: &str) -> TsgoApiResult<AttachSnapshot> {
        let value = self.request_update_snapshot(tsconfig_path).await?;
        let snap = decode_attach_snapshot(value)?;
        self.mark_project_leased(tsconfig_path).await;
        Ok(snap)
    }

    /// Record `tsconfig_path` as a successfully-opened GA `openProjects` lease.
    /// Idempotent (a re-open of an already-leased project is a no-op insert).
    async fn mark_project_leased(&self, tsconfig_path: &str) {
        self.leased_projects
            .lock()
            .await
            .insert(tsconfig_path.to_owned());
    }

    /// Send the `updateSnapshot` request and return its raw JSON-RPC result (the
    /// rail inspects the `snapshot` handle before decode).
    ///
    /// GA `openProjects` opens are REF-COUNTED, ADDITIVE, and PER-PROJECT. A
    /// project whose tsconfig is NOT yet in [`Self::leased_projects`] leases
    /// `openProjects: [tsconfig]`; an already-leased project OMITS it (sends an
    /// empty `{}`) because the open persists on the engine — re-sending it would
    /// double-increment the refcount (a leak). The decision keys on the
    /// per-project lease set (populated only after a full successful open), so
    /// EVERY distinct project the shared client serves opens exactly once and a
    /// refused/failed open re-leases on retry (fail-closed, refcount-safe). The
    /// lease is read under the mutex and released BEFORE the request await, so
    /// distinct projects open in parallel; a concurrent first-open of the SAME
    /// project may benignly double-send `openProjects` (an extra refcount), which
    /// is harmless.
    async fn request_update_snapshot(
        &self,
        tsconfig_path: &str,
    ) -> TsgoApiResult<serde_json::Value> {
        let already_leased = self.leased_projects.lock().await.contains(tsconfig_path);
        let params = if already_leased {
            serde_json::json!({})
        } else {
            serde_json::json!({ "openProjects": [tsconfig_path] })
        };
        self.conn.request(method::UPDATE_SNAPSHOT, params).await
    }

    /// Semantic diagnostics for `file` in `project` under `snapshot` — the OWNED
    /// diagnostics source (the typecheck surface). Returns the raw `--api`
    /// diagnostics; the consumer maps them to its own carrier (mapping back through
    /// the source map is the provider's job).
    pub async fn get_semantic_diagnostics(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
        file: &str,
    ) -> TsgoApiResult<Vec<Diagnostic>> {
        let params = serde_json::json!({
            "snapshot": snapshot,
            "project": project,
            "file": file,
        });
        let value = self
            .conn
            .request(method::GET_SEMANTIC_DIAGNOSTICS, params)
            .await?;
        deserialize(value)
    }

    /// Syntactic diagnostics for `file` in `project` under `snapshot`.
    pub async fn get_syntactic_diagnostics(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
        file: &str,
    ) -> TsgoApiResult<Vec<Diagnostic>> {
        let params = serde_json::json!({
            "snapshot": snapshot,
            "project": project,
            "file": file,
        });
        let value = self
            .conn
            .request(method::GET_SYNTACTIC_DIAGNOSTICS, params)
            .await?;
        deserialize(value)
    }

    /// The checker type at a byte offset in `file` (the reflection primitive the
    /// hover/definition oracle reads). `None` when the engine returns no type.
    pub async fn get_type_at_position(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
        file: &str,
        position: u32,
    ) -> TsgoApiResult<Option<TypeResponse>> {
        let params = serde_json::json!({
            "snapshot": snapshot,
            "project": project,
            "file": file,
            "position": position,
        });
        let value = self
            .conn
            .request(method::GET_TYPE_AT_POSITION, params)
            .await?;
        if value.is_null() {
            return Ok(None);
        }
        Ok(Some(deserialize(value)?))
    }

    /// Render a resolved type handle to its display string (`typeToString`). The
    /// type handle is the engine's opaque integer id (it flows from
    /// [`TypeResponse::id`]), so it rides into the request params as a bare integer.
    pub async fn type_to_string(
        &self,
        snapshot: &OpaqueHandle,
        project: &str,
        type_id: &OpaqueHandle,
    ) -> TsgoApiResult<String> {
        let params = serde_json::json!({
            "snapshot": snapshot,
            "project": project,
            "type": type_id,
        });
        let value = self.conn.request(method::TYPE_TO_STRING, params).await?;
        deserialize(value)
    }

    /// Close the attached connection.
    pub async fn close(&self) -> TsgoApiResult<()> {
        self.conn.close().await
    }

    /// Borrow the underlying connection (e.g. to abandon in-flight work on supersession).
    #[must_use]
    pub fn connection(&self) -> &JsonRpcConnection {
        &self.conn
    }
}

/// Deserialize a JSON-RPC `result` value into a typed DTO, mapping a parse failure
/// to a typed [`crate::error::TsgoApiError::Json`].
fn deserialize<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> TsgoApiResult<T> {
    serde_json::from_value(value)
        .map_err(|e| crate::error::TsgoApiError::Json(format!("--api result decode: {e}")))
}

/// Decode an `updateSnapshot` raw result into an [`AttachSnapshot`]. Shared by
/// the fast and cold-start paths so both decode identically.
fn decode_attach_snapshot(value: serde_json::Value) -> TsgoApiResult<AttachSnapshot> {
    let resp: crate::proto::types::UpdateSnapshotResponse = deserialize(value)?;
    Ok(AttachSnapshot {
        snapshot: resp.snapshot,
        projects: resp.projects,
    })
}

#[cfg(test)]
#[path = "api_attach_tests.rs"]
mod tests;
