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

use crate::tsgo::protocol::*;
use crate::tsgo::traits::{ProviderFuture, TypeProvider};

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
                    return Err(TypeProviderError::new(msg));
                }
                Ok(val.get("body").cloned().unwrap_or(serde_json::Value::Null))
            }
            Ok(Err(_)) => Err(TypeProviderError::new("response channel closed")),
            Err(_) => {
                // Timeout — clean up the pending entry to prevent leak
                self.pending.lock().await.remove(&seq);
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
fn parse_tsserver_diagnostic(
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
fn byte_offset_to_tsserver_pos(content: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let bytes = content.as_bytes();
    let mut line = 1u32;
    let mut line_start = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if i == offset {
            return (line, (offset - line_start + 1) as u32);
        }
        if b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    (line, (offset.min(bytes.len()) - line_start + 1) as u32)
}

/// Convert tsserver's 1-based (line, offset) position to a byte offset.
fn tsserver_pos_to_byte_offset(content: &str, line: u32, offset: u32) -> u32 {
    let target_line = line.saturating_sub(1) as usize;
    let target_col = offset.saturating_sub(1) as usize;
    let mut current_line = 0usize;
    let mut byte_offset = 0usize;
    let bytes = content.as_bytes();

    while current_line < target_line && byte_offset < bytes.len() {
        if bytes[byte_offset] == b'\n' {
            current_line += 1;
        }
        byte_offset += 1;
    }

    (byte_offset + target_col).min(bytes.len()) as u32
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

impl TsserverTypeProvider {
    /// Spawn a tsserver process and initialize it.
    ///
    /// `node_path`: path to the `node` executable.
    /// `tsserver_path`: path to `tsserver.js`.
    /// `workspace_root`: filesystem path to the workspace root.
    /// `plugin_path`: path to the directory containing `@verter/typescript-plugin`.
    pub async fn spawn(
        node_path: &str,
        tsserver_path: &str,
        workspace_root: &str,
        plugin_path: Option<&str>,
        crash_notify: Option<Arc<Notify>>,
    ) -> Result<Self, TypeProviderError> {
        let mut cmd = tokio::process::Command::new(node_path);
        cmd.arg(tsserver_path)
            .arg("--useSyntaxServer=false")
            .arg("--disableAutomaticTypingAcquisition");

        if let Some(pp) = plugin_path {
            cmd.arg("--globalPlugins")
                .arg("@verter/typescript-plugin")
                .arg("--pluginProbeLocations")
                .arg(pp);
        }

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

        // Send configure request
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

        // Send compilerOptions for inferred projects (fallback when no tsconfig.json matches).
        // These should be generous enough to handle Vue TSX without errors.
        let _ = transport
            .request(
                "compilerOptionsForInferredProjects",
                serde_json::json!({
                    "options": {
                        "module": "esnext",
                        "target": "esnext",
                        "moduleResolution": "bundler",
                        "jsx": "preserve",
                        "jsxImportSource": "vue",
                        "allowJs": true,
                        "strict": true,
                        "allowArbitraryExtensions": true,
                        "baseUrl": ws_root,
                    }
                }),
            )
            .await;

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
                .await
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        // For tsserver, load_file only caches the content locally — it does NOT
        // send an `open` command. Sending 500+ `open` commands during background
        // sync overwhelms tsserver and blocks user requests for 15-20 seconds.
        // The TypeScript plugin (@verter/typescript-plugin) handles .vue import
        // resolution lazily inside tsserver's process. When the user actually opens
        // a file, `open_file` will be called and tsserver will receive the content.
        let file = Self::normalize_path(path);
        let content = content.to_string();
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            contents_cache.lock().await.insert(file, content);
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
            contents_cache
                .lock()
                .await
                .insert(file.clone(), content.clone());

            let mut opened = opened_files.lock().await;
            if opened.contains(&file) {
                drop(opened);
                tracing::debug!("tsserver update_file: updateOpen for {file}");
                // Use updateOpen for already-open files (atomic open+close+open)
                transport
                    .command_no_response(
                        "updateOpen",
                        serde_json::json!({
                            "changedFiles": [{
                                "fileName": file,
                                "textChanges": [{
                                    "start": { "line": 1, "offset": 1 },
                                    "end": { "line": 1_000_000, "offset": 1 },
                                    "newText": content,
                                }]
                            }]
                        }),
                    )
                    .await
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
                    .await
            }
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        Box::pin(async move {
            contents_cache.lock().await.remove(&file);
            opened_files.lock().await.remove(&file);
            transport
                .command_no_response("close", serde_json::json!({ "file": file }))
                .await
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
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

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
                        return Ok(None);
                    }

                    let contents = format_quickinfo_hover(kind, display, docs);

                    Ok(Some(HoverInfo {
                        contents,
                        range_start: None,
                        range_end: None,
                    }))
                }
                Err(e) => {
                    tracing::warn!("tsserver quickinfo error for {file}: {e}");
                    Ok(None)
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

            let locs = result
                .as_array()
                .map(|arr| arr.iter().filter_map(parse_tsserver_location).collect())
                .unwrap_or_default();

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

            let locs = result
                .get("refs")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(parse_tsserver_location).collect())
                .unwrap_or_default();

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

            let locs = result
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
                            group
                                .get("locs")
                                .and_then(|v| v.as_array())
                                .into_iter()
                                .flat_map(move |spans| {
                                    let fp = file_path.clone();
                                    spans.iter().filter_map(move |span| {
                                        parse_tsserver_rename_span(span, &fp)
                                    })
                                })
                        })
                        .collect()
                })
                .unwrap_or_default();

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
                    let actions = body
                        .as_array()
                        .map(|arr| arr.iter().filter_map(parse_tsserver_code_action).collect())
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
            let end_line = content
                .as_ref()
                .map(|c| c.lines().count() as u32 + 1)
                .unwrap_or(1_000_000);

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
                    let canonical = crate::documents::uri_to_canonical_id_from_str(uri);
                    roots.retain(|r| r != &canonical);
                }
            }

            // Add new folders
            for folder in &added {
                if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                    let canonical = crate::documents::uri_to_canonical_id_from_str(uri);
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
                "allowJs": true,
                "strict": true,
                "allowArbitraryExtensions": true,
                "baseUrl": base_url,
                "paths": paths,
            });
            // Remove null paths (shouldn't happen but be safe)
            if options.get("paths").is_some_and(|v| v.is_null()) {
                options.as_object_mut().unwrap().remove("paths");
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
fn parse_tsserver_completion(item: &serde_json::Value) -> Option<Completion> {
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

/// Parse a tsserver location (used in definition/references responses).
///
/// tsserver locations have: `{ file, start: {line, offset}, end: {line, offset} }`
fn parse_tsserver_location(loc: &serde_json::Value) -> Option<TypeLocation> {
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

    // Convert 1-based to packed 0-based offsets
    let s = ((sl.saturating_sub(1)) << 16) | ((so.saturating_sub(1)) & 0xFFFF);
    let e = ((el.saturating_sub(1)) << 16) | ((eo.saturating_sub(1)) & 0xFFFF);

    Some(TypeLocation {
        path: file,
        start: s,
        end: e,
    })
}

/// Parse a tsserver rename span into a RenameLocation.
fn parse_tsserver_rename_span(span: &serde_json::Value, file: &str) -> Option<RenameLocation> {
    let start = span.get("start")?;
    let end = span.get("end")?;
    let sl = start.get("line")?.as_u64()? as u32;
    let so = start.get("offset")?.as_u64()? as u32;
    let el = end.get("line")?.as_u64()? as u32;
    let eo = end.get("offset")?.as_u64()? as u32;

    let s = ((sl.saturating_sub(1)) << 16) | ((so.saturating_sub(1)) & 0xFFFF);
    let e = ((el.saturating_sub(1)) << 16) | ((eo.saturating_sub(1)) & 0xFFFF);

    Some(RenameLocation {
        path: file.to_string(),
        start: s,
        end: e,
    })
}

/// Parse a tsserver code action / code fix.
fn parse_tsserver_code_action(action: &serde_json::Value) -> Option<TypeCodeAction> {
    let description = action.get("description")?.as_str()?.to_string();
    let changes = action.get("changes")?.as_array()?;

    let mut edits = Vec::new();
    for change in changes {
        let file = change
            .get("fileName")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .replace('\\', "/");
        if let Some(text_changes) = change.get("textChanges").and_then(|v| v.as_array()) {
            for tc in text_changes {
                let start = tc.get("start")?;
                let end = tc.get("end")?;
                let new_text = tc.get("newText")?.as_str()?.to_string();
                let sl = start.get("line")?.as_u64()? as u32;
                let so = start.get("offset")?.as_u64()? as u32;
                let el = end.get("line")?.as_u64()? as u32;
                let eo = end.get("offset")?.as_u64()? as u32;

                let s = ((sl.saturating_sub(1)) << 16) | ((so.saturating_sub(1)) & 0xFFFF);
                let e = ((el.saturating_sub(1)) << 16) | ((eo.saturating_sub(1)) & 0xFFFF);

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
fn concat_display_parts(parts: &[serde_json::Value]) -> String {
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
fn format_quickinfo_hover(kind: &str, display: &str, docs: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
