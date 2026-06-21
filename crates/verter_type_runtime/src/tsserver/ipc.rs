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
        crate::type_runtime_trace_scope_async!(
            "tsserver_transport_request",
            format!(
                "command={} {}",
                command,
                summarize_tsserver_args(&arguments),
            ),
            async {
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
        )
        .await
    }

    /// Send a tsserver command without waiting for a response.
    async fn command_no_response(
        &self,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<(), TypeProviderError> {
        crate::type_runtime_trace_scope_async!(
            "tsserver_transport_command",
            format!(
                "command={} {}",
                command,
                summarize_tsserver_args(&arguments),
            ),
            async {
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
        )
        .await
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
                    let file = verter_span::path::canonicalize_path(
                        body.get("file")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default(),
                    );
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

    // tsserver flags editor-facing tags via two booleans: `reportsUnnecessary`
    // (unused-symbol fade, e.g. TS6133) and `reportsDeprecated` (strikethrough).
    // Mirror them onto the provider-neutral carrier so the LSP merge can re-emit
    // them as `DiagnosticTag`s — this is what grays out an unused `.vue` import.
    let mut tags = Vec::new();
    if d.get("reportsUnnecessary")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        tags.push(TypeDiagnosticTag::Unnecessary);
    }
    if d.get("reportsDeprecated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        tags.push(TypeDiagnosticTag::Deprecated);
    }

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
        tags,
    })
}

/// Union the three tsserver-family diagnostic passes into one ordered, deduplicated set.
///
/// Native TypeScript surfaces three distinct diagnostic categories — SEMANTIC
/// (`semanticDiagnosticsSync`), SYNTACTIC (`syntacticDiagnosticsSync`, parse
/// errors), and SUGGESTION (`suggestionDiagnosticsSync`, unused-symbol / hint
/// findings). A semantic-only path drops parse errors and suggestions, leaving
/// the tsserver-family providers behind the native experience (and behind TSGO,
/// whose pull-diagnostics model already returns the full set). This shared helper
/// is the single merge point both [`TsserverTypeProvider`] and the extension
/// provider route through (one shared owner, not a per-provider fork).
///
/// All three passes return the SAME `parse_tsserver_diagnostic`-shaped value, so
/// the merge is provider-neutral. Order is semantic → syntactic → suggestion.
/// Duplicates (a diagnostic reported by more than one pass) collapse on the full
/// identity `(start, end, code, message)` — a same-span finding with a different
/// code or message is a DISTINCT diagnostic and is preserved.
///
/// The dedup key deliberately EXCLUDES editor tags (`reportsUnnecessary` /
/// `reportsDeprecated`), because the same finding can be reported once tagged and
/// once untagged across two passes. To keep the user-visible fade / strikethrough
/// regardless of pass ordering, a duplicate UNIONS its tags onto the already-kept
/// diagnostic instead of being discarded outright — so a tagless-then-tagged (or
/// tagged-then-tagless) ordering never loses the tag.
pub fn merge_diagnostic_sets(
    semantic: Vec<TypeDiagnostic>,
    syntactic: Vec<TypeDiagnostic>,
    suggestion: Vec<TypeDiagnostic>,
) -> Vec<TypeDiagnostic> {
    // Map the dedup identity to the index of the kept diagnostic so a later
    // duplicate can union its tags onto the survivor.
    let mut seen: HashMap<(u32, u32, Option<String>, String), usize> = HashMap::new();
    let mut merged: Vec<TypeDiagnostic> =
        Vec::with_capacity(semantic.len() + syntactic.len() + suggestion.len());
    for diag in semantic.into_iter().chain(syntactic).chain(suggestion) {
        let key = (
            diag.start,
            diag.end,
            diag.code.clone(),
            diag.message.clone(),
        );
        match seen.get(&key) {
            Some(&idx) => {
                // Same finding from another pass: keep the first occurrence but
                // union any tags the duplicate carries (union, never duplicate).
                for tag in diag.tags {
                    if !merged[idx].tags.contains(&tag) {
                        merged[idx].tags.push(tag);
                    }
                }
            }
            None => {
                seen.insert(key, merged.len());
                merged.push(diag);
            }
        }
    }
    merged
}

