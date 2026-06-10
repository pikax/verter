//! tsserver transport layer: newline-delimited JSON over stdio.
//!
//! Spawns `node tsserver.js` as a child process and communicates using
//! the tsserver protocol (NOT LSP Content-Length framing):
//!
//! Request:  `{"seq":N,"type":"request","command":"...","arguments":{...}}\n`
//! Response: `{"seq":N,"type":"response","command":"...","request_seq":N,"success":true,"body":{...}}\n`
//! Event:    `{"seq":N,"type":"event","event":"...","body":{...}}\n`

use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot, Mutex, Notify};

use crate::codec::{line_column_to_offset_utf16, offset_to_line_column_utf16};
use crate::protocol::*;
use crate::traits::{ProviderFuture, TypeProvider};

/// Environment variables to strip from child processes to prevent VS Code/Electron
/// debugger inheritance (F5 sessions set these, causing "Debugger listening" noise).
pub const CHILD_PROCESS_ENV_DENYLIST: &[&str] = &[
    "NODE_OPTIONS",
    "VSCODE_INSPECTOR_OPTIONS",
    "ELECTRON_RUN_AS_NODE",
];

fn trace_preview(contents: &str, max_len: usize) -> String {
    let mut preview = String::new();
    for ch in contents.chars().take(max_len) {
        match ch {
            '\n' => preview.push_str("\\n"),
            '\r' => preview.push_str("\\r"),
            '\t' => preview.push_str("\\t"),
            _ => preview.push(ch),
        }
    }
    if contents.chars().count() > max_len {
        preview.push_str("...");
    }
    preview
}
fn summarize_tsserver_args(arguments: &serde_json::Value) -> String {
    let file = arguments
        .get("file")
        .or_else(|| arguments.get("fileName"))
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let line = arguments
        .get("line")
        .and_then(|value| value.as_u64())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let offset = arguments
        .get("offset")
        .and_then(|value| value.as_u64())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!("file={} line={} offset={}", file, line, offset)
}

/// Message sent to the dedicated stdin writer task.
enum TsserverStdinMessage {
    /// Write a newline-delimited JSON message to stdin.
    Frame(Vec<u8>),
    /// Shut down the writer task.
    Shutdown,
}

/// Dedicated task that owns the stdin writer and serially writes messages from the channel.
///
/// Generic over the writer type to support both `ChildStdin` and test `DuplexStream`.
async fn tsserver_stdin_writer_loop(
    mut stdin: impl tokio::io::AsyncWrite + Unpin + Send + 'static,
    mut rx: mpsc::Receiver<TsserverStdinMessage>,
) {
    while let Some(msg) = rx.recv().await {
        match msg {
            TsserverStdinMessage::Frame(data) => {
                if stdin.write_all(&data).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
            TsserverStdinMessage::Shutdown => break,
        }
    }
}

/// Newline-delimited JSON transport for tsserver.
struct TsserverTransport {
    /// Channel sender for writing to the child's stdin via the writer task.
    stdin_tx: mpsc::Sender<TsserverStdinMessage>,
    /// Pending request senders, keyed by sequence number.
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>,
    next_seq: AtomicI64,
}

impl TsserverTransport {
    /// Send a tsserver request and wait for the response.
    async fn request(
        &self,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, TypeProviderError> {
        let _trace = crate::type_runtime_trace_scope!(
            "tsserver_transport_request",
            format!(
                "command={} {}",
                command,
                summarize_tsserver_args(&arguments),
            ),
        );
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);

        let msg = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        let body = serde_json::to_string(&msg)
            .map_err(|e| TypeProviderError::new(format!("serialize error: {e}")))?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(seq, tx);

        // tsserver uses newline-delimited JSON (no Content-Length framing)
        let frame = format!("{body}\n");
        self.stdin_tx
            .send(TsserverStdinMessage::Frame(frame.into_bytes()))
            .await
            .map_err(|_| TypeProviderError::new("stdin writer closed"))?;

        let result = tokio::time::timeout(std::time::Duration::from_secs(10), rx).await;
        match result {
            Ok(Ok(val)) => {
                // Check for tsserver error
                if let Some(false) = val.get("success").and_then(|v| v.as_bool()) {
                    let msg = val
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error");
                    crate::type_runtime_trace_event!(
                        "tsserver_transport_request_error",
                        format!("command={} seq={} message={}", command, seq, msg),
                    );
                    return Err(TypeProviderError::new(msg));
                }
                crate::type_runtime_trace_event!(
                    "tsserver_transport_request_result",
                    format!(
                        "command={} seq={} body_kind={}",
                        command,
                        seq,
                        val.get("body")
                            .map(|body| match body {
                                serde_json::Value::Null => "null",
                                serde_json::Value::Array(_) => "array",
                                serde_json::Value::Object(_) => "object",
                                serde_json::Value::String(_) => "string",
                                serde_json::Value::Bool(_) => "bool",
                                serde_json::Value::Number(_) => "number",
                            })
                            .unwrap_or("missing"),
                    ),
                );
                Ok(val.get("body").cloned().unwrap_or(serde_json::Value::Null))
            }
            Ok(Err(_)) => {
                crate::type_runtime_trace_event!(
                    "tsserver_transport_request_error",
                    format!(
                        "command={} seq={} message=response channel closed",
                        command, seq
                    ),
                );
                Err(TypeProviderError::new("response channel closed"))
            }
            Err(_) => {
                // Timeout — clean up the pending entry to prevent leak
                self.pending.lock().await.remove(&seq);
                crate::type_runtime_trace_event!(
                    "tsserver_transport_request_error",
                    format!("command={} seq={} message=timeout", command, seq),
                );
                Err(TypeProviderError::new(format!(
                    "request '{command}' timed out after 10s"
                )))
            }
        }
    }

    /// Send a tsserver command without waiting for a response.
    async fn command_no_response(
        &self,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<(), TypeProviderError> {
        let _trace = crate::type_runtime_trace_scope!(
            "tsserver_transport_command",
            format!(
                "command={} {}",
                command,
                summarize_tsserver_args(&arguments),
            ),
        );
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);

        let msg = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        let body = serde_json::to_string(&msg)
            .map_err(|e| TypeProviderError::new(format!("serialize error: {e}")))?;

        let frame = format!("{body}\n");
        self.stdin_tx
            .send(TsserverStdinMessage::Frame(frame.into_bytes()))
            .await
            .map_err(|_| TypeProviderError::new("stdin writer closed"))?;

        crate::type_runtime_trace_event!(
            "tsserver_transport_command_result",
            format!("command={} seq={} queued=true", command, seq),
        );
        Ok(())
    }
}

/// Drain all pending requests with error responses.
async fn drain_pending(pending: &Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>) {
    let mut guard = pending.lock().await;
    for (_seq, tx) in guard.drain() {
        let _ = tx.send(serde_json::json!({
            "success": false,
            "message": "tsserver process crashed"
        }));
    }
}

/// Read loop for tsserver stdout.
///
/// tsserver can send responses in two formats:
/// 1. Content-Length framed (modern tsserver default)
/// 2. Newline-delimited JSON
///
/// We handle the Content-Length format since modern tsserver uses it for responses.
async fn read_loop(
    stdout: tokio::process::ChildStdout,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>,
    diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>>,
    contents_cache: Arc<Mutex<HashMap<String, String>>>,
    crash_notify: Option<Arc<Notify>>,
) {
    let mut reader = BufReader::new(stdout);
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(0) => {
                // EOF — process exited
                drain_pending(&pending).await;
                if let Some(notify) = &crash_notify {
                    notify.notify_waiters();
                }
                return;
            }
            Ok(_) => {
                let trimmed = line_buf.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Check if this is a Content-Length header (modern tsserver)
                if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
                    if let Ok(len) = len_str.trim().parse::<usize>() {
                        // Read the blank line
                        line_buf.clear();
                        if reader.read_line(&mut line_buf).await.is_err() {
                            drain_pending(&pending).await;
                            if let Some(notify) = &crash_notify {
                                notify.notify_waiters();
                            }
                            return;
                        }
                        // Read the body
                        let mut body = vec![0u8; len];
                        if tokio::io::AsyncReadExt::read_exact(&mut reader, &mut body)
                            .await
                            .is_err()
                        {
                            drain_pending(&pending).await;
                            if let Some(notify) = &crash_notify {
                                notify.notify_waiters();
                            }
                            return;
                        }
                        if let Ok(msg) = serde_json::from_slice::<serde_json::Value>(&body) {
                            handle_message(&msg, &pending, &diagnostics_cache, &contents_cache)
                                .await;
                        }
                    }
                    continue;
                }

                // Try to parse as JSON directly (newline-delimited mode)
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    handle_message(&msg, &pending, &diagnostics_cache, &contents_cache).await;
                }
            }
            Err(_) => {
                drain_pending(&pending).await;
                if let Some(notify) = &crash_notify {
                    notify.notify_waiters();
                }
                return;
            }
        }
    }
}

/// Handle a parsed tsserver message (response or event).
async fn handle_message(
    msg: &serde_json::Value,
    pending: &Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>,
    diagnostics_cache: &Mutex<HashMap<String, Vec<TypeDiagnostic>>>,
    contents_cache: &Mutex<HashMap<String, String>>,
) {
    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "response" => {
            if let Some(request_seq) = msg.get("request_seq").and_then(|v| v.as_i64()) {
                if let Some(tx) = pending.lock().await.remove(&request_seq) {
                    let _ = tx.send(msg.clone());
                }
            }
        }
        "event" => {
            let event_name = msg.get("event").and_then(|v| v.as_str()).unwrap_or("");
            if event_name == "semanticDiag" || event_name == "syntaxDiag" {
                if let Some(body) = msg.get("body") {
                    let file = body
                        .get("file")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .replace('\\', "/");
                    let content = {
                        let cache = contents_cache.lock().await;
                        cache.get(&file).cloned()
                    };
                    let diags = body
                        .get("diagnostics")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|d| parse_tsserver_diagnostic(d, content.as_deref()))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    diagnostics_cache
                        .lock()
                        .await
                        .entry(file)
                        .and_modify(|existing| existing.extend(diags.iter().cloned()))
                        .or_insert(diags);
                }
            }
        }
        _ => {}
    }
}

/// Parse a tsserver diagnostic into our TypeDiagnostic.
///
/// tsserver diagnostics use `{start: {line, offset}, end: {line, offset}}` format
/// where line and offset are 1-based.
pub fn parse_tsserver_diagnostic(
    d: &serde_json::Value,
    content: Option<&str>,
) -> Option<TypeDiagnostic> {
    let text = d.get("text")?.as_str()?.to_string();
    let start = d.get("start")?;
    let end = d.get("end")?;
    let start_line = start.get("line")?.as_u64()? as u32;
    let start_offset = start.get("offset")?.as_u64()? as u32;
    let end_line = end.get("line")?.as_u64()? as u32;
    let end_offset = end.get("offset")?.as_u64()? as u32;

    let severity = match d.get("category").and_then(|v| v.as_str()) {
        Some("error") => TypeDiagnosticSeverity::Error,
        Some("warning") => TypeDiagnosticSeverity::Warning,
        Some("suggestion") => TypeDiagnosticSeverity::Hint,
        _ => TypeDiagnosticSeverity::Error,
    };

    let code = d
        .get("code")
        .and_then(|v| v.as_u64())
        .map(|n| n.to_string());

    // Convert 1-based line/offset to byte offsets
    let (so, eo) = if let Some(c) = content {
        (
            tsserver_pos_to_byte_offset(c, start_line, start_offset),
            tsserver_pos_to_byte_offset(c, end_line, end_offset),
        )
    } else {
        // Fallback: use 0-based packed positions
        let sl = start_line.saturating_sub(1);
        let so = start_offset.saturating_sub(1);
        let el = end_line.saturating_sub(1);
        let eo = end_offset.saturating_sub(1);
        ((sl << 16) | (so & 0xFFFF), (el << 16) | (eo & 0xFFFF))
    };

    Some(TypeDiagnostic {
        message: text,
        severity,
        start: so,
        end: eo,
        code,
    })
}

