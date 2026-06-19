//! TSGO `TypeProvider` implementation via LSP JSON-RPC over stdio.
//!
//! Spawns `tsgo --lsp --stdio` as a child process and communicates using
//! the Language Server Protocol over stdin/stdout with JSON-RPC framing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot, Mutex, Notify};

use crate::codec::{LineIndex, PositionEncoding};
use crate::protocol::*;
use crate::traits::{ProviderFuture, TypeProvider};
#[cfg(test)]
use crate::uri::percent_decode;
use crate::uri::{file_uri_to_path, normalize_file_uri_for_cache, path_to_file_uri_string};

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
fn summarize_lsp_params(params: &serde_json::Value) -> String {
    let uri = params
        .get("textDocument")
        .and_then(|value| value.get("uri"))
        .or_else(|| params.get("uri"))
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let line = params
        .get("position")
        .and_then(|value| value.get("line"))
        .and_then(|value| value.as_u64())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let character = params
        .get("position")
        .and_then(|value| value.get("character"))
        .and_then(|value| value.as_u64())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!("uri={} line={} character={}", uri, line, character)
}

/// Message sent to the dedicated stdin writer task.
enum StdinMessage {
    /// Write a framed LSP message to stdin.
    Frame(Vec<u8>),
    /// Shut down the writer task.
    Shutdown,
}

/// Maximum number of Normal-priority frames to flush before checking Interactive.
const NORMAL_BATCH_CAP: usize = 5;
/// Maximum number of Background-priority frames to flush before checking higher lanes.
const BACKGROUND_BATCH_CAP: usize = 3;

