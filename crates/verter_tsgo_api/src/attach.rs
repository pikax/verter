//! The `--api` ATTACH orchestration: attach an `--api` checker session to a
//! `tsgo --lsp` connection and drive BOTH surfaces over the ONE process.
//!
//! This is the reusable primitive behind the OWNED one-instance dual-surface
//! provider. It is built around two seams so a future phase can reuse it without
//! rewriting the provider or the transport:
//!
//! - [`TsgoLspConnection`] — a live `tsgo --lsp` connection (the stdio JSON-RPC
//!   connection plus its child handle). The attach operates on THIS, never on a
//!   hardcoded process.
//! - [`TsgoLspConnectionSource`] — how that connection is OBTAINED. The ONLY
//!   implementation here is [`SpawnOwnTsgoLsp`] (Verter spawns its own
//!   `tsgo --lsp`); structuring it as a seam lets a later phase add a second
//!   source (e.g. an editor's already-running connection) additively.
//!
//! ## The attach flow (proven live against the shipped binary)
//!
//! 1. Obtain a `tsgo --lsp` connection (the source).
//! 2. LSP `initialize` + `initialized` over that connection's stdio.
//! 3. Send `custom/initializeAPISession` (params `{}`) over the SAME connection →
//!    the server mints a pipe (`\\.\pipe\tsgo-api-<hex>-<hex>` on Windows, a UDS
//!    path on Unix) sharing the `--lsp` server's `project.Session`, returning
//!    `{ sessionId, pipe }`.
//! 4. Connect that pipe and drive the `--api` checker over it ([`ApiAttachClient`]).
//!
//! Carriers are off-disk overlays injected via `--lsp` `textDocument/didOpen` on
//! the connection from step 1; because the `--api` session shares the `--lsp`
//! server's `project.Session`, those overlays are visible to the attached checker.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::api_attach::ApiAttachClient;
use crate::error::{TsgoApiError, TsgoApiResult};
use crate::jsonrpc::JsonRpcConnection;
use crate::transport::pipe_attach::connect_attach_pipe;
use crate::transport::spawn::discover_tsgo;

/// The LSP `custom/` method that asks a `tsgo --lsp` server to mint an `--api`
/// session sharing its `project.Session`. Verified against the shipped rc
/// `typescript` engine binary (the JS package carries only the doc comment; the
/// method + result shape live in the Go server).
pub const INITIALIZE_API_SESSION_METHOD: &str = "custom/initializeAPISession";

/// A live `tsgo --lsp` connection: the JSON-RPC connection over its stdio plus the
/// child process handle (held so the process outlives the connection). This is the
/// input the attach primitive operates on.
pub struct TsgoLspConnection {
    /// The JSON-RPC connection over the child's stdio (the `--lsp` feature wire
    /// AND the channel `custom/initializeAPISession` is sent on).
    conn: JsonRpcConnection,
    /// The spawned child, kept alive for the life of the connection. `None` for a
    /// connection whose lifetime the source manages elsewhere (no S3 impl does).
    child: Option<tokio::process::Child>,
}

impl TsgoLspConnection {
    /// Build a connection from an already-established JSON-RPC connection + an
    /// optional child handle.
    #[must_use]
    pub fn new(conn: JsonRpcConnection, child: Option<tokio::process::Child>) -> Self {
        Self { conn, child }
    }

    /// The underlying `--lsp` JSON-RPC connection.
    #[must_use]
    pub fn connection(&self) -> &JsonRpcConnection {
        &self.conn
    }
}

impl std::fmt::Debug for TsgoLspConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsgoLspConnection")
            .field("has_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

/// How a `tsgo --lsp` connection is obtained. The attach primitive is generic over
/// this so the connection SOURCE is a seam: OWNED spawns Verter's own engine; a
/// future phase can add another source without touching the attach/provider code.
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

/// The OWNED connection source: spawn Verter's own `tsgo --lsp --stdio`. This is
/// the ONLY source implementation in S3 (OWNED mode).
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

    /// Build the source by discovering the engine under `workspace_root` (the
    /// production discovery: `VERTER_TSGO_BIN` > workspace `node_modules` > PATH >
    /// npm/npx cache), using `workspace_root` as the cwd.
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
    Ok(TsgoLspConnection::new(conn, Some(child)))
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
pub struct TsgoAttach {
    lsp: TsgoLspConnection,
    api: ApiAttachClient,
    session: ApiSessionHandle,
}

impl std::fmt::Debug for TsgoAttach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsgoAttach")
            .field("session_id", &self.session.session_id)
            .field("pipe", &self.session.pipe)
            .finish_non_exhaustive()
    }
}

impl TsgoAttach {
    /// Establish the full attach from a connection source: obtain the `tsgo --lsp`
    /// connection, run the LSP handshake, attach the `--api` checker over the
    /// minted pipe, and initialize the checker.
    ///
    /// `root_uri` is the workspace folder URI sent in LSP `initialize`.
    pub async fn establish(
        source: &dyn TsgoLspConnectionSource,
        root_uri: &str,
    ) -> TsgoApiResult<Self> {
        let lsp = source.establish().await?;
        Self::attach_over(lsp, root_uri).await
    }