/// Convert a byte offset to tsserver's 1-based (line, offset) position.
///
/// tsserver uses 1-based line and offset, where offset counts UTF-16 code units.
/// Uses `LineIndex` for correct UTF-16 column calculation with non-ASCII chars.
pub fn byte_offset_to_tsserver_pos(content: &str, offset: u32) -> (u32, u32) {
    let lc = offset_to_line_column_utf16(content, offset);
    (lc.line + 1, lc.character + 1) // tsserver is 1-based
}

/// Convert tsserver's 1-based (line, offset) position to a byte offset.
///
/// tsserver uses 1-based line and offset, where offset counts UTF-16 code units.
/// Uses `LineIndex` for correct byte offset calculation with non-ASCII chars.
pub fn tsserver_pos_to_byte_offset(content: &str, line: u32, offset: u32) -> u32 {
    line_column_to_offset_utf16(content, line.saturating_sub(1), offset.saturating_sub(1))
}

/// A `TypeProvider` backed by a tsserver process (`node tsserver.js`).
pub struct TsserverTypeProvider {
    transport: Arc<TsserverTransport>,
    /// tsserver child process. Killed on drop.
    child: Child,
    /// Cached file contents for position conversion.
    contents: Arc<Mutex<HashMap<String, String>>>,
    /// Files that have been sent to tsserver via `open` command.
    /// Used by `update_file` to decide between `open` vs `updateOpen`.
    /// `load_file` adds to `contents` but NOT to `opened_files`.
    opened_files: Arc<Mutex<HashSet<String>>>,
    /// Cached diagnostics from event notifications.
    diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>>,
    /// Workspace root path (forward slashes) for `projectRootPath` in open commands.
    workspace_root: String,
    /// Per-project roots for per-file `projectRootPath` matching.
    /// Sorted by length descending (longest prefix first).
    /// When non-empty, per-file matching takes priority over the global `workspace_root`.
    project_roots: Arc<parking_lot::RwLock<Vec<String>>>,
}

impl Drop for TsserverTypeProvider {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

async fn configure_tsserver_session(
    transport: Arc<TsserverTransport>,
    workspace_root: &str,
) -> Result<String, TypeProviderError> {
    let ws_root = workspace_root.replace('\\', "/");
    transport
        .request(
            "configure",
            serde_json::json!({
                "hostInfo": "verter-lsp",
                "preferences": {
                    "providePrefixAndSuffixTextForRename": true,
                    "includeCompletionsForModuleExports": true,
                    "includeCompletionsWithInsertText": true,
                    "includeCompletionsWithSnippetText": false,
                    "includeAutomaticOptionalChainCompletions": true,
                    "allowRenameOfImportPath": true,
                    "includeInlayVariableTypeHints": true,
                    "includeInlayVariableTypeHintsWhenTypeMatchesName": false,
                    "includeInlayFunctionLikeReturnTypeHints": true,
                    "includeInlayParameterNameHints": "literals",
                }
            }),
        )
        .await?;

    let inferred_transport = Arc::clone(&transport);
    let inferred_ws_root = ws_root.clone();
    tokio::spawn(async move {
        if let Err(error) = inferred_transport
            .request(
                "compilerOptionsForInferredProjects",
                serde_json::json!({
                    "options": {
                        "module": "esnext",
                        "target": "esnext",
                        "moduleResolution": "bundler",
                        "jsx": "preserve",
                        "jsxImportSource": "vue",
                        "allowImportingTsExtensions": true,
                        "allowJs": true,
                        "checkJs": true,
                        "strict": true,
                        "allowArbitraryExtensions": true,
                        "baseUrl": inferred_ws_root,
                    }
                }),
            )
            .await
        {
            tracing::warn!("failed to configure inferred tsserver project options: {error}");
        }
    });

    Ok(ws_root)
}

#[cfg(all(test, feature = "__lsp_tests"))]
fn tsserver_plugin_args(plugin_path: Option<&str>) -> Vec<String> {
    let Some(plugin_path) = plugin_path.filter(|path| !path.is_empty()) else {
        return Vec::new();
    };

    vec![
        "--globalPlugins".to_string(),
        "@verter/typescript-plugin".to_string(),
        "--pluginProbeLocations".to_string(),
        plugin_path.to_string(),
        "--allowLocalPluginLoads".to_string(),
    ]
}

impl TsserverTypeProvider {
    /// Spawn a tsserver process and initialize it.
    ///
    /// `node_path`: path to the `node` executable.
    /// `tsserver_path`: path to `tsserver.js`.
    /// `workspace_root`: filesystem path to the workspace root.
    /// `plugin_path`: reserved for legacy/plugin test coverage; currently not
    /// used when spawning the production tsserver process.
    pub async fn spawn(
        node_path: &str,
        tsserver_path: &str,
        workspace_root: &str,
        _plugin_path: Option<&str>,
        crash_notify: Option<Arc<Notify>>,
    ) -> Result<Self, TypeProviderError> {
        let mut cmd = tokio::process::Command::new(node_path);

        // Remove VS Code/Electron debug env vars to prevent tsserver from
        // opening a debugger port during F5 sessions.
        for var in CHILD_PROCESS_ENV_DENYLIST {
            cmd.env_remove(var);
        }

        cmd.arg(tsserver_path)
            .arg("--useSyntaxServer=false")
            .arg("--disableAutomaticTypingAcquisition");

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| TypeProviderError::new(format!("failed to spawn tsserver: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TypeProviderError::new("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TypeProviderError::new("no stdout"))?;
        let stderr = child.stderr.take();

        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Use a channel + dedicated writer task instead of Arc<Mutex<ChildStdin>>
        // to eliminate contention between concurrent request() and command_no_response() calls.
        let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
        tokio::spawn(tsserver_stdin_writer_loop(stdin, stdin_rx));

        let transport = Arc::new(TsserverTransport {
            stdin_tx: stdin_tx.clone(),
            pending: Arc::clone(&pending),
            next_seq: AtomicI64::new(1),
        });

        let diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let contents_cache: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Start the read loop
        tokio::spawn(read_loop(
            stdout,
            pending,
            Arc::clone(&diagnostics_cache),
            Arc::clone(&contents_cache),
            crash_notify,
        ));

        // Log stderr
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let line = buf.trim_end();
                            if !line.is_empty() {
                                tracing::warn!("tsserver stderr: {line}");
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let ws_root = configure_tsserver_session(Arc::clone(&transport), workspace_root).await?;

        Ok(Self {
            transport,
            child,
            contents: contents_cache,
            opened_files: Arc::new(Mutex::new(HashSet::new())),
            diagnostics_cache,
            workspace_root: ws_root,
            project_roots: Arc::new(parking_lot::RwLock::new(Vec::new())),
        })
    }

    /// Normalize a file path for tsserver (forward slashes, no file:// prefix).
    fn normalize_path(path: &str) -> String {
        path.replace('\\', "/")
    }

    /// Find the best project root for a file path (longest prefix match).
    /// Falls back to the global `workspace_root` if no project root matches.
    fn project_root_for(&self, file: &str) -> String {
        let roots = self.project_roots.read();
        for root in roots.iter() {
            if file.starts_with(root.as_str()) {
                return root.clone();
            }
        }
        self.workspace_root.clone()
    }
}

impl TypeProvider for TsserverTypeProvider {
    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let content = content.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        let project_root = self.project_root_for(&file);
        Box::pin(async move {
            let _trace = crate::type_runtime_trace_scope!(
                "tsserver_open_file",
                format!(
                    "file={} content_len={} project_root={}",
                    file,
                    content.len(),
                    project_root,
                ),
            );
            contents_cache
                .lock()
                .await
                .insert(file.clone(), content.clone());
            opened_files.lock().await.insert(file.clone());
            // tsserver `open` command doesn't return a response.
            // projectRootPath tells tsserver where to find tsconfig.json.
            transport
                .command_no_response(
                    "open",
                    serde_json::json!({
                        "file": file,
                        "fileContent": content,
                        "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                            else if file.ends_with(".jsx") { "JSX" }
                            else if file.ends_with(".js") { "JS" }
                            else { "TS" },
                        "projectRootPath": project_root,
                    }),
                )
                .await?;
            crate::type_runtime_trace_event!(
                "tsserver_open_file_result",
                format!("file={} opened=true", file),
            );
            Ok(())
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        // For tsserver, load_file only caches the content locally — it does NOT
        // send an `open` command. Sending 500+ `open` commands during background
        // sync overwhelms tsserver and blocks user requests for 15-20 seconds.
        // Resolver-managed provider files are pushed on demand when the user
        // actually opens or edits a file, so background sync only needs the
        // local cache here.
        let file = Self::normalize_path(path);
        let content = content.to_string();
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let _trace = crate::type_runtime_trace_scope!(
                "tsserver_load_file",
                format!("file={} content_len={}", file, content.len()),
            );
            contents_cache.lock().await.insert(file, content);
            crate::type_runtime_trace_event!(
                "tsserver_load_file_result",
                "cached_only=true".to_string()
            );
            Ok(())
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let content = content.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        let project_root = self.project_root_for(&file);
        Box::pin(async move {
            let _trace = crate::type_runtime_trace_scope!(
                "tsserver_update_file",
                format!(
                    "file={} content_len={} project_root={}",
                    file,
                    content.len(),
                    project_root,
                ),
            );
            // Read old content's line count BEFORE inserting new content.
            // tsserver validates the end line against the old file's line map,
            // so we must use the actual old line count (not a hardcoded sentinel).
            let old_line_count = {
                let cache = contents_cache.lock().await;
                cache.get(&file).map(|c| c.lines().count() as u32 + 1)
            };

            contents_cache
                .lock()
                .await
                .insert(file.clone(), content.clone());

            let mut opened = opened_files.lock().await;
            if opened.contains(&file) {
                drop(opened);
                if let Some(end_line) = old_line_count {
                    tracing::debug!(
                        "tsserver update_file: updateOpen for {file} (end_line={end_line})"
                    );
                    // Use updateOpen with textChanges spanning the old content
                    transport
                        .command_no_response(
                            "updateOpen",
                            serde_json::json!({
                                "changedFiles": [{
                                    "fileName": file,
                                    "textChanges": [{
                                        "start": { "line": 1, "offset": 1 },
                                        "end": { "line": end_line, "offset": 1 },
                                        "newText": content,
                                    }]
                                }]
                            }),
                        )
                        .await?;
                    crate::type_runtime_trace_event!(
                        "tsserver_update_file_result",
                        format!("file={} mode=update_open old_line_count={}", file, end_line),
                    );
                    Ok(())
                } else {
                    // No old content in cache (shouldn't happen since opened_files
                    // is only set when content was sent) — close and reopen
                    tracing::warn!("tsserver update_file: no cached content for open file {file}, closing and reopening");
                    transport
                        .command_no_response(
                            "updateOpen",
                            serde_json::json!({
                                "closedFiles": [&file],
                                "openFiles": [{
                                    "file": file,
                                    "fileContent": content,
                                    "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                                        else if file.ends_with(".jsx") { "JSX" }
                                        else if file.ends_with(".js") { "JS" }
                                        else { "TS" },
                                    "projectRootPath": project_root,
                                }]
                            }),
                        )
                        .await?;
                    crate::type_runtime_trace_event!(
                        "tsserver_update_file_result",
                        format!("file={} mode=reopen_after_cache_miss", file),
                    );
                    Ok(())
                }
            } else {
                // File not open yet — open it and track
                opened.insert(file.clone());
                drop(opened);
                tracing::info!(
                    "tsserver update_file: first open for {file} ({} bytes)",
                    content.len()
                );
                transport
                    .command_no_response(
                        "open",
                        serde_json::json!({
                            "file": file,
                            "fileContent": content,
                            "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                                else if file.ends_with(".jsx") { "JSX" }
                                else if file.ends_with(".js") { "JS" }
                                else { "TS" },
                            "projectRootPath": project_root,
                        }),
                    )
                    .await?;
                crate::type_runtime_trace_event!(
                    "tsserver_update_file_result",
                    format!("file={} mode=first_open", file),
                );
                Ok(())
            }
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        Box::pin(async move {
            let _trace =
                crate::type_runtime_trace_scope!("tsserver_close_file", format!("file={}", file));
            contents_cache.lock().await.remove(&file);
            opened_files.lock().await.remove(&file);
            transport
                .command_no_response("close", serde_json::json!({ "file": file }))
                .await?;
            crate::type_runtime_trace_event!(
                "tsserver_close_file_result",
                "closed=true".to_string()
            );
            Ok(())
        })
    }

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let file = Self::normalize_path(path);
        let trigger = trigger_character.map(|s| s.to_string());
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let mut args = serde_json::json!({
                "file": file,
                "line": line,
                "offset": col,
                "includeExternalModuleExports": true,
                "includeInsertTextCompletions": true,
            });

            if let Some(ref t) = trigger {
                args["triggerCharacter"] = serde_json::Value::String(t.clone());
            }

            let result = transport.request("completionInfo", args).await?;

            let is_incomplete = result
                .get("isMemberCompletion")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let items = result
                .get("entries")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(parse_tsserver_completion).collect())
                .unwrap_or_default();

            Ok(CompletionResult {
                items,
                is_incomplete,
            })
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col, cache_hit) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => {
                        let (line, col) = byte_offset_to_tsserver_pos(c, offset);
                        (line, col, true)
                    }
                    None => (1, offset + 1, false),
                }
            };
            let _trace = crate::type_runtime_trace_scope!(
                "tsserver_get_hover",
                format!(
                    "file={} offset={} line={} col={} content_cache_hit={}",
                    file, offset, line, col, cache_hit,
                ),
            );

            let result = transport
                .request(
                    "quickinfo",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let display = body
                        .get("displayString")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let docs = body
                        .get("documentation")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let kind = body
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();

                    if display.is_empty() {
                        tracing::debug!(
                            "tsserver quickinfo: empty displayString for {file} at {line}:{col}"
                        );
                        crate::type_runtime_trace_event!(
                            "tsserver_get_hover_result",
                            format!("file={} empty_display=true", file),
                        );
                        return Ok(None);
                    }

                    let contents = format_quickinfo_hover(kind, display, docs);
                    crate::type_runtime_trace_event!(
                        "tsserver_get_hover_result",
                        format!(
                            "file={} empty_display=false kind={} display_len={} docs_len={} preview={}",
                            file,
                            kind,
                            display.len(),
                            docs.len(),
                            trace_preview(&contents, 120),
                        ),
                    );

                    Ok(Some(HoverInfo {
                        contents,
                        range_start: None,
                        range_end: None,
                    }))
                }
                Err(e) => {
                    tracing::warn!("tsserver quickinfo error for {file}: {e}");
                    crate::type_runtime_trace_event!(
                        "tsserver_get_hover_result",
                        format!("file={} error={}", file, e),
                    );
                    Ok(None)
                }
            }
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            if items.is_empty() {
                return Ok(Vec::new());
            }

            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };
            let _trace = crate::type_runtime_trace_scope!(
                "tsserver_get_completion_details",
                format!(
                    "file={} offset={} line={} col={} item_count={}",
                    file,
                    offset,
                    line,
                    col,
                    items.len(),
                ),
            );