/// Dedicated task that owns the stdin writer and drains three priority lanes.
///
/// Priority order: Interactive > Normal > Background.
/// - Interactive: drained fully (unbounded) before checking lower lanes.
/// - Normal: drained up to `NORMAL_BATCH_CAP` frames, then back to check Interactive.
/// - Background: drained up to `BACKGROUND_BATCH_CAP` frames, then back to check higher.
///
/// Each flush is a separate `write_all + flush`. Interactive always preempts.
///
/// Generic over the writer type to support both `ChildStdin` and test `DuplexStream`.
async fn stdin_writer_loop(
    mut stdin: impl tokio::io::AsyncWrite + Unpin + Send + 'static,
    mut interactive_rx: mpsc::Receiver<StdinMessage>,
    mut normal_rx: mpsc::Receiver<StdinMessage>,
    mut background_rx: mpsc::Receiver<StdinMessage>,
) {
    let mut buffer = Vec::new();

    loop {
        // Wait for any message from any lane
        tokio::select! {
            biased; // Prefer higher priority
            msg = interactive_rx.recv() => {
                match msg {
                    Some(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                    Some(StdinMessage::Shutdown) | None => break,
                }
            }
            msg = normal_rx.recv() => {
                match msg {
                    Some(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                    Some(StdinMessage::Shutdown) | None => break,
                }
            }
            msg = background_rx.recv() => {
                match msg {
                    Some(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                    Some(StdinMessage::Shutdown) | None => break,
                }
            }
        }

        // Drain Interactive fully (unbounded)
        loop {
            match interactive_rx.try_recv() {
                Ok(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                Ok(StdinMessage::Shutdown) => {
                    let _ = stdin.write_all(&buffer).await;
                    let _ = stdin.flush().await;
                    return;
                }
                Err(_) => break,
            }
        }

        // Flush Interactive batch
        if !buffer.is_empty() {
            if stdin.write_all(&buffer).await.is_err() {
                break;
            }
            let _ = stdin.flush().await;
            buffer.clear();
        }

        // Drain Normal (capped)
        for _ in 0..NORMAL_BATCH_CAP {
            match normal_rx.try_recv() {
                Ok(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                Ok(StdinMessage::Shutdown) => {
                    let _ = stdin.write_all(&buffer).await;
                    let _ = stdin.flush().await;
                    return;
                }
                Err(_) => break,
            }
        }

        // Check Interactive again before flushing Normal
        loop {
            match interactive_rx.try_recv() {
                Ok(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                Ok(StdinMessage::Shutdown) => {
                    let _ = stdin.write_all(&buffer).await;
                    let _ = stdin.flush().await;
                    return;
                }
                Err(_) => break,
            }
        }

        if !buffer.is_empty() {
            if stdin.write_all(&buffer).await.is_err() {
                break;
            }
            let _ = stdin.flush().await;
            buffer.clear();
        }

        // Drain Background (capped)
        for _ in 0..BACKGROUND_BATCH_CAP {
            match background_rx.try_recv() {
                Ok(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                Ok(StdinMessage::Shutdown) => {
                    let _ = stdin.write_all(&buffer).await;
                    let _ = stdin.flush().await;
                    return;
                }
                Err(_) => break,
            }
        }

        // Check Interactive + Normal again before flushing Background
        loop {
            match interactive_rx.try_recv() {
                Ok(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                Ok(StdinMessage::Shutdown) => {
                    let _ = stdin.write_all(&buffer).await;
                    let _ = stdin.flush().await;
                    return;
                }
                Err(_) => break,
            }
        }
        loop {
            match normal_rx.try_recv() {
                Ok(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                Ok(StdinMessage::Shutdown) => {
                    let _ = stdin.write_all(&buffer).await;
                    let _ = stdin.flush().await;
                    return;
                }
                Err(_) => break,
            }
        }

        if !buffer.is_empty() {
            if stdin.write_all(&buffer).await.is_err() {
                break;
            }
            let _ = stdin.flush().await;
            buffer.clear();
        }
    }
}

/// Legacy single-channel wrapper for backward compat (tests).
#[cfg(test)]
async fn stdin_writer_loop_single(
    stdin: impl tokio::io::AsyncWrite + Unpin + Send + 'static,
    rx: mpsc::Receiver<StdinMessage>,
) {
    // Create dummy channels for normal and background that never receive
    let (_normal_tx, normal_rx) = mpsc::channel(1);
    let (_bg_tx, background_rx) = mpsc::channel(1);
    stdin_writer_loop(stdin, rx, normal_rx, background_rx).await;
}

/// LSP JSON-RPC transport over a child process's stdio.
struct LspTransport {
    /// Interactive-priority lane: hover, completion, definition, active-file sync.
    interactive_tx: mpsc::Sender<StdinMessage>,
    /// Normal-priority lane: imported-file warmup, tsconfig config, deferred API.
    normal_tx: mpsc::Sender<StdinMessage>,
    /// Background-priority lane: workspace scanner, shadow graph, diagnostics.
    background_tx: mpsc::Sender<StdinMessage>,
    /// Pending request senders, keyed by request ID. Shared with the read loop.
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>,
    next_id: AtomicI64,
    /// Counts consecutive request timeouts. Reset to 0 on any successful response.
    /// When this reaches `HANG_THRESHOLD`, fires `crash_notify` to trigger a restart
    /// via the existing `ResilientTypeProvider` crash recovery machinery.
    consecutive_failures: AtomicU32,
    /// Shared with `ResilientTypeProvider` — signaled when the provider appears hung.
    crash_notify: Option<Arc<Notify>>,
}

/// Default timeout for LSP requests (10 seconds).
const REQUEST_TIMEOUT_SECS: u64 = 10;

/// List-level cap on completion-detail enrichment
/// ([`TsgoTypeProvider::get_completion_details`]).
///
/// Each enriched item costs one `completionItem/resolve` round-trip (a
/// [`REQUEST_TIMEOUT_SECS`]-bounded request); a member enumeration can return a
/// large list, so only the leading (sorted-order = most relevant) items are
/// enriched and the tail passes through unchanged — still present in the list,
/// still lazily resolvable. Bounds the worst-case enrichment cost independent of
/// list size.
const MAX_COMPLETION_DETAIL_ENRICH: usize = 50;

/// Max in-flight `completionItem/resolve` requests while enriching a completion
/// list ([`TsgoTypeProvider::get_completion_details`]).
///
/// Bounds concurrency over the (already list-capped) enriched subset so the
/// worst case is `ceil(MAX_COMPLETION_DETAIL_ENRICH / this) × REQUEST_TIMEOUT_SECS`
/// rather than a serial `N × REQUEST_TIMEOUT_SECS`, without flooding the
/// single-process tsgo transport with the whole batch at once.
const COMPLETION_DETAIL_RESOLVE_CONCURRENCY: usize = 8;

/// Timeout for the `initialize` request (30 seconds).
/// The first request can be slow if tsgo is cold-started (e.g., npx download,
/// first launch, or heavy system load).
const INITIALIZE_TIMEOUT_SECS: u64 = 30;

/// Number of consecutive request timeouts before the transport signals a hang.
/// When reached, `crash_notify` is fired to trigger the `ResilientTypeProvider`'s
/// existing restart machinery (kill process, backoff, re-spawn, replay file cache).
const HANG_THRESHOLD: u32 = 3;

use crate::traits::ProviderPriority;

impl LspTransport {
    /// Get the sender for a given priority lane.
    fn tx_for_priority(&self, priority: ProviderPriority) -> &mpsc::Sender<StdinMessage> {
        match priority {
            ProviderPriority::Interactive => &self.interactive_tx,
            ProviderPriority::Normal => &self.normal_tx,
            ProviderPriority::Background => &self.background_tx,
        }
    }

    /// Send an LSP request at Interactive priority and wait for the response.
    async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, TypeProviderError> {
        self.request_with_priority(
            method,
            params,
            REQUEST_TIMEOUT_SECS,
            ProviderPriority::Interactive,
        )
        .await
    }

    /// Send an LSP request at a specific priority with a custom timeout.
    async fn request_with_priority(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout_secs: u64,
        priority: ProviderPriority,
    ) -> Result<serde_json::Value, TypeProviderError> {
        crate::type_runtime_trace_scope_async!(
            "tsgo_transport_request",
            format!(
                "method={} priority={:?} {}",
                method,
                priority,
                summarize_lsp_params(&params),
            ),
            async {
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);

                let msg = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params,
                });
                let body = serde_json::to_string(&msg)
                    .map_err(|e| TypeProviderError::new(format!("serialize error: {e}")))?;

                let (tx, rx) = oneshot::channel();
                self.pending.lock().await.insert(id, tx);

                let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
                self.tx_for_priority(priority)
                    .send(StdinMessage::Frame(frame.into_bytes()))
                    .await
                    .map_err(|_| TypeProviderError::new("stdin writer closed"))?;

                let result =
                    tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await;
                match result {
                    Ok(Ok(val)) => {
                        // Reset consecutive failures on any successful response
                        self.consecutive_failures.store(0, Ordering::Relaxed);
                        // Check for JSON-RPC error
                        if let Some(err) = val.get("error") {
                            let msg = err
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("unknown error");
                            crate::type_runtime_trace_event!(
                                "tsgo_transport_request_error",
                                format!("method={} id={} message={}", method, id, msg),
                            );
                            return Err(TypeProviderError::new(msg));
                        }
                        crate::type_runtime_trace_event!(
                            "tsgo_transport_request_result",
                            format!(
                                "method={} id={} result_kind={}",
                                method,
                                id,
                                val.get("result")
                                    .map(|result| match result {
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
                        Ok(val
                            .get("result")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null))
                    }
                    Ok(Err(_)) => {
                        crate::type_runtime_trace_event!(
                            "tsgo_transport_request_error",
                            format!(
                                "method={} id={} message=response channel closed",
                                method, id
                            ),
                        );
                        Err(TypeProviderError::new("response channel closed"))
                    }
                    Err(_) => {
                        // Timeout — clean up the pending entry to prevent leak
                        self.pending.lock().await.remove(&id);
                        let count = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
                        if count >= HANG_THRESHOLD {
                            tracing::error!(
                                "TSGO appears hung ({count} consecutive timeouts) — triggering restart"
                            );
                            if let Some(notify) = &self.crash_notify {
                                notify.notify_waiters();
                            }
                        }
                        crate::type_runtime_trace_event!(
                            "tsgo_transport_request_error",
                            format!("method={} id={} message=timeout", method, id),
                        );
                        Err(TypeProviderError::new(format!(
                            "request '{method}' timed out after {timeout_secs}s"
                        )))
                    }
                }
            }
        )
        .await
    }

    /// Send an LSP notification at a specific priority (no response expected).
    /// Uses `try_send()` to prevent backpressure from blocking the caller.
    async fn notify_with_priority(
        &self,
        method: &str,
        params: serde_json::Value,
        priority: ProviderPriority,
    ) -> Result<(), TypeProviderError> {
        crate::type_runtime_trace_scope_async!(
            "tsgo_transport_notify",
            format!(
                "method={} priority={:?} {}",
                method,
                priority,
                summarize_lsp_params(&params),
            ),
            async {
                let msg = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": method,
                    "params": params,
                });
                let body = serde_json::to_string(&msg)
                    .map_err(|e| TypeProviderError::new(format!("serialize error: {e}")))?;

                let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
                match self
                    .tx_for_priority(priority)
                    .try_send(StdinMessage::Frame(frame.into_bytes()))
                {
                    Ok(()) => {
                        crate::type_runtime_trace_event!(
                            "tsgo_transport_notify_result",
                            format!("method={} queued=true", method),
                        );
                        Ok(())
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        tracing::warn!(
                            "TSGO stdin channel full — dropping notification '{method}'"
                        );
                        crate::type_runtime_trace_event!(
                            "tsgo_transport_notify_result",
                            format!("method={} queued=false reason=full", method),
                        );
                        Err(TypeProviderError::new("channel full"))
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        crate::type_runtime_trace_event!(
                            "tsgo_transport_notify_result",
                            format!("method={} queued=false reason=closed", method),
                        );
                        Err(TypeProviderError::new("stdin writer closed"))
                    }
                }
            }
        )
        .await
    }

    /// Send an LSP notification at Interactive priority (no response expected).
    async fn notify(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), TypeProviderError> {
        self.notify_with_priority(method, params, ProviderPriority::Interactive)
            .await
    }
}

/// Drain all pending requests, sending crash error responses so callers
/// fail immediately instead of waiting for the 10s timeout.
async fn drain_pending(pending: &Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>) {
    let mut guard = pending.lock().await;
    for (_id, tx) in guard.drain() {
        let _ = tx.send(serde_json::json!({
            "error": { "code": -32099, "message": "tsgo process crashed" }
        }));
    }
}

/// Read loop that processes JSON-RPC messages from the child's stdout
/// and dispatches responses to pending request channels.
/// Also handles `textDocument/publishDiagnostics` notifications and
/// auto-responds to server→client requests (e.g., `client/registerCapability`).
///
/// When `crash_notify` is provided, it is signaled on any exit (EOF, I/O error,
/// read failure) so that the `ResilientTypeProvider` can detect the crash and restart.
async fn read_loop(
    stdout: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>,
    diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>>,
    contents_cache: Arc<Mutex<HashMap<String, String>>>,
    interactive_tx: mpsc::Sender<StdinMessage>,
    crash_notify: Option<Arc<Notify>>,
) {
    let mut reader = BufReader::new(stdout);
    let mut header_buf = String::new();

    loop {
        // Read headers until we find Content-Length
        let mut content_length: Option<usize> = None;
        header_buf.clear();

        loop {
            header_buf.clear();
            match reader.read_line(&mut header_buf).await {
                Ok(0) => {
                    // EOF — child process exited
                    drain_pending(&pending).await;
                    if let Some(notify) = &crash_notify {
                        notify.notify_waiters();
                    }
                    return;
                }
                Ok(_) => {
                    let line = header_buf.trim();
                    if line.is_empty() {
                        break; // End of headers
                    }
                    if let Some(len_str) = line.strip_prefix("Content-Length:") {
                        if let Ok(len) = len_str.trim().parse::<usize>() {
                            content_length = Some(len);
                        }
                    }
                }
                Err(_) => {
                    // I/O error — child likely crashed
                    drain_pending(&pending).await;
                    if let Some(notify) = &crash_notify {
                        notify.notify_waiters();
                    }
                    return;
                }
            }
        }

        let content_length = match content_length {
            Some(len) => len,
            None => continue, // Malformed message
        };

        // Read the body
        let mut body = vec![0u8; content_length];
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

        let msg: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Determine if this is a server→client request (has "method" + "id"),
        // a notification (has "method" but no "id"), or a response (has "id" but no "method").
        let has_method = msg.get("method").is_some();
        let msg_id = msg.get("id").cloned();

        if let (true, Some(id)) = (has_method, &msg_id) {
            // Server→client request: auto-respond to unblock TSGO.
            let method_str = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
            tracing::debug!("TSGO server→client request: {method_str}");

            // Some requests require specific response shapes to avoid crashing TSGO.
            let result = match method_str {
                // workspace/configuration expects an array of config values matching
                // the number of requested items. Returning `null` crashes tsgo.
                "workspace/configuration" => {
                    let items = msg
                        .get("params")
                        .and_then(|p| p.get("items"))
                        .and_then(|a| a.as_array());
                    let count = items.map(|a| a.len()).unwrap_or(0);
                    serde_json::Value::Array(
                        (0..count)
                            .map(|i| {
                                let section = items
                                    .and_then(|arr| arr.get(i))
                                    .and_then(|item| item.get("section"))
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("");
                                if section.contains("inlayHints") {
                                    serde_json::json!({
                                        "enabled": true,
                                        "variableTypes": { "enabled": true },
                                        "functionLikeReturnTypes": { "enabled": true },
                                        "parameterNames": { "enabled": "literals" }
                                    })
                                } else {
                                    serde_json::json!({})
                                }
                            })
                            .collect(),
                    )
                }
                // Most other requests (client/registerCapability, etc.) accept null.
                _ => serde_json::Value::Null,
            };

            let reply = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            });
            let body = serde_json::to_string(&reply).unwrap_or_default();
            let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
            let _ = interactive_tx
                .send(StdinMessage::Frame(frame.into_bytes()))
                .await;
            continue;
        }

        if has_method {
            // Notification (no id): handle known types
            if let Some(method) = msg.get("method").and_then(|v| v.as_str()) {
                if method == "textDocument/publishDiagnostics" {
                    if let Some(params) = msg.get("params") {
                        let raw_uri = params
                            .get("uri")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        // Normalize the URI so TSGO's percent-encoded lowercase URIs
                        // match our path_to_uri keys (literal colon, original case).
                        let uri = normalize_file_uri(raw_uri);
                        // Look up the file content so we can resolve LSP positions
                        // to byte offsets. The content cache is keyed by file path,
                        // so convert the URI first and normalize for case-insensitive
                        // matching on Windows.
                        let content = {
                            let path = uri_to_file_path(raw_uri);
                            let cache = contents_cache.lock().await;
                            // Try exact match first, then case-insensitive on Windows.
                            #[allow(clippy::unnecessary_lazy_evaluations)]
                            cache.get(&path).cloned().or_else(|| {
                                #[cfg(windows)]
                                {
                                    let lower = path.to_lowercase();
                                    cache
                                        .iter()
                                        .find(|(k, _)| k.to_lowercase() == lower)
                                        .map(|(_, v)| v.clone())
                                }
                                #[cfg(not(windows))]
                                {
                                    None
                                }
                            })
                        };
                        if content.is_some() {
                            let diags = params
                                .get("diagnostics")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|d| parse_lsp_diagnostic(d, content.as_deref()))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            tracing::debug!(
                                "TSGO publishDiagnostics: {} ({} diagnostics)",
                                uri,
                                diags.len()
                            );
                            diagnostics_cache.lock().await.insert(uri, diags);
                        } else {
                            // File not in our cache (tsconfig, node_modules, etc.) — skip
                            tracing::trace!(
                                "TSGO publishDiagnostics: {} (skipped — not a synced file)",
                                uri,
                            );
                        }
                    }
                }
            }
            continue;
        }

        // Response to our request: route by id
        if let Some(id_val) = msg_id {
            if let Some(id) = id_val.as_i64() {
                if let Some(tx) = pending.lock().await.remove(&id) {
                    let _ = tx.send(msg);
                }
            }
        }
    }
}

/// Parse a single LSP Diagnostic JSON value into a `TypeDiagnostic`.
///
/// When `content` is available, resolves LSP positions to byte offsets directly.
/// Otherwise falls back to packed `(line<<16)|character` encoding, which only
/// works for diagnostics on line 0.
///
/// `position_to_offset` interprets `character` as UTF-16 code units (LSP default).
fn parse_lsp_diagnostic(d: &serde_json::Value, content: Option<&str>) -> Option<TypeDiagnostic> {
    let range = d.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    let message = d.get("message")?.as_str()?.to_string();
    let severity = match d.get("severity").and_then(|v| v.as_u64()) {
        Some(1) => TypeDiagnosticSeverity::Error,
        Some(2) => TypeDiagnosticSeverity::Warning,
        Some(3) => TypeDiagnosticSeverity::Info,
        Some(4) => TypeDiagnosticSeverity::Hint,
        _ => TypeDiagnosticSeverity::Error,
    };
    let code = d.get("code").and_then(|v| {
        v.as_u64()
            .map(|n| n.to_string())
            .or_else(|| v.as_str().map(String::from))
    });

    // TSGO returns native LSP `DiagnosticTag`s (1 = Unnecessary, 2 = Deprecated)
    // in an integer array. Map the known values onto the provider-neutral carrier
    // (unknown tag numbers are ignored) so the LSP merge re-emits the fade /
    // strikethrough — the same gray-out parity the `.vue` path needs.
    let tags = d
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| match t.as_u64() {
                    Some(1) => Some(TypeDiagnosticTag::Unnecessary),
                    Some(2) => Some(TypeDiagnosticTag::Deprecated),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    let start_line = start.get("line")?.as_u64()? as u32;
    let start_char = start.get("character")?.as_u64()? as u32;
    let end_line = end.get("line")?.as_u64()? as u32;
    let end_char = end.get("character")?.as_u64()? as u32;

    // When content is available, resolve to actual byte offsets.
    // Without content, fall back to packed positions (only correct for line 0).
    let (start_offset, end_offset) = if let Some(c) = content {
        (
            position_to_offset(c, start_line, start_char),
            position_to_offset(c, end_line, end_char),
        )
    } else {
        (
            pack_position(start_line, start_char),
            pack_position(end_line, end_char),
        )
    };

    Some(TypeDiagnostic {
        message,
        severity,
        start: start_offset,
        end: end_offset,
        code,
        tags,
    })
}

/// Pack an LSP line/character into a u32 for storage.
fn pack_position(line: u32, character: u32) -> u32 {
    // This encoding works for files up to 65535 lines with columns up to 65535.
    (line << 16) | (character & 0xFFFF)
}

/// Convert an LSP `(line, character)` position to a byte offset in content.
///
/// `character` is interpreted according to the given encoding:
/// - UTF-16: character counts UTF-16 code units (tsserver, TSGO default)
/// - UTF-8: character counts bytes
/// - UTF-32: character counts Unicode scalar values
pub fn position_to_offset_with_encoding(
    content: &str,
    line: u32,
    character: u32,
    encoding: PositionEncoding,
) -> u32 {
    let idx = LineIndex::new(content, encoding);
    idx.position_to_offset(crate::codec::LineColumn { line, character })
        .unwrap_or({
            // Fallback: clamp to content length
            content.len() as u32
        })
}

/// Convert an LSP `(line, character)` position to a byte offset in content.
///
/// `character` is interpreted as UTF-16 code units (used by TSGO and tsserver).
fn position_to_offset(content: &str, line: u32, character: u32) -> u32 {
    position_to_offset_with_encoding(content, line, character, PositionEncoding::Utf16)
}

/// Parse an LSP Location JSON value into a `TypeLocation`, using content for offset resolution.
///
/// Converts TSGO's `file://` URI to a filesystem path so downstream code
/// (e.g., `path_to_uri()` in merge.rs) can construct correct URIs without
/// double-wrapping.
fn parse_lsp_location(loc: &serde_json::Value, content: Option<&str>) -> Option<TypeLocation> {
    let uri = loc.get("uri")?.as_str()?;

    // Convert file:// URI to filesystem path for consistent downstream handling.
    // TSGO returns URIs like "file:///d:/dev/.../file.ts", but TypeLocation.path
    // is treated as a filesystem path everywhere it's consumed.
    let path = uri_to_file_path(uri);

    let range = loc.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    let start_line = start.get("line")?.as_u64()? as u32;
    let start_char = start.get("character")?.as_u64()? as u32;
    let end_line = end.get("line")?.as_u64()? as u32;
    let end_char = end.get("character")?.as_u64()? as u32;

    let disk_content;
    let content = if let Some(content) = content {
        Some(content)
    } else {
        disk_content = std::fs::read_to_string(&path).ok();
        disk_content.as_deref()
    };

    let (start_offset, end_offset) = if let Some(c) = content {
        (
            position_to_offset(c, start_line, start_char),
            position_to_offset(c, end_line, end_char),
        )
    } else {
        // Fallback: store packed positions
        (
            pack_position(start_line, start_char),
            pack_position(end_line, end_char),
        )
    };

    Some(TypeLocation {
        path,
        start: start_offset,
        end: end_offset,
    })
}

/// Convert a `file://` URI from TSGO into the shared CANONICAL filesystem-path
/// ID used in every path-bearing DTO this provider returns (`TypeLocation`,
/// `RenameLocation`, `TypeCodeEdit`).
///
/// Handles both Windows (`file:///C:/...`) and Unix (`file:///home/...`) URIs and
/// percent-encoded TSGO URIs (`file:///c%3A/...`), then routes the result through
/// the single canonical-path owner so TSGO emits the SAME ID as the documents/
/// VFS layer (`c:/...`, not `D:/...`). Without this the TSGO provider would
/// split file identity on Windows for go-to-def / hover / rename / code-actions.
fn uri_to_file_path(uri: &str) -> String {
    verter_span::path::canonicalize_path(&file_uri_to_path(uri))
}

/// Percent-decode a URI string. Handles standard `%XX` encoding.
#[cfg(test)]
fn percent_decode_uri(uri: &str) -> String {
    percent_decode(uri)
}

/// Normalize a `file://` URI for use as a cache key.
///
/// TSGO sends URIs with percent-encoding and lowercase paths on Windows
/// (e.g., `file:///c%3A/users/someone/...`), while our `path_to_uri` produces
/// literal colons with original case (e.g., `file:///C:/Users/Someone/...`).
///
/// This function normalizes both forms to the same canonical representation:
/// 1. Percent-decodes the URI (so `%3A` → `:`)
/// 2. On Windows, lowercases the entire URI for case-insensitive matching
fn normalize_file_uri(uri: &str) -> String {
    normalize_file_uri_for_cache(uri)
}

/// Parse an LSP CompletionItem JSON value into a `Completion`.
fn parse_completion_item(item: &serde_json::Value, content: Option<&str>) -> Option<Completion> {
    let label = item.get("label")?.as_str()?.to_string();
    let kind = item.get("kind").and_then(|v| v.as_u64()).map(|k| match k {
        1 => CompletionKind::Text,
        2 => CompletionKind::Method,
        3 => CompletionKind::Function,
        4 => CompletionKind::Method, // Constructor — closest match
        5 => CompletionKind::Field,
        6 => CompletionKind::Variable,
        7 => CompletionKind::Class,
        8 => CompletionKind::Interface,
        9 => CompletionKind::Module,
        10 => CompletionKind::Property,
        12 => CompletionKind::Variable, // Value
        13 => CompletionKind::Enum,
        14 => CompletionKind::Keyword,
        15 => CompletionKind::Snippet,
        17 => CompletionKind::File,
        19 => CompletionKind::Folder,
        20 => CompletionKind::EnumMember,
        21 => CompletionKind::Constant,
        22 => CompletionKind::Class,    // Struct
        23 => CompletionKind::Property, // Event
        24 => CompletionKind::Keyword,  // Operator
        25 => CompletionKind::TypeParameter,
        _ => CompletionKind::Text,
    });
    let detail = item
        .get("detail")
        .and_then(|v| v.as_str())
        .map(String::from);
    let documentation = item.get("documentation").and_then(|v| {
        v.as_str()
            .map(String::from)
            .or_else(|| v.get("value").and_then(|v2| v2.as_str()).map(String::from))
    });
    let insert_text = item
        .get("insertText")
        .and_then(|v| v.as_str())
        .map(String::from);
    let sort_text = item
        .get("sortText")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Parse textEdit range for edit_range_start/end
    let (edit_range_start, edit_range_end) = item
        .get("textEdit")
        .and_then(|te| {
            let range = te.get("range")?;
            let start = range.get("start")?;
            let end = range.get("end")?;
            let sl = start.get("line")?.as_u64()? as u32;
            let sc = start.get("character")?.as_u64()? as u32;
            let el = end.get("line")?.as_u64()? as u32;
            let ec = end.get("character")?.as_u64()? as u32;
            if let Some(c) = content {
                Some((
                    Some(position_to_offset(c, sl, sc)),
                    Some(position_to_offset(c, el, ec)),
                ))
            } else {
                Some((Some(pack_position(sl, sc)), Some(pack_position(el, ec))))
            }
        })
        .unwrap_or((None, None));

    // Preserve the upstream-LSP resolve handle as the provider-pure
    // `CompletionResolveData::Lsp` variant: the item's own `label` plus its
    // opaque `data` blob, replayed verbatim by `resolve_completion`. An item
    // with no `data` carries no resolve handle.
    let data = item
        .get("data")
        .filter(|d| !d.is_null())
        .map(|d| CompletionResolveData::Lsp {
            label: label.clone(),
            data: d.clone(),
        });

    Some(Completion {
        label,
        kind,
        detail,
        documentation,
        edit_range_start,
        edit_range_end,
        insert_text,
        sort_text,
        data,
    })
}

/// Extract the lazy `detail` (signature) and `documentation` from an LSP
/// `completionItem/resolve` response.
///
/// LSP returns `documentation` as either a plain string or a
/// `MarkupContent { kind, value }` object; both spellings are handled. Either
/// field may be absent (the server returned no enrichment for that item), in
/// which case the corresponding slot is `None`.
fn extract_resolve_detail_and_documentation(
    resolve_response: &serde_json::Value,
) -> (Option<String>, Option<String>) {
    let detail = resolve_response
        .get("detail")
        .and_then(|v| v.as_str())
        .map(String::from);
    let documentation = resolve_response.get("documentation").and_then(|v| {
        v.as_str()
            .map(String::from)
            .or_else(|| v.get("value").and_then(|v2| v2.as_str()).map(String::from))
    });
    (detail, documentation)
}

/// Overlay a resolved `detail`/`documentation` onto a completion item without
/// discarding any of its other fields — crucially the typed resolve handle, so a
/// detail-enriched item can still be resolved for auto-import.
///
/// `None` for either slot leaves the item's list-time value untouched (the
/// resolve did not enrich that slot). This mirrors the tsserver-family
/// [`crate::tsserver::ipc::enrich_completion_with_entry_details`] convention so
/// the two provider families behave identically through
/// [`crate::traits::TypeProvider::get_completion_details`].
fn fold_lsp_resolve_detail_into_completion(
    item: &Completion,
    detail: Option<String>,
    documentation: Option<String>,
) -> Completion {
    Completion {
        label: item.label.clone(),
        kind: item.kind,
        detail: detail.or_else(|| item.detail.clone()),
        documentation: documentation.or_else(|| item.documentation.clone()),
        edit_range_start: item.edit_range_start,
        edit_range_end: item.edit_range_end,
        insert_text: item.insert_text.clone(),
        sort_text: item.sort_text.clone(),
        data: item.data.clone(),
    }
}

/// Convert a byte offset into an LSP `(line, character)` position with explicit encoding.
///
/// Returns `character` according to the given encoding:
/// - UTF-16: character counts UTF-16 code units (tsserver, TSGO default)
/// - UTF-8: character counts bytes
/// - UTF-32: character counts Unicode scalar values
pub fn offset_to_position_with_encoding(
    content: &str,
    offset: u32,
    encoding: PositionEncoding,
) -> (u32, u32) {
    let idx = LineIndex::new(content, encoding);
    match idx.offset_to_position(offset) {
        Some(pos) => (pos.line, pos.character),
        None => {
            // Fallback: clamp to end of content
            match idx.offset_to_position(content.len() as u32) {
                Some(pos) => (pos.line, pos.character),
                None => (0, 0),
            }
        }
    }
}

/// Convert a byte offset into an LSP `(line, character)` position.
///
/// Returns `character` as UTF-16 code units (used by TSGO and tsserver).
fn offset_to_position(content: &str, offset: u32) -> (u32, u32) {
    offset_to_position_with_encoding(content, offset, PositionEncoding::Utf16)
}

/// A `TypeProvider` backed by a real TSGO process (`tsgo --lsp --stdio`).
///
/// Spawns the process, initializes the LSP connection, and translates
/// `TypeProvider` method calls into LSP requests.
pub struct TsgoTypeProvider {
    transport: Arc<LspTransport>,
    /// TSGO child process. Killed on drop to prevent orphans.
    child: Child,
    /// Document version counter per path.
    versions: Arc<Mutex<HashMap<String, i32>>>,
    /// Cached file contents for byte-offset → LSP position conversion.
    contents: Arc<Mutex<HashMap<String, String>>>,
    /// Cached diagnostics from textDocument/publishDiagnostics push notifications.
    /// Used as fallback when pull diagnostics (textDocument/diagnostic) fails.
    diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>>,
}

impl Drop for TsgoTypeProvider {
    fn drop(&mut self) {
        // Kill the TSGO child process to prevent orphans.
        // start_kill() is non-blocking (sends TerminateProcess on Windows, SIGKILL on Unix).
        // This is a belt-and-suspenders backup — kill_on_drop(true) on the Command
        // already handles this, but an explicit Drop makes the intent clear.
        let _ = self.child.start_kill();
    }
}

impl TsgoTypeProvider {
    /// Spawn a TSGO process and initialize the LSP connection.
    ///
    /// `tsgo_bin` is the path to the tsgo binary (or just "tsgo" to find it on PATH).
    /// `root_uri` is the workspace root URI (e.g., `file:///tmp/my-project`).
    pub async fn spawn(tsgo_bin: &str, root_uri: &str) -> Result<Self, TypeProviderError> {
        Self::spawn_with_crash_signal(tsgo_bin, root_uri, None).await
    }

    /// Spawn a TSGO process with an optional crash notification signal.
    ///
    /// When `crash_notify` is `Some`, the `Notify` is signaled when the read loop
    /// exits (EOF, I/O error), allowing the `ResilientTypeProvider` to detect the
    /// crash and trigger a restart.
    pub async fn spawn_with_crash_signal(
        tsgo_bin: &str,
        root_uri: &str,
        crash_notify: Option<Arc<Notify>>,
    ) -> Result<Self, TypeProviderError> {
        let mut child = tokio::process::Command::new(tsgo_bin)
            .arg("--lsp")
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| TypeProviderError::new(format!("failed to spawn tsgo: {e}")))?;

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
        // to prevent the deadlock where read_loop blocks on stdin while request()/notify()
        // also hold it.
        // Buffer 1024 messages to accommodate background file sync bursts without backpressure.
        // Three priority channels: Interactive (hover/completion), Normal (imports),
        // Background (workspace scan). Buffer 1024 messages for background burst sync.
        let (interactive_tx, interactive_rx) = mpsc::channel::<StdinMessage>(1024);
        let (normal_tx, normal_rx) = mpsc::channel::<StdinMessage>(1024);
        let (background_tx, background_rx) = mpsc::channel::<StdinMessage>(1024);
        tokio::spawn(stdin_writer_loop(
            stdin,
            interactive_rx,
            normal_rx,
            background_rx,
        ));

        let transport = Arc::new(LspTransport {
            interactive_tx: interactive_tx.clone(),
            normal_tx,
            background_tx,
            pending: Arc::clone(&pending),
            next_id: AtomicI64::new(1),
            consecutive_failures: AtomicU32::new(0),
            crash_notify: crash_notify.as_ref().map(Arc::clone),
        });

        let diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let contents_cache: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Start the read loop in a background task (uses interactive_tx for auto-replies)
        tokio::spawn(read_loop(
            stdout,
            pending,
            Arc::clone(&diagnostics_cache),
            Arc::clone(&contents_cache),
            interactive_tx,
            crash_notify,
        ));

        // Log tsgo stderr in a background task so crashes are visible
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
                                tracing::warn!("TSGO stderr: {line}");
                            }
                        }
                        Err(_) => break,
                    }
                }
                tracing::debug!("TSGO stderr stream closed");
            });
        }

        // Send initialize request (use longer timeout for cold starts).
        //
        // Advertise diagnostic `tagSupport` on BOTH the push (`publishDiagnostics`)
        // and pull (`diagnostic`) channels. An LSP server only attaches
        // `DiagnosticTag`s (1 = Unnecessary fade, 2 = Deprecated strikethrough) when
        // the client declares it understands them; with empty capabilities TSGO
        // silently drops the tags, so an unused `.vue` import would never gray out.
        // The `valueSet` enumerates the tags we render.
        let init_result = transport
            .request_with_priority(
                "initialize",
                serde_json::json!({
                    "processId": std::process::id(),
                    "capabilities": {
                        "textDocument": {
                            "publishDiagnostics": {
                                "tagSupport": { "valueSet": [1, 2] }
                            },
                            "diagnostic": {
                                "tagSupport": { "valueSet": [1, 2] }
                            }
                        }
                    },
                    "rootUri": root_uri,
                    "workspaceFolders": [{
                        "uri": root_uri,
                        "name": "workspace"
                    }]
                }),
                INITIALIZE_TIMEOUT_SECS,
                ProviderPriority::Interactive,
            )
            .await?;

        tracing::debug!("TSGO initialized: {:?}", init_result);

        // Send initialized notification
        transport
            .notify("initialized", serde_json::json!({}))
            .await?;

        Ok(Self {
            transport,
            child,
            versions: Arc::new(Mutex::new(HashMap::new())),
            contents: contents_cache,
            diagnostics_cache,
        })
    }

    /// Convert a file path to a `file://` URI.
    fn path_to_uri(path: &str) -> String {
        path_to_file_uri_string(path)
    }

    /// Send `textDocument/didOpen` at a specific priority.
    fn open_file_with_priority(
        &self,
        path: &str,
        content: &str,
        priority: ProviderPriority,
    ) -> ProviderFuture<'_, ()> {
        let uri = Self::path_to_uri(path);
        let lang_id = if path.ends_with(".tsx") {
            "typescriptreact"
        } else if path.ends_with(".jsx") {
            "javascriptreact"
        } else if path.ends_with(".js") {
            "javascript"
        } else {
            "typescript"
        };
        let content = rewrite_vue_imports_for_tsgo(content, path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsgo_update_file",
                format!(
                    "path={} uri={} content_len={}",
                    path_owned,
                    uri,
                    content.len()
                ),
                async {
                    contents_cache
                        .lock()
                        .await
                        .insert(path_owned.clone(), content.clone());
                    versions.lock().await.insert(path_owned, 1);
                    transport
                        .notify_with_priority(
                            "textDocument/didOpen",
                            serde_json::json!({
                                "textDocument": {
                                    "uri": uri,
                                    "languageId": lang_id,
                                    "version": 1,
                                    "text": content,
                                }
                            }),
                            priority,
                        )
                        .await
                }
            )
            .await
        })
    }

    /// Send `textDocument/didChange` (or `didOpen` if needed) at a specific priority.
    fn update_file_with_priority(
        &self,
        path: &str,
        content: &str,
        priority: ProviderPriority,
    ) -> ProviderFuture<'_, ()> {
        let uri = Self::path_to_uri(path);
        let lang_id = if path.ends_with(".tsx") {
            "typescriptreact"
        } else if path.ends_with(".jsx") {
            "javascriptreact"
        } else if path.ends_with(".js") {
            "javascript"
        } else {
            "typescript"
        };
        let content = rewrite_vue_imports_for_tsgo(content, path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            contents_cache
                .lock()
                .await
                .insert(path_owned.clone(), content.clone());

            let mut vers = versions.lock().await;
            if let Some(v) = vers.get_mut(&path_owned) {
                *v += 1;
                let version = *v;
                drop(vers);
                transport
                    .notify_with_priority(
                        "textDocument/didChange",
                        serde_json::json!({
                            "textDocument": { "uri": uri, "version": version },
                            "contentChanges": [{ "text": content }]
                        }),
                        priority,
                    )
                    .await
            } else {
                vers.insert(path_owned.clone(), 1);
                drop(vers);
                transport
                    .notify_with_priority(
                        "textDocument/didOpen",
                        serde_json::json!({
                            "textDocument": {
                                "uri": uri,
                                "languageId": lang_id,
                                "version": 1,
                                "text": content,
                            }
                        }),
                        priority,
                    )
                    .await
            }
        })
    }

    /// Send `textDocument/didClose` at a specific priority.
    fn close_file_with_priority(
        &self,
        path: &str,
        priority: ProviderPriority,
    ) -> ProviderFuture<'_, ()> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            contents_cache.lock().await.remove(&path_owned);
            versions.lock().await.remove(&path_owned);
            transport
                .notify_with_priority(
                    "textDocument/didClose",
                    serde_json::json!({ "textDocument": { "uri": uri } }),
                    priority,
                )
                .await
        })
    }
}

