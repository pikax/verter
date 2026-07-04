//! The `--api` ATTACH orchestration: attach an `--api` checker session to a
//! `tsgo --lsp` connection and drive BOTH surfaces over the ONE process.
//!
//! This is the reusable primitive behind the one-instance dual-surface
//! provider. It is built around three seams so a consumer can reuse it without
//! rewriting the provider or the transport:
//!
//! - [`TsgoLspConnection`] — a live `tsgo --lsp` connection (the stdio JSON-RPC
//!   connection plus its child handle) tagged with its
//!   [`ConnectionOwnership`]: Verter either OWNS the engine (spawned it) or is
//!   merely ATTACHED to an editor-owned one. The attach operates on THIS,
//!   never on a hardcoded process.
//! - [`TsgoLspConnectionSource`] — how that connection is OBTAINED. The OWNED
//!   implementation is [`SpawnOwnTsgoLsp`] (Verter spawns its own
//!   `tsgo --lsp`); an editor-owned connection enters through
//!   [`TsgoAttach::attach_to_initialized`] instead.
//! - The composer split: [`TsgoAttach::lsp_handshake`] is the OWNED
//!   handshake-half (the SOLE Verter-originated `initialize`, reading the
//!   in-band `serverInfo.version` witness for the wire gate), and the
//!   source-agnostic attach-half mints + connects the `--api` session over an
//!   already-handshaken connection. [`TsgoAttach::attach_over`] composes both
//!   for an OWNED connection; [`TsgoAttach::attach_to_initialized`] attaches
//!   NON-OWNING to an already-initialized editor connection without ever
//!   re-`initialize`-ing it.
//!
//! ## The attach flow (proven live against the shipped binary)
//!
//! 1. Obtain a `tsgo --lsp` connection (the source).
//! 2. OWNED only: LSP `initialize` + `initialized` over that connection's
//!    stdio, gating the in-band `serverInfo.version` (an editor-owned
//!    connection is already initialized; its witness is supplied by the
//!    caller and gated per-attach).
//! 3. Send `custom/initializeAPISession` (params `{}`) over the SAME connection →
//!    the server mints a pipe (`\\.\pipe\tsgo-api-<hex>-<hex>` on Windows, a UDS
//!    path on Unix) sharing the `--lsp` server's `project.Session`, returning
//!    `{ sessionId, pipe }`.
//! 4. Connect that pipe and drive the `--api` checker over it ([`ApiAttachClient`]).
//!
//! Carriers are off-disk overlays injected via `--lsp` `textDocument/didOpen` on
//! the connection from step 1; because the `--api` session shares the `--lsp`
//! server's `project.Session`, those overlays are visible to the attached checker.
//!
//! ## Teardown is ownership-dispatched
//!
//! `teardown` is the SOLE public teardown entry on each ownership: an OWNED
//! engine gets the full private `shutdown` (`exit` + child kill); an
//! editor-owned engine gets the NON-OWNING [`TsgoAttach::detach`] — retract
//! Verter's own overlays via `textDocument/didClose` and drop the `--api`
//! pipe, never `exit`/`shutdown`/kill. The teardown DISPATCH is structural:
//! the `exit`-sending `shutdown` exists only on `TsgoAttach<Owned>` and is
//! private, and a non-owning attach holds no child/exit handle, so no
//! lifecycle/teardown path terminates an engine Verter did not spawn.
//!
//! ## The write surface is ownership-split
//!
//! [`TsgoAttach`] is generic over its [`AttachOwnership`] marker ([`Owned`] /
//! [`NonOwning`]), mirroring the runtime [`ConnectionOwnership`] tag the
//! constructors enforce. For non-owning/editor-owned attach handles, public
//! carrier writes are available ONLY through [`CarrierInjectionChannel`]. The
//! channel deny-by-default allowlist permits Verter overlay lifecycle
//! notifications, the ordered sync-barrier request, and
//! `custom/initializeAPISession` re-emission; ALL other methods are rejected
//! before reaching the wire. Owned attach handles may expose the raw
//! [`JsonRpcConnection`] (the raw accessor exists ONLY on
//! `TsgoAttach<Owned>`); non-owning attach handles must not expose or clone
//! it through public API. This layer does NOT claim read-side leak
//! suppression, feature-read routing, mode selection, live editor
//! attachment, or proof that injected carriers appear in the editor Program —
//! those concerns are OUT of this layer's scope.