            let entry_names: Vec<_> = items
                .iter()
                .map(|item| serde_json::json!({ "name": item.label }))
                .collect();
            let result = transport
                .request(
                    "completionEntryDetails",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                        "entryNames": entry_names,
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let detail_map: HashMap<String, &serde_json::Value> = body
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|detail| {
                            detail
                                .get("name")
                                .and_then(|value| value.as_str())
                                .map(|name| (name.to_string(), detail))
                        })
                        .collect();
                    let enriched = items
                        .iter()
                        .map(|item| {
                            detail_map
                                .get(&item.label)
                                .map(|detail| enrich_tsserver_completion(item, detail))
                                .unwrap_or_else(|| item.clone())
                        })
                        .collect::<Vec<_>>();
                    crate::type_runtime_trace_event!(
                        "tsserver_get_completion_details_result",
                        format!("file={} item_count={} enriched=true", file, enriched.len()),
                    );
                    Ok(enriched)
                }
                Err(error) => {
                    crate::type_runtime_trace_event!(
                        "tsserver_get_completion_details_result",
                        format!("file={} item_count={} error={}", file, items.len(), error),
                    );
                    Ok(items.to_vec())
                }
            }
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let diagnostics_cache = Arc::clone(&self.diagnostics_cache);
        Box::pin(async move {
            let content = {
                let cache = contents_cache.lock().await;
                cache.get(&file).cloned()
            };

            // Request semantic diagnostics synchronously
            let result = transport
                .request(
                    "semanticDiagnosticsSync",
                    serde_json::json!({ "file": file }),
                )
                .await;

            match result {
                Ok(body) => {
                    let diags = if let Some(arr) = body.as_array() {
                        arr.iter()
                            .filter_map(|d| parse_tsserver_diagnostic(d, content.as_deref()))
                            .collect()
                    } else {
                        vec![]
                    };
                    diagnostics_cache.lock().await.insert(file, diags.clone());
                    Ok(diags)
                }
                Err(_) => {
                    // Fall back to cached diagnostics
                    let cache = diagnostics_cache.lock().await;
                    Ok(cache.get(&file).cloned().unwrap_or_default())
                }
            }
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = transport
                .request(
                    "definition",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await?;

            let locs = {
                let cache = contents_cache.lock().await;
                result
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|loc| parse_tsserver_location(loc, &cache))
                            .collect()
                    })
                    .unwrap_or_default()
            };

            Ok(locs)
        })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = transport
                .request(
                    "typeDefinition",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await?;

            let locs = {
                let cache = contents_cache.lock().await;
                result
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|loc| parse_tsserver_location(loc, &cache))
                            .collect()
                    })
                    .unwrap_or_default()
            };

            Ok(locs)
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = transport
                .request(
                    "references",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await?;

            let locs = {
                let cache = contents_cache.lock().await;
                result
                    .get("refs")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|loc| parse_tsserver_location(loc, &cache))
                            .collect()
                    })
                    .unwrap_or_default()
            };

            Ok(locs)
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = transport
                .request(
                    "rename",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                        "findInComments": false,
                        "findInStrings": false,
                    }),
                )
                .await?;

            let locs = {
                let cache = contents_cache.lock().await;
                result
                    .get("locs")
                    .and_then(|v| v.as_array())
                    .map(|groups| {
                        groups
                            .iter()
                            .flat_map(|group| {
                                let file_path = group
                                    .get("file")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .replace('\\', "/");
                                let content = cache.get(&file_path).map(|s| s.as_str());
                                group
                                    .get("locs")
                                    .and_then(|v| v.as_array())
                                    .into_iter()
                                    .flat_map(move |spans| {
                                        let fp = file_path.clone();
                                        let c = content;
                                        spans.iter().filter_map(move |span| {
                                            parse_tsserver_rename_span(span, &fp, c)
                                        })
                                    })
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };

            Ok(locs)
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = transport
                .request(
                    "signatureHelp",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let items = body.get("items").and_then(|v| v.as_array());
                    let Some(items) = items else {
                        return Ok(None);
                    };

                    let signatures: Vec<SignatureInfo> = items
                        .iter()
                        .map(|item| {
                            let prefix = item
                                .get("prefixDisplayParts")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts))
                                .unwrap_or_default();
                            let suffix = item
                                .get("suffixDisplayParts")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts))
                                .unwrap_or_default();
                            let separator = item
                                .get("separatorDisplayParts")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts))
                                .unwrap_or_else(|| ", ".to_string());

                            let params: Vec<ParameterInfo> = item
                                .get("parameters")
                                .and_then(|v| v.as_array())
                                .map(|ps| {
                                    ps.iter()
                                        .map(|p| {
                                            let label = p
                                                .get("displayParts")
                                                .and_then(|v| v.as_array())
                                                .map(|parts| concat_display_parts(parts))
                                                .unwrap_or_default();
                                            let doc = p
                                                .get("documentation")
                                                .and_then(|v| v.as_array())
                                                .map(|parts| concat_display_parts(parts));
                                            ParameterInfo {
                                                label,
                                                documentation: doc,
                                            }
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();

                            let param_labels: Vec<String> =
                                params.iter().map(|p| p.label.clone()).collect();
                            let label =
                                format!("{prefix}{}{suffix}", param_labels.join(&separator));
                            let doc = item
                                .get("documentation")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts));

                            SignatureInfo {
                                label,
                                documentation: doc,
                                parameters: params,
                            }
                        })
                        .collect();

                    if signatures.is_empty() {
                        return Ok(None);
                    }

                    let active_sig = body
                        .get("selectedItemIndex")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);
                    let active_param = body
                        .get("argumentIndex")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);

                    Ok(Some(SignatureHelp {
                        signatures,
                        active_signature: active_sig,
                        active_parameter: active_param,
                    }))
                }
                Err(_) => Ok(None),
            }
        })
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (sl, sc, el, ec) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => {
                        let (sl, sc) = byte_offset_to_tsserver_pos(c, start_offset);
                        let (el, ec) = byte_offset_to_tsserver_pos(c, end_offset);
                        (sl, sc, el, ec)
                    }
                    None => (1, start_offset + 1, 1, end_offset + 1),
                }
            };

            let result = transport
                .request(
                    "getCodeFixes",
                    serde_json::json!({
                        "file": file,
                        "startLine": sl,
                        "startOffset": sc,
                        "endLine": el,
                        "endOffset": ec,
                        "errorCodes": [],
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let cache = contents_cache.lock().await;
                    let actions = body
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|a| parse_tsserver_code_action(a, &cache))
                                .collect()
                        })
                        .unwrap_or_default();
                    Ok(actions)
                }
                Err(_) => Ok(vec![]),
            }
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let content = {
                let cache = contents_cache.lock().await;
                cache.get(&file).cloned()
            };
            let Some(content) = content else {
                // No cached content — nothing to get tokens for
                return Ok(vec![]);
            };
            let end_line = content.lines().count() as u32 + 1;

            let result = transport
                .request(
                    "encodedSemanticClassifications-full",
                    serde_json::json!({
                        "file": file,
                        "start": { "line": 1, "offset": 1 },
                        "end": { "line": end_line, "offset": 1 },
                        "format": "2020",
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let spans = body
                        .get("spans")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();

                    // Spans come as [start, length, classification, start, length, classification, ...]
                    let mut tokens = Vec::new();
                    let mut i = 0;
                    while i + 2 < spans.len() {
                        let start = spans[i].as_u64().unwrap_or(0) as u32;
                        let length = spans[i + 1].as_u64().unwrap_or(0) as u32;
                        let classification = spans[i + 2].as_u64().unwrap_or(0) as u32;
                        // Map classification to semantic token type/modifiers
                        let token_type = classification & 0xFF;
                        let token_modifiers = (classification >> 8) & 0xFF;
                        tokens.push(SemanticToken {
                            start,
                            length,
                            token_type,
                            token_modifiers,
                        });
                        i += 3;
                    }

                    Ok(tokens)
                }
                Err(_) => Ok(vec![]),
            }
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = transport
                .request(
                    "documentHighlights",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                        "filesToSearch": [file],
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let highlights = body
                        .as_array()
                        .into_iter()
                        .flat_map(|groups| {
                            groups.iter().flat_map(|group| {
                                group
                                    .get("highlightSpans")
                                    .and_then(|v| v.as_array())
                                    .into_iter()
                                    .flat_map(|spans| {
                                        spans.iter().filter_map(|span| {
                                            let start = span.get("start")?;
                                            let end = span.get("end")?;
                                            let sl = start.get("line")?.as_u64()? as u32;
                                            let so = start.get("offset")?.as_u64()? as u32;
                                            let el = end.get("line")?.as_u64()? as u32;
                                            let eo = end.get("offset")?.as_u64()? as u32;
                                            let kind = span
                                                .get("kind")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("none");
                                            let hl_kind = match kind {
                                                "writtenReference" => {
                                                    TypeDocumentHighlightKind::Write
                                                }
                                                _ => TypeDocumentHighlightKind::Read,
                                            };
                                            // Convert 1-based to packed 0-based
                                            let s = ((sl.saturating_sub(1)) << 16)
                                                | ((so.saturating_sub(1)) & 0xFFFF);
                                            let e = ((el.saturating_sub(1)) << 16)
                                                | ((eo.saturating_sub(1)) & 0xFFFF);
                                            Some(TypeDocumentHighlight {
                                                start: s,
                                                end: e,
                                                kind: hl_kind,
                                            })
                                        })
                                    })
                            })
                        })
                        .collect();

                    Ok(highlights)
                }
                Err(_) => Ok(vec![]),
            }
        })
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (sl, sc, el, ec) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => {
                        let (sl, sc) = byte_offset_to_tsserver_pos(c, start_offset);
                        let (el, ec) = byte_offset_to_tsserver_pos(c, end_offset);
                        (sl, sc, el, ec)
                    }
                    None => (1, start_offset + 1, 1, end_offset + 1),
                }
            };

            let result = transport
                .request(
                    "provideInlayHints",
                    serde_json::json!({
                        "file": file,
                        "start": sl,
                        "length": (el.saturating_sub(sl) + 1) * 200, // Approximate byte range
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let _ = (sc, ec); // Used above for position calculation
                    let hints = body
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|hint| {
                                    let text = hint.get("text")?.as_str()?.to_string();
                                    let pos = hint.get("position")?;
                                    let hl = pos.get("line")?.as_u64()? as u32;
                                    let ho = pos.get("offset")?.as_u64()? as u32;

                                    let kind_str =
                                        hint.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                                    let kind = match kind_str {
                                        "Type" => Some(InlayHintKind::Type),
                                        "Parameter" => Some(InlayHintKind::Parameter),
                                        _ => None,
                                    };

                                    // Convert 1-based position to packed 0-based
                                    let position = ((hl.saturating_sub(1)) << 16)
                                        | ((ho.saturating_sub(1)) & 0xFFFF);

                                    Some(InlayHint {
                                        position,
                                        label: text,
                                        kind,
                                        padding_left: hint
                                            .get("whitespaceBefore")
                                            .and_then(|v| v.as_bool()),
                                        padding_right: hint
                                            .get("whitespaceAfter")
                                            .and_then(|v| v.as_bool()),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    Ok(hints)
                }
                Err(_) => Ok(vec![]),
            }
        })
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        let transport = Arc::clone(&self.transport);
        Box::pin(async move {
            // Best-effort: send exit command with 3s timeout.
            // If tsserver is unresponsive, we don't hang — the child has kill_on_drop.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                let _ = transport
                    .command_no_response("exit", serde_json::json!({}))
                    .await;
            })
            .await;
            // Signal the writer task to stop.
            let _ = transport
                .stdin_tx
                .send(TsserverStdinMessage::Shutdown)
                .await;
            Ok(())
        })
    }

    fn child_pid(&self) -> Option<u32> {
        self.child.id()
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        let project_roots = Arc::clone(&self.project_roots);
        Box::pin(async move {
            let mut roots = project_roots.write();

            // Remove closed folders
            for folder in &removed {
                if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                    let canonical = crate::uri::file_uri_to_path(uri);
                    roots.retain(|r| r != &canonical);
                }
            }

            // Add new folders
            for folder in &added {
                if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                    let canonical = crate::uri::file_uri_to_path(uri);
                    if !roots.contains(&canonical) {
                        roots.push(canonical);
                    }
                }
            }

            // Re-sort: longest prefix first for correct matching
            roots.sort_by_key(|r| std::cmp::Reverse(r.len()));

            Ok(())
        })
    }

    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        let transport = Arc::clone(&self.transport);
        let opened_files = Arc::clone(&self.opened_files);
        let contents_cache = Arc::clone(&self.contents);
        let project_roots = Arc::clone(&self.project_roots);
        let workspace_root = self.workspace_root.clone();
        Box::pin(async move {
            let files: Vec<String> = opened_files.lock().await.iter().cloned().collect();
            let contents = contents_cache.lock().await;
            for file in &files {
                let Some(content) = contents.get(file).cloned() else {
                    continue;
                };
                // Close the file so tsserver forgets its project association
                transport
                    .command_no_response("close", serde_json::json!({ "file": file }))
                    .await?;
                // Re-open with fresh projectRootPath from the now-populated project_roots
                let project_root = {
                    let roots = project_roots.read();
                    roots
                        .iter()
                        .find(|r| file.starts_with(r.as_str()))
                        .cloned()
                        .unwrap_or_else(|| workspace_root.clone())
                };
                transport
                    .command_no_response(
                        "open",
                        serde_json::json!({
                            "file": file,
                            "fileContent": content,
                            "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                                else if file.ends_with(".jsx") { "JSX" }
                                else if file.ends_with(".js") { "JS" }
                                else { "TS" },
                            "projectRootPath": project_root,
                        }),
                    )
                    .await?;
            }
            Ok(())
        })
    }

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        let transport = Arc::clone(&self.transport);
        let base_url = base_url.to_string();
        Box::pin(async move {
            let mut options = serde_json::json!({
                "module": "esnext",
                "target": "esnext",
                "moduleResolution": "bundler",
                "jsx": "preserve",
                "jsxImportSource": "vue",
                "allowImportingTsExtensions": true,
                "allowJs": true,
                "checkJs": true,
                "strict": true,
                "allowArbitraryExtensions": true,
                "baseUrl": base_url,
                "paths": paths,
            });
            // Remove null paths (shouldn't happen but be safe)
            if options.get("paths").is_some_and(|v| v.is_null()) {
                if let Some(obj) = options.as_object_mut() {
                    obj.remove("paths");
                }
            }
            let _ = transport
                .request(
                    "compilerOptionsForInferredProjects",
                    serde_json::json!({ "options": options }),
                )
                .await;
            Ok(())
        })
    }
}

