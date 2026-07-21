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
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
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
    /// Counts consecutive request timeouts. Reset to 0 on any successful response.
    /// When this reaches `HANG_THRESHOLD`, fires `crash_notify` to trigger a restart
    /// via the `ResilientProvider` crash-recovery machinery — a wedged-but-alive
    /// tsserver (accepts requests, never responds) must be detected and restarted,
    /// not silently time out every request for the rest of the session.
    consecutive_failures: AtomicU32,
    /// Shared with `ResilientProvider` — signaled when the provider appears hung.
    crash_notify: Option<Arc<Notify>>,
    /// Singleflight + cooldown stamp for `reloadProjects` membership recovery.
    /// Under a hover/diagnostics storm, dozens of concurrent cold-miss retries would
    /// each fire `reloadProjects` (a full all-projects rebuild), saturating tsserver.
    /// The stamp coalesces those to at most one reload per cooldown window.
    membership_recovery: Mutex<Option<std::time::Instant>>,
}

/// Number of consecutive request timeouts before the transport signals a hang.
/// Mirrors the tsgo transport's `HANG_THRESHOLD`: when reached, `crash_notify` is
/// fired so the `ResilientProvider` restarts the wedged process (kill, backoff,
/// re-spawn, replay desired state) instead of timing out forever.
const HANG_THRESHOLD: u32 = 3;

/// Minimum interval between `reloadProjects` membership-recovery sends. A cold
/// "Could not find source file" retry loop calls the recovery on every iteration;
/// without a cooldown a storm of concurrent cold queries fires a `reloadProjects`
/// per retry per query. The cooldown is sized to the cost of the operation: a
/// `reloadProjects` is a FULL all-projects rebuild (seconds) that itself drops
/// sibling companions' membership transiently — so each reload breeds the next
/// cold-miss wave. Capping the rate to roughly one rebuild's duration breaks that
/// self-reinforcing cycle: a single in-flight rebuild re-admits EVERY companion in
/// the publish store, so further reloads while one is settling are pure churn.
const MEMBERSHIP_RECOVERY_COOLDOWN: std::time::Duration = std::time::Duration::from_millis(2000);