use std::collections::HashSet;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex as StdMutex;

use crate::api_attach::{ApiAttachClient, AttachSnapshot};
use crate::error::{TsgoApiError, TsgoApiResult};
use crate::gate::{self, EngineVersionWitness, GateClearance, ObservedEngine};
use crate::jsonrpc::JsonRpcConnection;
use crate::relay::CarrierInjectionChannel;
use crate::transport::pipe_attach::connect_attach_pipe;
use crate::transport::spawn::discover_tsgo;

/// Seals [`AttachOwnership`]: the ownership markers are a closed set — the
/// owned/non-owning write-surface split is not extensible from outside.
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Owned {}
    impl Sealed for super::NonOwning {}
}

/// The compile-time ownership marker of a [`TsgoAttach`] handle. Mirrors the
/// runtime [`ConnectionOwnership`] tag (which the constructors enforce) and
/// selects the handle's PUBLIC write surface: only `TsgoAttach<Owned>`
/// exposes the raw [`JsonRpcConnection`]; `TsgoAttach<NonOwning>` writes
/// exclusively through the gated [`CarrierInjectionChannel`].
pub trait AttachOwnership: sealed::Sealed + Send + Sync + 'static {
    /// The runtime [`ConnectionOwnership`] tag this compile-time marker
    /// mirrors — the structural link `TsgoAttach::from_parts` asserts, so a
    /// type-state/runtime mismatch cannot be assembled silently.
    fn expected_connection_ownership() -> ConnectionOwnership;
}

/// Marker: Verter spawned (and owns) the engine behind the attach.
#[derive(Debug, Clone, Copy)]
pub struct Owned;

/// Marker: the attach rides an editor-owned engine Verter did not spawn.
#[derive(Debug, Clone, Copy)]
pub struct NonOwning;

impl AttachOwnership for Owned {
    fn expected_connection_ownership() -> ConnectionOwnership {
        ConnectionOwnership::Owned
    }
}
impl AttachOwnership for NonOwning {
    fn expected_connection_ownership() -> ConnectionOwnership {
        ConnectionOwnership::AttachedNonOwning
    }
}

/// The LSP `custom/` method that asks a `tsgo --lsp` server to mint an `--api`
/// session sharing its `project.Session`. Verified against the shipped rc
/// `typescript` engine binary (the JS package carries only the doc comment; the
/// method + result shape live in the Go server).
pub const INITIALIZE_API_SESSION_METHOD: &str = "custom/initializeAPISession";

/// Whether Verter OWNS the engine behind a [`TsgoLspConnection`] or is merely
/// ATTACHED to an editor-owned engine. Governs the handshake (never a 2nd
/// `initialize` on an editor-owned connection) and teardown (never
/// `exit`/`shutdown`/kill an engine Verter did not spawn).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionOwnership {
    /// Verter spawned this engine (e.g. [`SpawnOwnTsgoLsp`]). Verter runs the
    /// LSP handshake and, on teardown, may `exit`/close/kill.
    Owned,
    /// The connection is to an editor-owned engine ALREADY initialized by the
    /// editor. Verter MUST NOT originate a second `initialize`; teardown is
    /// NON-OWNING — `didClose` its own overlays + drop the `--api` pipe only,
    /// never `exit`/`shutdown`/kill.
    AttachedNonOwning,
}

/// A live `tsgo --lsp` connection: the JSON-RPC connection over its stdio plus the
/// child process handle (held so the process outlives the connection), tagged
/// with its [`ConnectionOwnership`]. This is the input the attach primitive
/// operates on.
pub struct TsgoLspConnection {
    /// The JSON-RPC connection over the child's stdio (the `--lsp` feature wire
    /// AND the channel `custom/initializeAPISession` is sent on).
    conn: JsonRpcConnection,
    /// The spawned child, kept alive for the life of the connection. Always
    /// `None` for an [`ConnectionOwnership::AttachedNonOwning`] connection —
    /// the editor owns the process; Verter holds no kill handle.
    child: Option<tokio::process::Child>,
    /// Whether Verter owns the engine behind this connection.
    ownership: ConnectionOwnership,
}