    /// The connection-source-agnostic half: given an ALREADY-established
    /// `tsgo --lsp` connection, run the LSP handshake, send
    /// `custom/initializeAPISession`, connect the minted pipe, and initialize the
    /// `--api` checker. A reusable primitive — it does not care how `lsp` was
    /// obtained.
    pub async fn attach_over(lsp: TsgoLspConnection, root_uri: &str) -> TsgoApiResult<Self> {
        // 1. LSP initialize + initialized over the --lsp connection's stdio.
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {},
            "workspaceFolders": [{ "uri": root_uri, "name": "verter" }],
        });
        lsp.conn.request("initialize", init_params).await?;
        lsp.conn
            .notify("initialized", serde_json::json!({}))
            .await?;

        // 2. Attach the --api session: mint the pipe over the SAME connection.
        let session = Self::initialize_api_session(&lsp.conn).await?;

        // 3. Connect the minted pipe and initialize the --api checker.
        let (read, write) = connect_attach_pipe(&session.pipe).await?;
        let api = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
        api.initialize().await?;

        Ok(Self { lsp, api, session })
    }

    /// Send `custom/initializeAPISession` over a `tsgo --lsp` connection and parse
    /// the `{ sessionId, pipe }` result. The reusable attach handshake — usable for
    /// ANY `tsgo --lsp` connection.
    pub async fn initialize_api_session(
        conn: &JsonRpcConnection,
    ) -> TsgoApiResult<ApiSessionHandle> {
        let value = conn
            .request(INITIALIZE_API_SESSION_METHOD, serde_json::json!({}))
            .await?;
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

    /// Inject an off-disk carrier as an LSP `textDocument/didOpen` overlay on the
    /// `--lsp` connection. Because the `--api` session shares the server's
    /// `project.Session`, the attached checker sees this overlay.
    pub async fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> TsgoApiResult<()> {
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": version,
                "text": text,
            }
        });
        self.lsp.conn.notify("textDocument/didOpen", params).await
    }

    /// Update an open carrier overlay via `textDocument/didChange` (full content).
    pub async fn did_change(&self, uri: &str, version: i64, text: &str) -> TsgoApiResult<()> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": text }],
        });
        self.lsp.conn.notify("textDocument/didChange", params).await
    }

    /// Barrier: force the `--lsp` server to drain pending document notifications
    /// (`didOpen` / `didChange`) BEFORE an `--api` `updateSnapshot` enumerates
    /// roots on the shared `project.Session`.
    ///
    /// The two surfaces ride DIFFERENT transports (the `--lsp` stdio and the
    /// `--api` pipe), so a fire-and-forget `didOpen` notification can otherwise
    /// race behind an `updateSnapshot` on the pipe and the just-opened overlay
    /// would not yet be a Program member. LSP processes messages in order ON ONE
    /// connection, so awaiting a `--lsp` REQUEST for `uri` after the `didOpen`
    /// guarantees the overlay is registered by the time it returns. The pull
    /// `textDocument/diagnostic` request serves as that barrier (its result is
    /// discarded — the OWNED diagnostics authority is the `--api` checker).
    pub async fn sync_overlay(&self, uri: &str) -> TsgoApiResult<()> {
        let params = serde_json::json!({ "textDocument": { "uri": uri } });
        // The result is intentionally discarded; we only need the round-trip's
        // ordering guarantee. A server that does not implement pull diagnostics
        // still processes the queued didOpen before answering (or erroring) here.
        let _ = self
            .lsp
            .conn
            .request("textDocument/diagnostic", params)
            .await;
        Ok(())
    }

    /// Open an off-disk carrier overlay and synchronize it (the common path):
    /// [`Self::did_open`] followed by [`Self::sync_overlay`], so a subsequent
    /// `--api` `updateSnapshot` sees the carrier as a Program member.
    pub async fn did_open_synced(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> TsgoApiResult<()> {
        self.did_open(uri, language_id, version, text).await?;
        self.sync_overlay(uri).await
    }

    /// The attached `--api` checker client (diagnostics + checker reflection).
    #[must_use]
    pub fn api(&self) -> &ApiAttachClient {
        &self.api
    }

    /// The `--lsp` feature connection (hover/definition/references/… requests).
    #[must_use]
    pub fn lsp(&self) -> &JsonRpcConnection {
        &self.lsp.conn
    }

    /// The minted `--api` session handle.
    #[must_use]
    pub fn session(&self) -> &ApiSessionHandle {
        &self.session
    }

    /// Shut down both surfaces and the child process.
    pub async fn shutdown(mut self) -> TsgoApiResult<()> {
        let _ = self.api.close().await;
        let _ = self.lsp.conn.notify("exit", serde_json::Value::Null).await;
        let _ = self.lsp.conn.close().await;
        if let Some(mut child) = self.lsp.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        Ok(())
    }
}

/// Discover the engine + cwd and build the OWNED spawn source in one step.
pub fn owned_source_for(workspace_root: impl AsRef<Path>) -> TsgoApiResult<SpawnOwnTsgoLsp> {
    SpawnOwnTsgoLsp::discover(workspace_root)
}

#[cfg(test)]
#[path = "attach_tests.rs"]
mod tests;