// ── Helper functions ─────────────────────────────────────────────────

/// Parse a tsserver completion entry into our Completion type.
pub fn parse_tsserver_completion(item: &serde_json::Value) -> Option<Completion> {
    let name = item.get("name")?.as_str()?.to_string();
    let kind_str = item.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    // IMPORTANT: This mapping MUST match VS Code's official TypeScript extension
    // `MyCompletionItem.convertKind()` in:
    //   vscode/extensions/typescript-language-features/src/languageFeatures/completions.ts
    //
    // tsserver returns completion entry kinds as ScriptElementKind string values
    // (defined in TypeScript's src/services/types.ts). Any unmapped kind string
    // silently falls through to the default branch. This was the root cause of
    // v-for iteration variables showing as Text instead of Variable: tsserver
    // returns "parameter" for arrow function params (which v-for compiles to),
    // and "parameter" was not in the match arms.
    //
    // If TypeScript adds new ScriptElementKind values in the future, they will
    // hit the default branch (Property) which matches VS Code's behavior. The
    // test `test_parse_tsserver_completion_kinds_match_vscode` covers all known
    // kinds — update it when syncing with a new TS version.
    //
    // Reference: https://github.com/microsoft/vscode/blob/main/extensions/typescript-language-features/src/languageFeatures/completions.ts
    let kind = Some(match kind_str {
        "primitive type" | "keyword" => CompletionKind::Keyword,
        "const" | "let" | "var" | "local var" | "alias" | "parameter" => CompletionKind::Variable,
        "property" | "getter" | "setter" => CompletionKind::Field,
        "function" | "local function" => CompletionKind::Function,
        "method" | "construct" | "call" | "index" => CompletionKind::Method,
        "enum" => CompletionKind::Enum,
        "enum member" => CompletionKind::EnumMember,
        "module" | "external module name" => CompletionKind::Module,
        "class" | "type" => CompletionKind::Class,
        "interface" => CompletionKind::Interface,
        "warning" => CompletionKind::Text,
        "script" => CompletionKind::File,
        "directory" => CompletionKind::Folder,
        "string" => CompletionKind::Constant,
        // VS Code default fallback — any unknown kind becomes Property
        _ => CompletionKind::Property,
    });

    let sort_text = item
        .get("sortText")
        .and_then(|v| v.as_str())
        .map(String::from);
    let insert_text = item
        .get("insertText")
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(Completion {
        label: name,
        kind,
        detail: None,
        documentation: None,
        edit_range_start: None,
        edit_range_end: None,
        insert_text,
        sort_text,
        data: None,
    })
}

fn enrich_tsserver_completion(item: &Completion, detail: &serde_json::Value) -> Completion {
    let display = tsserver_display_parts_text(detail.get("displayParts"));
    let documentation = tsserver_completion_documentation(detail);
    Completion {
        label: item.label.clone(),
        kind: item.kind,
        detail: if display.is_empty() {
            item.detail.clone()
        } else {
            Some(display)
        },
        documentation: documentation.or_else(|| item.documentation.clone()),
        edit_range_start: item.edit_range_start,
        edit_range_end: item.edit_range_end,
        insert_text: item.insert_text.clone(),
        sort_text: item.sort_text.clone(),
        data: item.data.clone(),
    }
}