impl TsgoLspConnection {
    /// Base constructor shared by the ownership-explicit constructors.
    fn new(
        conn: JsonRpcConnection,
        child: Option<tokio::process::Child>,
        ownership: ConnectionOwnership,
    ) -> Self {
        Self {
            conn,
            child,
            ownership,
        }
    }

    /// Build an OWNED connection from an already-established JSON-RPC
    /// connection + an optional child handle (the spawn path passes the child;
    /// tests may pass `None`).
    #[must_use]
    pub fn new_owned(conn: JsonRpcConnection, child: Option<tokio::process::Child>) -> Self {
        Self::new(conn, child, ConnectionOwnership::Owned)
    }

    /// Build a NON-OWNING connection to an editor-owned engine. Carries no
    /// child handle by construction — Verter must never hold a kill handle to
    /// a process it did not spawn.
    #[must_use]
    pub fn new_attached(conn: JsonRpcConnection) -> Self {
        Self::new(conn, None, ConnectionOwnership::AttachedNonOwning)
    }

    /// Whether Verter owns the engine behind this connection.
    #[must_use]
    pub fn ownership(&self) -> ConnectionOwnership {
        self.ownership
    }
}

impl std::fmt::Debug for TsgoLspConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsgoLspConnection")
            .field("has_child", &self.child.is_some())
            .field("ownership", &self.ownership)
            .finish_non_exhaustive()
    }
}

/// How a `tsgo --lsp` connection is obtained. The attach primitive is generic over
/// this so the connection SOURCE is a seam: OWNED spawns Verter's own engine; an
/// editor-owned connection is handed to [`TsgoAttach::attach_to_initialized`]
/// directly, without touching the attach/provider code.
pub trait TsgoLspConnectionSource: Send + Sync {
    /// Establish a `tsgo --lsp` connection (spawn-and-connect, or hand back an
    /// existing one). The returned connection's stdio carries both the `--lsp`
    /// features and the `custom/initializeAPISession` attach request.
    fn establish(
        &self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TsgoApiResult<TsgoLspConnection>> + Send + '_>,
    >;
}

/// The OWNED connection source: spawn Verter's own `tsgo --lsp --stdio`.
#[derive(Debug, Clone)]
pub struct SpawnOwnTsgoLsp {
    exe: PathBuf,
    cwd: PathBuf,
}

impl SpawnOwnTsgoLsp {
    /// Build the source for an explicit engine binary + working directory.
    #[must_use]
    pub fn new(exe: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            exe: exe.into(),
            cwd: cwd.into(),
        }
    }

    /// Build the source by discovering the engine under `workspace_root`
    /// (used as the cwd). Discovery searches ONLY the workspace
    /// `node_modules` — the pnpm `.pnpm` store layout and the classic
    /// `@typescript/<name>` sibling layout — for the rc `typescript` package's
    /// `tsc` binary (see [`discover_tsgo`]). There is NO env-var override, NO
    /// PATH search, and NO npm/npx cache probe; an explicit binary goes
    /// through [`SpawnOwnTsgoLsp::new`] instead.
    pub fn discover(workspace_root: impl AsRef<Path>) -> TsgoApiResult<Self> {
        let root = workspace_root.as_ref();
        let exe = discover_tsgo(root)?;
        Ok(Self {
            exe,
            cwd: root.to_path_buf(),
        })
    }

    /// The discovered/explicit engine binary path.
    #[must_use]
    pub fn exe(&self) -> &Path {
        &self.exe
    }
}

impl TsgoLspConnectionSource for SpawnOwnTsgoLsp {
    fn establish(
        &self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TsgoApiResult<TsgoLspConnection>> + Send + '_>,
    > {
        Box::pin(async move { spawn_own_lsp_connection(&self.exe, &self.cwd).await })
    }
}

/// Spawn `tsgo --lsp --stdio` and wrap its stdio in a [`JsonRpcConnection`].
async fn spawn_own_lsp_connection(exe: &Path, cwd: &Path) -> TsgoApiResult<TsgoLspConnection> {
    let mut child = tokio::process::Command::new(exe)
        .arg("--lsp")
        .arg("--stdio")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            TsgoApiError::Spawn(format!("spawn `{} --lsp --stdio`: {e}", exe.display()))
        })?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| TsgoApiError::Spawn("tsgo --lsp child stdin not piped".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| TsgoApiError::Spawn("tsgo --lsp child stdout not piped".into()))?;

    let conn = JsonRpcConnection::connect(stdout, stdin);
    Ok(TsgoLspConnection::new_owned(conn, Some(child)))
}