/// Parse a `*DiagnosticsSync` response body into a `TypeDiagnostic` vec.
///
/// All three tsserver diagnostic-pull commands (`semanticDiagnosticsSync`,
/// `syntacticDiagnosticsSync`, `suggestionDiagnosticsSync`) return an array of
/// the same diagnostic shape, so a single parser serves them all.
fn parse_tsserver_diagnostics_body(
    body: &serde_json::Value,
    content: Option<&str>,
) -> Vec<TypeDiagnostic> {
    body.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|d| parse_tsserver_diagnostic(d, content))
                .collect()
        })
        .unwrap_or_default()
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

/// Convert tsserver's 1-based (line, offset) to a byte offset, returning `None` when the position
/// is OUT OF RANGE for `content` instead of clamping it to EOF.
///
/// The shared codec ([`line_column_to_offset_utf16`]) fails OPEN: a past-EOF line or a column past
/// the line's end is silently clamped to a valid-looking offset (`content.len()` / the line end).
/// That is acceptable for a navigation sentinel, but for an EDIT a clamped wrong offset corrupts
/// the file — so the edit path validates the position is real and DROPS it otherwise. The check is
/// EDIT-PATH-LOCAL: it does not change the shared codec.
///
/// Validates against the content's own UTF-16 [`LineIndex`]: the 1-based line must exist, and the
/// 0-based UTF-16 column must not exceed that line's UTF-16 length (a column AT the line end is in
/// range; past it is not).
fn tsserver_pos_to_byte_offset_checked(content: &str, line: u32, offset: u32) -> Option<u32> {
    let line0 = line.checked_sub(1)?; // 1-based → 0-based; line 0 is malformed
    let col0 = offset.checked_sub(1)?; // 1-based → 0-based; offset 0 is malformed
    let idx = crate::codec::LineIndex::new(content, crate::codec::PositionEncoding::Utf16);
    if line0 as usize >= idx.line_count() {
        return None; // past-EOF line
    }
    // The line's UTF-16 width: bytes from this line's start to the next line's start (or EOF),
    // measured in the same UTF-16 space tsserver columns use. A column past it would clamp.
    let line_start = idx.line_start(line0 as usize)?;
    let line_end = idx.line_end(line0 as usize)?; // before the newline / EOF
    let line_text = content.get(line_start as usize..line_end as usize)?;
    let line_utf16_len: u32 = line_text.encode_utf16().count() as u32;
    if col0 > line_utf16_len {
        return None; // column past the line end
    }
    let target = crate::codec::LineColumn {
        line: line0,
        character: col0,
    };
    let offset = idx.position_to_offset(target)?;
    // A column landing between the two halves of an astral (surrogate-pair) character is not a
    // UTF-16 scalar boundary; the codec rounds it to an adjacent character, yielding an offset that
    // does NOT map back to the requested column. Require the round-trip to be exact so an EDIT is
    // only accepted at a real boundary; drop it otherwise.
    if idx.offset_to_position(offset)? != target {
        return None;
    }
    Some(offset)
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
    let ws_root = verter_span::path::canonicalize_path(workspace_root);
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

#[cfg(test)]
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

    /// Normalize a file path for tsserver (canonical forward-slash form).
    fn normalize_path(path: &str) -> String {
        verter_span::path::canonicalize_path(path)
    }

    /// Find the best project root for a file path (longest directory-boundary
    /// match). Falls back to the global `workspace_root` if none match.
    fn project_root_for(&self, file: &str) -> String {
        let roots = self.project_roots.read();
        verter_span::path::longest_project_root(file, &roots, &self.workspace_root).to_string()
    }
}