/// Rewrite carrier import specifiers (`.vue` / `.svelte`) to their api `.ts`
/// virtual file for TSGO cross-file resolution.
///
/// TSGO resolves cross-file carrier imports through the public API output
/// (`Foo.vue.ts` / `Bar.svelte.ts`), which has a proper `export default` for
/// component types. The IDE output (`.tsx`) is a full JSX file that can leak
/// DOM types into importers. The carrier-extension set is the registry's
/// (`LanguageRegistry::carrier_extensions()`) — the single classification
/// authority — not a hand-matched literal.
///
/// NOTE: We use `.vue.ts` (not `.d.vue.ts`) because TypeScript treats
/// `.d.vue.ts` as a declaration file and forbids regular imports from it.
pub(crate) fn rewrite_vue_imports_for_tsgo(content: &str, _path: &str) -> String {
    let mut out = content.to_string();
    for ext in verter_language::LanguageRegistry::global().carrier_extensions() {
        // `ext` is the bare extension WITHOUT a leading dot (`vue` / `svelte`).
        out = out
            .replace(&format!(".{ext}'"), &format!(".{ext}.ts'"))
            .replace(&format!(".{ext}\""), &format!(".{ext}.ts\""));
    }
    out
}

/// Build the `workspace/didChangeConfiguration` payload for TSGO path aliases.
///
/// TSGO 7.0 rejects `baseUrl` (TS5102), so we only send `paths`. TSGO resolves
/// `paths` relative to the tsconfig location, making `baseUrl` unnecessary.
fn build_paths_config_payload(paths: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "settings": {
            "typescript": {
                "tsserver": {
                    "compilerOptions": {
                        "paths": paths,
                    }
                }
            }
        }
    })
}