/// The result of `custom/initializeAPISession`: the minted pipe path + session id.
#[derive(Debug, Clone)]
pub struct ApiSessionHandle {
    /// The opaque server-assigned session id (`InitializeAPISessionResult.sessionId`).
    pub session_id: String,
    /// The server-minted pipe path to connect the `--api` checker to.
    pub pipe: String,
}

/// A live one-instance dual-surface attach: ONE `tsgo --lsp` connection + the
/// `--api` checker attached over the minted pipe. Both surfaces ride the ONE
/// process / ONE shared `project.Session`.
///
/// The [`AttachOwnership`] marker `O` selects the public write surface:
/// `TsgoAttach<Owned>` (the default) exposes the raw connection via its
/// owned-only raw accessor; `TsgoAttach<NonOwning>` writes ONLY through the
/// gated [`TsgoAttach::injection_channel`].
pub struct TsgoAttach<O: AttachOwnership = Owned> {
    lsp: TsgoLspConnection,
    api: ApiAttachClient,
    session: ApiSessionHandle,
    /// The engine version the wire gate accepted for this attach — the value
    /// that flows to the `--api` `updateSnapshot` rail.
    observed_version: String,
    /// How that version was observed (in-band `serverInfo` vs a probe).
    witness: EngineVersionWitness,
    /// The overlay URIs Verter itself opened via
    /// [`CarrierInjectionChannel::did_open`] on this attach's channel,
    /// tracked so a NON-OWNING [`TsgoAttach::detach`] can retract exactly
    /// them. A std Mutex: lock, mutate, drop the guard — NEVER held across an
    /// `.await`.
    open_overlays: StdMutex<HashSet<String>>,
    /// The compile-time ownership marker (mirrors `lsp.ownership()`).
    _own: PhantomData<O>,
}

impl<O: AttachOwnership> std::fmt::Debug for TsgoAttach<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsgoAttach")
            .field("session_id", &self.session.session_id)
            .field("pipe", &self.session.pipe)
            .field("ownership", &self.lsp.ownership)
            .field("observed_version", &self.observed_version)
            .finish_non_exhaustive()
    }
}

/// Ownership-agnostic surface: reads, the `--api` snapshot rail, the
/// `--api` session mint, and the gated carrier-injection write channel.
impl<O: AttachOwnership> TsgoAttach<O> {
    /// Assemble an attach from its parts, storing the gate clearance's
    /// accepted version + witness and an empty overlay set. Private: the
    /// public constructors enforce that the runtime [`ConnectionOwnership`]
    /// tag matches the compile-time marker, and the mirror is asserted
    /// structurally here via
    /// [`AttachOwnership::expected_connection_ownership`].
    fn from_parts(
        lsp: TsgoLspConnection,
        api: ApiAttachClient,
        session: ApiSessionHandle,
        clearance: GateClearance,
    ) -> Self {
        debug_assert_eq!(
            lsp.ownership(),
            O::expected_connection_ownership(),
            "the runtime ConnectionOwnership tag must mirror the compile-time \
             AttachOwnership marker"
        );
        Self {
            lsp,
            api,
            session,
            observed_version: clearance.observed_version,
            witness: clearance.witness,
            open_overlays: StdMutex::new(HashSet::new()),
            _own: PhantomData,
        }
    }

    /// The connection-source-agnostic attach-half: mint the `--api` session
    /// over an already-handshaken connection, connect the minted pipe,
    /// construct + initialize the `--api` checker. Reusable for ANY connection
    /// source (owned OR editor-owned).
    async fn attach_api_session(
        conn: &JsonRpcConnection,
    ) -> TsgoApiResult<(ApiSessionHandle, ApiAttachClient)> {
        let session = Self::initialize_api_session(conn).await?;
        let (read, write) = connect_attach_pipe(&session.pipe).await?;
        let api = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
        api.initialize().await?;
        Ok((session, api))
    }