impl TypeProvider for TsserverTypeProvider {
    fn provider_id(&self) -> &'static str {
        "tsserver"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let content = content.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        let project_root = self.project_root_for(&file);
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsserver_open_file",
                format!(
                    "file={} content_len={} project_root={}",
                    file,
                    content.len(),
                    project_root,
                ),
                async {
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
                }
            )
            .await
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
            crate::type_runtime_trace_scope_async!(
                "tsserver_load_file",
                format!("file={} content_len={}", file, content.len()),
                async {
                    contents_cache.lock().await.insert(file, content);
                    crate::type_runtime_trace_event!(
                        "tsserver_load_file_result",
                        "cached_only=true".to_string()
                    );
                    Ok(())
                }
            )
            .await
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
            crate::type_runtime_trace_scope_async!(
                "tsserver_update_file",
                format!(
                    "file={} content_len={} project_root={}",
                    file,
                    content.len(),
                    project_root,
                ),
                async {
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
                                format!(
                                    "file={} mode=update_open old_line_count={}",
                                    file, end_line
                                ),
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
                }
            )
            .await
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsserver_close_file",
                format!("file={}", file),
                async {
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
                }
            )
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
                .map(|arr| {
                    arr.iter()
                        .filter_map(parse_tsserver_completion)
                        .map(|item| stamp_tsserver_completion_offset(item, offset))
                        .collect()
                })
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
            crate::type_runtime_trace_scope_async!(
                "tsserver_get_hover",
                format!(
                    "file={} offset={} line={} col={} content_cache_hit={}",
                    file, offset, line, col, cache_hit,
                ),
                async {
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
                }
            )
            .await
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
            crate::type_runtime_trace_scope_async!(
                "tsserver_get_completion_details",
                format!(
                    "file={} offset={} line={} col={} item_count={}",
                    file,
                    offset,
                    line,
                    col,
                    items.len(),
                ),
                async {
                    let entry_names: Vec<_> = items
                        .iter()
                        .map(build_completion_entry_details_request)
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
                                format!(
                                    "file={} item_count={} enriched=true",
                                    file,
                                    enriched.len()
                                ),
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
                }
            )
            .await
        })
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            // tsserver resolves through `completionEntryDetails`. A non-tsserver
            // resolve key cannot have originated here — fail closed.
            let CompletionResolveData::TsserverEntry {
                name,
                source,
                data,
                offset,
            } = data
            else {
                return Ok(None);
            };

            // Re-issue `completionEntryDetails` at the SAME completion-site
            // position the entry came from; tsserver keys the entry's auto-import
            // `codeActions` on (position, name, source/data).
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let entry = build_entry_names_entry(&name, source.as_deref(), data.as_ref());

            let result = transport
                .request(
                    "completionEntryDetails",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                        "entryNames": [entry],
                    }),
                )
                .await?;

            let Some(detail) = result.as_array().and_then(|arr| arr.first()) else {
                return Ok(None);
            };
            let contents_cache = contents_cache.lock().await.clone();
            Ok(completion_entry_details_to_resolve_result(
                detail,
                &file,
                &contents_cache,
            ))
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

            // Pull all three tsserver diagnostic passes synchronously and union
            // them: SEMANTIC (type errors), SYNTACTIC (parse errors), and
            // SUGGESTION (unused-symbol / hint findings). A semantic-only request
            // would drop parse errors and suggestions that the native TS
            // experience (and TSGO's pull model) surface — the tsserver-family
            // parity gap (GAP-2). The semantic pass is authoritative for the
            // success/fallback decision; syntactic/suggestion failures degrade to
            // an empty set for that category rather than failing the whole pull.
            let semantic_result = transport
                .request(
                    "semanticDiagnosticsSync",
                    serde_json::json!({ "file": file }),
                )
                .await;

            match semantic_result {
                Ok(semantic_body) => {
                    let semantic =
                        parse_tsserver_diagnostics_body(&semantic_body, content.as_deref());

                    let syntactic = transport
                        .request(
                            "syntacticDiagnosticsSync",
                            serde_json::json!({ "file": file }),
                        )
                        .await
                        .ok()
                        .map(|body| parse_tsserver_diagnostics_body(&body, content.as_deref()))
                        .unwrap_or_default();

                    let suggestion = transport
                        .request(
                            "suggestionDiagnosticsSync",
                            serde_json::json!({ "file": file }),
                        )
                        .await
                        .ok()
                        .map(|body| parse_tsserver_diagnostics_body(&body, content.as_deref()))
                        .unwrap_or_default();

                    let diags = merge_diagnostic_sets(semantic, syntactic, suggestion);
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

            // Snapshot the contents cache, then RELEASE the async mutex BEFORE parsing: the
            // per-target parse runs a blocking `std::fs::read_to_string` disk fallback, and a
            // multi-file rename could stall the provider if that disk I/O ran under the lock.
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                guard.clone()
            };
            let locs = {
                // Bind a `Copy` `&HashMap` for the per-target closures; the lock is already dropped,
                // so the disk fallback inside the parser runs unlocked.
                let cache: &HashMap<String, String> = &cache_snapshot;
                result
                    .get("locs")
                    .and_then(|v| v.as_array())
                    .map(|groups| {
                        groups
                            .iter()
                            .flat_map(|group| {
                                let file_path = verter_span::path::canonicalize_path(
                                    group
                                        .get("file")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default(),
                                );
                                group
                                    .get("locs")
                                    .and_then(|v| v.as_array())
                                    .into_iter()
                                    .flat_map(move |spans| {
                                        let fp = file_path.clone();
                                        spans.iter().filter_map(move |span| {
                                            parse_tsserver_rename_span(span, &fp, cache)
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
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        // tsserver's `getCodeFixes` keys fixes off the diagnostic error codes in
        // the requested range. With no numeric codes there is nothing to fix, so
        // short-circuit rather than issue a useless round-trip.
        let error_codes = dedup_error_codes(diagnostics);
        Box::pin(async move {
            if error_codes.is_empty() {
                return Ok(vec![]);
            }
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
                        "errorCodes": error_codes,
                    }),
                )
                .await;

            let raw_fixes = match result {
                Ok(body) => body.as_array().cloned().unwrap_or_default(),
                Err(_) => return Ok(vec![]),
            };

            // Snapshot the contents cache, then RELEASE the async mutex BEFORE parsing: each edit's
            // parse runs a blocking `std::fs::read_to_string` disk fallback, and a fix-all touching
            // many files could stall the provider if that disk I/O ran under the lock.
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                guard.clone()
            };

            // Single-fix actions first, then their combined "fix all" companions —
            // a stable order independent of provider response ordering.
            let mut actions: Vec<TypeCodeAction> = raw_fixes
                .iter()
                .filter_map(|a| parse_tsserver_code_action(a, &cache_snapshot))
                .collect();

            // Any fix carrying a `fixId` is combinable: tsserver exposes a
            // `getCombinedCodeFix` companion that applies the fix across the whole
            // file (e.g. "Delete all unused declarations" for TS6133). Follow each
            // DISTINCT `fixId` once, titled from the fix's own `fixAllDescription`
            // — the combinability decision is the typed `fixId` field, never a
            // title-string match.
            let mut combined: Vec<TypeCodeAction> = Vec::new();
            let mut seen_fix_ids: HashSet<String> = HashSet::new();
            for fix in &raw_fixes {
                let Some(fix_id) = fix.get("fixId").and_then(|v| v.as_str()) else {
                    continue;
                };
                if fix_id.is_empty() || !seen_fix_ids.insert(fix_id.to_string()) {
                    continue;
                }
                let fix_all_title = fix
                    .get("fixAllDescription")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let combined_result = transport
                    .request("getCombinedCodeFix", combined_code_fix_args(&file, fix_id))
                    .await;
                if let Ok(body) = combined_result {
                    // Re-snapshot fresh (the request may have synced new files) and RELEASE the lock
                    // before parsing — the parse runs a blocking disk fallback per edit.
                    let cache = {
                        let guard = contents_cache.lock().await;
                        guard.clone()
                    };
                    if let Some(action) =
                        parse_tsserver_combined_code_fix(&body, fix_all_title.as_deref(), &cache)
                    {
                        combined.push(action);
                    }
                }
            }

            actions.extend(combined);
            Ok(actions)
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
                    let canonical =
                        verter_span::path::canonicalize_path(&crate::uri::file_uri_to_path(uri));
                    roots.retain(|r| r != &canonical);
                }
            }

            // Add new folders
            for folder in &added {
                if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                    let canonical =
                        verter_span::path::canonicalize_path(&crate::uri::file_uri_to_path(uri));
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
                    verter_span::path::longest_project_root(file, &roots, &workspace_root)
                        .to_string()
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

    // Preserve the tsserver resolve handle: the entry's `name` plus the
    // `source`/`data` an external-module (auto-import) entry carries. Hard-coding
    // `data: None` here was the root cause of broken auto-import — without the
    // handle the LSP could never re-issue `completionEntryDetails`. The
    // completion-site `offset` is stamped by `get_completions` (it is identical
    // for every entry in one request and not visible at the per-entry level).
    let source = item
        .get("source")
        .and_then(|v| v.as_str())
        .map(String::from);
    let resolve_data = item.get("data").filter(|d| !d.is_null()).cloned();
    let data = Some(CompletionResolveData::TsserverEntry {
        name: name.clone(),
        source,
        data: resolve_data,
        offset: 0,
    });

    Some(Completion {
        label: name,
        kind,
        detail: None,
        documentation: None,
        edit_range_start: None,
        edit_range_end: None,
        insert_text,
        sort_text,
        data,
    })
}

/// Stamp the completion-site `offset` onto a freshly-parsed tsserver-family
/// completion's resolve handle.
///
/// `parse_tsserver_completion` runs per entry and cannot see the request
/// position; the offset is identical for every entry in one `completionInfo`
/// request, so both the tsserver and extension `get_completions` apply it here.
/// `completionItem/resolve` later re-issues `completionEntryDetails` at this
/// offset. Items without a tsserver resolve handle pass through unchanged.
pub fn stamp_tsserver_completion_offset(mut item: Completion, request_offset: u32) -> Completion {
    if let Some(CompletionResolveData::TsserverEntry { offset, .. }) = item.data.as_mut() {
        *offset = request_offset;
    }
    item
}

/// Build one `completionEntryDetails` `entryNames` entry from a completion's
/// typed resolve handle.
///
/// tsserver keys an entry's auto-import `codeActions` on `(name, source, data)` —
/// an external-module (auto-import) entry resolves against a DIFFERENT module
/// than a local member, so the `source`/`data` recovered from the entry's
/// [`CompletionResolveData::TsserverEntry`] handle MUST be forwarded. An item
/// with no tsserver handle (or a non-tsserver one) degrades to a bare `{ name }`
/// keyed on the label.
///
/// Shared by the tsserver and extension `get_completion_details` paths so they
/// build byte-identical detail requests (review finding H4 — the tsserver path
/// previously sent `{ name }` only, dropping the auto-import keys the extension
/// path forwarded).
pub fn build_completion_entry_details_request(item: &Completion) -> serde_json::Value {
    match &item.data {
        Some(CompletionResolveData::TsserverEntry {
            name, source, data, ..
        }) => build_entry_names_entry(name, source.as_deref(), data.as_ref()),
        _ => serde_json::json!({ "name": item.label }),
    }
}

/// Build one `completionEntryDetails` `entryNames` entry from a resolve key's
/// fields. Shared by the tsserver and extension `resolve_completion` paths so the
/// single-entry resolve request is built identically across providers.
pub fn build_entry_names_entry(
    name: &str,
    source: Option<&str>,
    data: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut entry = serde_json::json!({ "name": name });
    if let Some(source) = source {
        entry["source"] = serde_json::Value::String(source.to_string());
    }
    if let Some(data) = data {
        entry["data"] = data.clone();
    }
    entry
}

/// Shared tsserver-family `completionEntryDetails` enrichment.
///
/// Folds the resolved `displayParts` (detail) and combined documentation/tags
/// onto an item WITHOUT discarding its resolve handle, so a lazily-enriched item
/// can still be resolved for auto-import. Used by both the tsserver and
/// extension `get_completion_details` paths.
pub fn enrich_completion_with_entry_details(
    item: &Completion,
    detail: &serde_json::Value,
) -> Completion {
    enrich_tsserver_completion(item, detail)
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
    let file = verter_span::path::canonicalize_path(
        loc.get("file").and_then(|v| v.as_str()).unwrap_or_default(),
    );
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
/// A tsserver rename response groups spans by file, so each span's REAL byte offset is into the
/// GROUP's `file` — which may be a cross-file rename target the queried session never opened
/// (e.g. an imported component's carrier or a `.ts` declaration). Resolve each span against THAT
/// file's own content: the in-memory `contents_cache` first, then a per-target disk read on a
/// cache miss — the SAME content-resolution [`parse_tsserver_location`] gives references /
/// definition, and the tsgo rename path gives via `parse_range_to_offsets_with_disk_fallback`.
///
/// The disk fallback recovers a cross-file target absent from the cache, so its rename edit lands
/// at the real range instead of being dropped. FAIL CLOSED otherwise: when NEITHER cache nor disk
/// has the content the span is DROPPED (returns `None`) — a rename location is a WRITE edit, so a
/// packed `(line << 16) | col` sentinel applied at a bogus byte offset would CORRUPT the file. An
/// out-of-range position (the shared codec would clamp it to EOF) and an inverted `start > end`
/// span also drop. The caller collects via `filter_map`, so one dropped span never aborts the
/// whole rename.
pub fn parse_tsserver_rename_span(
    span: &serde_json::Value,
    file: &str,
    contents_cache: &HashMap<String, String>,
) -> Option<RenameLocation> {
    let start = span.get("start")?;
    let end = span.get("end")?;
    let sl = start.get("line")?.as_u64()? as u32;
    let so = start.get("offset")?.as_u64()? as u32;
    let el = end.get("line")?.as_u64()? as u32;
    let eo = end.get("offset")?.as_u64()? as u32;

    let disk_content;
    let content = if let Some(content) = contents_cache.get(file) {
        Some(content.as_str())
    } else {
        disk_content = std::fs::read_to_string(file).ok();
        disk_content.as_deref()
    };

    // FAIL CLOSED: a rename location is a WRITE edit — same corruption class as a code edit. When the
    // target content is unavailable (cache miss AND disk read fails) DROP the span — never pack a
    // `(line << 16) | col` sentinel the merge layer would apply at a bogus byte offset and corrupt
    // the file. The checked converter additionally drops an out-of-range position (the shared codec
    // would clamp it to a valid-looking EOF offset), and an inverted `start > end` span drops too.
    // The caller collects via `filter_map`, so a dropped span skips that one location, not the
    // whole rename.
    let c = content?;
    let s = tsserver_pos_to_byte_offset_checked(c, sl, so)?;
    let e = tsserver_pos_to_byte_offset_checked(c, el, eo)?;
    if s > e {
        return None;
    }

    Some(RenameLocation {
        path: file.to_string(),
        start: s,
        end: e,
    })
}

/// Sorted, de-duplicated integer error codes from the request's diagnostics.
///
/// tsserver's `getCodeFixes` keys fixes off the diagnostic error codes present in
/// the requested range; the same code may appear on several diagnostics, so it is
/// deduped to one entry. A stable sort keeps the request shape deterministic.
pub fn dedup_error_codes(diagnostics: &[ProviderDiagnosticContext]) -> Vec<u32> {
    let mut codes: Vec<u32> = diagnostics.iter().map(|d| d.code).collect();
    codes.sort_unstable();
    codes.dedup();
    codes
}

/// Build the `getCombinedCodeFix` request args for a combinable `fixId` scoped to
/// a single file. Shared by the out-of-process tsserver provider and the
/// in-process extension provider so neither hand-rolls the scope shape.
pub fn combined_code_fix_args(file: &str, fix_id: &str) -> serde_json::Value {
    serde_json::json!({
        "scope": { "type": "file", "args": { "file": file } },
        "fixId": fix_id,
    })
}

/// Parse the `changes` array shared by `getCodeFixes` items and
/// `getCombinedCodeFix` responses into byte-offset [`TypeCodeEdit`]s.
///
/// Resolves each edit's 1-based tsserver position against ITS OWN target file's content: the
/// in-memory `contents_cache` first, then the file's on-disk content as a per-target fallback (the
/// same content resolution the rename/location paths use). FAIL CLOSED: when neither yields the
/// target's content, the edit is DROPPED — a wrong-location edit corrupts the file, so unlike the
/// rename/location paths the EDIT path emits no packed line:col sentinel. Propagates `None` on a
/// malformed `textChanges` entry.
fn parse_tsserver_file_code_edits(
    changes: &[serde_json::Value],
    contents_cache: &HashMap<String, String>,
) -> Option<Vec<TypeCodeEdit>> {
    let mut edits = Vec::new();
    for change in changes {
        let file = verter_span::path::canonicalize_path(
            change
                .get("fileName")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        );
        let disk_content;
        let content = if let Some(content) = contents_cache.get(&file) {
            Some(content.as_str())
        } else {
            disk_content = std::fs::read_to_string(&file).ok();
            disk_content.as_deref()
        };
        if let Some(text_changes) = change.get("textChanges").and_then(|v| v.as_array()) {
            for tc in text_changes {
                let start = tc.get("start")?;
                let end = tc.get("end")?;
                let new_text = tc.get("newText")?.as_str()?.to_string();
                let sl = start.get("line")?.as_u64()? as u32;
                let so = start.get("offset")?.as_u64()? as u32;
                let el = end.get("line")?.as_u64()? as u32;
                let eo = end.get("offset")?.as_u64()? as u32;

                // FAIL CLOSED: no content for this target → DROP the edit (never a packed sentinel
                // that would write at a bogus byte offset).
                let Some(c) = content else {
                    continue;
                };
                // FAIL CLOSED on an OUT-OF-RANGE position: the shared codec clamps a past-EOF
                // line/col to a valid-looking offset, which for an EDIT would corrupt the file. The
                // checked converter drops it instead. A malformed `start > end` also drops.
                let (Some(s), Some(e)) = (
                    tsserver_pos_to_byte_offset_checked(c, sl, so),
                    tsserver_pos_to_byte_offset_checked(c, el, eo),
                ) else {
                    continue;
                };
                if s > e {
                    continue;
                }

                edits.push(TypeCodeEdit {
                    path: file.clone(),
                    start: s,
                    end: e,
                    new_text,
                });
            }
        }
    }
    Some(edits)
}

/// Parse a tsserver code action / code fix.
///
/// Each edit's 1-based tsserver positions convert to byte offsets against the edit's own target
/// content (cache → disk). FAIL CLOSED: an edit whose target content is unavailable, or whose
/// position is out of range for that content, is DROPPED — the EDIT path emits NO packed line:col
/// sentinel (a wrong-offset edit corrupts the file). An action whose edits all drop is dropped.
pub fn parse_tsserver_code_action(
    action: &serde_json::Value,
    contents_cache: &HashMap<String, String>,
) -> Option<TypeCodeAction> {
    let description = action.get("description")?.as_str()?.to_string();
    let changes = action.get("changes")?.as_array()?;
    let edits = parse_tsserver_file_code_edits(changes, contents_cache)?;
    // An edit-less single fix is not actionable — drop it, mirroring the
    // combined-fix path (`parse_tsserver_combined_code_fix`). The merge layer
    // already discards empty-change actions, so this only makes the two parsers
    // symmetric (no edit-less action ever leaves the parse boundary).
    if edits.is_empty() {
        return None;
    }

    Some(TypeCodeAction {
        title: description,
        kind: Some("quickfix".to_string()),
        edits,
    })
}

/// Parse a `getCombinedCodeFix` response (`CombinedCodeActions { changes }`) into
/// a single "fix all" code action.
///
/// The combined response carries only the file edits; the user-facing title comes
/// from the originating fix's `fixAllDescription` (e.g. "Delete all unused
/// declarations"). When that title is absent the action is dropped — an untitled
/// fix-all is not surfaced.
pub fn parse_tsserver_combined_code_fix(
    body: &serde_json::Value,
    fix_all_title: Option<&str>,
    contents_cache: &HashMap<String, String>,
) -> Option<TypeCodeAction> {
    let title = fix_all_title?.to_string();
    let changes = body.get("changes")?.as_array()?;
    let edits = parse_tsserver_file_code_edits(changes, contents_cache)?;
    if edits.is_empty() {
        return None;
    }
    Some(TypeCodeAction {
        title,
        kind: Some("quickfix".to_string()),
        edits,
    })
}

/// Map a single `completionEntryDetails` entry into a [`CompletionResolveResult`].
///
/// This is the SHARED tsserver-family resolve mapping — used by both the
/// out-of-process tsserver provider and the in-process extension provider, so
/// neither carries its own copy of the `codeActions → byte edits` logic.
///
/// The tsserver `completionEntryDetails` response for an auto-importable entry
/// carries `codeActions: [{ description, changes: [{ fileName, textChanges }] }]`
/// (the auto-import insertion) alongside `displayParts`/`documentation`. We:
///
/// * fold every code action's `textChanges` that target `target_file` into
///   ordered [`ResolvedTextEdit`]s (generated-file byte offsets), reusing
///   [`parse_tsserver_code_action`]. Cross-file edits are dropped here — the LSP
///   carrier re-anchor maps the generated-TSX edits back to the `.vue` source;
/// * surface `displayParts`→`detail` and the combined documentation/tags so the
///   lazy resolve also enriches the item's hover text.
///
/// Returns `None` when the entry yields neither edits nor enrichment, so the
/// caller can treat "nothing to resolve" uniformly.
pub fn completion_entry_details_to_resolve_result(
    detail: &serde_json::Value,
    target_file: &str,
    contents_cache: &HashMap<String, String>,
) -> Option<CompletionResolveResult> {
    let canonical_target = verter_span::path::canonicalize_path(target_file);

    let mut additional_text_edits = Vec::new();
    if let Some(code_actions) = detail.get("codeActions").and_then(|v| v.as_array()) {
        for action in code_actions {
            let Some(parsed) = parse_tsserver_code_action(action, contents_cache) else {
                continue;
            };
            for edit in parsed.edits {
                // Same-file edits only: the generated-TSX file the completion was
                // requested in. The LSP carrier re-anchor owns the
                // generated-TSX → `.vue` mapping; cross-file edits (an import
                // added to a different module) are not part of the in-carrier
                // auto-import insertion and are dropped here.
                if edit.path == canonical_target {
                    additional_text_edits.push(ResolvedTextEdit {
                        start: edit.start,
                        end: edit.end,
                        new_text: edit.new_text,
                    });
                }
            }
        }
    }

    let display = tsserver_display_parts_text(detail.get("displayParts"));
    let resolved_detail = (!display.is_empty()).then_some(display);
    let resolved_documentation = tsserver_completion_documentation(detail);

    if additional_text_edits.is_empty()
        && resolved_detail.is_none()
        && resolved_documentation.is_none()
    {
        return None;
    }

    Some(CompletionResolveResult {
        additional_text_edits,
        detail: resolved_detail,
        documentation: resolved_documentation,
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
#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;