impl TypeProvider for TsgoTypeProvider {
    fn provider_id(&self) -> &'static str {
        "tsgo"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        tracing::debug!("TSGO open_file: {} ({} bytes)", path, content.len());
        let uri = Self::path_to_uri(path);
        let lang_id = if path.ends_with(".tsx") {
            "typescriptreact"
        } else if path.ends_with(".jsx") {
            "javascriptreact"
        } else if path.ends_with(".js") {
            "javascript"
        } else {
            "typescript"
        };
        let content = rewrite_vue_imports_for_tsgo(content, path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsgo_open_file",
                format!(
                    "path={} uri={} content_len={}",
                    path_owned,
                    uri,
                    content.len()
                ),
                async {
                    contents_cache
                        .lock()
                        .await
                        .insert(path_owned.clone(), content.clone());
                    // Mark as opened with version 1
                    versions.lock().await.insert(path_owned, 1);
                    transport
                        .notify(
                            "textDocument/didOpen",
                            serde_json::json!({
                                "textDocument": {
                                    "uri": uri,
                                    "languageId": lang_id,
                                    "version": 1,
                                    "text": content,
                                }
                            }),
                        )
                        .await?;
                    crate::type_runtime_trace_event!(
                        "tsgo_open_file_result",
                        "opened=true version=1".to_string()
                    );
                    Ok(())
                }
            )
            .await
        })
    }

    /// Cache file content for import resolution without sending `didOpen`.
    ///
    /// Unlike `open_file`, this does NOT notify TSGO — avoiding diagnostic computation
    /// for background-synced files. The content is stored locally so that when the file
    /// IS eventually opened (user navigates to it), `update_file` can send `didOpen`
    /// with the cached content.
    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        tracing::debug!("TSGO load_file: {} ({} bytes)", path, content.len());
        let path_owned = path.to_string();
        let content_owned = rewrite_vue_imports_for_tsgo(content, path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsgo_load_file",
                format!("path={} content_len={}", path_owned, content_owned.len()),
                async {
                    contents_cache
                        .lock()
                        .await
                        .insert(path_owned, content_owned);
                    crate::type_runtime_trace_event!(
                        "tsgo_load_file_result",
                        "cached_only=true".to_string()
                    );
                    Ok(())
                }
            )
            .await
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        tracing::debug!("TSGO update_file: {} ({} bytes)", path, content.len());
        let uri = Self::path_to_uri(path);
        let lang_id = if path.ends_with(".tsx") {
            "typescriptreact"
        } else if path.ends_with(".jsx") {
            "javascriptreact"
        } else if path.ends_with(".js") {
            "javascript"
        } else {
            "typescript"
        };
        let content = rewrite_vue_imports_for_tsgo(content, path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            contents_cache
                .lock()
                .await
                .insert(path_owned.clone(), content.clone());

            let mut vers = versions.lock().await;
            if let Some(v) = vers.get_mut(&path_owned) {
                // File already opened — send didChange
                *v += 1;
                let version = *v;
                drop(vers);
                transport
                    .notify(
                        "textDocument/didChange",
                        serde_json::json!({
                            "textDocument": {
                                "uri": uri,
                                "version": version,
                            },
                            "contentChanges": [{
                                "text": content,
                            }]
                        }),
                    )
                    .await?;
                crate::type_runtime_trace_event!(
                    "tsgo_update_file_result",
                    format!("path={} mode=didChange version={}", path_owned, version),
                );
                Ok(())
            } else {
                // File never opened — must send didOpen first (LSP protocol requirement).
                // Sending didChange without didOpen causes tsgo to panic with
                // "overlay not found for changed file".
                vers.insert(path_owned.clone(), 1);
                drop(vers);
                transport
                    .notify(
                        "textDocument/didOpen",
                        serde_json::json!({
                            "textDocument": {
                                "uri": uri,
                                "languageId": lang_id,
                                "version": 1,
                                "text": content,
                            }
                        }),
                    )
                    .await?;
                crate::type_runtime_trace_event!(
                    "tsgo_update_file_result",
                    format!("path={} mode=didOpen version=1", path_owned),
                );
                Ok(())
            }
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        tracing::debug!("TSGO close_file: {}", path);
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsgo_close_file",
                format!("path={} uri={}", path_owned, uri),
                async {
                    contents_cache.lock().await.remove(&path_owned);
                    versions.lock().await.remove(&path_owned);
                    transport
                        .notify(
                            "textDocument/didClose",
                            serde_json::json!({
                                "textDocument": { "uri": uri }
                            }),
                        )
                        .await?;
                    crate::type_runtime_trace_event!(
                        "tsgo_close_file_result",
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
        tracing::debug!(
            "TSGO get_completions: {} at offset {} (trigger={:?})",
            path,
            offset,
            trigger_character
        );
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let trigger_owned = trigger_character.map(|s| s.to_string());
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => (0, offset, None),
                }
            };

            // Build context with trigger info so TSGO can optimize the response
            let context = if let Some(ref ch) = trigger_owned {
                serde_json::json!({
                    "triggerKind": 2, // TriggerCharacter
                    "triggerCharacter": ch,
                })
            } else {
                serde_json::json!({
                    "triggerKind": 1, // Invoked
                })
            };

            let result = transport
                .request(
                    "textDocument/completion",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                        "context": context,
                    }),
                )
                .await?;

            // Parse: result can be CompletionList { items: [], isIncomplete } or CompletionItem[]
            let (items_slice, is_incomplete) = if let Some(arr) = result.as_array() {
                (arr.as_slice(), false)
            } else if let Some(arr) = result.get("items").and_then(|v| v.as_array()) {
                let incomplete = result
                    .get("isIncomplete")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                (arr.as_slice(), incomplete)
            } else {
                return Ok(CompletionResult {
                    items: vec![],
                    is_incomplete: false,
                });
            };

            let items = items_slice
                .iter()
                .filter_map(|item| parse_completion_item(item, content_snapshot.as_deref()))
                .collect();

            Ok(CompletionResult {
                items,
                is_incomplete,
            })
        })
    }

    /// Enrich a completion list with lazy `detail`/`documentation` via the LSP
    /// `completionItem/resolve` round-trip.
    ///
    /// The bare `textDocument/completion` list omits the signature detail and
    /// documentation for most entries (the server computes them lazily on
    /// resolve). TSGO inheriting the trait default returned items UNCHANGED, so
    /// TSGO-backed members reached the type-expansion backend
    /// (`TypeProviderAdapter::query_members_at_offset`) with no detail while
    /// tsserver-backed members carried `completionEntryDetails` enrichment — the
    /// completion-detail parity gap (GAP-1).
    ///
    /// Only an item carrying the upstream-LSP resolve handle
    /// ([`CompletionResolveData::Lsp`]) can be resolved; an item without one
    /// (no `data` at list time) passes through unchanged. Each resolved item
    /// folds its `detail`/`documentation` via
    /// [`fold_lsp_resolve_detail_into_completion`], preserving its resolve handle
    /// so a later auto-import resolve still works. A per-item resolve failure
    /// degrades to the un-enriched item (never drops it). Returns a list the SAME
    /// length and ORDER as the input (empty only when the input is empty) so the
    /// adapter's `if detailed.is_empty()` fallback keeps the original list.
    ///
    /// **Bounded** (review finding: an unbounded serial hot-path). Each item
    /// needs its own `completionItem/resolve` round-trip (10s transport timeout
    /// each); a naive serial loop costs `N × 10s` worst case on a wedged
    /// provider, and `N` can be large for a member enumeration (`obj.` over a
    /// wide type, a namespace import) reached through
    /// [`crate::provider_adapter::TypeProviderAdapter::query_members_at_offset`].
    /// Two bounds cap that:
    ///   - a LIST-LEVEL cap ([`MAX_COMPLETION_DETAIL_ENRICH`]) — only the leading
    ///     items (sorted-order = most relevant) are enriched; the tail passes
    ///     through unchanged (still present, still resolvable lazily);
    ///   - BOUNDED CONCURRENCY ([`COMPLETION_DETAIL_RESOLVE_CONCURRENCY`]) over
    ///     the enriched subset, so the worst case is
    ///     `ceil(cap / concurrency) × 10s`, not `N × 10s`.
    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        _offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        let uri = Self::path_to_uri(path);
        let transport = Arc::clone(&self.transport);
        Box::pin(async move {
            if items.is_empty() {
                return Ok(Vec::new());
            }
            let enrich_count = items.len().min(MAX_COMPLETION_DETAIL_ENRICH);
            crate::type_runtime_trace_scope_async!(
                "tsgo_get_completion_details",
                format!(
                    "path={} uri={} item_count={} enrich_count={}",
                    path,
                    uri,
                    items.len(),
                    enrich_count
                ),
                async {
                    // Bounded-concurrency enrichment of the leading `enrich_count` items.
                    // Each task owns its inputs (the future cannot borrow `items`) and
                    // reports its index so the output preserves input order. A semaphore
                    // caps in-flight resolves.
                    let semaphore = Arc::new(tokio::sync::Semaphore::new(
                        COMPLETION_DETAIL_RESOLVE_CONCURRENCY,
                    ));
                    let mut join_set: tokio::task::JoinSet<(usize, Completion)> =
                        tokio::task::JoinSet::new();
                    // Re-parent spawned resolves under this scope's span. A spawned
                    // task runs on its own task-local, so capture the active context
                    // here and seed each child future with it.
                    let trace_ctx = crate::trace::current_type_runtime_trace_context();
                    for (idx, item) in items.iter().take(enrich_count).enumerate() {
                        // Only an upstream-LSP resolve handle can be re-issued via
                        // `completionItem/resolve`; an item without one cannot be enriched
                        // and is passed through unchanged (no task spawned).
                        let Some(CompletionResolveData::Lsp { label, data }) = item.data.as_ref()
                        else {
                            continue;
                        };
                        let resolve_item = serde_json::json!({
                            "label": label,
                            "data": data,
                            "textDocument": { "uri": uri },
                        });
                        let transport = Arc::clone(&transport);
                        let semaphore = Arc::clone(&semaphore);
                        let item = item.clone();
                        let resolve_future = async move {
                            // The permit bounds in-flight resolves; if the semaphore is
                            // somehow closed, fall back to the un-enriched item.
                            let _permit = match semaphore.acquire().await {
                                Ok(permit) => permit,
                                Err(_) => return (idx, item),
                            };
                            match transport
                                .request("completionItem/resolve", resolve_item)
                                .await
                            {
                                Ok(resolved) => {
                                    let (detail, documentation) =
                                        extract_resolve_detail_and_documentation(&resolved);
                                    let folded = fold_lsp_resolve_detail_into_completion(
                                        &item,
                                        detail,
                                        documentation,
                                    );
                                    (idx, folded)
                                }
                                // A per-item resolve failure must not drop the item.
                                Err(_) => (idx, item),
                            }
                        };
                        // Only seed the per-task trace state when there is an active
                        // context to re-parent under. With tracing disabled (`trace_ctx
                        // == None`) the spawn skips the task-local wrapper entirely, so
                        // the default path pays no task-local install cost per resolve.
                        match trace_ctx {
                            Some(_) => {
                                join_set.spawn(
                                    crate::trace::with_type_runtime_trace_context_async(
                                        trace_ctx,
                                        resolve_future,
                                    ),
                                );
                            }
                            None => {
                                join_set.spawn(resolve_future);
                            }
                        }
                    }

                    // Start from a verbatim clone (preserves the tail beyond the cap and
                    // any leading item without a resolve handle), then overlay enriched
                    // items by index.
                    let mut enriched: Vec<Completion> = items.to_vec();
                    while let Some(joined) = join_set.join_next().await {
                        if let Ok((idx, completion)) = joined {
                            enriched[idx] = completion;
                        }
                        // A panicked/cancelled task leaves the verbatim clone in place.
                    }

                    crate::type_runtime_trace_event!(
                        "tsgo_get_completion_details_result",
                        format!(
                            "path={} item_count={} enriched_count={} enriched=true",
                            path,
                            enriched.len(),
                            enrich_count
                        ),
                    );
                    Ok(enriched)
                }
            )
            .await
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        tracing::debug!("TSGO get_hover: {} at offset {}", path, offset);
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, cache_hit) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (line, character) = offset_to_position(c, offset);
                        (line, character, true)
                    }
                    None => (0, offset, false),
                }
            };
            crate::type_runtime_trace_scope_async!(
                "tsgo_get_hover",
                format!(
                    "path={} uri={} offset={} line={} character={} content_cache_hit={}",
                    path_owned, uri, offset, line, character, cache_hit,
                ),
                async {
                    let result = transport
                        .request(
                            "textDocument/hover",
                            serde_json::json!({
                                "textDocument": { "uri": uri },
                                "position": { "line": line, "character": character },
                            }),
                        )
                        .await?;

                    if result.is_null() {
                        crate::type_runtime_trace_event!(
                            "tsgo_get_hover_result",
                            format!("path={} has_hover=false", path_owned),
                        );
                        return Ok(None);
                    }

                    tracing::debug!("TSGO hover raw response: {result}");

                    // Parse hover result — handles all LSP content formats:
                    //   MarkupContent: { kind, value }
                    //   MarkedString:  { language, value } | string
                    //   MarkedString[]: array of MarkedString
                    let contents = if let Some(c) = result.get("contents") {
                        if let Some(arr) = c.as_array() {
                            // MarkedString[] — language blocks become fenced code,
                            // plain strings become documentation outside the fence.
                            let mut code_parts = Vec::new();
                            let mut doc_parts = Vec::new();
                            for item in arr {
                                if let Some(s) = item.as_str() {
                                    doc_parts.push(s.to_string());
                                } else if let Some(lang) =
                                    item.get("language").and_then(|l| l.as_str())
                                {
                                    let val = item
                                        .get("value")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default();
                                    code_parts.push(format!("```{lang}\n{val}\n```"));
                                } else if let Some(val) = item.get("value").and_then(|v| v.as_str())
                                {
                                    code_parts.push(val.to_string());
                                }
                            }
                            let mut result = code_parts.join("\n");
                            if !doc_parts.is_empty() {
                                if !result.is_empty() {
                                    result.push_str("\n\n");
                                }
                                result.push_str(&doc_parts.join("\n\n"));
                            }
                            result
                        } else if let Some(value) = c.get("value").and_then(|v| v.as_str()) {
                            value.to_string()
                        } else if let Some(s) = c.as_str() {
                            s.to_string()
                        } else {
                            format!("{c}")
                        }
                    } else {
                        crate::type_runtime_trace_event!(
                            "tsgo_get_hover_result",
                            format!("path={} has_hover=false missing_contents=true", path_owned),
                        );
                        return Ok(None);
                    };

                    crate::type_runtime_trace_event!(
                        "tsgo_get_hover_result",
                        format!(
                            "path={} has_hover=true contents_len={} preview={}",
                            path_owned,
                            contents.len(),
                            trace_preview(&contents, 120),
                        ),
                    );

                    Ok(Some(HoverInfo {
                        contents,
                        range_start: None,
                        range_end: None,
                    }))
                }
            )
            .await
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let diagnostics_cache = Arc::clone(&self.diagnostics_cache);
        Box::pin(async move {
            // Use pull diagnostics (textDocument/diagnostic) — TSGO supports this
            // model rather than push (publishDiagnostics). Pull is synchronous:
            // we send a request and get the diagnostics back directly.
            let result = transport
                .request(
                    "textDocument/diagnostic",
                    serde_json::json!({
                        "textDocument": { "uri": uri }
                    }),
                )
                .await;

            match result {
                Ok(val) => {
                    let content = {
                        let cache = contents_cache.lock().await;
                        cache.get(&path_owned).cloned()
                    };

                    let diags = val
                        .get("items")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|d| parse_lsp_diagnostic(d, content.as_deref()))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    tracing::debug!(
                        "get_diagnostics: pull returned {} diagnostics for {}",
                        diags.len(),
                        uri
                    );

                    // Cache for future reference
                    let cache_key = normalize_file_uri(&uri);
                    diagnostics_cache
                        .lock()
                        .await
                        .insert(cache_key, diags.clone());

                    Ok(diags)
                }
                Err(e) => {
                    // Pull diagnostics failed — fall back to push diagnostics cache.
                    tracing::debug!(
                        "get_diagnostics: pull failed ({e}), falling back to cache for {}",
                        uri
                    );
                    let cache_key = normalize_file_uri(&uri);
                    let cache = diagnostics_cache.lock().await;
                    let result = cache.get(&cache_key).cloned().unwrap_or_default();
                    Ok(result)
                }
            }
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        tracing::debug!("TSGO get_definition: {} at offset {}", path, offset);
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch)
                    }
                    None => (0, offset),
                }
            };
            let result = transport
                .request(
                    "textDocument/definition",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                    }),
                )
                .await?;

            let locations = if result.is_array() {
                result.as_array().cloned().unwrap_or_default()
            } else if result.is_object() {
                vec![result]
            } else {
                return Ok(vec![]);
            };

            let cache = contents_cache.lock().await;
            Ok(locations
                .iter()
                .filter_map(|loc| {
                    let target_path = loc
                        .get("uri")
                        .and_then(|value| value.as_str())
                        .map(uri_to_file_path)?;
                    let target_content = if target_path == path_owned {
                        cache.get(&path_owned).map(|text| text.as_str())
                    } else {
                        cache.get(&target_path).map(|text| text.as_str())
                    };
                    parse_lsp_location(loc, target_content)
                })
                .collect())
        })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        tracing::debug!("TSGO get_type_definition: {} at offset {}", path, offset);
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch)
                    }
                    None => (0, offset),
                }
            };
            let result = transport
                .request(
                    "textDocument/typeDefinition",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                    }),
                )
                .await?;

            let locations = if result.is_array() {
                result.as_array().cloned().unwrap_or_default()
            } else if result.is_object() {
                vec![result]
            } else {
                return Ok(vec![]);
            };

            let cache = contents_cache.lock().await;
            Ok(locations
                .iter()
                .filter_map(|loc| {
                    let target_path = loc
                        .get("uri")
                        .and_then(|value| value.as_str())
                        .map(uri_to_file_path)?;
                    let target_content = if target_path == path_owned {
                        cache.get(&path_owned).map(|text| text.as_str())
                    } else {
                        cache.get(&target_path).map(|text| text.as_str())
                    };
                    parse_lsp_location(loc, target_content)
                })
                .collect())
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        tracing::debug!("TSGO get_references: {} at offset {}", path, offset);
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => (0, offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/references",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                        "context": { "includeDeclaration": true }
                    }),
                )
                .await?;

            let locations = result.as_array().cloned().unwrap_or_default();
            Ok(locations
                .iter()
                .filter_map(|loc| parse_lsp_location(loc, content_snapshot.as_deref()))
                .collect())
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => (0, offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/rename",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                        "newName": "__verter_rename_probe__",
                    }),
                )
                .await?;

            if result.is_null() {
                return Ok(vec![]);
            }

            let mut locations = Vec::new();
            parse_workspace_edit_locations(&result, content_snapshot.as_deref(), &mut locations);
            Ok(locations)
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => offset_to_position(c, offset),
                    None => (0, offset),
                }
            };
            let result = transport
                .request(
                    "textDocument/signatureHelp",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                    }),
                )
                .await?;

            if result.is_null() {
                return Ok(None);
            }

            Ok(Some(parse_signature_help(&result)))
        })
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (start_line, start_char, end_line, end_char, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (sl, sc) = offset_to_position(c, start_offset);
                        let (el, ec) = offset_to_position(c, end_offset);
                        (sl, sc, el, ec, Some(c.clone()))
                    }
                    None => (0, start_offset, 0, end_offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/codeAction",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "range": {
                            "start": { "line": start_line, "character": start_char },
                            "end": { "line": end_line, "character": end_char },
                        },
                        "context": { "diagnostics": [] },
                    }),
                )
                .await?;

            let items = result.as_array().cloned().unwrap_or_default();
            Ok(items
                .iter()
                .filter_map(|item| parse_code_action(item, content_snapshot.as_deref()))
                .collect())
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let content_snapshot = {
                let cache = contents_cache.lock().await;
                cache.get(&path_owned).cloned()
            };
            let result = transport
                .request(
                    "textDocument/semanticTokens/full",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                    }),
                )
                .await?;

            let data = result
                .get("data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            Ok(decode_semantic_tokens(&data, content_snapshot.as_deref()))
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => (0, offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/documentHighlight",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                    }),
                )
                .await?;

            let items = result.as_array().cloned().unwrap_or_default();
            Ok(items
                .iter()
                .filter_map(|item| parse_document_highlight(item, content_snapshot.as_deref()))
                .collect())
        })
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (start_line, start_char, end_line, end_char, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (sl, sc) = offset_to_position(c, start_offset);
                        let (el, ec) = offset_to_position(c, end_offset);
                        (sl, sc, el, ec, Some(c.clone()))
                    }
                    None => (0, start_offset, 0, end_offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/inlayHint",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "range": {
                            "start": { "line": start_line, "character": start_char },
                            "end": { "line": end_line, "character": end_char },
                        },
                    }),
                )
                .await?;

            let items = result.as_array().cloned().unwrap_or_default();
            Ok(items
                .iter()
                .filter_map(|item| parse_inlay_hint(item, content_snapshot.as_deref()))
                .collect())
        })
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        let uri = Self::path_to_uri(path);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let path_owned = path.to_string();
        Box::pin(async move {
            // TSGO resolves through the upstream-LSP handle. A non-LSP resolve
            // key cannot have originated from this provider — fail closed.
            let CompletionResolveData::Lsp { label, data } = data else {
                return Ok(None);
            };
            // Reissue the upstream `completionItem/resolve` with the entry's own
            // `label` + opaque `data` carried on the typed `Lsp` resolve handle
            // (both captured by `parse_tsgo_completion` at list time). The label
            // is the entry's real label — the upstream server needs the original
            // completion item identity to resolve its `additionalTextEdits`.
            let resolve_item = serde_json::json!({
                "label": label,
                "data": data,
                "textDocument": { "uri": uri },
            });

            let result = transport
                .request("completionItem/resolve", resolve_item)
                .await?;

            // Parse additionalTextEdits from the response
            let edits = result
                .get("additionalTextEdits")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if edits.is_empty() {
                return Ok(None);
            }

            let content_snapshot = {
                let cache = contents_cache.lock().await;
                cache.get(&path_owned).cloned()
            };

            let additional_text_edits: Vec<ResolvedTextEdit> = edits
                .iter()
                .filter_map(|edit| {
                    let range = edit.get("range")?;
                    let start = range.get("start")?;
                    let end = range.get("end")?;
                    let sl = start.get("line")?.as_u64()? as u32;
                    let sc = start.get("character")?.as_u64()? as u32;
                    let el = end.get("line")?.as_u64()? as u32;
                    let ec = end.get("character")?.as_u64()? as u32;
                    let new_text = edit.get("newText")?.as_str()?.to_string();

                    let (start_offset, end_offset) = if let Some(ref c) = content_snapshot {
                        (position_to_offset(c, sl, sc), position_to_offset(c, el, ec))
                    } else {
                        (pack_position(sl, sc), pack_position(el, ec))
                    };

                    Some(ResolvedTextEdit {
                        start: start_offset,
                        end: end_offset,
                        new_text,
                    })
                })
                .collect();

            if additional_text_edits.is_empty() {
                Ok(None)
            } else {
                Ok(Some(CompletionResolveResult {
                    additional_text_edits,
                    ..Default::default()
                }))
            }
        })
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        let transport = Arc::clone(&self.transport);
        Box::pin(async move {
            // Best-effort: try shutdown request + exit notification with overall 3s timeout.
            // If TSGO is unresponsive, we don't hang — the child has kill_on_drop.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                let _ = transport.request("shutdown", serde_json::Value::Null).await;
                let _ = transport.notify("exit", serde_json::Value::Null).await;
            })
            .await;
            // Signal the writer task to stop.
            let _ = transport.interactive_tx.send(StdinMessage::Shutdown).await;
            Ok(())
        })
    }

    fn configure_paths(&self, _base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        let transport = Arc::clone(&self.transport);
        Box::pin(async move {
            transport
                .notify(
                    "workspace/didChangeConfiguration",
                    build_paths_config_payload(paths),
                )
                .await
        })
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        let transport = Arc::clone(&self.transport);
        Box::pin(async move {
            transport
                .notify(
                    "workspace/didChangeWorkspaceFolders",
                    serde_json::json!({
                        "event": {
                            "added": added,
                            "removed": removed,
                        }
                    }),
                )
                .await
        })
    }

    fn child_pid(&self) -> Option<u32> {
        self.child.id()
    }

    // ── Background-priority overrides ────────────────────────────────

    fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file_with_priority(path, content, ProviderPriority::Background)
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        // load_file is local-only (no TSGO notification), priority irrelevant
        self.load_file(path, content)
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.update_file_with_priority(path, content, ProviderPriority::Background)
    }

    fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.close_file_with_priority(path, ProviderPriority::Background)
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let diagnostics_cache = Arc::clone(&self.diagnostics_cache);
        Box::pin(async move {
            let result = transport
                .request_with_priority(
                    "textDocument/diagnostic",
                    serde_json::json!({ "textDocument": { "uri": uri } }),
                    REQUEST_TIMEOUT_SECS,
                    ProviderPriority::Background,
                )
                .await;
            match result {
                Ok(val) => {
                    let items = val
                        .get("items")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let content = contents_cache.lock().await.get(&path_owned).cloned();
                    Ok(items
                        .iter()
                        .filter_map(|d| parse_lsp_diagnostic(d, content.as_deref()))
                        .collect())
                }
                Err(_) => {
                    let cache = diagnostics_cache.lock().await;
                    let normalized = normalize_file_uri(&Self::path_to_uri(&path_owned));
                    Ok(cache.get(&normalized).cloned().unwrap_or_default())
                }
            }
        })
    }

    fn configure_paths_background(
        &self,
        _base_url: &str,
        paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        let transport = Arc::clone(&self.transport);
        Box::pin(async move {
            transport
                .notify_with_priority(
                    "workspace/didChangeConfiguration",
                    build_paths_config_payload(paths),
                    ProviderPriority::Background,
                )
                .await
        })
    }

    fn update_workspace_folders_background(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        let transport = Arc::clone(&self.transport);
        Box::pin(async move {
            transport
                .notify_with_priority(
                    "workspace/didChangeWorkspaceFolders",
                    serde_json::json!({
                        "event": { "added": added, "removed": removed }
                    }),
                    ProviderPriority::Background,
                )
                .await
        })
    }

    // ── Normal-priority overrides ────────────────────────────────────

    fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file_with_priority(path, content, ProviderPriority::Normal)
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.load_file(path, content)
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.update_file_with_priority(path, content, ProviderPriority::Normal)
    }

    fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.close_file_with_priority(path, ProviderPriority::Normal)
    }
}