    /// Send `custom/initializeAPISession` over a `tsgo --lsp` connection and parse
    /// the `{ sessionId, pipe }` result. The reusable attach handshake — usable for
    /// ANY `tsgo --lsp` connection (owned OR editor-owned), which is why it
    /// lives on the ownership-agnostic surface.
    pub async fn initialize_api_session(
        conn: &JsonRpcConnection,
    ) -> TsgoApiResult<ApiSessionHandle> {
        let value = conn
            .request(INITIALIZE_API_SESSION_METHOD, serde_json::json!({}))
            .await?;
        parse_api_session_handle(&value)
    }

    /// The gated carrier-injection write surface over this attach's `--lsp`
    /// connection: overlay lifecycle (`didOpen`/`didChange`/`didClose`), the
    /// ordered sync barrier, and `custom/initializeAPISession` re-emission —
    /// everything else is refused before the wire (deny-by-default). On a
    /// non-owning attach this is the only public `--lsp` carrier-write
    /// surface (the `--api` snapshot rail — `api()` / `update_snapshot()` —
    /// is separate).
    #[must_use]
    pub fn injection_channel(&self) -> CarrierInjectionChannel<'_> {
        CarrierInjectionChannel::new(&self.lsp.conn, &self.open_overlays)
    }

    /// The attached `--api` checker client (diagnostics + checker reflection).
    #[must_use]
    pub fn api(&self) -> &ApiAttachClient {
        &self.api
    }

    /// The minted `--api` session handle.
    #[must_use]
    pub fn session(&self) -> &ApiSessionHandle {
        &self.session
    }

    /// The engine version the wire gate accepted for this attach — the value
    /// the `--api` `updateSnapshot` rail is driven with.
    #[must_use]
    pub fn observed_version(&self) -> &str {
        &self.observed_version
    }

    /// How the accepted engine version was observed.
    #[must_use]
    pub fn witness(&self) -> EngineVersionWitness {
        self.witness
    }

    /// Open / refresh the configured-project snapshot using the STORED
    /// gate-accepted engine version (the in-band witness) — the convenience
    /// over [`ApiAttachClient::update_snapshot_open_project`] so callers never
    /// re-supply (or hardcode) the version the gate already validated.
    pub async fn update_snapshot(&self, tsconfig_path: &str) -> TsgoApiResult<AttachSnapshot> {
        self.api
            .update_snapshot_open_project(tsconfig_path, &self.observed_version)
            .await
    }
}

/// OWNED-only surface: the handshake, the owned composer, the raw wire, and
/// the engine-terminating teardown.
impl TsgoAttach<Owned> {
    /// Establish the full OWNED attach from a connection source: obtain the
    /// `tsgo --lsp` connection, run the LSP handshake, attach the `--api`
    /// checker over the minted pipe, and initialize the checker.
    ///
    /// `root_uri` is the workspace folder URI sent in LSP `initialize`.
    pub async fn establish(
        source: &dyn TsgoLspConnectionSource,
        root_uri: &str,
    ) -> TsgoApiResult<Self> {
        let lsp = source.establish().await?;
        Self::attach_over(lsp, root_uri).await
    }