impl TsserverTransport {
    /// Send a tsserver request and wait for the response.
    async fn request(
        &self,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, TypeProviderError> {
        self.request_with_timeout(command, arguments, std::time::Duration::from_secs(10))
            .await
    }

    /// Send a tsserver request with a custom response timeout. Split from
    /// [`TsserverTransport::request`] so tests can exercise the timeout / hang
    /// detection path without waiting the full production timeout.
    async fn request_with_timeout(
        &self,
        command: &str,
        arguments: serde_json::Value,
        timeout: std::time::Duration,
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

                let result = tokio::time::timeout(timeout, rx).await;
                match result {
                    Ok(Ok(val)) => {
                        // Any response (even a tsserver-level error) proves the process
                        // is alive and answering — reset the hang detector.
                        self.consecutive_failures.store(0, Ordering::Relaxed);
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
                        let count = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
                        if count >= HANG_THRESHOLD {
                            tracing::error!(
                                "tsserver appears hung ({count} consecutive timeouts) — triggering restart"
                            );
                            if let Some(notify) = &self.crash_notify {
                                notify.notify_waiters();
                            }
                        }
                        crate::type_runtime_trace_event!(
                            "tsserver_transport_request_error",
                            format!("command={} seq={} message=timeout", command, seq),
                        );
                        Err(TypeProviderError::new(format!(
                            "request '{command}' timed out after {}s",
                            timeout.as_secs()
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
    contents_cache: Arc<Mutex<HashMap<String, Arc<str>>>>,
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
    contents_cache: &Mutex<HashMap<String, Arc<str>>>,
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
                                .filter_map(|d| {
                                    parse_tsserver_diagnostic(
                                        d,
                                        content.as_deref(),
                                        Some(file.as_str()),
                                    )
                                })
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
    file_path: Option<&str>,
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

    // `relatedInformation` carries the secondary "see declaration here" spans
    // (e.g. duplicate-identifier "also declared here"). Each entry has its own
    // `span` with the related file's own `file`. `parse_tsserver_related_info`
    // keeps ONLY a same-file related span whose content is available AND whose
    // 1-based line/offset is in range — it converts through the CHECKED offset
    // converter and DROPS the entry for a cross-file/no-content span OR an
    // out-of-range same-file position (never stores a packed position, never clamps
    // to EOF). A dropped secondary link beats a bogus one.
    let primary_file = file_path.map(verter_span::path::canonicalize_path);
    let related_information = d
        .get("relatedInformation")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|ri| parse_tsserver_related_info(ri, content, primary_file.as_deref()))
                .collect()
        })
        .unwrap_or_default();

    Some(TypeDiagnostic {
        message: text,
        severity,
        start: so,
        end: eo,
        code,
        tags,
        related_information,
    })
}

/// Parse one tsserver `relatedInformation` entry into a [`DiagnosticRelatedInfo`].
///
/// The entry shape is `{ message, span: { start:{line,offset}, end:{line,offset},
/// file } }`. [`DiagnosticRelatedInfo::start`]/[`DiagnosticRelatedInfo::end`] are
/// REAL byte offsets in `path` — never a packed `(line<<16)|col` position. A real
/// offset is available ONLY when the related `file` is the SAME canonical file the
/// parser holds content for (`primary_file` / `primary_content`); both sides are
/// canonicalized ([`verter_span::path::canonicalize_path`]) before the equality so
/// a same file spelled differently (slashes, drive case, `\\?\`) still matches.
///
/// Returns `None` (skip this entry, never fabricate, never store a packed value)
/// when the message/span fields are missing, when the related span is cross-file
/// (no content for it), OR when a same-file 1-based line/offset is OUT OF RANGE for
/// the content — fail-closed: a dropped secondary link beats a bogus one.
fn parse_tsserver_related_info(
    ri: &serde_json::Value,
    primary_content: Option<&str>,
    primary_file: Option<&str>,
) -> Option<DiagnosticRelatedInfo> {
    let message = ri.get("message")?.as_str()?.to_string();
    let span = ri.get("span")?;
    let start = span.get("start")?;
    let end = span.get("end")?;
    // CHECKED `u64 → u32`: a malformed coordinate larger than `u32::MAX` (e.g.
    // `2^32 + 1`) would WRAP to an in-range 1-based line/offset under a lossy
    // `as u32` cast, then PASS `tsserver_pos_to_byte_offset_checked` (which only
    // rejects line/offset 0 and past-EOF positions), fabricating a valid-looking
    // but WRONG related link. Dropping the whole related entry (fail-closed) on an
    // out-of-u32-range coordinate is the only defense, because the corruption
    // would happen in the cast BEFORE the converter runs.
    let start_line = u32::try_from(start.get("line")?.as_u64()?).ok()?;
    let start_offset = u32::try_from(start.get("offset")?.as_u64()?).ok()?;
    let end_line = u32::try_from(end.get("line")?.as_u64()?).ok()?;
    let end_offset = u32::try_from(end.get("offset")?.as_u64()?).ok()?;
    let file = verter_span::path::canonicalize_path(span.get("file")?.as_str()?);

    // A real byte offset exists only for a same-file related span (the parser holds
    // that file's content). A cross-file span has no content here, so there is no
    // real offset — DROP it rather than store a packed position the merge would
    // mis-read as a byte offset. Both paths are already canonicalized.
    let same_file = primary_file == Some(file.as_str());
    let content = primary_content.filter(|_| same_file)?;
    // Even a same-file related span can be MALFORMED (a 1-based line/offset past
    // EOF). The fail-open `tsserver_pos_to_byte_offset` would CLAMP that to
    // `content.len()` and forge a bogus "see declaration" link at EOF, so the
    // related-info path uses the CHECKED converter and DROPS the entry (returns
    // `None`) when the position is out of range — never clamps. The primary-span
    // path keeps its own clamp/recovery behavior (out of scope here).
    let start_byte = tsserver_pos_to_byte_offset_checked(content, start_line, start_offset)?;
    let end_byte = tsserver_pos_to_byte_offset_checked(content, end_line, end_offset)?;

    Some(DiagnosticRelatedInfo {
        path: file,
        start: start_byte,
        end: end_byte,
        message,
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

/// The tsserver substring that signals the diagnostics file ARGUMENT itself is
/// not (yet) a valid source file in the program. On a cold configured-project
/// build the just-published `.vue.tsx` / `.svelte.tsx` companion is transiently
/// absent from the program tsserver type-checks, so `getValidSourceFile` throws
/// and `semanticDiagnosticsSync` fails the whole command with this message —
/// distinct from a SUCCESS-body `TS2307` ("Cannot find module …") about a user
/// import, which never reaches the transport-error path.
const TSSERVER_SOURCE_FILE_NOT_IN_PROGRAM: &str = "Could not find source file";

/// tsserver's `ThrowNoProject` message — the carrier's owning configured project
/// is not loaded yet, so a `projectFileName`-targeted request misses
/// (`getProject(projectFileName)` is undefined) and falls through to
/// `ensureDefaultProjectForFile`, which throws for a virtual companion that lives
/// on no real-disk path. Recoverable by `reloadProjects` (loads the configured
/// projects from their on-disk tsconfigs).
const TSSERVER_NO_PROJECT: &str = "No Project";

/// Does this transport-error message signal the diagnostics companion is not yet
/// in the program (a transient COLD condition), rather than a terminal failure or
/// a genuine module-not-found the user must see?
///
/// NARROW by construction: matches the two cold-membership throws —
/// `getValidSourceFile` ("Could not find source file": the configured project
/// exists but the companion is not yet a `getExternalFiles` member) and
/// `ThrowNoProject` ("No Project": the carrier's owning configured project is not
/// loaded at all). Both recover via `reloadProjects`. A genuine `TS2307` arrives
/// as a SUCCESS-body diagnostic, so its text never reaches here; transport
/// timeouts and closed channels are distinct terminal strings that must NOT be
/// treated as cold.
fn tsserver_diag_error_is_companion_not_ready(message: &str) -> bool {
    message.contains(TSSERVER_SOURCE_FILE_NOT_IN_PROGRAM) || message.contains(TSSERVER_NO_PROJECT)
}

/// Recover a companion's configured-project membership after a cold "Could not
/// find source file" miss. The caller re-issues its query after this returns.
///
/// The companion's membership is owned by the plugin's `getExternalFiles`, which
/// tsserver consults only when it (re)evaluates project STRUCTURE — re-opening
/// the file alone does not re-query it. `reloadProjects` is the lever that
/// re-invokes `getExternalFiles`, admitting the now-published companion into its
/// configured project.
///
/// This is scoped to the cold-error path ONLY (a warm query never reaches here),
/// so the heavier all-projects reload is paid solely while a freshly built
/// project is still settling, never on a warm pull. Best-effort: a failure is
/// swallowed so a mid-restart provider never turns a cold-recovery touch into a
/// hard error.
async fn recover_companion_membership(transport: &TsserverTransport) {
    // Singleflight + cooldown: under a hover/diagnostics storm dozens of concurrent
    // cold-miss retries reach here together. Without a gate each would fire its own
    // `reloadProjects` (a full all-projects rebuild), stampeding tsserver. Stamp the
    // send under the lock (released before the network send) so at most one reload is
    // issued per cooldown window across ALL concurrent queries; the cold retry loops
    // keep re-querying and observe the first reload's effect.
    {
        let mut last = transport.membership_recovery.lock().await;
        if let Some(last_fired) = *last {
            if last_fired.elapsed() < MEMBERSHIP_RECOVERY_COOLDOWN {
                return;
            }
        }
        *last = Some(std::time::Instant::now());
    }
    let _ = transport
        .command_no_response("reloadProjects", serde_json::json!({}))
        .await;
}

/// Parse a `*DiagnosticsSync` response body into a `TypeDiagnostic` vec.
///
/// All three tsserver diagnostic-pull commands (`semanticDiagnosticsSync`,
/// `syntacticDiagnosticsSync`, `suggestionDiagnosticsSync`) return an array of
/// the same diagnostic shape, so a single parser serves them all.
fn parse_tsserver_diagnostics_body(
    body: &serde_json::Value,
    content: Option<&str>,
    file_path: Option<&str>,
) -> Vec<TypeDiagnostic> {
    body.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|d| parse_tsserver_diagnostic(d, content, file_path))
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

/// How a tracked open file was opened — the discriminant a resync replays on.
///
/// A `Source` file (a real `.ts`/`.tsx` or an editor-open buffer) is reopened
/// WITH its `fileContent`: tsserver IS its content authority. A `CarrierCompanion`
/// (a published `{name}.vue.tsx` / `{name}.vue.verter.ts`) is reopened
/// CONTENTLESSLY — the `@verter/typescript-plugin`'s `getScriptSnapshot` is the
/// SOLE engine-side content authority, so a resync must never resend its bytes
/// (which would convert it back into a tsserver-owned content buffer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenKind {
    Source,
    CarrierCompanion,
}

/// A `TypeProvider` backed by a tsserver process (`node tsserver.js`).
pub struct TsserverTypeProvider {
    transport: Arc<TsserverTransport>,
    /// tsserver child process. Killed on drop.
    child: Child,
    /// Cached file contents for position conversion.
    contents: Arc<Mutex<HashMap<String, Arc<str>>>>,
    /// Files that have been sent to tsserver via `open` command, tagged by
    /// [`OpenKind`] so a resync replays a source WITH content but a carrier
    /// companion CONTENTLESSLY. Used by `update_file` to decide between `open` vs
    /// `updateOpen`. `load_file` adds to `contents` but NOT to `opened_files`.
    opened_files: Arc<Mutex<HashMap<String, OpenKind>>>,
    /// Cached diagnostics from event notifications.
    diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>>,
    /// Workspace root path (forward slashes) for `projectRootPath` in open commands.
    workspace_root: String,
    /// Per-project roots for per-file `projectRootPath` matching.
    /// Sorted by length descending (longest prefix first).
    /// When non-empty, per-file matching takes priority over the global `workspace_root`.
    project_roots: Arc<parking_lot::RwLock<Vec<String>>>,
    /// Published-carrier companion path → owning configured-project tsconfig path.
    /// Populated by [`TypeProvider::register_carrier_member`] from the LSP publish
    /// path's resolved `ProjectBinding`. A carrier query (diagnostics / definition /
    /// hover / completion) looks the companion up here and passes the owning
    /// tsconfig as `projectFileName`, so the companion is type-checked in its REAL
    /// configured project (where `getExternalFiles` admitted it) instead of a fresh
    /// inferred/default project that would return empty/wrong results.
    carrier_projects: Arc<parking_lot::RwLock<HashMap<String, String>>>,
    /// Published companion path -> configured Program source identity.
    ///
    /// Non-editor plugin projects admit generated carrier content under the
    /// authored source path. Diagnostic protocol requests must therefore name
    /// that source while positions continue to decode against the companion bytes
    /// cached locally. This map is routing metadata only: the source is never
    /// opened by this provider, so the plugin remains the sole content authority.
    carrier_sources: Arc<parking_lot::RwLock<HashMap<String, String>>>,
    /// Per-file content generation: a globally-unique, monotonically-increasing
    /// stamp written in lockstep with every `contents` write (open / load /
    /// update / carrier register) and dropped on close. A resync captures each
    /// file's generation alongside its content snapshot and re-checks it
    /// immediately before the reopen send; if a concurrent `update_file` has
    /// stamped a newer value — or a close dropped it — the resync SKIPS the
    /// now-stale reopen (the update already pushed the newer bytes), so a resync
    /// can never reopen a source with bytes a concurrent edit has already
    /// superseded. Because each stamp is drawn from a single process-monotonic
    /// counter, a reopen of a since-closed path receives a FRESH value rather
    /// than a recycled per-file count, so a stale captured generation can never
    /// alias a reopened file (no ABA). Guarded by a synchronous lock taken only
    /// while the async `contents` guard is held, so the `(content, generation)`
    /// pair is consistent and no lock spans an `.await`.
    content_generations: Arc<ContentGenerations>,
    /// Monotonic publication token delivered to the Verter tsserver plugin on
    /// every carrier-store advance. The plugin uses token changes to reload only
    /// ScriptInfos whose manifest identity changed; an `updateOpen` touch alone
    /// does not replace an already-warm external-file snapshot.
    carrier_store_refresh_generation: AtomicU64,
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

    // Tell tsserver to accept framework-carrier SOURCE extensions (`.vue`/
    // `.svelte`) as program members so a `getExternalFiles`-advertised carrier
    // source (served the generated TSX by the `@verter/typescript-plugin` host
    // hooks) enters its configured project's Program. The extensions are derived
    // from the shared language registry (framework-agnostic — a new carrier
    // participates automatically); each is `scriptKind: TSX` (TypeScript value 4),
    // `isMixedContent: false` (the plugin serves the full generated TSX, not the
    // raw carrier text tsserver would otherwise try to scan).
    const TS_SCRIPT_KIND_TSX: u8 = 4;
    let extra_file_extensions: Vec<serde_json::Value> = verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .into_iter()
        .map(|ext| {
            serde_json::json!({
                "extension": ext,
                "isMixedContent": false,
                "scriptKind": TS_SCRIPT_KIND_TSX,
            })
        })
        .collect();

    transport
        .request(
            "configure",
            serde_json::json!({
                "hostInfo": "verter-lsp",
                "extraFileExtensions": extra_file_extensions,
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

    // A framework carrier is a member of its REAL configured project (the
    // `@verter/typescript-plugin` makes it one via `getExternalFiles` +
    // `extraFileExtensions`), so the carrier sees the project's own
    // `paths`/`baseUrl`/`types`/`lib`/`jsx`/`moduleResolution`/references. The
    // session therefore injects NO inferred-project compiler options — there is no
    // config-less inferred carrier to configure.
    Ok(ws_root)
}

/// The tsserver CLI args that load `@verter/typescript-plugin` as a global
/// language-service plugin from `plugin_path`. The plugin is what makes a
/// framework carrier a member of its configured project (`getExternalFiles` +
/// `extraFileExtensions`), so loading it is the load-bearing half of the
/// project-bound contract. Empty when no plugin probe location was supplied.
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
    /// `plugin_path`: the directory containing `@verter/typescript-plugin`. When
    /// `Some`, the plugin is loaded as a tsserver global language-service plugin
    /// (`--globalPlugins @verter/typescript-plugin --pluginProbeLocations <path>
    /// --allowLocalPluginLoads`). The plugin is what makes a framework carrier a
    /// member of its configured project, so loading it is required for
    /// project-bound carrier membership.
    /// `carrier_store_dir`: the resolved per-workspace carrier-publish store dir
    /// the Rust LSP publishes carriers into. When `Some`, it is delivered to the
    /// plugin via the `VERTER_CARRIER_STORE_DIR` environment variable so the
    /// plugin reads the SAME store the LSP writes. The caller (the LSP) computes
    /// this from its shared publish store so the two agree.
    /// `plugin_response_remap`: whether the plugin should map carrier-companion
    /// RESPONSES (definition/references/rename/code-action edits/completion-detail
    /// edits) back to `.vue`/`.svelte` source. This is the verter_lsp-INTERNAL
    /// backend, where the Rust `verter_lsp` merge layer is the SOLE response
    /// mapper — so production callers pass `false` (the plugin returns RAW
    /// companion responses and the Rust layer maps, with no double-mapping). The
    /// VS Code DIRECT surface (the editor's own TS server + the plugin, no
    /// verter_lsp in the response path) leaves it `true`, where the plugin IS the
    /// only mapper; that surface is represented by a test that passes `true`.
    pub async fn spawn(
        node_path: &str,
        tsserver_path: &str,
        workspace_root: &str,
        plugin_path: Option<&str>,
        carrier_store_dir: Option<&str>,
        plugin_response_remap: bool,
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

        // Load `@verter/typescript-plugin` so carriers become configured-project
        // members. The plugin reads the carrier-publish store synchronously.
        for plugin_arg in tsserver_plugin_args(plugin_path) {
            cmd.arg(plugin_arg);
        }

        // Deliver the carrier-publish store dir to the plugin. The plugin reads
        // it from `VERTER_CARRIER_STORE_DIR` (its config-key fallback); the LSP
        // computes the SAME dir from its shared publish store, so the plugin reads
        // exactly the bytes the LSP wrote.
        if let Some(store_dir) = carrier_store_dir.filter(|d| !d.is_empty()) {
            cmd.env("VERTER_CARRIER_STORE_DIR", store_dir);
        }

        // Gate the plugin's companion→source RESPONSE remap by surface. On the
        // verter_lsp-internal backend (`plugin_response_remap == false`, the
        // production default) the Rust `verter_lsp` merge layer is the SOLE
        // response mapper — it owns the authoritative position mapper, strict
        // offset mapping, preamble-import re-anchor, and the inserted-import
        // specifier rewrite. Were the plugin to ALSO pre-map companion responses,
        // the Rust merge layer would receive an already-`.vue`-source edit and
        // double-map / drop it. So `"0"` DISABLES the plugin remap here. The VS
        // Code DIRECT surface (no verter_lsp in the response path) leaves it
        // `true`, where the plugin IS the only mapper. Delivered on the SAME
        // channel as the carrier store dir; the plugin reads
        // `VERTER_PLUGIN_RESPONSE_REMAP` (default ENABLED when unset).
        cmd.env(
            "VERTER_PLUGIN_RESPONSE_REMAP",
            if plugin_response_remap { "1" } else { "0" },
        );

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
            consecutive_failures: AtomicU32::new(0),
            crash_notify: crash_notify.clone(),
            membership_recovery: Mutex::new(None),
        });

        let diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let contents_cache: Arc<Mutex<HashMap<String, Arc<str>>>> =
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
            opened_files: Arc::new(Mutex::new(HashMap::new())),
            diagnostics_cache,
            workspace_root: ws_root,
            project_roots: Arc::new(parking_lot::RwLock::new(Vec::new())),
            carrier_projects: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            carrier_sources: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            content_generations: Arc::new(ContentGenerations::default()),
            carrier_store_refresh_generation: AtomicU64::new(0),
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

    /// The owning configured-project tsconfig path for a registered carrier
    /// companion, or `None` for a non-carrier (real `.ts`/`.tsx`) file. A carrier
    /// query passes this as `projectFileName` so the companion is type-checked in
    /// the project where `getExternalFiles` admitted it — `file` is already in
    /// normalized form (the carrier map is keyed by normalized companion paths).
    fn project_file_name_for(&self, file: &str) -> Option<String> {
        self.carrier_projects.read().get(file).cloned()
    }
}

/// Inject `projectFileName` into a tsserver request's args when the file is a
/// registered carrier companion (`project_file_name` is `Some`). tsserver resolves
/// a request's project as `getProject(projectFileName) ||
/// ensureDefaultProjectForFile(file)`; for a carrier (an EXTERNAL
/// `getExternalFiles` member, never a root) the default-project fallback can pick
/// the wrong / a fresh inferred project and return empty results, so the owning
/// tsconfig MUST be named explicitly. A non-carrier file (`None`) leaves `args`
/// untouched (its default project is correct). Captured `Option<String>` form so
/// it works inside the request closures after `self` fields are moved out.
fn inject_project_file_name(
    mut args: serde_json::Value,
    project_file_name: &Option<String>,
) -> serde_json::Value {
    if let Some(name) = project_file_name {
        if let Some(map) = args.as_object_mut() {
            map.insert(
                "projectFileName".to_string(),
                serde_json::Value::String(name.clone()),
            );
        }
    }
    args
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
        let content_generations = Arc::clone(&self.content_generations);
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
                    store_content_bump_generation(
                        &contents_cache,
                        &content_generations,
                        &file,
                        Arc::from(content.as_str()),
                    )
                    .await;
                    opened_files
                        .lock()
                        .await
                        .insert(file.clone(), OpenKind::Source);
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
        let content_generations = Arc::clone(&self.content_generations);
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsserver_load_file",
                format!("file={} content_len={}", file, content.len()),
                async {
                    store_content_bump_generation(
                        &contents_cache,
                        &content_generations,
                        &file,
                        content.into(),
                    )
                    .await;
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
        let content_generations = Arc::clone(&self.content_generations);
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

                    store_content_bump_generation(
                        &contents_cache,
                        &content_generations,
                        &file,
                        Arc::from(content.as_str()),
                    )
                    .await;

                    let mut opened = opened_files.lock().await;
                    if opened.contains_key(&file) {
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
                        // File not open yet — open it and track. `update_file` is the
                        // editor-content path, so this is a `Source` open (it carries
                        // `fileContent`); a carrier companion is never first-opened
                        // here (it enters only via `register_carrier_member`).
                        opened.insert(file.clone(), OpenKind::Source);
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
        let content_generations = Arc::clone(&self.content_generations);
        let opened_files = Arc::clone(&self.opened_files);
        let carrier_projects = Arc::clone(&self.carrier_projects);
        let carrier_sources = Arc::clone(&self.carrier_sources);
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsserver_close_file",
                format!("file={}", file),
                async {
                    forget_content(&contents_cache, &content_generations, &file).await;
                    opened_files.lock().await.remove(&file);
                    // Retract the carrier→project routing for a closed companion so
                    // it no longer injects `projectFileName` (a closed companion is
                    // no longer a member; a stale route would target a project the
                    // companion left). A no-op for a real `.ts`/`.tsx` file (never in
                    // the carrier map).
                    carrier_projects.write().remove(&file);
                    carrier_sources.write().remove(&file);
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

    fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(companion_path);
        let transport = Arc::clone(&self.transport);
        let refresh_generation = self
            .carrier_store_refresh_generation
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        Box::pin(async move {
            // Advance the plugin's project-scoped carrier-store token first. The
            // plugin compares manifest versions, reloads changed virtual ScriptInfos,
            // clears that configured project's semantic cache, and refreshes its
            // external-file list on the next event-loop turn. This is the warm-
            // snapshot invalidation path; the updateOpen touch below remains the
            // cold negative-resolution eviction path.
            transport
                .command_no_response(
                    "configurePlugin",
                    serde_json::json!({
                        "pluginName": "@verter/typescript-plugin",
                        "configuration": {
                            "carrierStoreRefreshToken": refresh_generation,
                        }
                    }),
                )
                .await?;
            // Evict tsserver's sticky resolution cache for a companion whose
            // content the carrier-publish store has now warmed. `updateOpen` with
            // an empty `changedFiles` edit is the documented file-changed signal:
            // it forces tsserver to re-resolve references to `file` (clearing a
            // negative `fileExists`/module-resolution result cached while the
            // companion's blob did not yet exist on disk) without mutating its
            // content — the plugin serves the now-ready bytes on the re-read.
            transport
                .command_no_response(
                    "updateOpen",
                    serde_json::json!({
                        "changedFiles": [{
                            "fileName": file,
                            "textChanges": [],
                        }]
                    }),
                )
                .await?;
            // `configurePlugin` deliberately schedules graph mutation with
            // `setImmediate` so it cannot corrupt a project being updated
            // re-entrantly. Complete one response round-trip after the two
            // fire-and-forget notifications: the next provider query is then
            // written on a later host turn, after the plugin's scheduled refresh
            // had an opportunity to reload the changed ScriptInfo/project roots.
            // `projectInfo` can legitimately report that a just-published member
            // is still cold; its response is the ordering fence, not its body.
            let _ = transport
                .request(
                    "projectInfo",
                    serde_json::json!({
                        "file": file,
                        "needFileNameList": false,
                    }),
                )
                .await;
            Ok(())
        })
    }

    fn register_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        let source = Self::normalize_path(source_path);
        let file = Self::normalize_path(companion_path);
        let content: Arc<str> = Arc::from(content);
        let project_file_name = Self::normalize_path(project_file_name);
        let contents_cache = Arc::clone(&self.contents);
        let content_generations = Arc::clone(&self.content_generations);
        let carrier_projects = Arc::clone(&self.carrier_projects);
        let carrier_sources = Arc::clone(&self.carrier_sources);
        let opened_files = Arc::clone(&self.opened_files);
        let transport = Arc::clone(&self.transport);
        let script_kind_name = if file.ends_with(".jsx") {
            "JSX"
        } else if file.ends_with(".tsx") {
            "TSX"
        } else if file.ends_with(".js") {
            "JS"
        } else {
            "TS"
        };
        let is_ide_companion = file.ends_with(".tsx") || file.ends_with(".jsx");
        // `projectRootPath` for the project-load `open` is the tsconfig's directory.
        let project_root = project_file_name
            .rsplit_once('/')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_else(|| project_file_name.clone());
        Box::pin(async move {
            // Hydrate the LOCAL position-conversion content for the companion —
            // the bytes are NEVER forwarded to tsserver (the `open` below carries
            // NO `fileContent`; the `@verter/typescript-plugin`'s `getScriptSnapshot`
            // serves the engine-side content from the publish store). Filling
            // `contents` lets `byte_offset_to_tsserver_pos` (request offsets) and
            // `parse_tsserver_location` (response spans) work for the carrier instead
            // of the `(1, offset + 1)` line-1 sentinel.
            store_content_bump_generation(
                &contents_cache,
                &content_generations,
                &file,
                Arc::clone(&content),
            )
            .await;
            // Record the owning configured project so carrier queries route there
            // via `projectFileName` (the companion is an EXTERNAL `getExternalFiles`
            // member, so its default-project resolution is otherwise undecided —
            // `ensureDefaultProjectForFile` would throw `No Project` for a virtual
            // companion on no real-disk path).
            carrier_projects
                .write()
                .insert(file.clone(), project_file_name.clone());

            // CONTENTLESS open of the companion: tsserver does not compute
            // diagnostics for a file that is not a session buffer, and a carrier's
            // owning CONFIGURED PROJECT is not created until a file it contains is
            // opened (without it, `getProject(projectFileName)` misses and
            // `ensureDefaultProjectForFile` throws `No Project`). The `open` carries
            // a `projectRootPath` so tsserver creates/finds the configured project
            // and admits the companion via `getExternalFiles`, and carries NO
            // `fileContent` so the plugin's `getScriptSnapshot` remains the SOLE
            // content authority (this is a membership + diagnostics-buffer signal,
            // never a competing carrier-content open). BOTH companions are opened:
            // the IDE `.tsx` is the diagnostics surface, and the public-API
            // `.verter.ts` is re-exported by the IDE companion
            // (`export { default } from './{name}.verter.ts'`), so its program
            // contribution must also be loaded for the IDE surface's own
            // diagnostics (e.g. a no-default-export error) to resolve. Tracked in
            // `opened_files` (idempotent; a later `close_file` retracts it). Tagged
            // `CarrierCompanion` so a resync replays it CONTENTLESSLY — never
            // resending bytes that would make tsserver its content authority.
            open_carrier_companion_contentless(
                &transport,
                &opened_files,
                &carrier_projects,
                &file,
                script_kind_name,
                &project_root,
            )
            .await?;
            // Record diagnostic routing only after companion membership signaling
            // succeeds. This never opens the source or sends source content.
            if is_ide_companion {
                carrier_sources.write().insert(file.clone(), source);
            }
            // No per-open project verification: the carrier reaching this point is
            // ALREADY a confirmed configured-project member. The fail-closed gate
            // against an inferred / ownerless / ambiguous carrier lives UPSTREAM at
            // the publish boundary — `WorkspaceProjectResolver` only mints a
            // `ProjectBinding` (→ `BoundProject` → publish → this registration) for a
            // resolved configured owner; `NoProject` / `Ambiguous` / scratch publish
            // and register NOTHING, so an ownerless carrier never opens. A contentless
            // open transiently associating with tsserver's inferred/default project is
            // a LOAD-TIMING state, not a wrong owner: carrier queries route with the
            // resolved `projectFileName`, and the lazy cold-read `reloadProjects`
            // recovery (`recover_companion_membership`, fired only when a real query
            // hits "Could not find source file" / "No Project") settles a not-yet-
            // loaded project on demand. A synchronous per-open `projectInfo` round-trip
            // would add latency to every carrier open AND race-close a legitimately-
            // owned companion that is merely still settling.
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
        let project_file_name = self.project_file_name_for(&file);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let mut args = inject_project_file_name(
                serde_json::json!({
                    "file": file,
                    "line": line,
                    "offset": col,
                    "includeExternalModuleExports": true,
                    "includeInsertTextCompletions": true,
                }),
                &project_file_name,
            );

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
        let project_file_name = self.project_file_name_for(&file);
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
                    // COLD-build recovery (mirrors `get_diagnostics`): a hover on a
                    // companion not yet a configured-project member fails with
                    // "Could not find source file". On that NARROW cold error,
                    // recover the companion's membership (re-query
                    // `getExternalFiles`) and re-issue `quickinfo`, bounded by a
                    // short deadline with cooperative sleeps. The recovery fires
                    // ONLY on the cold miss, never on a warm hover.
                    let cold_deadline =
                        std::time::Instant::now() + std::time::Duration::from_millis(2500);
                    let result = loop {
                        let r = transport
                            .request(
                                "quickinfo",
                                inject_project_file_name(
                                    serde_json::json!({
                                        "file": file,
                                        "line": line,
                                        "offset": col,
                                    }),
                                    &project_file_name,
                                ),
                            )
                            .await;
                        match r {
                            Err(e)
                                if tsserver_diag_error_is_companion_not_ready(&e.message)
                                    && std::time::Instant::now() < cold_deadline =>
                            {
                                recover_companion_membership(&transport).await;
                                tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                            }
                            other => break other,
                        }
                    };

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
            // The entry's auto-import `codeActions` parse into `additionalTextEdits`,
            // so this is an edit-producing response: snapshot ONLY the files those
            // code actions target, taken FRESH after the await — never a whole-map
            // clone of the contents cache.
            let target_paths =
                crate::contents_snapshot::tsserver_completion_entry_details_target_paths(detail);
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                crate::contents_snapshot::targeted_contents_snapshot(&guard, &target_paths)
            };
            Ok(completion_entry_details_to_resolve_result(
                detail,
                &file,
                &cache_snapshot,
            ))
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let file = Self::normalize_path(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let diagnostics_cache = Arc::clone(&self.diagnostics_cache);
        // Route a carrier companion's diagnostic passes to its OWNING configured
        // project (so `semanticDiagnosticsSync` type-checks it where
        // `getExternalFiles` admitted it, not a fresh inferred project that returns
        // empty). `None` for a non-carrier file (its default project is correct).
        let project_file_name = self.project_file_name_for(&file);
        let diagnostic_file = self
            .carrier_sources
            .read()
            .get(&file)
            .cloned()
            .unwrap_or_else(|| file.clone());
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
            //
            // COLD-build re-poll: on a freshly built configured project the
            // just-published companion is not yet a program member tsserver
            // type-checks, so the semantic pass fails the whole command with
            // "Could not find source file: <companion>". On that NARROW error,
            // recover the companion's configured-project membership (re-query
            // `getExternalFiles` — see `recover_companion_membership`) and re-issue
            // the semantic pass, bounded by a short deadline with cooperative
            // sleeps (never a busy-spin). The recovery fires ONLY on this cold miss,
            // so a warm pull never pays it. Only this error is retried: a genuine
            // module-not-found arrives in the SUCCESS body (so it never reaches the
            // error path) and timeouts / closed channels are distinct terminal
            // strings that fall straight through.
            let cold_deadline = std::time::Instant::now() + std::time::Duration::from_millis(2500);
            let semantic_result = loop {
                let result = transport
                    .request(
                        "semanticDiagnosticsSync",
                        inject_project_file_name(
                            serde_json::json!({ "file": diagnostic_file }),
                            &project_file_name,
                        ),
                    )
                    .await;
                match result {
                    Err(e)
                        if tsserver_diag_error_is_companion_not_ready(&e.message)
                            && std::time::Instant::now() < cold_deadline =>
                    {
                        recover_companion_membership(&transport).await;
                        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                    }
                    other => break other,
                }
            };

            match semantic_result {
                Ok(semantic_body) => {
                    let semantic = parse_tsserver_diagnostics_body(
                        &semantic_body,
                        content.as_deref(),
                        Some(file.as_str()),
                    );

                    let syntactic = transport
                        .request(
                            "syntacticDiagnosticsSync",
                            inject_project_file_name(
                                serde_json::json!({ "file": diagnostic_file }),
                                &project_file_name,
                            ),
                        )
                        .await
                        .ok()
                        .map(|body| {
                            parse_tsserver_diagnostics_body(
                                &body,
                                content.as_deref(),
                                Some(file.as_str()),
                            )
                        })
                        .unwrap_or_default();

                    let suggestion = transport
                        .request(
                            "suggestionDiagnosticsSync",
                            inject_project_file_name(
                                serde_json::json!({ "file": diagnostic_file }),
                                &project_file_name,
                            ),
                        )
                        .await
                        .ok()
                        .map(|body| {
                            parse_tsserver_diagnostics_body(
                                &body,
                                content.as_deref(),
                                Some(file.as_str()),
                            )
                        })
                        .unwrap_or_default();

                    let diags = merge_diagnostic_sets(semantic, syntactic, suggestion);
                    diagnostics_cache.lock().await.insert(file, diags.clone());
                    Ok(diags)
                }
                Err(e) if tsserver_diag_error_is_companion_not_ready(&e.message) => {
                    // The companion is STILL not in the program after the bounded
                    // cold-build re-poll. Surface this as a NOT-READY error (do not
                    // mask it to an empty set, which would warm a torn empty result
                    // and let it read as "no diagnostics"). Propagating lets the
                    // caller's diagnostics retry loop re-pull once the project
                    // finishes building.
                    Err(e)
                }
                Err(_) => {
                    // Any other failure (timeout, closed channel) falls back to the
                    // last cached diagnostics for this file.
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
        let project_file_name = self.project_file_name_for(&file);
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
                    inject_project_file_name(
                        serde_json::json!({
                            "file": file,
                            "line": line,
                            "offset": col,
                        }),
                        &project_file_name,
                    ),
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
        let project_file_name = self.project_file_name_for(&file);
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
                    inject_project_file_name(
                        serde_json::json!({
                            "file": file,
                            "line": line,
                            "offset": col,
                        }),
                        &project_file_name,
                    ),
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
        let project_file_name = self.project_file_name_for(&file);
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
                    inject_project_file_name(
                        serde_json::json!({
                            "file": file,
                            "line": line,
                            "offset": col,
                        }),
                        &project_file_name,
                    ),
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

            // Snapshot ONLY this response's target files, then RELEASE the async mutex BEFORE
            // parsing: the per-target parse runs a blocking `std::fs::read_to_string` disk fallback,
            // and a multi-file rename could stall the provider if that disk I/O ran under the lock.
            // Scanning the response keeps the snapshot bounded by the files it touches and current
            // as of this response, not the whole cache.
            let target_paths = crate::contents_snapshot::tsserver_rename_target_paths(&result);
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                crate::contents_snapshot::targeted_contents_snapshot(&guard, &target_paths)
            };
            let locs = {
                // Bind a `Copy` `&HashMap` for the per-target closures; the lock is already dropped,
                // so the disk fallback inside the parser runs unlocked.
                let cache: &HashMap<String, Arc<str>> = &cache_snapshot;
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

                    // tsserver gives a single top-level active param
                    // (`argumentIndex`) and active signature (`selectedItemIndex`),
                    // not per-overload values. Read both up front so each signature
                    // can stamp the active param onto the SELECTED overload only.
                    let active_sig = body
                        .get("selectedItemIndex")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);
                    let active_param = body
                        .get("argumentIndex")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);

                    let signatures: Vec<SignatureInfo> = items
                        .iter()
                        .enumerate()
                        .map(|(sig_idx, item)| {
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

                            // Collect each parameter's display text + docs first;
                            // the text is exactly what occupies the param's slot in
                            // the assembled label, so offsets computed from these
                            // texts are exact.
                            let param_parts: Vec<(String, Option<String>)> = item
                                .get("parameters")
                                .and_then(|v| v.as_array())
                                .map(|ps| {
                                    ps.iter()
                                        .map(|p| {
                                            let text = p
                                                .get("displayParts")
                                                .and_then(|v| v.as_array())
                                                .map(|parts| concat_display_parts(parts))
                                                .unwrap_or_default();
                                            let doc = p
                                                .get("documentation")
                                                .and_then(|v| v.as_array())
                                                .map(|parts| concat_display_parts(parts));
                                            (text, doc)
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();

                            // Borrow each param's text in place (no clone): the
                            // assembler reads the slices and records offsets in one
                            // pass. Offsets (vs. plain `Simple`) let the client bold
                            // the exact active-parameter span; this is strictly
                            // richer and is computed from the wire display parts.
                            let param_labels: Vec<&str> =
                                param_parts.iter().map(|(t, _)| t.as_str()).collect();
                            let assembled = assemble_signature_label(
                                &prefix,
                                &param_labels,
                                &separator,
                                &suffix,
                            );
                            let params: Vec<ParameterInfo> = param_parts
                                .into_iter()
                                .zip(assembled.param_offsets.iter())
                                .map(|((_, doc), &(start, end))| ParameterInfo {
                                    label: ParameterLabelKind::Offsets(start, end),
                                    documentation: doc,
                                })
                                .collect();
                            let doc = item
                                .get("documentation")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts));

                            // Stamp the top-level active param onto the selected
                            // overload only; tsserver does not give per-overload
                            // active params, so the param index only meaningfully
                            // applies to the active signature.
                            let sig_active_param = if active_sig == Some(sig_idx as u32) {
                                active_param
                            } else {
                                None
                            };

                            SignatureInfo {
                                label: assembled.label,
                                documentation: doc,
                                parameters: params,
                                active_parameter: sig_active_param,
                            }
                        })
                        .collect();

                    if signatures.is_empty() {
                        return Ok(None);
                    }

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

            // Snapshot ONLY the files these fixes target, then RELEASE the async mutex BEFORE
            // parsing: each edit's parse runs a blocking `std::fs::read_to_string` disk fallback,
            // and a fix-all touching many files could stall the provider if that disk I/O ran under
            // the lock. Scanning the responses keeps the snapshot bounded by the touched files.
            let mut target_paths: HashSet<String> = HashSet::new();
            for fix in &raw_fixes {
                target_paths.extend(crate::contents_snapshot::tsserver_code_action_target_paths(
                    fix,
                ));
            }
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                crate::contents_snapshot::targeted_contents_snapshot(&guard, &target_paths)
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
                    // Snapshot ONLY this combined response's target files, taken FRESH (the request
                    // may have synced new files), and RELEASE the lock before parsing — the parse
                    // runs a blocking disk fallback per edit.
                    let target_paths =
                        crate::contents_snapshot::tsserver_combined_code_fix_target_paths(&body);
                    let cache = {
                        let guard = contents_cache.lock().await;
                        crate::contents_snapshot::targeted_contents_snapshot(&guard, &target_paths)
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
        let content_generations = Arc::clone(&self.content_generations);
        let carrier_projects = Arc::clone(&self.carrier_projects);
        let project_roots = Arc::clone(&self.project_roots);
        let workspace_root = self.workspace_root.clone();
        Box::pin(async move {
            resync_open_files_inner(
                transport,
                opened_files,
                contents_cache,
                content_generations,
                carrier_projects,
                project_roots,
                workspace_root,
            )
            .await
        })
    }
}

/// The tsserver `scriptKindName` for a file path.
fn script_kind_name(file: &str) -> &'static str {
    if file.ends_with(".tsx") {
        "TSX"
    } else if file.ends_with(".jsx") {
        "JSX"
    } else if file.ends_with(".js") {
        "JS"
    } else {
        "TS"
    }
}

/// Per-file content-generation tracker.
///
/// Each content write stamps the file with the NEXT value of a single
/// process-monotonic counter, so every generation is globally unique and
/// strictly increasing. A close removes the file's generation; because a later
/// reopen draws a FRESH counter value (never a recycled per-file count), a stale
/// captured generation can never alias a reopened file (no ABA), and a resync
/// that captured a since-closed or since-edited file fails its re-check and
/// skips the now-stale reopen.
#[derive(Default)]
struct ContentGenerations {
    /// `file` → its content generation at the last write. Synchronous lock taken
    /// only while the async `contents` guard is held, so the `(content,
    /// generation)` pair is observed consistently and no lock spans an `.await`.
    map: parking_lot::Mutex<HashMap<String, u64>>,
    /// Source of the next, globally-unique generation value.
    counter: AtomicU64,
}

impl ContentGenerations {
    /// The next globally-unique, monotonically-increasing generation value.
    fn next_generation(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Store a file's content and stamp its content generation atomically.
///
/// The generation is drawn and recorded under a SYNCHRONOUS lock taken (and
/// released) while the async `contents` guard is held, so a concurrent resync
/// capture observes a consistent `(content, generation)` pair, the generation is
/// stamped in content-write order, and no lock spans an `.await` (the only await
/// is acquiring `contents`).
async fn store_content_bump_generation(
    contents: &Mutex<HashMap<String, Arc<str>>>,
    generations: &ContentGenerations,
    file: &str,
    content: Arc<str>,
) {
    let mut guard = contents.lock().await;
    let next = generations.next_generation();
    guard.insert(file.to_string(), content);
    generations.map.lock().insert(file.to_string(), next);
}

/// Forget a file's content AND its generation (on close). Combined with the
/// globally-unique counter, a later reopen of the same path draws a fresh
/// generation a stale captured one cannot match.
async fn forget_content(
    contents: &Mutex<HashMap<String, Arc<str>>>,
    generations: &ContentGenerations,
    file: &str,
) {
    let mut guard = contents.lock().await;
    guard.remove(file);
    generations.map.lock().remove(file);
}

/// The contentless carrier-companion open, factored out of `register_carrier_member`
/// so the open-failure rollback is unit-testable against a bare transport WITHOUT
/// spawning a tsserver child. The trait method delegates here, so this IS the
/// production carrier-open path.
///
/// Atomically marks the companion opened (`opened_files`) and, only if newly marked,
/// issues the CONTENTLESS `open`. On a transport-send FAILURE it ROLLS BACK the
/// optimistic `opened_files` mark AND the `carrier_projects` routing entry, so a
/// later registration RE-ATTEMPTS the open instead of observing a phantom "already
/// opened" (`opened_now == false`) and skipping it forever — which would leave the
/// companion never a configured-project member (a phantom-registered carrier). The
/// atomic check-and-mark (not a check-then-mark) keeps two concurrent registrations
/// from both issuing the open.
async fn open_carrier_companion_contentless(
    transport: &TsserverTransport,
    opened_files: &Arc<Mutex<HashMap<String, OpenKind>>>,
    carrier_projects: &Arc<parking_lot::RwLock<HashMap<String, String>>>,
    file: &str,
    script_kind_name: &str,
    project_root: &str,
) -> Result<(), TypeProviderError> {
    let opened_now = opened_files
        .lock()
        .await
        .insert(file.to_string(), OpenKind::CarrierCompanion)
        .is_none();
    if opened_now {
        if let Err(error) = transport
            .command_no_response(
                "open",
                serde_json::json!({
                    "file": file,
                    "scriptKindName": script_kind_name,
                    "projectRootPath": project_root,
                }),
            )
            .await
        {
            opened_files.lock().await.remove(file);
            carrier_projects.write().remove(file);
            return Err(error);
        }
    }
    Ok(())
}

/// A per-file resync plan entry captured atomically from the live caches.
struct ResyncEntry {
    file: String,
    kind: OpenKind,
    /// The captured `Source` content (`None` for a carrier companion, which is
    /// always reopened contentlessly).
    content: Option<Arc<str>>,
    /// The per-file content generation at capture time; re-checked before a
    /// `Source` reopen so a concurrent edit's newer bytes are never overwritten.
    generation: u64,
}

/// Capture the resync plan: each opened file's kind, its content snapshot, and
/// its content generation. The `(content, generation)` pair is read under the
/// `contents` guard so it is consistent with `store_content_bump_generation`
/// (no writer can be observed half-applied), and no lock spans an `.await`.
async fn resync_capture(
    opened_files: &Mutex<HashMap<String, OpenKind>>,
    contents_cache: &Mutex<HashMap<String, Arc<str>>>,
    content_generations: &ContentGenerations,
) -> Vec<ResyncEntry> {
    let files: Vec<(String, OpenKind)> = opened_files
        .lock()
        .await
        .iter()
        .map(|(file, kind)| (file.clone(), *kind))
        .collect();
    let guard = contents_cache.lock().await;
    let generations = content_generations.map.lock();
    files
        .into_iter()
        .map(|(file, kind)| {
            let content = guard.get(&file).map(Arc::clone);
            let generation = generations.get(&file).copied().unwrap_or(0);
            ResyncEntry {
                file,
                kind,
                content,
                generation,
            }
        })
        .collect()
}

/// Apply a captured resync plan: close+reopen each file.
///
/// A `Source` entry is reopened WITH its captured content (tsserver is the
/// source's content authority), but ONLY after a generation re-check confirms a
/// concurrent `update_file` has not landed newer bytes since capture; if it has,
/// the now-stale reopen is SKIPPED — the update already pushed the current bytes,
/// so resending the captured ones would overwrite them (the stale-reopen bug this
/// gate closes). A `CarrierCompanion` entry is reopened CONTENTLESSLY and routed
/// to its OWN configured project (the plugin's `getScriptSnapshot` stays the sole
/// engine-side content authority — resending bytes would make tsserver the
/// carrier's content owner); it carries no bytes, so it needs no generation gate.
async fn resync_apply(
    transport: &TsserverTransport,
    entries: Vec<ResyncEntry>,
    contents_cache: &Mutex<HashMap<String, Arc<str>>>,
    content_generations: &ContentGenerations,
    carrier_projects: &parking_lot::RwLock<HashMap<String, String>>,
    project_roots: &parking_lot::RwLock<Vec<String>>,
    workspace_root: &str,
) -> Result<(), TypeProviderError> {
    for entry in entries {
        let kind_name = script_kind_name(&entry.file);
        match entry.kind {
            OpenKind::Source => {
                let Some(content) = entry.content else {
                    continue;
                };
                // Generation gate: re-read the live generation under the contents
                // guard (so a writer's atomic content+generation update is never
                // observed half-applied), immediately before sending the reopen.
                // If it advanced past the captured value — or the file was closed
                // (no entry) — a concurrent edit/close already superseded these
                // bytes; skip the stale reopen rather than clobber the newer state.
                let still_current = {
                    let _contents = contents_cache.lock().await;
                    content_generations.map.lock().get(&entry.file).copied()
                        == Some(entry.generation)
                };
                if !still_current {
                    continue;
                }
                transport
                    .command_no_response("close", serde_json::json!({ "file": entry.file }))
                    .await?;
                let project_root = {
                    let roots = project_roots.read();
                    verter_span::path::longest_project_root(&entry.file, &roots, workspace_root)
                        .to_string()
                };
                transport
                    .command_no_response(
                        "open",
                        serde_json::json!({
                            "file": entry.file,
                            "fileContent": content,
                            "scriptKindName": kind_name,
                            "projectRootPath": project_root,
                        }),
                    )
                    .await?;
            }
            OpenKind::CarrierCompanion => {
                let project_root = carrier_projects
                    .read()
                    .get(&entry.file)
                    .and_then(|tsconfig| tsconfig.rsplit_once('/').map(|(dir, _)| dir.to_string()))
                    .unwrap_or_else(|| {
                        let roots = project_roots.read();
                        verter_span::path::longest_project_root(&entry.file, &roots, workspace_root)
                            .to_string()
                    });
                transport
                    .command_no_response("close", serde_json::json!({ "file": entry.file }))
                    .await?;
                transport
                    .command_no_response(
                        "open",
                        serde_json::json!({
                            "file": entry.file,
                            "scriptKindName": kind_name,
                            "projectRootPath": project_root,
                        }),
                    )
                    .await?;
            }
        }
    }
    Ok(())
}

/// The `resync_open_files` body, factored out of the trait method (which only owns
/// `Arc`-cloned state) so it is unit-testable against a bare transport + caches
/// WITHOUT spawning a tsserver child. The method delegates here, so this IS the
/// production resync path: a [`resync_capture`] snapshot (content + generation)
/// followed by a generation-gated [`resync_apply`].
async fn resync_open_files_inner(
    transport: Arc<TsserverTransport>,
    opened_files: Arc<Mutex<HashMap<String, OpenKind>>>,
    contents_cache: Arc<Mutex<HashMap<String, Arc<str>>>>,
    content_generations: Arc<ContentGenerations>,
    carrier_projects: Arc<parking_lot::RwLock<HashMap<String, String>>>,
    project_roots: Arc<parking_lot::RwLock<Vec<String>>>,
    workspace_root: String,
) -> Result<(), TypeProviderError> {
    let entries = resync_capture(&opened_files, &contents_cache, &content_generations).await;
    resync_apply(
        &transport,
        entries,
        &contents_cache,
        &content_generations,
        &carrier_projects,
        &project_roots,
        &workspace_root,
    )
    .await
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
    // tsserver signals a snippet entry with `isSnippet: true` (its genuine
    // snippet signal — the entry's `insertText` then carries `$0`/`${n:…}`
    // placeholders). Map it to the neutral carrier; a non-snippet entry leaves
    // `None` (never fabricate a format from the label or kind). NOTE: tsserver
    // only EMITS snippet entries when the session enables
    // `includeCompletionsWithSnippetText`; the parse is correct regardless of
    // whether that preference is on.
    let insert_text_format = match item.get("isSnippet").and_then(|v| v.as_bool()) {
        Some(true) => Some(CompletionInsertTextFormat::Snippet),
        _ => None,
    };
    // tsserver may carry `commitCharacters` on an entry; parse if present via the
    // SAME strict, fail-closed helper the TSGO provider uses (empty/malformed →
    // `None`, never `Some(vec![])`).
    let commit_characters = parse_commit_characters(item.get("commitCharacters"));
    let filter_text = item
        .get("filterText")
        .and_then(|v| v.as_str())
        .map(String::from);
    // tsserver's `isRecommended` flags the entry the editor should pre-select.
    let preselect = match item.get("isRecommended").and_then(|v| v.as_bool()) {
        Some(true) => Some(true),
        _ => None,
    };

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
        // tsserver completion entries carry no `textEdit`; the surviving-edit
        // payload is absent and the plain-insert text rides `insert_text`.
        text_edit_new_text: None,
        insert_text,
        sort_text,
        insert_text_format,
        commit_characters,
        filter_text,
        preselect,
        // tsserver completion ENTRIES do not carry label details at list time
        // (they surface only at `completionEntryDetails` time); leave `None`
        // here. The resolve path may recover a `description` later.
        label_details: None,
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
        text_edit_new_text: item.text_edit_new_text.clone(),
        insert_text: item.insert_text.clone(),
        sort_text: item.sort_text.clone(),
        insert_text_format: item.insert_text_format,
        commit_characters: item.commit_characters.clone(),
        filter_text: item.filter_text.clone(),
        preselect: item.preselect,
        label_details: item.label_details.clone(),
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
    contents_cache: &HashMap<String, Arc<str>>,
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
        Some(content.as_ref())
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
/// definition, and the tsgo rename path gives via `parse_range_to_offsets_strict_with_disk_fallback`.
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
    contents_cache: &HashMap<String, Arc<str>>,
) -> Option<RenameLocation> {
    let start = span.get("start")?;
    let end = span.get("end")?;
    let sl = u32::try_from(start.get("line")?.as_u64()?).ok()?;
    let so = u32::try_from(start.get("offset")?.as_u64()?).ok()?;
    let el = u32::try_from(end.get("line")?.as_u64()?).ok()?;
    let eo = u32::try_from(end.get("offset")?.as_u64()?).ok()?;

    let disk_content;
    let content = if let Some(content) = contents_cache.get(file) {
        Some(content.as_ref())
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
    contents_cache: &HashMap<String, Arc<str>>,
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
            Some(content.as_ref())
        } else {
            disk_content = std::fs::read_to_string(&file).ok();
            disk_content.as_deref()
        };
        if let Some(text_changes) = change.get("textChanges").and_then(|v| v.as_array()) {
            for tc in text_changes {
                let start = tc.get("start")?;
                let end = tc.get("end")?;
                let new_text = tc.get("newText")?.as_str()?.to_string();
                // FAIL CLOSED on a u64>u32::MAX position: a lossy `as u32` would wrap a huge
                // line/offset into an in-range value the checked converter accepts, landing the
                // WRITE at the wrong location. `try_from` DROPS this edit instead — `continue`, not
                // `?`, so one overflowing edit never discards the other (valid) edits in the batch.
                let (Some(sl), Some(so), Some(el), Some(eo)) = (
                    u32::try_from(start.get("line")?.as_u64()?).ok(),
                    u32::try_from(start.get("offset")?.as_u64()?).ok(),
                    u32::try_from(end.get("line")?.as_u64()?).ok(),
                    u32::try_from(end.get("offset")?.as_u64()?).ok(),
                ) else {
                    continue;
                };

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
    contents_cache: &HashMap<String, Arc<str>>,
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
    contents_cache: &HashMap<String, Arc<str>>,
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
    contents_cache: &HashMap<String, Arc<str>>,
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

    // tsserver's `completionEntryDetails` response carries NO `labelDetails` wire
    // field — `sourceDisplay`/`source` are the originating MODULE specifier, a
    // DIFFERENT LSP slot than `CompletionItemLabelDetails.description`. Reusing
    // them here would fabricate a label-details signal the wire never sent, so the
    // carrier stays `None` (fail-closed — parse only what the wire genuinely
    // carries as that field). tsserver completion details also carry no `command`
    // — always `None`.

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
        label_details: None,
        command: None,
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

/// The assembled signature label plus each parameter's UTF-16 offset span within
/// it, returned together so the offsets stay consistent with the exact label they
/// were measured against.
pub struct AssembledSignatureLabel {
    /// The full signature label: `{prefix}{params joined by separator}{suffix}`.
    pub label: String,
    /// Per-parameter `[start, end)` offset, in parameter order, in **UTF-16 code
    /// units** relative to `label` (the LSP `ParameterInformation.label` offset
    /// encoding). Same length as the input `param_labels`.
    pub param_offsets: Vec<(u32, u32)>,
}

/// Assemble a tsserver signature label from its display-part segments and compute
/// each parameter's `[start, end)` span within the assembled label.
///
/// The label is `{prefix}{param_labels joined by separator}{suffix}` — identical
/// to how tsserver's own client renders it — and each parameter occupies a
/// contiguous run, so its span is exact (this is data assembly over
/// provider-supplied parts, not semantic inference).
///
/// IMPORTANT (encoding): LSP parameter-label offsets are **UTF-16 code units**, so
/// every running length is measured with `encode_utf16().count()`, never bytes and
/// never `char`s — otherwise a multi-byte / astral character in a type name would
/// misalign the bold span.
pub fn assemble_signature_label(
    prefix: &str,
    param_labels: &[impl AsRef<str>],
    separator: &str,
    suffix: &str,
) -> AssembledSignatureLabel {
    // Single pass: build the label string AND the per-param UTF-16 offset spans
    // together (no intermediate `Vec<String>` clone, no throwaway `join`). Each
    // param's span is recorded against the running UTF-16 cursor as its text is
    // appended, so the offsets stay exactly consistent with the label bytes.
    let separator_u16 = separator.encode_utf16().count() as u32;
    let mut label = String::with_capacity(prefix.len() + separator.len() + suffix.len());
    label.push_str(prefix);
    let mut cursor = prefix.encode_utf16().count() as u32;
    let mut param_offsets = Vec::with_capacity(param_labels.len());
    for (i, p) in param_labels.iter().enumerate() {
        let p = p.as_ref();
        if i > 0 {
            label.push_str(separator);
            cursor += separator_u16;
        }
        let start = cursor;
        label.push_str(p);
        cursor += p.encode_utf16().count() as u32;
        param_offsets.push((start, cursor));
    }
    label.push_str(suffix);

    AssembledSignatureLabel {
        label,
        param_offsets,
    }
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