/// Extract rename locations from a WorkspaceEdit JSON response.
fn parse_workspace_edit_locations(
    result: &serde_json::Value,
    content: Option<&str>,
    locations: &mut Vec<RenameLocation>,
) {
    // Handle `changes: { [uri]: TextEdit[] }` format
    if let Some(changes) = result.get("changes").and_then(|v| v.as_object()) {
        for (change_uri, edits) in changes {
            if let Some(arr) = edits.as_array() {
                for edit in arr {
                    if let Some(loc) = parse_rename_edit(change_uri, edit, content) {
                        locations.push(loc);
                    }
                }
            }
        }
    }
    // Handle `documentChanges: TextDocumentEdit[]` format
    if let Some(doc_changes) = result.get("documentChanges").and_then(|v| v.as_array()) {
        for dc in doc_changes {
            let dc_uri = dc
                .get("textDocument")
                .and_then(|td| td.get("uri"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if let Some(edits) = dc.get("edits").and_then(|v| v.as_array()) {
                for edit in edits {
                    if let Some(loc) = parse_rename_edit(dc_uri, edit, content) {
                        locations.push(loc);
                    }
                }
            }
        }
    }
}

fn parse_rename_edit(
    uri: &str,
    edit: &serde_json::Value,
    content: Option<&str>,
) -> Option<RenameLocation> {
    let range = edit.get("range")?;
    let (start, end) = parse_range_to_offsets(range, content)?;
    Some(RenameLocation {
        // Canonical filesystem-path ID, matching `TypeLocation.path` and the
        // tsserver provider — NOT the raw `file://` URI (which would split file
        // identity vs the documents/VFS layer on Windows).
        path: uri_to_file_path(uri),
        start,
        end,
    })
}

/// Parse a SignatureHelp from a JSON response.
fn parse_signature_help(result: &serde_json::Value) -> SignatureHelp {
    let signatures = result
        .get("signatures")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|sig| {
                    let label = sig.get("label")?.as_str()?.to_string();
                    let documentation = sig.get("documentation").and_then(extract_markup_string);
                    let parameters = sig
                        .get("parameters")
                        .and_then(|v| v.as_array())
                        .map(|params| {
                            params
                                .iter()
                                .filter_map(|p| {
                                    let plabel = p.get("label")?.as_str()?.to_string();
                                    let pdoc =
                                        p.get("documentation").and_then(extract_markup_string);
                                    Some(ParameterInfo {
                                        label: plabel,
                                        documentation: pdoc,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(SignatureInfo {
                        label,
                        documentation,
                        parameters,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    SignatureHelp {
        signatures,
        active_signature: result
            .get("activeSignature")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        active_parameter: result
            .get("activeParameter")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
    }
}

/// Parse a CodeAction from a JSON response.
fn parse_code_action(item: &serde_json::Value, content: Option<&str>) -> Option<TypeCodeAction> {
    let title = item.get("title")?.as_str()?.to_string();
    let kind = item.get("kind").and_then(|v| v.as_str()).map(String::from);

    let mut edits = Vec::new();
    if let Some(edit) = item.get("edit") {
        if let Some(changes) = edit.get("changes").and_then(|v| v.as_object()) {
            for (change_uri, text_edits) in changes {
                if let Some(arr) = text_edits.as_array() {
                    for te in arr {
                        if let Some(ce) = parse_text_edit_to_code_edit(change_uri, te, content) {
                            edits.push(ce);
                        }
                    }
                }
            }
        }
        if let Some(doc_changes) = edit.get("documentChanges").and_then(|v| v.as_array()) {
            for dc in doc_changes {
                let dc_uri = dc
                    .get("textDocument")
                    .and_then(|td| td.get("uri"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if let Some(arr) = dc.get("edits").and_then(|v| v.as_array()) {
                    for te in arr {
                        if let Some(ce) = parse_text_edit_to_code_edit(dc_uri, te, content) {
                            edits.push(ce);
                        }
                    }
                }
            }
        }
    }

    Some(TypeCodeAction { title, kind, edits })
}

fn parse_text_edit_to_code_edit(
    uri: &str,
    te: &serde_json::Value,
    content: Option<&str>,
) -> Option<TypeCodeEdit> {
    let range = te.get("range")?;
    let new_text = te.get("newText")?.as_str()?.to_string();
    let (start, end) = parse_range_to_offsets(range, content)?;
    Some(TypeCodeEdit {
        // Canonical filesystem-path ID (see `parse_rename_edit`), not the raw URI.
        path: uri_to_file_path(uri),
        start,
        end,
        new_text,
    })
}

/// Decode delta-encoded semantic tokens into absolute-offset tokens.
fn decode_semantic_tokens(data: &[serde_json::Value], content: Option<&str>) -> Vec<SemanticToken> {
    if data.len() < 5 {
        return vec![];
    }
    let mut tokens = Vec::new();
    let mut current_line = 0u32;
    let mut current_start = 0u32;

    for chunk in data.chunks_exact(5) {
        let delta_line = chunk[0].as_u64().unwrap_or(0) as u32;
        let delta_start = chunk[1].as_u64().unwrap_or(0) as u32;
        let length = chunk[2].as_u64().unwrap_or(0) as u32;
        let token_type = chunk[3].as_u64().unwrap_or(0) as u32;
        let token_modifiers = chunk[4].as_u64().unwrap_or(0) as u32;

        if delta_line > 0 {
            current_line += delta_line;
            current_start = delta_start;
        } else {
            current_start += delta_start;
        }

        let start = if let Some(c) = content {
            position_to_offset(c, current_line, current_start)
        } else {
            pack_position(current_line, current_start)
        };

        tokens.push(SemanticToken {
            start,
            length,
            token_type,
            token_modifiers,
        });
    }

    tokens
}

/// Parse a DocumentHighlight from a JSON value.
fn parse_document_highlight(
    item: &serde_json::Value,
    content: Option<&str>,
) -> Option<TypeDocumentHighlight> {
    let range = item.get("range")?;
    let (start, end) = parse_range_to_offsets(range, content)?;
    let kind = match item.get("kind").and_then(|v| v.as_u64()) {
        Some(2) => TypeDocumentHighlightKind::Read,
        Some(3) => TypeDocumentHighlightKind::Write,
        _ => TypeDocumentHighlightKind::Text,
    };
    Some(TypeDocumentHighlight { start, end, kind })
}

/// Parse an LSP InlayHint JSON value into an `InlayHint`.
fn parse_inlay_hint(item: &serde_json::Value, content: Option<&str>) -> Option<InlayHint> {
    let pos = item.get("position")?;
    let line = pos.get("line")?.as_u64()? as u32;
    let character = pos.get("character")?.as_u64()? as u32;

    let offset = if let Some(c) = content {
        position_to_offset(c, line, character)
    } else {
        pack_position(line, character)
    };

    // label can be a string or an array of InlayHintLabelPart
    let label = if let Some(s) = item.get("label").and_then(|v| v.as_str()) {
        s.to_string()
    } else if let Some(parts) = item.get("label").and_then(|v| v.as_array()) {
        parts
            .iter()
            .filter_map(|p| p.get("value").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("")
    } else {
        return None;
    };

    let kind = item
        .get("kind")
        .and_then(|v| v.as_u64())
        .and_then(|k| match k {
            1 => Some(InlayHintKind::Type),
            2 => Some(InlayHintKind::Parameter),
            _ => None,
        });

    Some(InlayHint {
        position: offset,
        label,
        kind,
        padding_left: item.get("paddingLeft").and_then(|v| v.as_bool()),
        padding_right: item.get("paddingRight").and_then(|v| v.as_bool()),
    })
}

/// Parse a JSON range `{ start: { line, character }, end: { line, character } }` to byte offsets.
fn parse_range_to_offsets(range: &serde_json::Value, content: Option<&str>) -> Option<(u32, u32)> {
    let start = range.get("start")?;
    let end = range.get("end")?;
    let sl = start.get("line")?.as_u64()? as u32;
    let sc = start.get("character")?.as_u64()? as u32;
    let el = end.get("line")?.as_u64()? as u32;
    let ec = end.get("character")?.as_u64()? as u32;

    if let Some(c) = content {
        Some((position_to_offset(c, sl, sc), position_to_offset(c, el, ec)))
    } else {
        Some((pack_position(sl, sc), pack_position(el, ec)))
    }
}

/// Extract a string from a MarkupContent or plain string JSON value.
fn extract_markup_string(v: &serde_json::Value) -> Option<String> {
    v.as_str()
        .map(String::from)
        .or_else(|| v.get("value").and_then(|v2| v2.as_str()).map(String::from))
}

/// The explicit, highest-precedence tsgo-binary override env var.
///
/// Mirrors how `--tsdk` lets a user pin the tsserver SDK: when set and pointing
/// at an existing file, this exact path wins over every discovered location. It
/// is the escape hatch for a non-standard install (e.g. a hand-built tsgo) and
/// keeps the canonical precedence honest (explicit override first).
pub const TSGO_BINARY_ENV: &str = "VERTER_TSGO_BIN";

/// Canonical tsgo discovery for production and tests.
///
/// Searches in strict precedence order so the same tsgo is found regardless of
/// entry point (R-Shared-Optimized-Codebase: one shared discovery path, not a
/// test-harness-only fork):
///
/// 1. **Explicit override** — the `VERTER_TSGO_BIN` env var, when it names an
///    existing file (the analog of `--tsdk` for tsserver).
/// 2. **Workspace `node_modules`** — the `@typescript/native-preview-*` binary
///    installed as a workspace dependency (flat-npm OR pnpm layout). This is the
///    common real-project case (a project that pins `@typescript/native-preview`
///    in `package.json`) that PATH + the npm/npx cache miss.
/// 3. **PATH** — a `tsgo` on `PATH`.
/// 4. **npm/npx cache** — the native binary / shim under the npm or npx cache.
///
/// `workspace_root` is the directory whose `node_modules` is searched in tier 2;
/// pass `None` (or a root without a matching `node_modules`) to skip straight to
/// PATH + cache. Returns the existing [`TsgoBinaryLookupError`] (PATH + cache
/// checked-locations) when no binary is found in any tier.
pub fn find_tsgo_binary_canonical(
    workspace_root: Option<&std::path::Path>,
) -> Result<String, TsgoBinaryLookupError> {
    // Tier 1: explicit override.
    if let Some(path) = tsgo_binary_env_override() {
        tracing::debug!("TSGO discovery: using {TSGO_BINARY_ENV} override at {path}");
        return Ok(path);
    }

    // Tier 2: workspace node_modules (flat-npm + pnpm).
    if let Some(root) = workspace_root {
        let node_modules = root.join("node_modules");
        if let Some(path) = find_tsgo_binary_under_node_modules(&node_modules) {
            tracing::debug!("TSGO discovery: found in workspace node_modules at {path}");
            return Ok(path);
        }
    }

    // Tiers 3 + 4: PATH, then npm/npx cache.
    find_tsgo_binary()
}

/// Read the [`TSGO_BINARY_ENV`] override, returning it only when it names an
/// existing file. An unset or stale (non-existent) override is ignored so a
/// leftover env var never wedges discovery.
fn tsgo_binary_env_override() -> Option<String> {
    let raw = std::env::var_os(TSGO_BINARY_ENV)?;
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(&raw);
    path.is_file().then(|| path.to_string_lossy().to_string())
}

/// Find the tsgo binary on PATH or the npm/npx cache.
///
/// Checks (in order):
/// 1. `tsgo` on PATH
/// 2. Native binary from npm/npx cache (`@typescript/native-preview-{platform}/lib/tsgo`)
/// 3. npm/npx shims in cache
///
/// This is tiers 3+4 of [`find_tsgo_binary_canonical`]; production should call
/// the canonical entry point so the explicit override and workspace
/// `node_modules` tiers are honored.
pub fn find_tsgo_binary() -> Result<String, TsgoBinaryLookupError> {
    let cache_roots = collect_npm_cache_roots(
        npm_config_cache_from_env(),
        npm_config_get_cache(),
        default_npm_cache_root(),
    );
    tracing::debug!("TSGO discovery: cache roots = {:?}", cache_roots);

    let result = find_tsgo_binary_in(which_cmd("tsgo"), &cache_roots);
    match &result {
        Ok(path) => tracing::debug!("TSGO discovery: selected binary at {path}"),
        Err(err) => tracing::debug!("TSGO discovery failed: {err}"),
    }
    result
}

/// Resolve the tsgo native binary from a workspace `node_modules` directory.
///
/// `find_tsgo_binary` searches PATH + the npm/npx cache, which misses a tsgo
/// installed as a workspace dependency (pnpm or flat npm layout). This locates
/// the platform-specific `@typescript/native-preview-{plat}-{arch}` binary
/// directly under `<node_modules>`:
///
/// - flat npm: `<node_modules>/@typescript/native-preview-{plat}/lib/tsgo[.exe]`
/// - pnpm:     `<node_modules>/.pnpm/@typescript+native-preview-{plat}@*/node_modules/@typescript/native-preview-{plat}/lib/tsgo[.exe]`
///
/// Platform-aware (reuses [`tsgo_native_binary_rel_paths`]); returns `None` when
/// no binary is present. Paths are built with `Path::join`, never string
/// concatenation, so it is portable across macOS / Windows / Linux.
pub fn find_tsgo_binary_under_node_modules(node_modules: &std::path::Path) -> Option<String> {
    // Flat npm layout: <node_modules>/@typescript/native-preview-*/lib/tsgo[.exe]
    for candidate in flat_npm_tsgo_candidate_paths(node_modules) {
        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }

    // pnpm layout: <node_modules>/.pnpm/<pkg>@<ver>/node_modules/@typescript/native-preview-*/lib/tsgo[.exe]
    let pnpm_dir = node_modules.join(".pnpm");
    if let Ok(entries) = std::fs::read_dir(&pnpm_dir) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("@typescript+native-preview-"))
                    .unwrap_or(false)
            })
            .collect();
        // Prefer the most recently modified store entry (newest install).
        dirs.sort_by_key(|b| std::cmp::Reverse(entry_modified(b)));
        for dir in dirs {
            for candidate in pnpm_store_tsgo_candidate_paths(&dir) {
                if candidate.exists() {
                    return Some(candidate.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

/// Build the flat-npm tsgo candidate paths under a `node_modules` directory.
///
/// Pure path construction (no filesystem access) so the layout math is unit
/// testable on every platform: `<node_modules>/@typescript/native-preview-{plat}-{arch}/lib/tsgo[.exe]`.
/// Built with `Path::join` (never string concatenation) for portability.
fn flat_npm_tsgo_candidate_paths(node_modules: &std::path::Path) -> Vec<PathBuf> {
    tsgo_native_binary_rel_paths()
        .into_iter()
        .map(|rel| {
            // `rel` is rooted at "node_modules/…"; strip that prefix to join
            // under the given node_modules dir.
            let rel_under_nm = rel.strip_prefix("node_modules/").unwrap_or(rel);
            node_modules.join(rel_under_nm)
        })
        .collect()
}

/// Build the pnpm-store tsgo candidate paths under a single pnpm store entry
/// (`<node_modules>/.pnpm/@typescript+native-preview-{plat}@{ver}`).
///
/// Pure path construction (no filesystem access): the store entry nests a real
/// `node_modules/@typescript/native-preview-{plat}-{arch}/lib/tsgo[.exe]`, so
/// the relative paths join verbatim. Built with `Path::join` for portability.
fn pnpm_store_tsgo_candidate_paths(store_entry: &std::path::Path) -> Vec<PathBuf> {
    tsgo_native_binary_rel_paths()
        .into_iter()
        .map(|rel| store_entry.join(rel))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsgoBinaryLookupError {
    checked_locations: Vec<String>,
}

impl TsgoBinaryLookupError {
    fn new(checked_locations: Vec<String>) -> Self {
        Self { checked_locations }
    }
}

impl std::fmt::Display for TsgoBinaryLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.checked_locations.is_empty() {
            write!(f, "tsgo binary not found")
        } else {
            write!(
                f,
                "tsgo binary not found; checked {}",
                self.checked_locations.join(", ")
            )
        }
    }
}

impl std::error::Error for TsgoBinaryLookupError {}

fn find_tsgo_binary_in(
    path_hit: Option<String>,
    cache_roots: &[PathBuf],
) -> Result<String, TsgoBinaryLookupError> {
    if let Some(path) = path_hit {
        tracing::debug!("TSGO discovery: found on PATH at {path}");
        return Ok(path);
    }

    let mut checked_locations = Vec::new();
    let mut npx_entries = Vec::new();

    for cache_root in cache_roots {
        push_checked_location(&mut checked_locations, cache_root.display().to_string());
        let npx_dir = cache_root.join("_npx");
        push_checked_location(&mut checked_locations, npx_dir.display().to_string());

        let Ok(entries) = std::fs::read_dir(&npx_dir) else {
            continue;
        };

        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                npx_entries.push(entry.path());
            }
        }
    }

    npx_entries.sort_by_key(|b| std::cmp::Reverse(entry_modified(b)));

    for entry in &npx_entries {
        for rel_path in tsgo_native_binary_rel_paths() {
            let candidate = entry.join(rel_path);
            push_checked_location(&mut checked_locations, candidate.display().to_string());
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
            }
        }
    }

    for entry in &npx_entries {
        for rel_path in tsgo_shim_rel_paths() {
            let candidate = entry.join(rel_path);
            push_checked_location(&mut checked_locations, candidate.display().to_string());
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
            }
        }
    }

    Err(TsgoBinaryLookupError::new(checked_locations))
}

fn which_cmd(cmd: &str) -> Option<String> {
    let which = if cfg!(windows) { "where" } else { "which" };
    std::process::Command::new(which)
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| pick_best_which_candidate(&s).map(|c| c.to_string()))
}

/// Pick the best candidate from `where`/`which` output.
///
/// On Windows, `where tsgo` may return multiple lines:
/// ```text
/// C:\Program Files\nodejs\tsgo       ← POSIX shell script (npm shim for Git Bash)
/// C:\Program Files\nodejs\tsgo.cmd   ← Windows cmd shim
/// ```
/// A POSIX shell script is not executable via `CreateProcess`, so we prefer
/// `.exe` > `.cmd` > `.bat` > first candidate. On Unix, `which` returns a
/// single line so the preference is a no-op.
fn pick_best_which_candidate(output: &str) -> Option<&str> {
    let candidates: Vec<&str> = output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if candidates.is_empty() {
        return None;
    }

    // Priority order: .exe > .cmd > .bat
    let extensions: &[&str] = &[".exe", ".cmd", ".bat"];
    for ext in extensions {
        if let Some(c) = candidates
            .iter()
            .find(|c| c.len() >= ext.len() && c[c.len() - ext.len()..].eq_ignore_ascii_case(ext))
        {
            return Some(c);
        }
    }

    // Fallback: first candidate (Unix `which` output, or Windows without known extensions)
    Some(candidates[0])
}

fn collect_npm_cache_roots(
    env_cache: Option<PathBuf>,
    npm_config_cache: Option<PathBuf>,
    default_root: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    push_cache_root(&mut roots, env_cache);
    push_cache_root(&mut roots, npm_config_cache);
    push_cache_root(&mut roots, default_root);
    roots
}

fn npm_config_cache_from_env() -> Option<PathBuf> {
    std::env::var_os("NPM_CONFIG_CACHE")
        .or_else(|| std::env::var_os("npm_config_cache"))
        .map(PathBuf::from)
}

fn npm_config_get_cache() -> Option<PathBuf> {
    let npm_cmd = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let output = std::process::Command::new(npm_cmd)
        .args(["config", "get", "cache"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let cache = stdout.lines().next()?.trim();
    if cache.is_empty() || matches!(cache, "undefined" | "null") {
        None
    } else {
        Some(PathBuf::from(cache))
    }
}

fn default_npm_cache_root() -> Option<PathBuf> {
    // On Windows: %LOCALAPPDATA%/npm-cache
    // On Unix: ~/.npm
    if cfg!(windows) {
        std::env::var("LOCALAPPDATA")
            .ok()
            .map(|d| PathBuf::from(d).join("npm-cache"))
    } else {
        dirs_or_home().map(|d| d.join(".npm"))
    }
}

fn dirs_or_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn tsgo_native_binary_rel_paths() -> Vec<&'static str> {
    let mut rel_paths = Vec::new();

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    rel_paths.push("node_modules/@typescript/native-preview-win32-x64/lib/tsgo.exe");
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    rel_paths.push("node_modules/@typescript/native-preview-win32-arm64/lib/tsgo.exe");
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    rel_paths.push("node_modules/@typescript/native-preview-linux-x64/lib/tsgo");
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    rel_paths.push("node_modules/@typescript/native-preview-linux-arm64/lib/tsgo");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    rel_paths.push("node_modules/@typescript/native-preview-darwin-x64/lib/tsgo");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    rel_paths.push("node_modules/@typescript/native-preview-darwin-arm64/lib/tsgo");

    for rel_path in [
        "node_modules/@typescript/native-preview-win32-x64/lib/tsgo.exe",
        "node_modules/@typescript/native-preview-win32-arm64/lib/tsgo.exe",
        "node_modules/@typescript/native-preview-linux-x64/lib/tsgo",
        "node_modules/@typescript/native-preview-linux-arm64/lib/tsgo",
        "node_modules/@typescript/native-preview-darwin-x64/lib/tsgo",
        "node_modules/@typescript/native-preview-darwin-arm64/lib/tsgo",
    ] {
        if !rel_paths.contains(&rel_path) {
            rel_paths.push(rel_path);
        }
    }

    rel_paths
}

fn tsgo_shim_rel_paths() -> &'static [&'static str] {
    if cfg!(windows) {
        &[
            "node_modules/.bin/tsgo.cmd",
            "node_modules/.bin/tsgo.bat",
            "node_modules/.bin/tsgo",
        ]
    } else {
        &["node_modules/.bin/tsgo"]
    }
}

fn entry_modified(path: &Path) -> std::time::SystemTime {
    path.metadata()
        .and_then(|meta| meta.modified())
        .unwrap_or(UNIX_EPOCH)
}

fn push_checked_location(checked_locations: &mut Vec<String>, location: String) {
    if !checked_locations
        .iter()
        .any(|existing| existing == &location)
    {
        checked_locations.push(location);
    }
}

fn push_cache_root(roots: &mut Vec<PathBuf>, root: Option<PathBuf>) {
    let Some(root) = root.map(normalize_npm_cache_root) else {
        return;
    };
    if !roots
        .iter()
        .any(|existing| cache_root_key(existing) == cache_root_key(&root))
    {
        roots.push(root);
    }
}

fn normalize_npm_cache_root(root: PathBuf) -> PathBuf {
    if root.file_name().and_then(|name| name.to_str()) == Some("_npx") {
        root.parent().map(PathBuf::from).unwrap_or(root)
    } else {
        root
    }
}

fn cache_root_key(path: &Path) -> String {
    let value = path.to_string_lossy();
    if cfg!(windows) {
        value.replace('\\', "/").to_ascii_lowercase()
    } else {
        value.into_owned()
    }
}

/// Create a temporary project directory with tsconfig.json for testing.
pub fn create_test_project(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "jsx": "preserve",
    "jsxImportSource": "vue",
    "allowImportingTsExtensions": true,
    "paths": {}
  },
  "include": ["**/*.ts", "**/*.tsx"]
}"#,
    )?;
    Ok(())
}

// Ungated unit tests for the pure URI→canonical-path DTO parsers. These run in
// the canonical `cargo nextest run --workspace` gate so the G1 contract — every
// path-bearing TSGO DTO carries the shared CANONICAL filesystem ID, not a raw
// `file://` URI — is enforced.
#[cfg(test)]
mod dto_path_canonicalization_tests {
    use super::{parse_rename_edit, parse_text_edit_to_code_edit, uri_to_file_path};

    fn edit_json() -> serde_json::Value {
        serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 1 }
            }
        })
    }

    #[test]
    fn uri_to_file_path_canonicalizes_drive_and_unc() {
        // Drive lowered to the shared canonical form (was `D:/…` raw).
        assert_eq!(uri_to_file_path("file:///D:/x/App.vue"), "d:/x/App.vue");
        assert_ne!(uri_to_file_path("file:///D:/x/App.vue"), "D:/x/App.vue");
        // UNC authority preserved + canonical.
        assert_eq!(
            uri_to_file_path("file://srv/share/App.vue"),
            "//srv/share/App.vue"
        );
        // Unix is a no-op.
        assert_eq!(
            uri_to_file_path("file:///home/u/App.vue"),
            "/home/u/App.vue"
        );
    }

    #[test]
    fn parse_rename_edit_stores_canonical_path_not_raw_uri() {
        // The DTO path must be the canonical filesystem ID, NEVER the raw URI.
        // Reverting `parse_rename_edit` to `path: uri.to_string()` fails this.
        let loc = parse_rename_edit("file:///D:/proj/App.vue", &edit_json(), None).unwrap();
        assert_eq!(loc.path, "d:/proj/App.vue");
        assert_ne!(loc.path, "file:///D:/proj/App.vue");
        assert!(!loc.path.starts_with("file://"));
    }

    #[test]
    fn parse_text_edit_to_code_edit_stores_canonical_path_not_raw_uri() {
        let te = serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 1 }
            },
            "newText": "x"
        });
        let edit = parse_text_edit_to_code_edit("file:///D:/proj/App.vue", &te, None).unwrap();
        assert_eq!(edit.path, "d:/proj/App.vue");
        assert_ne!(edit.path, "file:///D:/proj/App.vue");
        assert!(!edit.path.starts_with("file://"));
    }
}

// Transport-level tests that use runtime-local types live in the sibling
// `ipc_tests.rs`. Tests that depend on LSP-internal types (PositionMapper,
// uri_to_canonical_id, merge) or on `verter_session` compilation stay in
// `verter_lsp`.
#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;