    /// The OWNED handshake-half: run LSP `initialize`/`initialized` on a
    /// connection Verter spawned, READ the in-band `serverInfo.version`
    /// witness from the `initialize` result, and feed it to the wire gate
    /// (fail-closed on an unknown channel). Returns the accepted clearance
    /// (its `observed_version` flows to the `--api` `updateSnapshot` rail).
    /// This is the ONLY place Verter originates an `initialize` — an
    /// editor-owned connection is already initialized and MUST NOT be
    /// re-initialized.
    pub async fn lsp_handshake(
        conn: &JsonRpcConnection,
        root_uri: &str,
    ) -> TsgoApiResult<GateClearance> {
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {},
            "workspaceFolders": [{ "uri": root_uri, "name": "verter" }],
        });
        let init = conn.request("initialize", init_params).await?;
        let version = init
            .get("serverInfo")
            .and_then(|s| s.get("version"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                TsgoApiError::UnsupportedTsgoWire(format!(
                    "`--lsp` initialize result carried no serverInfo.version; \
                     cannot gate the engine: {init}"
                ))
            })?;
        let clearance = gate::validate(&ObservedEngine::from_in_band_server_info(version))?;
        conn.notify("initialized", serde_json::json!({})).await?;
        Ok(clearance)
    }

    /// The OWNED composer: given an OWNED, freshly-spawned `tsgo --lsp`
    /// connection, run the LSP handshake (gating the in-band
    /// `serverInfo.version`), then attach + initialize the `--api` checker.
    ///
    /// REFUSES a non-owning connection: an editor-owned connection is already
    /// initialized, and a second Verter-originated `initialize` would be a
    /// protocol violation — use [`TsgoAttach::attach_to_initialized`] instead.
    pub async fn attach_over(lsp: TsgoLspConnection, root_uri: &str) -> TsgoApiResult<Self> {
        if lsp.ownership() != ConnectionOwnership::Owned {
            return Err(TsgoApiError::Transport(
                "attach_over runs the OWNED handshake and must not re-`initialize` an \
                 editor-owned connection; use `attach_to_initialized` for a non-owning attach"
                    .into(),
            ));
        }
        let clearance = Self::lsp_handshake(&lsp.conn, root_uri).await?;
        let (session, api) = Self::attach_api_session(&lsp.conn).await?;
        Ok(Self::from_parts(lsp, api, session, clearance))
    }

    /// The raw `--lsp` feature connection (hover/definition/references/…
    /// requests) — OWNED ONLY. Verter spawned this engine, so unrestricted
    /// raw-wire access cannot affect an engine it does not own. A non-owning
    /// attach has NO raw accessor: its sole public write surface is the
    /// deny-by-default [`TsgoAttach::injection_channel`].
    #[must_use]
    pub fn lsp(&self) -> &JsonRpcConnection {
        &self.lsp.conn
    }

    /// OWNED full teardown — PRIVATE: reachable only through the owned
    /// [`TsgoAttach::teardown`]. Sends `exit`, closes the connection, and
    /// kills the child. Keeping this private (and owned-only) makes the
    /// teardown DISPATCH structural: no lifecycle/teardown path sends `exit`
    /// on an editor-owned connection.
    async fn shutdown(mut self) -> TsgoApiResult<()> {
        let _ = self.api.close().await;
        let _ = self.lsp.conn.notify("exit", serde_json::Value::Null).await;
        let _ = self.lsp.conn.close().await;
        if let Some(mut child) = self.lsp.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        Ok(())
    }

    /// OWNED teardown: the full private `shutdown` (`exit` + child kill).
    /// The private `shutdown` is the SOLE `exit`-sender, and it exists only
    /// on `TsgoAttach<Owned>` — no teardown API terminates a non-owning
    /// attach.
    pub async fn teardown(self) -> TsgoApiResult<()> {
        self.shutdown().await
    }
}

/// NON-OWNING surface: the non-owning composer and the non-terminating
/// teardown. NO raw-wire accessor exists here — public writes on a
/// non-owning attach go exclusively through the gated
/// [`TsgoAttach::injection_channel`].
impl TsgoAttach<NonOwning> {
    /// Attach the `--api` checker to an editor-owned, ALREADY-initialized
    /// `--lsp` connection WITHOUT re-initializing it. The in-band
    /// `serverInfo.version` witness is supplied by the caller (a relay reads
    /// it from the pass-through `initialize`); it is fed to the wire gate
    /// per-attach (fail-closed). NON-OWNING: teardown never
    /// `exit`/`shutdown`/kills this connection.
    ///
    /// REFUSES an owned connection — symmetric with
    /// [`TsgoAttach::attach_over`]'s refusal of a non-owning one: an `Owned`
    /// connection must not enter the non-owning composer (its `teardown`
    /// would otherwise skip the engine-terminating owned arm the connection
    /// requires); use [`TsgoAttach::attach_over`] instead.
    pub async fn attach_to_initialized(
        lsp: TsgoLspConnection,
        observed_version: impl Into<String>,
    ) -> TsgoApiResult<Self> {
        if lsp.ownership() != ConnectionOwnership::AttachedNonOwning {
            return Err(TsgoApiError::Transport(
                "attach_to_initialized requires a non-owning (editor-owned) connection; \
                 use `attach_over` for an owned connection Verter spawned"
                    .into(),
            ));
        }
        // Gate the supplied in-band witness BEFORE opening the session (the
        // per-attach gate).
        let clearance = gate::validate(&ObservedEngine::from_in_band_server_info(
            observed_version.into(),
        ))?;
        let (session, api) = Self::attach_api_session(&lsp.conn).await?;
        Ok(Self::from_parts(lsp, api, session, clearance))
    }