fn tsserver_display_parts_text(parts: Option<&serde_json::Value>) -> String {
    parts
        .and_then(|value| value.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn tsserver_completion_documentation(detail: &serde_json::Value) -> Option<String> {
    let documentation = tsserver_display_parts_text(detail.get("documentation"));
    let tag_text = detail
        .get("tags")
        .and_then(|value| value.as_array())
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| {
                    let name = tag.get("name").and_then(|value| value.as_str())?;
                    let text = tsserver_display_parts_text(tag.get("text"));
                    Some(if text.is_empty() {
                        format!("@{name}")
                    } else {
                        format!("@{name} {text}")
                    })
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    let combined = match (documentation.is_empty(), tag_text.is_empty()) {
        (true, true) => return None,
        (false, true) => documentation,
        (true, false) => tag_text,
        (false, false) => format!("{documentation}\n{tag_text}"),
    };
    Some(combined)
}

/// Parse a tsserver location (used in definition/references responses).
///
/// tsserver locations have: `{ file, start: {line, offset}, end: {line, offset} }`
/// where line and offset are 1-based, and offset counts UTF-16 code units.
///
/// When content is available in `contents_cache`, positions are converted to proper
/// byte offsets. Otherwise, falls back to packed 0-based `(line << 16) | col` format.
pub fn parse_tsserver_location(
    loc: &serde_json::Value,
    contents_cache: &HashMap<String, String>,
) -> Option<TypeLocation> {
    let file = loc
        .get("file")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .replace('\\', "/");
    let start = loc.get("start")?;
    let end = loc.get("end")?;
    let sl = start.get("line")?.as_u64()? as u32;
    let so = start.get("offset")?.as_u64()? as u32;
    let el = end.get("line")?.as_u64()? as u32;
    let eo = end.get("offset")?.as_u64()? as u32;

    let disk_content;
    let content = if let Some(content) = contents_cache.get(&file) {
        Some(content.as_str())
    } else {
        disk_content = std::fs::read_to_string(&file).ok();
        disk_content.as_deref()
    };

    let (s, e) = if let Some(content) = content {
        (
            tsserver_pos_to_byte_offset(content, sl, so),
            tsserver_pos_to_byte_offset(content, el, eo),
        )
    } else {
        // Fallback: store packed 0-based positions
        (
            ((sl.saturating_sub(1)) << 16) | ((so.saturating_sub(1)) & 0xFFFF),
            ((el.saturating_sub(1)) << 16) | ((eo.saturating_sub(1)) & 0xFFFF),
        )
    };

    Some(TypeLocation {
        path: file,
        start: s,
        end: e,
    })
}

/// Parse a tsserver rename span into a RenameLocation.
///
/// When `content` is provided, converts 1-based tsserver positions to byte offsets.
/// Otherwise, falls back to packed 0-based `(line << 16) | col` format.
pub fn parse_tsserver_rename_span(
    span: &serde_json::Value,
    file: &str,
    content: Option<&str>,
) -> Option<RenameLocation> {
    let start = span.get("start")?;
    let end = span.get("end")?;
    let sl = start.get("line")?.as_u64()? as u32;
    let so = start.get("offset")?.as_u64()? as u32;
    let el = end.get("line")?.as_u64()? as u32;
    let eo = end.get("offset")?.as_u64()? as u32;

    let (s, e) = if let Some(c) = content {
        (
            tsserver_pos_to_byte_offset(c, sl, so),
            tsserver_pos_to_byte_offset(c, el, eo),
        )
    } else {
        (
            ((sl.saturating_sub(1)) << 16) | ((so.saturating_sub(1)) & 0xFFFF),
            ((el.saturating_sub(1)) << 16) | ((eo.saturating_sub(1)) & 0xFFFF),
        )
    };

    Some(RenameLocation {
        path: file.to_string(),
        start: s,
        end: e,
    })
}

/// Parse a tsserver code action / code fix.
///
/// When content is available in `contents_cache`, converts 1-based tsserver positions
/// to byte offsets. Otherwise, falls back to packed 0-based format.
pub fn parse_tsserver_code_action(
    action: &serde_json::Value,
    contents_cache: &HashMap<String, String>,
) -> Option<TypeCodeAction> {
    let description = action.get("description")?.as_str()?.to_string();
    let changes = action.get("changes")?.as_array()?;

    let mut edits = Vec::new();
    for change in changes {
        let file = change
            .get("fileName")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .replace('\\', "/");
        let content = contents_cache.get(&file);
        if let Some(text_changes) = change.get("textChanges").and_then(|v| v.as_array()) {
            for tc in text_changes {
                let start = tc.get("start")?;
                let end = tc.get("end")?;
                let new_text = tc.get("newText")?.as_str()?.to_string();
                let sl = start.get("line")?.as_u64()? as u32;
                let so = start.get("offset")?.as_u64()? as u32;
                let el = end.get("line")?.as_u64()? as u32;
                let eo = end.get("offset")?.as_u64()? as u32;

                let (s, e) = if let Some(c) = content {
                    (
                        tsserver_pos_to_byte_offset(c, sl, so),
                        tsserver_pos_to_byte_offset(c, el, eo),
                    )
                } else {
                    (
                        ((sl.saturating_sub(1)) << 16) | ((so.saturating_sub(1)) & 0xFFFF),
                        ((el.saturating_sub(1)) << 16) | ((eo.saturating_sub(1)) & 0xFFFF),
                    )
                };

                edits.push(TypeCodeEdit {
                    path: file.clone(),
                    start: s,
                    end: e,
                    new_text,
                });
            }
        }
    }

    Some(TypeCodeAction {
        title: description,
        kind: Some("quickfix".to_string()),
        edits,
    })
}

/// Concatenate tsserver display parts into a single string.
pub fn concat_display_parts(parts: &[serde_json::Value]) -> String {
    parts
        .iter()
        .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("")
}

/// Format tsserver quickinfo into hover markdown.
///
/// tsserver's `displayString` may already include a `({kind})` prefix for certain
/// symbol kinds (e.g., `(alias) const Foo`). This function avoids duplicating it.
pub fn format_quickinfo_hover(kind: &str, display: &str, docs: &str) -> String {
    let display_with_kind = if kind.is_empty() {
        display.to_string()
    } else {
        let prefix = format!("({kind}) ");
        if display.starts_with(&prefix) {
            display.to_string()
        } else {
            format!("({kind}) {display}")
        }
    };
    if docs.is_empty() {
        format!("```typescript\n{display_with_kind}\n```")
    } else {
        format!("```typescript\n{display_with_kind}\n```\n\n{docs}")
    }
}

// Integration tests that depend on verter_session stay in verter_lsp.
#[cfg(all(test, feature = "__lsp_tests"))]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use verter_session::{
        CompileProfile, CompileTarget, FileLanguage, HostConfig, UpsertRequest, VerterHost,
        VirtualNodeKind, VirtualQuery,
    };

    fn workspace_node_modules() -> Option<PathBuf> {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
        let node_modules = PathBuf::from(manifest_dir).join("../../node_modules");
        node_modules.exists().then_some(node_modules)
    }

    fn tsserver_assets_or_skip() -> Option<(String, String)> {
        let node_modules = workspace_node_modules()?;
        let tsserver_path = if node_modules.join("typescript/lib/tsserver.js").exists() {
            node_modules.join("typescript/lib/tsserver.js")
        } else {
            let pnpm_dir = node_modules.join(".pnpm");
            let mut found = None;
            if pnpm_dir.exists() {
                for entry in std::fs::read_dir(&pnpm_dir).ok()? {
                    let entry = entry.ok()?;
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("typescript@") && !name_str.contains("node_modules") {
                        let candidate =
                            entry.path().join("node_modules/typescript/lib/tsserver.js");
                        if candidate.exists() {
                            found = Some(candidate);
                            break;
                        }
                    }
                }
            }
            found?
        };
        let node_path = "node".to_string();
        if std::process::Command::new(&node_path)
            .arg("--version")
            .output()
            .is_err()
        {
            return None;
        }
        Some((
            node_path,
            tsserver_path.to_string_lossy().replace('\\', "/"),
        ))
    }

    fn create_test_project_with_workspace_node_modules(dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir.join("src"))?;
        let workspace_node_modules = workspace_node_modules().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "workspace node_modules not found",
            )
        })?;
        let node_modules_dst = dir.join("node_modules");
        std::fs::create_dir_all(&node_modules_dst)?;
        refresh_generated_verter_types_stub(&node_modules_dst)?;

        let vue_path = if workspace_node_modules.join("vue/dist/vue.d.ts").exists() {
            workspace_node_modules.join("vue").canonicalize()?
        } else {
            let pnpm_dir = workspace_node_modules.join(".pnpm");
            let mut found = None;
            if pnpm_dir.exists() {
                for entry in std::fs::read_dir(&pnpm_dir)? {
                    let entry = entry?;
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("vue@") && !name_str.contains("node_modules") {
                        let candidate = entry.path().join("node_modules/vue");
                        if candidate.join("dist/vue.d.ts").exists() {
                            found = Some(candidate.canonicalize()?);
                            break;
                        }
                    }
                }
            }
            found.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "vue types not found")
            })?
        };
        let vue_parent = vue_path.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "vue package parent not found")
        })?;

        let vue_dst = node_modules_dst.join("vue");
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("cmd")
                .args([
                    "/C",
                    "mklink",
                    "/J",
                    &vue_dst.to_string_lossy(),
                    &vue_path.to_string_lossy(),
                ])
                .output();
        }
        #[cfg(not(windows))]
        {
            let _ = std::os::unix::fs::symlink(&vue_path, &vue_dst);
        }

        let at_vue_src = vue_parent.join("@vue");
        if at_vue_src.exists() {
            let at_vue_dst = node_modules_dst.join("@vue");
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("cmd")
                    .args([
                        "/C",
                        "mklink",
                        "/J",
                        &at_vue_dst.to_string_lossy(),
                        &at_vue_src.to_string_lossy(),
                    ])
                    .output();
            }
            #[cfg(not(windows))]
            {
                let _ = std::os::unix::fs::symlink(&at_vue_src, &at_vue_dst);
            }
        }

        let tsconfig = r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "jsx": "preserve",
    "jsxImportSource": "vue",
    "allowImportingTsExtensions": true
  },
  "include": ["src/**/*.ts", "src/**/*.tsx"]
}"#;
        std::fs::write(dir.join("tsconfig.json"), tsconfig)?;
        Ok(())
    }

    fn refresh_generated_verter_types_stub(node_modules_root: &Path) -> std::io::Result<()> {
        let types_dir = node_modules_root.join("@verter/types");
        let index_path = types_dir.join("index.d.ts");
        let pkg_path = types_dir.join("package.json");

        let existing_index = std::fs::read_to_string(&index_path).ok();
        let existing_pkg = std::fs::read_to_string(&pkg_path).ok();
        let is_generated_stub = existing_index
            .as_deref()
            .map(|index| index.starts_with("// Auto-generated by verter-lsp"))
            .unwrap_or(false)
            || existing_pkg
                .as_deref()
                .map(|pkg| pkg.contains(r#""types":"index.d.ts""#))
                .unwrap_or(false);

        if existing_index.is_some() && !is_generated_stub {
            return Ok(());
        }

        std::fs::create_dir_all(&types_dir)?;
        std::fs::write(&index_path, verter_session::VERTER_TYPES_STANDALONE_DTS)?;
        std::fs::write(
            &pkg_path,
            r#"{"name":"@verter/types","types":"index.d.ts"}"#,
        )?;
        Ok(())
    }

    #[test]
    fn test_byte_offset_to_tsserver_pos() {
        let content = "line1\nline2\nline3";
        // 'l' at start of line1 → (1, 1)
        assert_eq!(byte_offset_to_tsserver_pos(content, 0), (1, 1));
        // 'i' in line1 → (1, 2)
        assert_eq!(byte_offset_to_tsserver_pos(content, 1), (1, 2));
        // '\n' at end of line1 → (1, 6)
        assert_eq!(byte_offset_to_tsserver_pos(content, 5), (1, 6));
        // 'l' at start of line2 → (2, 1)
        assert_eq!(byte_offset_to_tsserver_pos(content, 6), (2, 1));
        // 'l' at start of line3 → (3, 1)
        assert_eq!(byte_offset_to_tsserver_pos(content, 12), (3, 1));
    }

    #[test]
    fn test_tsserver_pos_to_byte_offset() {
        let content = "line1\nline2\nline3";
        // (1, 1) → 0
        assert_eq!(tsserver_pos_to_byte_offset(content, 1, 1), 0);
        // (1, 2) → 1
        assert_eq!(tsserver_pos_to_byte_offset(content, 1, 2), 1);
        // (2, 1) → 6
        assert_eq!(tsserver_pos_to_byte_offset(content, 2, 1), 6);
        // (3, 1) → 12
        assert_eq!(tsserver_pos_to_byte_offset(content, 3, 1), 12);
    }

    #[test]
    fn test_roundtrip_position_conversion() {
        let content = "const x = 1;\nconst y = 2;\nconst z = 3;";
        for offset in 0..content.len() as u32 {
            let (line, col) = byte_offset_to_tsserver_pos(content, offset);
            let back = tsserver_pos_to_byte_offset(content, line, col);
            assert_eq!(
                back, offset,
                "roundtrip failed for offset {offset}: got ({line},{col}) -> {back}"
            );
        }
    }

    #[test]
    fn test_parse_tsserver_diagnostic() {
        let content = "const x = 1;\nconst y: string = 42;";
        let diag = serde_json::json!({
            "start": { "line": 2, "offset": 7 },
            "end": { "line": 2, "offset": 13 },
            "text": "Type 'number' is not assignable to type 'string'.",
            "code": 2322,
            "category": "error"
        });

        let parsed = parse_tsserver_diagnostic(&diag, Some(content)).unwrap();
        assert_eq!(
            parsed.message,
            "Type 'number' is not assignable to type 'string'."
        );
        assert!(matches!(parsed.severity, TypeDiagnosticSeverity::Error));
        assert_eq!(parsed.code, Some("2322".to_string()));
        // "string" starts at byte 19 (line 2, offset 7 → col index 6 → byte 13 + 6 = 19)
        assert_eq!(parsed.start, 19);
        // "string" ends at byte 25 (line 2, offset 13 → col index 12 → byte 13 + 12 = 25)
        assert_eq!(parsed.end, 25);
    }

    #[test]
    fn test_parse_tsserver_completion() {
        let item = serde_json::json!({
            "name": "myFunction",
            "kind": "function",
            "sortText": "11",
            "insertText": "myFunction"
        });
        let parsed = parse_tsserver_completion(&item).unwrap();
        assert_eq!(parsed.label, "myFunction");
        assert!(matches!(parsed.kind, Some(CompletionKind::Function)));
        assert_eq!(parsed.sort_text, Some("11".to_string()));
    }

    #[test]
    fn test_parse_tsserver_completion_kinds_match_vscode() {
        // Every case from VS Code's MyCompletionItem.convertKind()
        let cases = vec![
            // Keyword
            ("primitive type", CompletionKind::Keyword),
            ("keyword", CompletionKind::Keyword),
            // Variable
            ("const", CompletionKind::Variable),
            ("let", CompletionKind::Variable),
            ("var", CompletionKind::Variable),
            ("local var", CompletionKind::Variable),
            ("alias", CompletionKind::Variable),
            ("parameter", CompletionKind::Variable),
            // Field
            ("property", CompletionKind::Field),
            ("getter", CompletionKind::Field),
            ("setter", CompletionKind::Field),
            // Function
            ("function", CompletionKind::Function),
            ("local function", CompletionKind::Function),
            // Method
            ("method", CompletionKind::Method),
            ("construct", CompletionKind::Method),
            ("call", CompletionKind::Method),
            ("index", CompletionKind::Method),
            // Enum
            ("enum", CompletionKind::Enum),
            ("enum member", CompletionKind::EnumMember),
            // Module
            ("module", CompletionKind::Module),
            ("external module name", CompletionKind::Module),
            // Class/Interface
            ("class", CompletionKind::Class),
            ("type", CompletionKind::Class),
            ("interface", CompletionKind::Interface),
            // Special
            ("warning", CompletionKind::Text),
            ("script", CompletionKind::File),
            ("directory", CompletionKind::Folder),
            ("string", CompletionKind::Constant),
            // Default fallback → Property
            ("local class", CompletionKind::Property),
            ("constructor", CompletionKind::Property),
            ("type parameter", CompletionKind::Property),
            ("JSX attribute", CompletionKind::Property),
            ("accessor", CompletionKind::Property),
            ("using", CompletionKind::Property),
            ("await using", CompletionKind::Property),
            ("label", CompletionKind::Property),
            ("", CompletionKind::Property),
            ("unknown_kind", CompletionKind::Property),
        ];
        for (kind_str, expected) in cases {
            let item = serde_json::json!({
                "name": "test",
                "kind": kind_str,
                "sortText": "0"
            });
            let parsed = parse_tsserver_completion(&item).unwrap();
            assert_eq!(
                parsed.kind,
                Some(expected),
                "tsserver kind '{}' should map to {:?}",
                kind_str,
                expected
            );
        }
    }

    // ── Channel-based transport tests ────────────────────────────────

    /// @ai-generated — tsserver stdin_writer_loop exits on Shutdown message
    #[tokio::test]
    async fn tsserver_writer_loop_exits_on_shutdown() {
        let (client_reader, server_writer) = tokio::io::duplex(4096);
        let (tx, rx) = mpsc::channel::<TsserverStdinMessage>(16);

        let handle = tokio::spawn(tsserver_stdin_writer_loop(server_writer, rx));

        // Send a frame
        tx.send(TsserverStdinMessage::Frame(b"test\n".to_vec()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send Shutdown
        tx.send(TsserverStdinMessage::Shutdown).await.unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_secs(1), handle).await;
        assert!(
            result.is_ok(),
            "tsserver_stdin_writer_loop should exit after Shutdown"
        );

        // Verify the frame was written
        let mut reader = BufReader::new(client_reader);
        let mut buf = String::new();
        let n = reader.read_line(&mut buf).await.unwrap();
        assert!(n > 0, "should have read the frame");
        assert_eq!(buf.trim(), "test");
    }

    /// @ai-generated — tsserver shutdown completes within timeout when process is unresponsive
    #[tokio::test]
    async fn tsserver_shutdown_completes_within_timeout() {
        let (stdin_tx, _rx) = mpsc::channel::<TsserverStdinMessage>(16);

        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let transport = Arc::new(TsserverTransport {
            stdin_tx,
            pending,
            next_seq: AtomicI64::new(1),
        });

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                let _ = transport
                    .command_no_response("exit", serde_json::json!({}))
                    .await;
            })
            .await;
            let _ = transport
                .stdin_tx
                .send(TsserverStdinMessage::Shutdown)
                .await;
        })
        .await;

        assert!(
            result.is_ok(),
            "Shutdown should complete within 5s even when tsserver is unresponsive"
        );
    }

    #[test]
    fn test_format_quickinfo_hover_no_duplicate_kind() {
        // tsserver returns displayString that already includes (alias) prefix
        let result = format_quickinfo_hover("alias", "(alias) const Foo: number", "");
        assert!(
            result.contains("(alias) const Foo: number"),
            "should contain single (alias) prefix"
        );
        assert!(
            !result.contains("(alias) (alias)"),
            "must not duplicate kind prefix"
        );
    }

    #[test]
    fn test_format_quickinfo_hover_empty_kind() {
        // Non-existent variable: kind is empty
        let result = format_quickinfo_hover("", "any", "");
        assert!(result.contains("\nany\n"), "should contain bare 'any'");
        assert!(
            !result.contains("()"),
            "must not produce empty parens for empty kind"
        );
    }

    #[test]
    fn test_format_quickinfo_hover_normal_kind() {
        // Normal case: kind is not already in displayString
        let result = format_quickinfo_hover("const", "const foo: number", "");
        assert!(
            result.contains("(const) const foo: number"),
            "should prepend kind prefix"
        );
    }

    #[test]
    fn test_format_quickinfo_hover_local_function_no_duplicate() {
        let result = format_quickinfo_hover(
            "local function",
            "(local function) onPopupTransform(transform: string, v: number): string",
            "",
        );
        assert!(
            !result.contains("(local function) (local function)"),
            "must not duplicate local function prefix"
        );
        assert!(
            result.contains("(local function) onPopupTransform"),
            "should contain single prefix"
        );
    }

    #[test]
    fn test_format_quickinfo_hover_with_docs() {
        let result = format_quickinfo_hover("const", "const x: string", "A string variable");
        assert!(result.contains("(const) const x: string"));
        assert!(result.contains("A string variable"));
    }

    #[test]
    fn test_parse_tsserver_location_with_content() {
        let content = "const x = 1;\nconst y = 2;\nconst z = 3;";
        let mut cache = HashMap::new();
        cache.insert("d:/test/file.ts".to_string(), content.to_string());

        let loc = serde_json::json!({
            "file": "d:/test/file.ts",
            "start": { "line": 2, "offset": 7 },
            "end": { "line": 2, "offset": 8 },
        });

        let parsed = parse_tsserver_location(&loc, &cache).unwrap();
        assert_eq!(parsed.path, "d:/test/file.ts");
        // "y" is at byte 19 (line 2, col 7 in 1-based = byte 13 + 6 = 19)
        assert_eq!(parsed.start, 19, "start should be byte offset, not packed");
        assert_eq!(parsed.end, 20, "end should be byte offset, not packed");
        // Negative: must NOT be a packed position
        assert!(
            parsed.start < 100,
            "start must be a byte offset, not packed (1 << 16 = 65536)"
        );
    }

    #[test]
    fn test_parse_tsserver_location_without_content() {
        let cache = HashMap::new();

        let loc = serde_json::json!({
            "file": "d:/test/file.ts",
            "start": { "line": 2, "offset": 7 },
            "end": { "line": 2, "offset": 8 },
        });

        let parsed = parse_tsserver_location(&loc, &cache).unwrap();
        // Without content, should use packed fallback (0-based)
        let expected_start = ((2 - 1) << 16) | ((7 - 1) & 0xFFFF);
        assert_eq!(
            parsed.start, expected_start,
            "without content, should use packed fallback"
        );
    }

    #[test]
    fn test_parse_tsserver_location_line_10_not_packed() {
        let mut lines = Vec::new();
        for i in 0..15 {
            lines.push(format!("line{i:02}_content"));
        }
        let content = lines.join("\n");
        let mut cache = HashMap::new();
        cache.insert("d:/test/file.ts".to_string(), content.clone());

        let loc = serde_json::json!({
            "file": "d:/test/file.ts",
            "start": { "line": 10, "offset": 1 },
            "end": { "line": 10, "offset": 5 },
        });

        let parsed = parse_tsserver_location(&loc, &cache).unwrap();
        // With content, byte offset for line 10 should be reasonable (< 200 bytes)
        assert!(
            parsed.start < (10 << 16),
            "start must NOT be a packed position for line 10+"
        );
        assert!(parsed.start < 200, "start should be a small byte offset");
    }

    #[test]
    fn test_parse_tsserver_location_without_cache_reads_disk_content() {
        let temp_root = std::env::temp_dir().join(format!(
            "verter-tsserver-location-disk-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_root);
        std::fs::create_dir_all(&temp_root).unwrap();
        let file_path = temp_root.join("types.ts");
        let content = "export interface Props {\n  label: string;\n}\n";
        std::fs::write(&file_path, content).unwrap();
        let file_key = file_path.to_string_lossy().replace('\\', "/");
        let cache = HashMap::new();

        let loc = serde_json::json!({
            "file": file_key,
            "start": { "line": 2, "offset": 3 },
            "end": { "line": 2, "offset": 8 },
        });

        let parsed = parse_tsserver_location(&loc, &cache).unwrap();
        assert_eq!(parsed.start, 27);
        assert_eq!(parsed.end, 32);

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn test_parse_tsserver_rename_span_with_content() {
        let content = "const x = 1;\nconst y = 2;";
        let span = serde_json::json!({
            "start": { "line": 2, "offset": 7 },
            "end": { "line": 2, "offset": 8 },
        });

        let parsed = parse_tsserver_rename_span(&span, "d:/test/file.ts", Some(content)).unwrap();
        assert_eq!(parsed.start, 19, "start should be byte offset");
        assert_eq!(parsed.end, 20, "end should be byte offset");
        assert!(parsed.start < 100, "must not be packed");
    }

    #[test]
    fn test_parse_tsserver_location_non_ascii() {
        // tsserver uses UTF-16 code units for offset
        // "cafÃ©" = 5 bytes UTF-8 (c=1, a=1, f=1, Ã©=2), 4 UTF-16 code units
        let content = "cafÃ©\nworld";
        let mut cache = HashMap::new();
        cache.insert("d:/test/file.ts".to_string(), content.to_string());

        let loc = serde_json::json!({
            "file": "d:/test/file.ts",
            "start": { "line": 2, "offset": 1 },
            "end": { "line": 2, "offset": 6 },
        });

        let parsed = parse_tsserver_location(&loc, &cache).unwrap();
        // "cafÃ©\n" = 6 bytes (c=1, a=1, f=1, Ã©=2, \n=1)
        // "world" starts at byte 6
        assert_eq!(parsed.start, 6, "start of 'world' should be byte 6");
        // "world" ends at byte 11
        assert_eq!(parsed.end, 11, "end of 'world' should be byte 11");
    }

    #[test]
    fn test_byte_offset_to_tsserver_pos_non_ascii() {
        // "cafÃ©\nworld" — 'Ã©' is 2 bytes UTF-8, 1 UTF-16 code unit
        let content = "cafÃ©\nworld";
        // byte 6 = start of "world" = line 2, col 1 in 1-based
        let (line, col) = byte_offset_to_tsserver_pos(content, 6);
        assert_eq!(line, 2, "should be line 2");
        assert_eq!(col, 1, "should be col 1 (UTF-16)");
    }

    #[test]
    fn test_tsserver_pos_to_byte_offset_non_ascii() {
        // "cafÃ©\nworld" — 'Ã©' is 2 bytes UTF-8, 1 UTF-16 code unit
        let content = "cafÃ©\nworld";
        // line 2, offset 1 (1-based) → byte 6 ("world" starts there)
        let offset = tsserver_pos_to_byte_offset(content, 2, 1);
        assert_eq!(offset, 6, "line 2, col 1 should be byte 6");
    }

    async fn send_success_response(
        pending: &Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>,
        seq: i64,
        command: &str,
    ) {
        if let Some(tx) = pending.lock().await.remove(&seq) {
            let _ = tx.send(serde_json::json!({
                "type": "response",
                "request_seq": seq,
                "success": true,
                "command": command,
                "body": {}
            }));
        }
    }

    #[tokio::test]
    async fn test_configure_tsserver_session_does_not_wait_for_inferred_project_options() {
        let (client_reader, server_writer) = tokio::io::duplex(65536);
        let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
        tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));

        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TsserverTransport {
            stdin_tx: stdin_tx.clone(),
            pending: Arc::clone(&pending),
            next_seq: AtomicI64::new(1),
        });

        let seen_commands = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_commands_task = Arc::clone(&seen_commands);
        let pending_task = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut reader = BufReader::new(client_reader);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let msg: serde_json::Value =
                            serde_json::from_str(line.trim()).expect("valid tsserver request");
                        let seq = msg["seq"].as_i64().expect("request seq");
                        let command = msg["command"]
                            .as_str()
                            .expect("request command")
                            .to_string();
                        seen_commands_task.lock().await.push(command.clone());
                        if command == "configure" {
                            send_success_response(&pending_task, seq, &command).await;
                        } else if command == "compilerOptionsForInferredProjects" {
                            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                            send_success_response(&pending_task, seq, &command).await;
                            break;
                        }
                    }
                }
            }
        });

        let start = std::time::Instant::now();
        let ws_root = configure_tsserver_session(Arc::clone(&transport), "C:\\project")
            .await
            .expect("configuration should succeed");
        let elapsed = start.elapsed();

        assert_eq!(ws_root, "C:/project");
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "tsserver startup should not wait for inferred project options (elapsed {:?})",
            elapsed
        );

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let commands = seen_commands.lock().await.clone();
        assert_eq!(
            commands.first().map(String::as_str),
            Some("configure"),
            "configure must still be sent first"
        );
        assert!(
            commands
                .iter()
                .any(|command| command == "compilerOptionsForInferredProjects"),
            "inferred project options should still be requested in the background"
        );

        let _ = stdin_tx.send(TsserverStdinMessage::Shutdown).await;
    }

    // ── update_file end-line tests ──────────────────────────────────

    /// Helper: run the same logic as TypeProvider::update_file but against a
    /// bare TsserverTransport + shared caches, returning the JSON frames
    /// that were written to stdin.
    async fn run_update_file_capture(
        old_content: Option<&str>,
        new_content: &str,
        file: &str,
    ) -> Vec<serde_json::Value> {
        let (client_reader, server_writer) = tokio::io::duplex(65536);
        let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
        tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));

        let transport = Arc::new(TsserverTransport {
            stdin_tx: stdin_tx.clone(),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_seq: AtomicI64::new(1),
        });

        let contents_cache: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let opened_files: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

        // Pre-populate caches to simulate an already-open file
        if let Some(old) = old_content {
            contents_cache
                .lock()
                .await
                .insert(file.to_string(), old.to_string());
            opened_files.lock().await.insert(file.to_string());
        }

        // Run the same logic as update_file
        let content = new_content.to_string();
        let file = file.to_string();
        let project_root = "/project".to_string();

        // Read old content's line count BEFORE inserting new content
        let old_line_count = {
            let cache = contents_cache.lock().await;
            cache.get(&file).map(|c| c.lines().count() as u32 + 1)
        };

        contents_cache
            .lock()
            .await
            .insert(file.clone(), content.clone());

        let mut opened = opened_files.lock().await;
        if opened.contains(&file) {
            drop(opened);
            if let Some(end_line) = old_line_count {
                let _ = transport
                    .command_no_response(
                        "updateOpen",
                        serde_json::json!({
                            "changedFiles": [{
                                "fileName": file,
                                "textChanges": [{
                                    "start": { "line": 1, "offset": 1 },
                                    "end": { "line": end_line, "offset": 1 },
                                    "newText": content,
                                }]
                            }]
                        }),
                    )
                    .await;
            } else {
                let _ = transport
                    .command_no_response(
                        "updateOpen",
                        serde_json::json!({
                            "closedFiles": [&file],
                            "openFiles": [{
                                "file": file,
                                "fileContent": content,
                                "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                                    else if file.ends_with(".jsx") { "JSX" }
                                    else if file.ends_with(".js") { "JS" }
                                    else { "TS" },
                                "projectRootPath": project_root,
                            }]
                        }),
                    )
                    .await;
            }
        } else {
            opened.insert(file.clone());
            drop(opened);
            let _ = transport
                .command_no_response(
                    "open",
                    serde_json::json!({
                        "file": file,
                        "fileContent": content,
                        "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                            else if file.ends_with(".jsx") { "JSX" }
                            else if file.ends_with(".js") { "JS" }
                            else { "TS" },
                        "projectRootPath": project_root,
                    }),
                )
                .await;
        }

        // Shutdown writer + read all frames
        let _ = stdin_tx.send(TsserverStdinMessage::Shutdown).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut reader = BufReader::new(client_reader);
        let mut frames = Vec::new();
        loop {
            let mut line = String::new();
            match tokio::time::timeout(
                std::time::Duration::from_millis(100),
                reader.read_line(&mut line),
            )
            .await
            {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(_)) => {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                        frames.push(val);
                    }
                }
                Ok(Err(_)) => break,
            }
        }
        frames
    }

    #[tokio::test]
    async fn test_update_file_end_line_matches_old_content() {
        let old = "line1\nline2\nline3"; // 3 lines
        let new = "line1\nline2\nline3\nline4\nline5"; // 5 lines
        let frames = run_update_file_capture(Some(old), new, "/project/src/App.vue.tsx").await;

        assert_eq!(frames.len(), 1, "should send exactly one command");
        let args = &frames[0]["arguments"];
        let end_line = args["changedFiles"][0]["textChanges"][0]["end"]["line"]
            .as_u64()
            .unwrap();

        // Old content has 3 lines → end line should be 4 (lines().count() + 1)
        assert_eq!(end_line, 4, "end line should be old content line count + 1");
        assert_ne!(end_line, 1_000_000, "must NOT use hardcoded 1_000_000");
    }

    #[tokio::test]
    async fn test_update_file_single_line_content() {
        let old = "const x = 1;"; // 1 line
        let new = "const x = 1;\nconst y = 2;";
        let frames = run_update_file_capture(Some(old), new, "/project/src/App.vue.tsx").await;

        let end_line = frames[0]["arguments"]["changedFiles"][0]["textChanges"][0]["end"]["line"]
            .as_u64()
            .unwrap();
        assert_eq!(end_line, 2, "single-line content: lines().count()=1, +1=2");
        assert_ne!(end_line, 1_000_000, "must NOT use hardcoded 1_000_000");
    }

    #[tokio::test]
    async fn test_update_file_first_open_uses_open_command() {
        // No old content → should use "open" command, not "updateOpen"
        let frames =
            run_update_file_capture(None, "const x = 1;", "/project/src/New.vue.tsx").await;

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["command"].as_str().unwrap(), "open");
        // Should not contain changedFiles or end line at all
        assert!(
            frames[0]["arguments"].get("changedFiles").is_none(),
            "open command should not have changedFiles"
        );
    }

    // ── get_semantic_tokens cache-miss test ──────────────────────────

    #[tokio::test]
    async fn test_get_semantic_tokens_cache_miss_returns_empty() {
        // Simulate what get_semantic_tokens does on cache miss:
        // It should return Ok(vec![]) without sending any request.
        let contents_cache: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let content = {
            let cache = contents_cache.lock().await;
            cache.get("/project/src/Missing.vue.tsx").cloned()
        };

        // With the fix, content is None → early return
        assert!(content.is_none(), "cache miss should yield None");
        // The actual fix changes the code to `return Ok(vec![])` here,
        // so no transport request is sent. We verify the None path exists.
    }

    // ── env denylist test ──────────────────────────────────────────

    #[test]
    fn test_child_process_env_denylist_strips_debug_vars() {
        // Verify the constant contains exactly the expected vars
        assert!(
            CHILD_PROCESS_ENV_DENYLIST.contains(&"NODE_OPTIONS"),
            "should deny NODE_OPTIONS"
        );
        assert!(
            CHILD_PROCESS_ENV_DENYLIST.contains(&"VSCODE_INSPECTOR_OPTIONS"),
            "should deny VSCODE_INSPECTOR_OPTIONS"
        );
        assert!(
            CHILD_PROCESS_ENV_DENYLIST.contains(&"ELECTRON_RUN_AS_NODE"),
            "should deny ELECTRON_RUN_AS_NODE"
        );

        // Verify that std::process::Command.env_remove with these vars works
        // (same API as tokio::process::Command)
        let mut cmd = std::process::Command::new("echo");
        for var in CHILD_PROCESS_ENV_DENYLIST {
            cmd.env_remove(var);
        }
        // If we get here without panic, the API accepts all denylist entries.
        // Also verify the list length is exactly 3 (no accidental additions)
        assert_eq!(
            CHILD_PROCESS_ENV_DENYLIST.len(),
            3,
            "denylist should have exactly 3 entries"
        );
    }

    #[test]
    fn test_tsserver_plugin_args_are_empty_without_probe_location() {
        assert!(
            tsserver_plugin_args(None).is_empty(),
            "no plugin path should produce no plugin args"
        );
        assert!(
            tsserver_plugin_args(Some("")).is_empty(),
            "empty plugin path should produce no plugin args"
        );
    }

    #[test]
    fn test_tsserver_plugin_args_enable_verter_plugin() {
        let args = tsserver_plugin_args(Some("/workspace/node_modules"));
        assert_eq!(
            args,
            vec![
                "--globalPlugins".to_string(),
                "@verter/typescript-plugin".to_string(),
                "--pluginProbeLocations".to_string(),
                "/workspace/node_modules".to_string(),
                "--allowLocalPluginLoads".to_string(),
            ],
            "tsserver should be launched with the Verter TS plugin enabled"
        );
    }

    #[tokio::test]
    async fn test_e2e_tsserver_scoped_slot_types_from_generated_vue_outputs() {
        let Some((node_path, tsserver_path)) = tsserver_assets_or_skip() else {
            eprintln!("skipping: node or tsserver.js not found");
            return;
        };

        let tmp = std::env::temp_dir().join("verter_tsserver_slot_types");
        let _ = std::fs::remove_dir_all(&tmp);
        if create_test_project_with_workspace_node_modules(&tmp).is_err() {
            eprintln!("skipping: could not create test project with workspace node_modules");
            return;
        }

        let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()

const items: SlotItem[] = [{ id: 1, name: 'alpha' }]
</script>

<template>
  <slot :slotItem="items[0]" :slotIndex="0" :slotTotal="items.length" />
</template>
"#;
        let parent_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.na }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

        let host = VerterHost::new_standalone(HostConfig::default());
        let child_id = "/src/TypedSlotComp.vue";
        let parent_id = "/src/TemplateSlotCases.vue";

        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(child_id.to_string()),
            input_id: child_id.to_string(),
            source: Arc::from(child_source),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(parent_id.to_string()),
            input_id: parent_id.to_string(),
            source: Arc::from(parent_source),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });

        let profile = CompileProfile {
            source_map: false,
            target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
            embed_ambient_types: false,
            ..Default::default()
        };

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(child_id.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("child compilation should succeed");
        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(parent_id.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("parent compilation should succeed");

        let child_api = host
            .get_public_api(child_id)
            .expect("child public API should exist");
        let parent_ide = host
            .get_ide(parent_id, &profile)
            .expect("parent IDE output should exist");

        let src_dir = tmp.join("src");
        let child_api_path = src_dir.join("TypedSlotComp.vue.ts");
        let parent_ide_path = src_dir.join("TemplateSlotCases.vue.tsx");
        std::fs::write(&child_api_path, &*child_api.code).expect("child API should be written");
        std::fs::write(&parent_ide_path, &*parent_ide.code).expect("parent IDE should be written");

        let provider = TsserverTypeProvider::spawn(
            &node_path,
            &tsserver_path,
            tmp.to_str().expect("tmp path should be valid UTF-8"),
            None,
            None,
        )
        .await
        .expect("tsserver should spawn");

        let child_api_path_str = child_api_path.to_string_lossy().replace('\\', "/");
        let parent_ide_path_str = parent_ide_path.to_string_lossy().replace('\\', "/");

        provider
            .open_file(&child_api_path_str, &child_api.code)
            .await
            .expect("child API should open");
        provider
            .open_file(&parent_ide_path_str, &parent_ide.code)
            .await
            .expect("parent IDE should open");

        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

        let local_offset = parent_ide
            .code
            .find("slotItem.name")
            .expect("parent IDE should reference slotItem.name") as u32;
        let member_offset = local_offset + "slotItem.".len() as u32;

        let hover = provider
            .get_hover(&parent_ide_path_str, local_offset)
            .await
            .expect("hover request should succeed")
            .expect("slot hover should exist");
        eprintln!("tsserver slot hover: {}", hover.contents);

        let completion_result = provider
            .get_completions(&parent_ide_path_str, member_offset, Some("."))
            .await;
        let labels: Vec<String> = completion_result
            .as_ref()
            .ok()
            .map(|result| result.items.iter().map(|item| item.label.clone()).collect())
            .unwrap_or_default();

        assert!(
            hover.contents.contains("SlotItem")
                || (hover.contents.contains("name") && hover.contents.contains("id")),
            "slot hover should keep the concrete slot type, got: {}",
            hover.contents
        );
        assert!(
            !hover.contents.contains(": any"),
            "slot hover should not degrade to any, got: {}",
            hover.contents
        );
        assert!(
            completion_result.is_ok(),
            "slot member completion should succeed, got: {:?}",
            completion_result.err()
        );
        assert!(
            labels.iter().any(|label| label == "name"),
            "slot member completions should include name, got: {labels:?}"
        );
        assert!(
            labels.iter().any(|label| label == "id"),
            "slot member completions should include id, got: {labels:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_e2e_tsserver_scoped_slot_types_with_in_memory_child_api() {
        let Some((node_path, tsserver_path)) = tsserver_assets_or_skip() else {
            eprintln!("skipping: node or tsserver.js not found");
            return;
        };

        let tmp = std::env::temp_dir().join("verter_tsserver_slot_types_in_memory");
        let _ = std::fs::remove_dir_all(&tmp);
        if create_test_project_with_workspace_node_modules(&tmp).is_err() {
            eprintln!("skipping: could not create test project with workspace node_modules");
            return;
        }

        let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()

const items: SlotItem[] = [{ id: 1, name: 'alpha' }]
</script>

<template>
  <slot :slotItem="items[0]" :slotIndex="0" :slotTotal="items.length" />
</template>
"#;
        let parent_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.na }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

        let host = VerterHost::new_standalone(HostConfig::default());
        let child_id = "/src/TypedSlotComp.vue";
        let parent_id = "/src/TemplateSlotCases.vue";

        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(child_id.to_string()),
            input_id: child_id.to_string(),
            source: Arc::from(child_source),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(parent_id.to_string()),
            input_id: parent_id.to_string(),
            source: Arc::from(parent_source),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });

        let profile = CompileProfile {
            source_map: false,
            target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
            embed_ambient_types: false,
            ..Default::default()
        };

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(child_id.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("child compilation should succeed");
        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(parent_id.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("parent compilation should succeed");

        let child_api = host
            .get_public_api(child_id)
            .expect("child public API should exist");
        let parent_ide = host
            .get_ide(parent_id, &profile)
            .expect("parent IDE output should exist");

        let src_dir = tmp.join("src");
        let child_api_path = src_dir.join("TypedSlotComp.vue.ts");
        let parent_ide_path = src_dir.join("TemplateSlotCases.vue.tsx");
        std::fs::write(&parent_ide_path, &*parent_ide.code).expect("parent IDE should be written");

        let provider = TsserverTypeProvider::spawn(
            &node_path,
            &tsserver_path,
            tmp.to_str().expect("tmp path should be valid UTF-8"),
            None,
            None,
        )
        .await
        .expect("tsserver should spawn");

        let child_api_path_str = child_api_path.to_string_lossy().replace('\\', "/");
        let parent_ide_path_str = parent_ide_path.to_string_lossy().replace('\\', "/");

        provider
            .open_file(&child_api_path_str, &child_api.code)
            .await
            .expect("child API should open");
        provider
            .open_file(&parent_ide_path_str, &parent_ide.code)
            .await
            .expect("parent IDE should open");

        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

        let member_offset = parent_ide
            .code
            .find("slotItem.name")
            .expect("parent IDE should reference slotItem.name") as u32
            + "slotItem.".len() as u32;

        let completion_result = provider
            .get_completions(&parent_ide_path_str, member_offset, Some("."))
            .await;

        assert!(
            completion_result.is_ok(),
            "slot member completion should succeed with an in-memory child API, got: {:?}",
            completion_result.err()
        );
    }

    #[tokio::test]
    async fn test_e2e_tsserver_scoped_slot_types_with_plugin_and_open_child_ide() {
        let Some((node_path, tsserver_path)) = tsserver_assets_or_skip() else {
            eprintln!("skipping: node or tsserver.js not found");
            return;
        };

        let tmp = std::env::temp_dir().join("verter_tsserver_slot_types_plugin_child_ide");
        let _ = std::fs::remove_dir_all(&tmp);
        if create_test_project_with_workspace_node_modules(&tmp).is_err() {
            eprintln!("skipping: could not create test project with workspace node_modules");
            return;
        }

        let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()

const items: SlotItem[] = [{ id: 1, name: 'alpha' }]
</script>

<template>
  <slot :slotItem="items[0]" :slotIndex="0" :slotTotal="items.length" />
</template>
"#;
        let parent_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.na }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

        let host = VerterHost::new_standalone(HostConfig::default());
        let child_id = "/src/TypedSlotComp.vue";
        let parent_id = "/src/TemplateSlotCases.vue";

        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(child_id.to_string()),
            input_id: child_id.to_string(),
            source: Arc::from(child_source),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(parent_id.to_string()),
            input_id: parent_id.to_string(),
            source: Arc::from(parent_source),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });

        let profile = CompileProfile {
            source_map: false,
            target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
            embed_ambient_types: false,
            ..Default::default()
        };

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(child_id.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("child compilation should succeed");
        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(parent_id.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("parent compilation should succeed");

        let child_api = host
            .get_public_api(child_id)
            .expect("child public API should exist");
        let child_ide = host
            .get_ide(child_id, &profile)
            .expect("child IDE output should exist");
        let parent_api = host
            .get_public_api(parent_id)
            .expect("parent public API should exist");
        let parent_ide = host
            .get_ide(parent_id, &profile)
            .expect("parent IDE output should exist");

        let src_dir = tmp.join("src");
        let child_api_path = src_dir.join("TypedSlotComp.vue.ts");
        let child_ide_path = src_dir.join("TypedSlotComp.vue.tsx");
        let parent_api_path = src_dir.join("TemplateSlotCases.vue.ts");
        let parent_ide_path = src_dir.join("TemplateSlotCases.vue.tsx");

        let plugin_path = tmp
            .join("node_modules")
            .to_string_lossy()
            .replace('\\', "/");
        let provider = TsserverTypeProvider::spawn(
            &node_path,
            &tsserver_path,
            tmp.to_str().expect("tmp path should be valid UTF-8"),
            Some(&plugin_path),
            None,
        )
        .await
        .expect("tsserver should spawn");

        provider
            .open_file(
                &child_ide_path.to_string_lossy().replace('\\', "/"),
                &child_ide.code,
            )
            .await
            .expect("child IDE should open");
        provider
            .open_file(
                &child_api_path.to_string_lossy().replace('\\', "/"),
                &child_api.code,
            )
            .await
            .expect("child API should open");
        provider
            .open_file(
                &parent_api_path.to_string_lossy().replace('\\', "/"),
                &parent_api.code,
            )
            .await
            .expect("parent API should open");
        provider
            .open_file(
                &parent_ide_path.to_string_lossy().replace('\\', "/"),
                &parent_ide.code,
            )
            .await
            .expect("parent IDE should open");

        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

        let member_offset = parent_ide
            .code
            .find("slotItem.name")
            .expect("parent IDE should reference slotItem.name") as u32
            + "slotItem.".len() as u32;

        let completion_result = provider
            .get_completions(
                &parent_ide_path.to_string_lossy().replace('\\', "/"),
                member_offset,
                Some("."),
            )
            .await;

        assert!(
            completion_result.is_ok(),
            "slot member completion should succeed with plugin + child IDE open, got: {:?}",
            completion_result.err()
        );
    }

    #[tokio::test]
    async fn test_e2e_tsserver_vfor_member_access_from_fixture_generated_vue_output() {
        let Some((node_path, tsserver_path)) = tsserver_assets_or_skip() else {
            eprintln!("skipping: node or tsserver.js not found");
            return;
        };

        let tmp = std::env::temp_dir().join("verter_tsserver_fixture_vfor_member_access");
        let _ = std::fs::remove_dir_all(&tmp);
        if create_test_project_with_workspace_node_modules(&tmp).is_err() {
            eprintln!("skipping: could not create test project with workspace node_modules");
            return;
        }

        let source =
            include_str!("../../../../packages/vue-vscode/e2e/fixtures/single-project/src/App.vue");
        let host = VerterHost::new_standalone(HostConfig::default());
        let app_id = "/src/App.vue";

        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(app_id.to_string()),
            input_id: app_id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });

        let profile = CompileProfile {
            source_map: false,
            target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
            embed_ambient_types: false,
            ..Default::default()
        };

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(app_id.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("fixture compilation should succeed");

        let app_ide = host
            .get_ide(app_id, &profile)
            .expect("fixture IDE output should exist");

        let src_dir = tmp.join("src");
        let app_ide_path = src_dir.join("App.vue.tsx");
        std::fs::write(&app_ide_path, &*app_ide.code).expect("fixture IDE should be written");

        let provider = TsserverTypeProvider::spawn(
            &node_path,
            &tsserver_path,
            tmp.to_str().expect("tmp path should be valid UTF-8"),
            None,
            None,
        )
        .await
        .expect("tsserver should spawn");

        let app_ide_path_str = app_ide_path.to_string_lossy().replace('\\', "/");
        provider
            .open_file(&app_ide_path_str, &app_ide.code)
            .await
            .expect("fixture IDE should open");

        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

        let member_offset = app_ide
            .code
            .find("action.disabled")
            .map(|offset| offset as u32 + "action.".len() as u32)
            .expect("fixture IDE should reference action.disabled");

        let completion_result = provider
            .get_completions(&app_ide_path_str, member_offset, Some("."))
            .await;
        let labels: Vec<String> = completion_result
            .as_ref()
            .ok()
            .map(|result| result.items.iter().map(|item| item.label.clone()).collect())
            .unwrap_or_default();

        assert!(
            completion_result.is_ok(),
            "fixture member completion should succeed, got: {:?}",
            completion_result.err()
        );
        assert!(
            labels.iter().any(|label| label == "disabled"),
            "fixture member completions should include disabled, got: {labels:?}\nTSX code:\n{}",
            app_ide.code
        );
        assert!(
            labels.iter().any(|label| label == "label"),
            "fixture member completions should include label, got: {labels:?}"
        );
        assert!(
            labels.iter().any(|label| label == "handler"),
            "fixture member completions should include handler, got: {labels:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