    /// NON-OWNING teardown: retract EXACTLY the carrier overlays Verter opened on
    /// this attach — every one is tracked in `open_overlays` because
    /// [`CarrierInjectionChannel::did_open`] (the only overlay-open path) records
    /// it — via `textDocument/didClose`, and drop the `--api` pipe. NEVER sends
    /// `exit`/`shutdown` and NEVER kills the
    /// process — the editor owns the engine's lifecycle. Leaves the `--lsp`
    /// connection otherwise untouched. (An [`ConnectionOwnership::AttachedNonOwning`]
    /// connection carries no child handle by construction, so no kill path is
    /// reachable here even structurally.) The retractions ride the gated
    /// carrier-injection channel — the same deny-by-default write path as
    /// every other non-owning write.
    ///
    /// The `didClose` retractions are best-effort BY DESIGN: the editor may
    /// have already closed the connection, and a non-owning teardown must not
    /// hard-fail on an unreachable peer. The guaranteed invariant is "issues
    /// no `exit`/`shutdown`/kill", not "guarantees didClose delivery".
    ///
    /// The non-owning guarantee is about what Verter SENDS; the OS process
    /// lifecycle belongs to the editor. Contract on the supplied connection:
    /// a live editor attachment hands the non-owning composer a transport
    /// whose Verter-side drop cannot terminate the editor engine — a
    /// relay-interposed transport (see [`crate::relay::LspRelay`]), never
    /// the engine's own stdio. Detach only stops Verter's use of that
    /// transport; it cannot make a caller-supplied engine-fatal transport
    /// safe.
    pub async fn detach(self) -> TsgoApiResult<()> {
        // Lock, snapshot, drop the guard — never held across the awaits below.
        let uris: Vec<String> = { self.open_overlays.lock().unwrap().iter().cloned().collect() };
        let channel = self.injection_channel();
        for uri in uris {
            // Retract through the typed lifecycle op (the same deny-by-default
            // gate); best-effort — a closed peer must not hard-fail teardown.
            let _ = channel.did_close(&uri).await;
        }
        // Drop the --api pipe; the --lsp connection and the engine stay alive.
        let _ = self.api.close().await;
        Ok(())
    }

    /// NON-OWNING teardown entry: [`TsgoAttach::detach`] — retract Verter's
    /// own overlays and drop the `--api` pipe, never `exit`/`shutdown`/kill.
    pub async fn teardown(self) -> TsgoApiResult<()> {
        self.detach().await
    }
}

/// Parse a `custom/initializeAPISession` result value into its
/// [`ApiSessionHandle`] — the ONE parse shared by the direct attach handshake
/// and the gated channel's session re-emission.
pub(crate) fn parse_api_session_handle(
    value: &serde_json::Value,
) -> TsgoApiResult<ApiSessionHandle> {
    let pipe = value
        .get("pipe")
        .and_then(|p| p.as_str())
        .ok_or_else(|| {
            TsgoApiError::Transport(format!(
                "{INITIALIZE_API_SESSION_METHOD} result missing `pipe`: {value}"
            ))
        })?
        .to_string();
    let session_id = value
        .get("sessionId")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    Ok(ApiSessionHandle { session_id, pipe })
}

/// Discover the engine + cwd and build the OWNED spawn source in one step.
pub fn owned_source_for(workspace_root: impl AsRef<Path>) -> TsgoApiResult<SpawnOwnTsgoLsp> {
    SpawnOwnTsgoLsp::discover(workspace_root)
}

#[cfg(test)]
#[path = "attach_tests.rs"]
mod tests;
