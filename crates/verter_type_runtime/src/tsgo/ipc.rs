//! TSGO `TypeProvider` implementation via LSP JSON-RPC over stdio.
//!
//! Spawns `tsgo --lsp --stdio` as a child process and communicates using
//! the Language Server Protocol over stdin/stdout with JSON-RPC framing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
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

/// Per-lane bounded-channel capacity for the Interactive / Normal / Background
/// stdin lanes. The Control lane is unbounded (never blocks the read loop).
const DEFAULT_LANE_CAPACITY: usize = 1024;

/// Maximum number of Normal-priority frames to flush before checking Interactive.
const NORMAL_BATCH_CAP: usize = 5;
/// Maximum number of Background-priority frames to flush before checking higher lanes.
const BACKGROUND_BATCH_CAP: usize = 3;

/// Writer-stall watchdog window. A `stdin.write_all` that does not complete
/// within this window means the child process has stopped reading its stdin —
/// the write side of a bidirectional stdio-pipe deadlock. When it trips, the
/// writer fires `crash_notify` (unless a deliberate teardown is in flight) so
/// the `ResilientTypeProvider` restart machinery (kill, backoff, respawn,
/// replay) recovers the session, and the writer task ends. Generous by design:
/// a child not draining stdin for this long is wedged, not merely busy.
const WRITER_STALL_TIMEOUT_SECS: u64 = 10;

/// Flush `buffer` to `stdin` under the writer-stall watchdog.
///
/// Returns `true` when the caller may continue draining lanes, `false` when the
/// writer loop must stop (I/O error or a stall that tripped the watchdog). On a
/// stall the child is not reading stdin, so `crash_notify` is fired (unless a
/// deliberate teardown is in flight) to trigger the resilient restart.
async fn flush_stdin_guarded<W>(
    stdin: &mut W,
    buffer: &mut Vec<u8>,
    writer_stall: std::time::Duration,
    crash_notify: &Option<Arc<Notify>>,
    teardown_intent: &AtomicBool,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if buffer.is_empty() {
        return true;
    }
    match tokio::time::timeout(writer_stall, stdin.write_all(buffer)).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => return false,
        Err(_) => {
            if teardown_intent.load(Ordering::SeqCst) {
                tracing::debug!(
                    "TSGO stdin write stalled during deliberate teardown — not a crash"
                );
            } else {
                tracing::error!(
                    "TSGO stdin write stalled for {writer_stall:?} — child is not reading stdin; \
                     signalling restart"
                );
                if let Some(notify) = crash_notify {
                    notify.notify_waiters();
                }
            }
            return false;
        }
    }
    let _ = tokio::time::timeout(writer_stall, stdin.flush()).await;
    buffer.clear();
    true
}

/// Dedicated task that owns the stdin writer and drains four priority lanes.
///
/// Priority order: Control > Interactive > Normal > Background.
/// - Control: drained fully (unbounded) at top priority. Carries the read
///   loop's auto-responses to the child's server→client requests (which must
///   never wait behind feature traffic, or the child blocks) and the `Shutdown`
///   signal. An unbounded lane so the read loop's enqueue is lossless and can
///   NEVER block on a full lane — the structural break in the stdout-read /
///   stdin-write deadlock cycle.
/// - Interactive: drained fully (unbounded) before checking lower lanes.
/// - Normal: drained up to `NORMAL_BATCH_CAP` frames, then back to check higher.
/// - Background: drained up to `BACKGROUND_BATCH_CAP` frames, then back to check higher.
///
/// Every flush routes through [`flush_stdin_guarded`], so a child that stops
/// reading stdin trips the writer-stall watchdog and fires `crash_notify`
/// instead of parking the writer (and therefore all lanes) forever.
///
/// Generic over the writer type to support both `ChildStdin` and test `DuplexStream`.
#[allow(clippy::too_many_arguments)]
async fn stdin_writer_loop(
    mut stdin: impl tokio::io::AsyncWrite + Unpin + Send + 'static,
    mut control_rx: mpsc::UnboundedReceiver<StdinMessage>,
    mut interactive_rx: mpsc::Receiver<StdinMessage>,
    mut normal_rx: mpsc::Receiver<StdinMessage>,
    mut background_rx: mpsc::Receiver<StdinMessage>,
    crash_notify: Option<Arc<Notify>>,
    teardown_intent: Arc<AtomicBool>,
    writer_stall: std::time::Duration,
) {
    let mut buffer = Vec::new();

    // Drain a lane FULLY (unbounded) into `buffer`; evaluates to `true` when a
    // `Shutdown` was seen (caller flushes then returns).
    macro_rules! drain_full {
        ($rx:expr) => {{
            let mut shutdown = false;
            loop {
                match $rx.try_recv() {
                    Ok(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                    Ok(StdinMessage::Shutdown) => {
                        shutdown = true;
                        break;
                    }
                    Err(_) => break,
                }
            }
            shutdown
        }};
    }
    // Drain a lane up to `$cap` frames into `buffer`; evaluates to `true` on
    // `Shutdown`.
    macro_rules! drain_capped {
        ($rx:expr, $cap:expr) => {{
            let mut shutdown = false;
            for _ in 0..$cap {
                match $rx.try_recv() {
                    Ok(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                    Ok(StdinMessage::Shutdown) => {
                        shutdown = true;
                        break;
                    }
                    Err(_) => break,
                }
            }
            shutdown
        }};
    }
    macro_rules! flush_or_break {
        () => {
            if !flush_stdin_guarded(
                &mut stdin,
                &mut buffer,
                writer_stall,
                &crash_notify,
                &teardown_intent,
            )
            .await
            {
                break;
            }
        };
    }
    macro_rules! flush_and_return {
        () => {{
            let _ = flush_stdin_guarded(
                &mut stdin,
                &mut buffer,
                writer_stall,
                &crash_notify,
                &teardown_intent,
            )
            .await;
            return;
        }};
    }

    loop {
        // Wait for any message from any lane. `biased` prefers Control first.
        tokio::select! {
            biased;
            msg = control_rx.recv() => {
                match msg {
                    Some(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                    Some(StdinMessage::Shutdown) | None => flush_and_return!(),
                }
            }
            msg = interactive_rx.recv() => {
                match msg {
                    Some(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                    Some(StdinMessage::Shutdown) | None => flush_and_return!(),
                }
            }
            msg = normal_rx.recv() => {
                match msg {
                    Some(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                    Some(StdinMessage::Shutdown) | None => flush_and_return!(),
                }
            }
            msg = background_rx.recv() => {
                match msg {
                    Some(StdinMessage::Frame(data)) => buffer.extend_from_slice(&data),
                    Some(StdinMessage::Shutdown) | None => flush_and_return!(),
                }
            }
        }

        // Control fully, then Interactive fully, then flush.
        if drain_full!(control_rx) {
            flush_and_return!();
        }
        if drain_full!(interactive_rx) {
            flush_and_return!();
        }
        flush_or_break!();

        // Normal (capped); re-check Control + Interactive; flush.
        if drain_capped!(normal_rx, NORMAL_BATCH_CAP) {
            flush_and_return!();
        }
        if drain_full!(control_rx) {
            flush_and_return!();
        }
        if drain_full!(interactive_rx) {
            flush_and_return!();
        }
        flush_or_break!();

        // Background (capped); re-check higher lanes; flush.
        if drain_capped!(background_rx, BACKGROUND_BATCH_CAP) {
            flush_and_return!();
        }
        if drain_full!(control_rx) {
            flush_and_return!();
        }
        if drain_full!(interactive_rx) {
            flush_and_return!();
        }
        if drain_capped!(normal_rx, NORMAL_BATCH_CAP) {
            flush_and_return!();
        }
        flush_or_break!();
    }
}

/// Legacy single-channel wrapper for backward compat (tests).
#[cfg(test)]
async fn stdin_writer_loop_single(
    stdin: impl tokio::io::AsyncWrite + Unpin + Send + 'static,
    rx: mpsc::Receiver<StdinMessage>,
) {
    // Create dummy channels for the other lanes that never receive.
    let (_control_tx, control_rx) = mpsc::unbounded_channel();
    let (_normal_tx, normal_rx) = mpsc::channel(1);
    let (_bg_tx, background_rx) = mpsc::channel(1);
    stdin_writer_loop(
        stdin,
        control_rx,
        rx,
        normal_rx,
        background_rx,
        None,
        Arc::new(AtomicBool::new(false)),
        std::time::Duration::from_secs(WRITER_STALL_TIMEOUT_SECS),
    )
    .await;
}

/// LSP JSON-RPC transport over a child process's stdio.
struct LspTransport {
    /// Unbounded control lane, drained at top priority by the writer. Carries the
    /// read loop's auto-responses to the child's server→client requests and the
    /// `Shutdown` signal. Unbounded so the read loop's enqueue is lossless and
    /// never blocks on a full lane — the structural break in the bidirectional
    /// stdout-read / stdin-write deadlock cycle.
    control_tx: mpsc::UnboundedSender<StdinMessage>,
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
    /// Deliberate-teardown intent. Set by `shutdown()` BEFORE the `shutdown`/`exit`
    /// pair is sent, so the child's resulting exit (stdout EOF) and any in-flight
    /// request timeouts are recognized as the teardown they are — NEVER surfaced on
    /// `crash_notify` as an engine crash (which would mint a spurious
    /// "crashed. Restarting" notification and respawn an engine into a dying
    /// session).
    teardown_intent: Arc<AtomicBool>,
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

/// Build a JSON-RPC message body, OMITTING the `params` key entirely when the
/// caller has none (`Value::Null`). LSP methods like `shutdown`/`exit` declare
/// NO params; sending `"params": null` makes strict engines (tsgo) log
/// `InvalidParams: expected no params, got null` while handling every teardown.
fn jsonrpc_body(id: Option<i64>, method: &str, params: &serde_json::Value) -> serde_json::Value {
    let mut msg = serde_json::Map::new();
    msg.insert("jsonrpc".into(), serde_json::Value::from("2.0"));
    if let Some(id) = id {
        msg.insert("id".into(), serde_json::Value::from(id));
    }
    msg.insert("method".into(), serde_json::Value::from(method));
    if !params.is_null() {
        msg.insert("params".into(), params.clone());
    }
    serde_json::Value::Object(msg)
}

impl LspTransport {
    /// Get the sender for a given priority lane.
    fn tx_for_priority(&self, priority: ProviderPriority) -> &mpsc::Sender<StdinMessage> {
        match priority {
            ProviderPriority::Interactive => &self.interactive_tx,
            ProviderPriority::Normal => &self.normal_tx,
            ProviderPriority::Background => &self.background_tx,
        }
    }

    /// Record a request-level failure (response timeout OR a stdin-enqueue stall)
    /// toward hang detection. At [`HANG_THRESHOLD`] consecutive failures fire
    /// `crash_notify` so the [`crate::resilient::ResilientTypeProvider`] restart
    /// machinery recovers the session — unless a deliberate teardown is in flight.
    ///
    /// Only the response-timeout arm used to count, so a request parked on a
    /// full lane behind a stalled writer never reached this path, so the wedge
    /// detector never fired for a stdin-side deadlock.
    fn note_hang_failure(&self) {
        let count = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= HANG_THRESHOLD {
            if self.teardown_intent.load(Ordering::SeqCst) {
                tracing::debug!("TSGO request failures during deliberate teardown — not a hang");
            } else {
                tracing::error!(
                    "TSGO appears hung ({count} consecutive failures) — triggering restart"
                );
                if let Some(notify) = &self.crash_notify {
                    notify.notify_waiters();
                }
            }
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

                let msg = jsonrpc_body(Some(id), method, &params);
                let body = serde_json::to_string(&msg)
                    .map_err(|e| TypeProviderError::new(format!("serialize error: {e}")))?;

                let (tx, rx) = oneshot::channel();
                self.pending.lock().await.insert(id, tx);

                let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
                // The lane send and the response wait SHARE one deadline so the
                // whole round-trip is bounded by `timeout_secs`. An unbounded enqueue
                // did an UNBOUNDED `send().await` here, so a full lane behind a
                // writer stalled on a busy child parked the request forever —
                // BEFORE the response timeout even started, and without counting
                // toward hang detection. Bounding the enqueue closes that gap.
                let deadline =
                    tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
                let send_budget = deadline.saturating_duration_since(tokio::time::Instant::now());
                match self
                    .tx_for_priority(priority)
                    .send_timeout(StdinMessage::Frame(frame.into_bytes()), send_budget)
                    .await
                {
                    Ok(()) => {}
                    Err(mpsc::error::SendTimeoutError::Closed(_)) => {
                        self.pending.lock().await.remove(&id);
                        return Err(TypeProviderError::new("stdin writer closed"));
                    }
                    Err(mpsc::error::SendTimeoutError::Timeout(_)) => {
                        self.pending.lock().await.remove(&id);
                        self.note_hang_failure();
                        crate::type_runtime_trace_event!(
                            "tsgo_transport_request_error",
                            format!("method={} id={} message=stdin-enqueue-timeout", method, id),
                        );
                        return Err(TypeProviderError::new(format!(
                            "request '{method}' stdin enqueue timed out after {timeout_secs}s"
                        )));
                    }
                }

                let rx_budget = deadline.saturating_duration_since(tokio::time::Instant::now());
                let result = tokio::time::timeout(rx_budget, rx).await;
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
                        self.note_hang_failure();
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

    /// Enqueue an LSP notification onto `priority`'s lane WITHOUT awaiting.
    ///
    /// Synchronous by construction. A caller that must commit local state in the
    /// SAME non-cancellable step as an accepted enqueue — the document-sync
    /// ledger below — cannot express that against an async send: a dropped future
    /// would leave the state written and the frame unsent.
    fn try_notify_with_priority(
        &self,
        method: &str,
        params: &serde_json::Value,
        priority: ProviderPriority,
    ) -> Result<(), TypeProviderError> {
        let msg = jsonrpc_body(None, method, params);
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
                tracing::warn!("TSGO stdin channel full — refusing notification '{method}'");
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

    /// Send an LSP notification at a specific priority (no response expected).
    /// Never blocks the caller: a lane with no free slot refuses the frame rather
    /// than applying backpressure.
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
            async { self.try_notify_with_priority(method, &params, priority) }
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

/// The LSP `languageId` for a provider path.
fn document_language_id(path: &str) -> &'static str {
    if path.ends_with(".tsx") {
        "typescriptreact"
    } else if path.ends_with(".jsx") {
        "javascriptreact"
    } else if path.ends_with(".js") {
        "javascript"
    } else {
        "typescript"
    }
}

/// Which notification a document sync should deliver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocumentSyncIntent {
    /// Always a fresh `didOpen` at version 1 — the caller knows the child does
    /// not hold this document.
    Open,
    /// `didChange` when the ledger already records the document as open, else the
    /// `didOpen` the LSP protocol requires first (a `didChange` for a document the
    /// child never opened makes tsgo panic with "overlay not found").
    Update,
}

/// What a document sync actually delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocumentSyncMode {
    DidOpen,
    DidChange { version: i32 },
}

/// Deliver a `didOpen`/`didChange` for `path` and commit the local ledger ONLY
/// once the transport has ACCEPTED the frame.
///
/// `versions` + `contents` are this provider's record of what the child engine was
/// actually told: `versions` decides `didOpen` vs `didChange` on the next sync, and
/// `contents` backs offset↔position conversion. Committing either BEFORE the
/// transport accepts claims a sync the child never received — and a refused enqueue
/// (a full lane behind a writer stalled on a busy child) then strands the document
/// indefinitely, because every later sync reads the ledger, believes the document is
/// open, and sends a `didChange` the child cannot apply.
///
/// Both maps are locked before the enqueue, and the commit runs in the same step as
/// an accepted synchronous `try_send` with no `.await` between them. So there is no
/// cancellation point at which an optimistic write could outlive a dropped future,
/// and the read-then-write on `versions` stays mutually exclusive against a
/// concurrent sync of the same path.
async fn deliver_document_sync(
    transport: &LspTransport,
    versions: &Mutex<HashMap<String, i32>>,
    contents: &Mutex<HashMap<String, Arc<str>>>,
    path: &str,
    content: &str,
    intent: DocumentSyncIntent,
    priority: ProviderPriority,
) -> Result<DocumentSyncMode, TypeProviderError> {
    let uri = TsgoTypeProvider::path_to_uri(path);
    let mut versions_guard = versions.lock().await;
    let mut contents_guard = contents.lock().await;

    let mode = match (intent, versions_guard.get(path)) {
        (DocumentSyncIntent::Update, Some(version)) => DocumentSyncMode::DidChange {
            version: version + 1,
        },
        _ => DocumentSyncMode::DidOpen,
    };
    let (method, params) = match mode {
        DocumentSyncMode::DidOpen => (
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": document_language_id(path),
                    "version": 1,
                    "text": content,
                }
            }),
        ),
        DocumentSyncMode::DidChange { version } => (
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": content }]
            }),
        ),
    };

    transport.try_notify_with_priority(method, &params, priority)?;

    let version = match mode {
        DocumentSyncMode::DidOpen => 1,
        DocumentSyncMode::DidChange { version } => version,
    };
    versions_guard.insert(path.to_string(), version);
    contents_guard.insert(contents_key(path), Arc::from(content));
    Ok(mode)
}

/// Deliver a `didClose` for `path` and retire the ledger entry ONLY once the
/// transport has accepted the frame.
///
/// A refused `didClose` leaves the entry in place, which is the accurate record:
/// the child still holds the document open, so the next sync must keep treating it
/// as open rather than replaying a `didOpen` over a live buffer.
async fn deliver_document_close(
    transport: &LspTransport,
    versions: &Mutex<HashMap<String, i32>>,
    contents: &Mutex<HashMap<String, Arc<str>>>,
    path: &str,
    priority: ProviderPriority,
) -> Result<(), TypeProviderError> {
    let uri = TsgoTypeProvider::path_to_uri(path);
    let mut versions_guard = versions.lock().await;
    let mut contents_guard = contents.lock().await;

    transport.try_notify_with_priority(
        "textDocument/didClose",
        &serde_json::json!({ "textDocument": { "uri": uri } }),
        priority,
    )?;

    versions_guard.remove(path);
    contents_guard.remove(&contents_key(path));
    Ok(())
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
///
/// `teardown_intent` disarms that signal: a deliberate `shutdown()` sets it BEFORE
/// sending `shutdown`/`exit`, so the child's resulting EOF is recognized as the
/// requested teardown — never surfaced as a crash (which would mint a spurious
/// "crashed. Restarting" notification and respawn an engine into a dying session).
async fn read_loop(
    stdout: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>,
    diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>>,
    contents_cache: Arc<Mutex<HashMap<String, Arc<str>>>>,
    control_tx: mpsc::UnboundedSender<StdinMessage>,
    crash_notify: Option<Arc<Notify>>,
    teardown_intent: Arc<AtomicBool>,
) {
    let signal_crash = |crash_notify: &Option<Arc<Notify>>| {
        if teardown_intent.load(Ordering::SeqCst) {
            tracing::debug!("TSGO stdout closed during deliberate teardown — not a crash");
            return;
        }
        if let Some(notify) = crash_notify {
            notify.notify_waiters();
        }
    };
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
                    signal_crash(&crash_notify);
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
                    signal_crash(&crash_notify);
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
            signal_crash(&crash_notify);
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
            // Enqueue the auto-response on the UNBOUNDED control lane. A blocking enqueue here
            // this was a blocking `interactive_tx.send().await` on a bounded lane:
            // when the interactive lane was full behind a stalled writer, the read
            // loop parked HERE and stopped draining the child's stdout — the child
            // then blocked on its own full stdout and could never resume reading
            // our stdin, a permanent two-pipe deadlock. An unbounded, non-blocking
            // enqueue keeps stdout reading unconditional and breaks that cycle.
            let _ = control_tx.send(StdinMessage::Frame(frame.into_bytes()));
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
                        // to byte offsets. The content cache is keyed by file path, so
                        // convert the URI first; on a case-insensitive filesystem
                        // (Windows / default macOS) fall back to a case-folded match so
                        // a case-variant key still resolves.
                        let content = {
                            let path = uri_to_file_path(raw_uri);
                            let cache = contents_cache.lock().await;
                            // Exact match first, then — only on a case-insensitive
                            // filesystem — a folded match through the single shared
                            // FS-identity policy (`verter_span::path`), so the case
                            // policy never diverges per OS at this call site.
                            cache.get(&path).cloned().or_else(|| {
                                if verter_span::path::fs_is_case_insensitive() {
                                    cache
                                        .iter()
                                        .find(|(k, _)| verter_span::path::fs_paths_equal(k, &path))
                                        .map(|(_, v)| v.clone())
                                } else {
                                    None
                                }
                            })
                        };
                        if content.is_some() {
                            let diag_file = uri_to_file_path(raw_uri);
                            let diags = params
                                .get("diagnostics")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|d| {
                                            parse_lsp_diagnostic(
                                                d,
                                                content.as_deref(),
                                                Some(diag_file.as_str()),
                                            )
                                        })
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
fn parse_lsp_diagnostic(
    d: &serde_json::Value,
    content: Option<&str>,
    file_path: Option<&str>,
) -> Option<TypeDiagnostic> {
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

    // LSP `relatedInformation` carries the secondary "see declaration here" spans
    // (each `{ location: { uri, range }, message }`). `parse_lsp_related_info` keeps
    // ONLY a same-file related span whose content is available AND whose position is
    // in range — it converts through the CHECKED offset converter and DROPS the
    // entry for a cross-file/no-content span OR an out-of-range same-file position
    // (never stores a packed position, never clamps to EOF). A dropped secondary
    // link beats a bogus one.
    let primary_file = file_path.map(verter_span::path::canonicalize_path);
    let related_information = d
        .get("relatedInformation")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|ri| parse_lsp_related_info(ri, content, primary_file.as_deref()))
                .collect()
        })
        .unwrap_or_default();

    Some(TypeDiagnostic {
        message,
        severity,
        start: start_offset,
        end: end_offset,
        code,
        tags,
        related_information,
    })
}

/// Parse one LSP `relatedInformation` entry into a [`DiagnosticRelatedInfo`].
///
/// The entry shape is `{ location: { uri, range: { start, end } }, message }`.
/// [`DiagnosticRelatedInfo::start`]/[`DiagnosticRelatedInfo::end`] are REAL byte
/// offsets in `path` — never a packed `(line<<16)|character` position. A real
/// offset is available ONLY when the related `location.uri` resolves to the SAME
/// canonical file the parser holds content for (`primary_file` / `primary_content`);
/// both sides are canonicalized before the equality (`primary_file` upstream,
/// `path` via [`uri_to_file_path`]) so a same file spelled differently still
/// matches.
///
/// Returns `None` (skip this entry, never fabricate, never store a packed value)
/// when the message/location/uri/range fields are missing, when the related span
/// is cross-file (no content for it), OR when a same-file position is OUT OF RANGE
/// for the content — fail-closed: a dropped secondary link beats a bogus one.
fn parse_lsp_related_info(
    ri: &serde_json::Value,
    primary_content: Option<&str>,
    primary_file: Option<&str>,
) -> Option<DiagnosticRelatedInfo> {
    let message = ri.get("message")?.as_str()?.to_string();
    let location = ri.get("location")?;
    let path = uri_to_file_path(location.get("uri")?.as_str()?);
    let range = location.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    // CHECKED `u64 → u32`: a malformed coordinate larger than `u32::MAX` (e.g.
    // `2^32`) would WRAP to an in-range line/column under a lossy `as u32` cast,
    // then PASS the checked position converter (which only rejects past-EOF
    // positions), fabricating a valid-looking but WRONG related link. Dropping the
    // whole related entry (fail-closed) on an out-of-u32-range coordinate is the
    // only defense, because the corruption would happen in the cast BEFORE the
    // converter runs.
    let start_line = u32::try_from(start.get("line")?.as_u64()?).ok()?;
    let start_char = u32::try_from(start.get("character")?.as_u64()?).ok()?;
    let end_line = u32::try_from(end.get("line")?.as_u64()?).ok()?;
    let end_char = u32::try_from(end.get("character")?.as_u64()?).ok()?;

    // A real byte offset exists only for a same-file related span (the parser holds
    // that file's content). A cross-file span has no content here, so there is no
    // real offset — DROP it rather than store a packed position the merge would
    // mis-read as a byte offset. Both paths are already canonicalized.
    let same_file = primary_file == Some(path.as_str());
    let content = primary_content.filter(|_| same_file)?;
    // Even a same-file related span can be MALFORMED (a line/col past EOF). The
    // fail-open `position_to_offset` would CLAMP that to `content.len()` and forge a
    // bogus "see declaration" link at EOF, so the related-info path uses the CHECKED
    // converter and DROPS the entry (returns `None`) when the position is out of
    // range — never clamps. The primary-span path keeps its own clamp/recovery
    // behavior (out of scope here).
    let start_byte = position_to_offset_checked(content, start_line, start_char)?;
    let end_byte = position_to_offset_checked(content, end_line, end_char)?;

    Some(DiagnosticRelatedInfo {
        path,
        start: start_byte,
        end: end_byte,
        message,
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

/// Convert an LSP 0-based `(line, character)` to a byte offset, returning `None` when the position
/// is OUT OF RANGE for `content` instead of clamping it to EOF.
///
/// [`position_to_offset_with_encoding`] fails OPEN — a past-EOF line or a column past the line end
/// clamps to `content.len()` / the line end and returns a valid-looking WRONG offset. That is
/// acceptable for a navigation sentinel, but for an EDIT a clamped wrong offset corrupts the file,
/// so the edit path validates the position is real and DROPS it otherwise. EDIT-PATH-LOCAL: does
/// not change the shared codec. `character` is UTF-16 code units.
fn position_to_offset_checked(content: &str, line: u32, character: u32) -> Option<u32> {
    let idx = LineIndex::new(content, PositionEncoding::Utf16);
    if line as usize >= idx.line_count() {
        return None; // past-EOF line
    }
    // The line's UTF-16 width; a column past it would clamp.
    let line_start = idx.line_start(line as usize)?;
    let line_end = idx.line_end(line as usize)?; // before the newline / EOF
    let line_text = content.get(line_start as usize..line_end as usize)?;
    let line_utf16_len: u32 = line_text.encode_utf16().count() as u32;
    if character > line_utf16_len {
        return None; // column past the line end
    }
    let target = crate::codec::LineColumn { line, character };
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

/// Parse a batch of LSP `Location` JSON values into `TypeLocation`s, resolving EACH location's
/// byte offsets against that location's OWN file content.
///
/// References (and definition / type-definition) span multiple files: a location in file `B`
/// returned from a query on file `A` carries a `range` whose line:col must be converted to a byte
/// offset using `B`'s content, NOT `A`'s. Passing the queried file's single snapshot to every
/// location converts cross-file ranges against the wrong file, packing garbage byte offsets that
/// surface downstream as line-0 / wrong-position results.
///
/// `content_for(target_path)` hands back the target file's content (from the contents cache);
/// `parse_lsp_location` falls back to a disk read when it returns `None`.
fn parse_lsp_locations_per_target<'a>(
    locations: &[serde_json::Value],
    content_for: impl Fn(&str) -> Option<&'a str>,
) -> Vec<TypeLocation> {
    locations
        .iter()
        .filter_map(|loc| {
            let target_path = loc
                .get("uri")
                .and_then(|value| value.as_str())
                .map(uri_to_file_path)?;
            parse_lsp_location(loc, content_for(&target_path))
        })
        .collect()
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
/// On a case-insensitive filesystem (Windows + default macOS) TSGO may send URIs with
/// percent-encoding and lowercased path segments (e.g., `file:///c%3A/users/someone/...`),
/// while our `path_to_uri` produces literal colons with original case
/// (e.g., `file:///C:/Users/Someone/...`).
///
/// This function normalizes both forms to the same canonical representation:
/// 1. Percent-decodes the URI (so `%3A` → `:`)
/// 2. Folds the whole decoded URI to lowercase IFF the host filesystem is
///    case-insensitive — the single shared `verter_span::path::fs_is_case_insensitive`
///    policy (Windows + default macOS fold; a case-sensitive filesystem (Linux) preserves
///    case).
fn normalize_file_uri(uri: &str) -> String {
    normalize_file_uri_for_cache(uri)
}

/// The single key convention for the `contents` cache.
///
/// The cache is keyed by canonical filesystem path so every producer/consumer of
/// a carrier's content agrees on identity: the file-lifecycle inserts, the
/// position-mapping reads, `cached_content` (read cross-surface by the OWNED
/// `--api` diagnostics route with the engine's own `root_files` form), and the
/// edit-target snapshot scanners in [`crate::contents_snapshot`] (which key by
/// this same `canonicalize_path`). A raw-string insert paired with a differently
/// slashed / drive-cased lookup (e.g. the engine echoes `c:/…` while a didOpen
/// used `C:\…`) is a FALSE miss that would strand a carrier's content; routing
/// every access through this helper makes the insert and lookup forms agree.
fn contents_key(path: &str) -> String {
    verter_span::path::canonicalize_path(path)
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
    // The `textEdit.newText` — the text a SURVIVING replace-range commits (per
    // LSP the editor applies it and ignores `insertText`), and the preferred
    // plain-insert fallback when the range is dropped fail-closed below.
    let text_edit_new_text = item
        .get("textEdit")
        .and_then(|te| te.get("newText"))
        .and_then(|v| v.as_str())
        .map(String::from);
    // The explicit `insertText` — the plain-insert text used only when there is
    // no `textEdit`. Carried distinctly from `text_edit_new_text` so the consumer
    // chooses the right text per LSP semantics (newText for an edit; insertText
    // for a plain insert) and never falls back to the display `label`.
    let insert_text = item
        .get("insertText")
        .and_then(|v| v.as_str())
        .map(String::from);
    let sort_text = item
        .get("sortText")
        .and_then(|v| v.as_str())
        .map(String::from);
    // tsgo speaks LSP, so an item carries the real `insertTextFormat` (1 = plain,
    // 2 = snippet) when it is a snippet completion. Map the wire integer to the
    // neutral carrier; any unknown value is treated as no signal (fail-closed).
    let insert_text_format = item
        .get("insertTextFormat")
        .and_then(|v| v.as_u64())
        .and_then(|n| match n {
            1 => Some(CompletionInsertTextFormat::PlainText),
            2 => Some(CompletionInsertTextFormat::Snippet),
            _ => None,
        });
    // Strict, fail-closed, shared with the tsserver provider: a malformed or
    // empty `commitCharacters` array yields `None` rather than `Some(vec![])`.
    let commit_characters = parse_commit_characters(item.get("commitCharacters"));
    let filter_text = item
        .get("filterText")
        .and_then(|v| v.as_str())
        .map(String::from);
    let preselect = item.get("preselect").and_then(|v| v.as_bool());
    // LSP `labelDetails` ({ detail?, description? }). Only minted when at least
    // one sub-field is a string — an empty object carries no signal.
    let label_details = item.get("labelDetails").and_then(parse_label_details);

    // The textEdit replace-range is applied as a REAL edit when the completion is accepted, so it
    // is fail-closed: when the content is unavailable, or the range cannot be proven against it,
    // the range is DROPPED (endpoints stay `None`) and the consumer degrades to a plain insert
    // rather than emitting a packed or clamped offset that would corrupt the file.
    let (edit_range_start, edit_range_end) = item
        .get("textEdit")
        .and_then(|te| {
            let range = te.get("range")?;
            let start = range.get("start")?;
            let end = range.get("end")?;
            let sl = u32::try_from(start.get("line")?.as_u64()?).ok()?;
            let sc = u32::try_from(start.get("character")?.as_u64()?).ok()?;
            let el = u32::try_from(end.get("line")?.as_u64()?).ok()?;
            let ec = u32::try_from(end.get("character")?.as_u64()?).ok()?;
            let c = content?;
            let s = position_to_offset_checked(c, sl, sc)?;
            let e = position_to_offset_checked(c, el, ec)?;
            if s > e {
                return None;
            }
            Some((Some(s), Some(e)))
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
        text_edit_new_text,
        insert_text,
        sort_text,
        insert_text_format,
        commit_characters,
        filter_text,
        preselect,
        label_details,
        data,
    })
}

/// Parse an LSP `CompletionItemLabelDetails`-shaped JSON value
/// (`{ detail?, description? }`) into the neutral [`CompletionLabelDetails`]
/// carrier. Returns `None` unless at least one sub-field is a string, so an
/// empty `{}` (no signal) does not mint an empty carrier.
fn parse_label_details(value: &serde_json::Value) -> Option<CompletionLabelDetails> {
    let detail = value
        .get("detail")
        .and_then(|v| v.as_str())
        .map(String::from);
    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from);
    if detail.is_none() && description.is_none() {
        return None;
    }
    Some(CompletionLabelDetails {
        detail,
        description,
    })
}

/// Parse an LSP `Command`-shaped JSON value (`{ title, command, arguments? }`)
/// into the neutral [`CompletionCommand`] carrier. Returns `None` unless BOTH
/// `title` and `command` are strings — a partial object is not a valid command
/// and is dropped fail-closed (never fabricated).
fn parse_lsp_command(value: Option<&serde_json::Value>) -> Option<CompletionCommand> {
    let value = value?;
    let title = value.get("title").and_then(|v| v.as_str())?.to_string();
    let command = value.get("command").and_then(|v| v.as_str())?.to_string();
    let arguments = value
        .get("arguments")
        .and_then(|v| v.as_array())
        .map(|arr| arr.to_vec());
    Some(CompletionCommand {
        title,
        command,
        arguments,
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

/// The `--api` checker session minted by [`TsgoTypeProvider::initialize_api_session`]:
/// the opaque server session id plus the pipe path the `--api` checker connects to.
#[derive(Debug, Clone)]
pub struct TsgoApiSession {
    /// The server-assigned session id (`InitializeAPISessionResult.sessionId`).
    pub session_id: String,
    /// The server-minted pipe path (a Windows named pipe / Unix-domain socket).
    pub pipe: String,
}

/// A `TypeProvider` backed by a real TSGO process (`tsgo --lsp --stdio`).
///
/// Spawns the process, initializes the LSP connection, and translates
/// `TypeProvider` method calls into LSP requests.
pub struct TsgoTypeProvider {
    transport: Arc<LspTransport>,
    /// TSGO child process when this provider owns one. `None` for an already-
    /// initialized, non-owning editor connection; that mode must never kill or
    /// originate shutdown/exit toward the editor's engine.
    child: Option<StdMutex<Option<Child>>>,
    /// Document version counter per path.
    versions: Arc<Mutex<HashMap<String, i32>>>,
    /// Cached file contents for byte-offset → LSP position conversion.
    contents: Arc<Mutex<HashMap<String, Arc<str>>>>,
    /// Cached diagnostics from textDocument/publishDiagnostics push notifications.
    /// Used as fallback when pull diagnostics (textDocument/diagnostic) fails.
    diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>>,
    /// Deliberate-teardown intent, shared with the transport + read loop. See
    /// [`LspTransport::teardown_intent`].
    teardown_intent: Arc<AtomicBool>,
}

impl Drop for TsgoTypeProvider {
    fn drop(&mut self) {
        // Kill the TSGO child process to prevent orphans.
        // start_kill() is non-blocking (sends TerminateProcess on Windows, SIGKILL on Unix).
        // This is a belt-and-suspenders backup — kill_on_drop(true) on the Command
        // already handles this, but an explicit Drop makes the intent clear.
        if let Some(slot) = &mut self.child {
            let child = slot
                .get_mut()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(child) = child {
                let _ = child.start_kill();
            }
        }
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

        let provider = Self::from_transport_parts(stdout, stdin, Some(child), crash_notify);

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
        // The client capabilities are built by `build_client_capabilities()` —
        // an LSP server gates every optional feature on what the client
        // advertises, so a capability the client never declares is silently
        // dropped and TSGO never emits data this provider's handlers are ready
        // to consume. The helper advertises EXACTLY the completion- and
        // diagnostic-channel capabilities TSGO's handlers consume (diagnostic
        // `tagSupport` on both channels; `completionItem.resolveSupport` for the
        // `completionItem/resolve` round-trips; `contextSupport` + the
        // `completionItemKind` valueSet). TSGO's base features (hover / definition /
        // references / rename / signatureHelp / codeAction / semanticTokens /
        // documentHighlight / inlayHint / pull-diagnostic) are left to TSGO's static
        // server-side registration and are not advertised here.
        let init_result = match provider
            .transport
            .request_with_priority(
                "initialize",
                serde_json::json!({
                    "processId": std::process::id(),
                    "capabilities": build_client_capabilities(),
                    "rootUri": root_uri,
                    "workspaceFolders": [{
                        "uri": root_uri,
                        "name": "workspace"
                    }]
                }),
                INITIALIZE_TIMEOUT_SECS,
                ProviderPriority::Interactive,
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                let _ = TypeProvider::shutdown(&provider).await;
                return Err(error);
            }
        };

        tracing::debug!("TSGO initialized: {:?}", init_result);

        // Send initialized notification
        if let Err(error) = provider
            .transport
            .notify("initialized", serde_json::json!({}))
            .await
        {
            let _ = TypeProvider::shutdown(&provider).await;
            return Err(error);
        }

        Ok(provider)
    }

    /// Build a feature provider over an already-initialized, non-owning LSP
    /// transport. This sends no `initialize`/`initialized`, owns no process, and
    /// never originates `shutdown`/`exit`; it is the editor-session reuse seam.
    pub fn from_initialized_transport<R, W>(read: R, write: W) -> Self
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        Self::from_transport_parts(read, write, None, None)
    }

    fn from_transport_parts<R, W>(
        read: R,
        write: W,
        child: Option<Child>,
        crash_notify: Option<Arc<Notify>>,
    ) -> Self
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        Self::from_transport_parts_configured(
            read,
            write,
            child,
            crash_notify,
            DEFAULT_LANE_CAPACITY,
            std::time::Duration::from_secs(WRITER_STALL_TIMEOUT_SECS),
        )
    }

    /// [`Self::from_transport_parts`] with a caller-chosen lane capacity and
    /// writer-stall watchdog window. Production uses [`DEFAULT_LANE_CAPACITY`]
    /// and [`WRITER_STALL_TIMEOUT_SECS`]; the deadlock-repro tests inject a tiny
    /// capacity and a short watchdog so a full lane / stalled child is reachable
    /// deterministically.
    fn from_transport_parts_configured<R, W>(
        read: R,
        write: W,
        child: Option<Child>,
        crash_notify: Option<Arc<Notify>>,
        lane_capacity: usize,
        writer_stall: std::time::Duration,
    ) -> Self
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (control_tx, control_rx) = mpsc::unbounded_channel::<StdinMessage>();
        let (interactive_tx, interactive_rx) = mpsc::channel::<StdinMessage>(lane_capacity);
        let (normal_tx, normal_rx) = mpsc::channel::<StdinMessage>(lane_capacity);
        let (background_tx, background_rx) = mpsc::channel::<StdinMessage>(lane_capacity);

        let teardown_intent = Arc::new(AtomicBool::new(false));
        tokio::spawn(stdin_writer_loop(
            write,
            control_rx,
            interactive_rx,
            normal_rx,
            background_rx,
            crash_notify.as_ref().map(Arc::clone),
            Arc::clone(&teardown_intent),
            writer_stall,
        ));

        let transport = Arc::new(LspTransport {
            control_tx: control_tx.clone(),
            interactive_tx,
            normal_tx,
            background_tx,
            pending: Arc::clone(&pending),
            next_id: AtomicI64::new(1),
            consecutive_failures: AtomicU32::new(0),
            crash_notify: crash_notify.as_ref().map(Arc::clone),
            teardown_intent: Arc::clone(&teardown_intent),
        });
        let diagnostics_cache = Arc::new(Mutex::new(HashMap::new()));
        let contents = Arc::new(Mutex::new(HashMap::new()));
        tokio::spawn(read_loop(
            read,
            pending,
            Arc::clone(&diagnostics_cache),
            Arc::clone(&contents),
            control_tx,
            crash_notify,
            Arc::clone(&teardown_intent),
        ));

        Self {
            transport,
            child: child.map(|child| StdMutex::new(Some(child))),
            versions: Arc::new(Mutex::new(HashMap::new())),
            contents,
            diagnostics_cache,
            teardown_intent,
        }
    }

    /// Attach an `--api` checker session to THIS `tsgo --lsp` process by sending
    /// `custom/initializeAPISession` over the existing `--lsp` connection. Returns
    /// the server-minted pipe path the `--api` checker connects to.
    ///
    /// This is the OWNED one-instance attach seam: the dual-surface provider drives
    /// BOTH the `--lsp` features (this provider's existing methods) AND the `--api`
    /// checker over the SAME process / shared `project.Session`. The pipe path is
    /// returned verbatim (a `\\.\pipe\tsgo-api-…` on Windows, a UDS path on Unix);
    /// the caller connects it. No second process is spawned.
    pub async fn initialize_api_session(&self) -> Result<TsgoApiSession, TypeProviderError> {
        let result = self
            .transport
            .request(
                verter_tsgo_api::attach::INITIALIZE_API_SESSION_METHOD,
                serde_json::json!({}),
            )
            .await?;
        let pipe = result
            .get("pipe")
            .and_then(|p| p.as_str())
            .ok_or_else(|| {
                TypeProviderError::new(format!(
                    "custom/initializeAPISession result missing `pipe`: {result}"
                ))
            })?
            .to_string();
        let session_id = result
            .get("sessionId")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        Ok(TsgoApiSession { session_id, pipe })
    }

    /// The cached content of an opened file (`didOpen`/`update` overlay), if
    /// present. The `--api` diagnostic normalization (UTF-16 offset → byte /
    /// (line,col)) needs the carrier's exact text; the `--lsp` surface already
    /// caches every opened document's content for its own byte-offset ↔ position
    /// conversion, so the dual-surface OWNED provider reads it back through here
    /// rather than re-tracking a second copy.
    pub async fn cached_content(&self, path: &str) -> Option<Arc<str>> {
        self.contents.lock().await.get(&contents_key(path)).cloned()
    }

    /// Forget locally cached content without emitting an LSP lifecycle write.
    /// Used by a non-owning feature facade whose real overlay lifecycle is driven
    /// through the relay's separately tracked carrier-injection channel.
    pub async fn forget_cached_content(&self, path: &str) {
        self.contents.lock().await.remove(&contents_key(path));
        self.versions.lock().await.remove(path);
    }

    /// Pull and parse the current document diagnostics without degrading a wire
    /// failure to the push-diagnostic cache. Non-owning editor-session consumers use
    /// this to distinguish a legitimate empty report from a failed relay request, so
    /// the caller can make an explicit fallback decision.
    pub async fn get_diagnostics_strict(
        &self,
        path: &str,
    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {
        let uri = Self::path_to_uri(path);
        let value = self
            .transport
            .request(
                "textDocument/diagnostic",
                serde_json::json!({ "textDocument": { "uri": uri } }),
            )
            .await?;
        let content = self.contents.lock().await.get(&contents_key(path)).cloned();
        let diagnostics = value
            .get("items")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|diagnostic| {
                        parse_lsp_diagnostic(diagnostic, content.as_deref(), Some(path))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.diagnostics_cache
            .lock()
            .await
            .insert(normalize_file_uri(&uri), diagnostics.clone());
        Ok(diagnostics)
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
        let content = content.to_string();
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
                    deliver_document_sync(
                        &transport,
                        &versions,
                        &contents_cache,
                        &path_owned,
                        &content,
                        DocumentSyncIntent::Open,
                        priority,
                    )
                    .await
                    .map(|_| ())
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
        let content = content.to_string();
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            deliver_document_sync(
                &transport,
                &versions,
                &contents_cache,
                &path_owned,
                &content,
                DocumentSyncIntent::Update,
                priority,
            )
            .await
            .map(|_| ())
        })
    }

    /// Send `textDocument/didClose` at a specific priority.
    fn close_file_with_priority(
        &self,
        path: &str,
        priority: ProviderPriority,
    ) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            deliver_document_close(
                &transport,
                &versions,
                &contents_cache,
                &path_owned,
                priority,
            )
            .await
        })
    }
}

/// Build the LSP `ClientCapabilities` object sent in the tsgo `initialize` request.
///
/// An LSP server gates every optional feature on what the client advertises: a
/// capability the client never declares is silently dropped, so tsgo would never
/// emit data this provider's handlers are ready to consume. This helper owns the
/// COMPLETION and DIAGNOSTIC channels, and advertises EXACTLY the completion- and
/// diagnostic-channel capabilities tsgo's handlers actually consume — no more (an
/// over-claimed completion/diagnostic capability invites the server to compute
/// work the client discards, or register a sub-feature this thin provider cannot
/// service):
///
/// Scope note: this helper does NOT enumerate tsgo's BASE features — hover,
/// definition, typeDefinition, references, rename, signatureHelp, codeAction,
/// semanticTokens, documentHighlight, inlayHint, and the pull `textDocument/diagnostic`
/// request itself. The provider fully consumes those responses today; they work
/// because tsgo statically registers those providers server-side regardless of
/// client capabilities, and no OPTIONAL gated sub-feature of them is consumed. They
/// are intentionally left to tsgo's static server-side registration and are not
/// advertised here.
///
/// - `textDocument.publishDiagnostics.tagSupport` / `textDocument.diagnostic.tagSupport`
///   (`valueSet [1, 2]`) — tsgo attaches `DiagnosticTag`s (1 = Unnecessary fade,
///   2 = Deprecated strikethrough) only when the client understands them; the
///   `parse_lsp_diagnostic` tag mapping re-emits the fade / strikethrough on both
///   the push and pull channels. The `valueSet` enumerates the tags we render.
///   Spec note: the PUSH-channel `publishDiagnostics.tagSupport` is the
///   spec-defined capability (`PublishDiagnosticsClientCapabilities.tagSupport`).
///   The PULL-channel `textDocument.diagnostic.tagSupport` is NOT a field defined
///   by LSP 3.17 (`DiagnosticClientCapabilities` has no `tagSupport` member) — it
///   is a NON-SPEC field retained for compatibility (tsgo may read it as a private
///   extension to gate pull-diagnostic tags) and is intentionally kept as-is.
/// - `textDocument.completion.completionItem.resolveSupport` (`documentation`,
///   `detail`, `additionalTextEdits`) — tsgo computes a `completionItem/resolve`
///   property lazily only when the client lists it. This provider folds `detail` +
///   `documentation` back in [`TsgoTypeProvider::get_completion_details`] and
///   `additionalTextEdits` (the auto-import edits) in
///   [`TsgoTypeProvider::resolve_completion`]; WITHOUT this, tsgo silently drops
///   `additionalTextEdits` and completion-driven auto-import never applies its
///   import edit.
/// - `textDocument.completion.contextSupport` (`true`) —
///   [`TsgoTypeProvider::get_completions`] ALWAYS sends `CompletionParams.context`
///   (the trigger kind/character). Per LSP 3.17 a server honours that field only
///   when the client declares `contextSupport: true`; WITHOUT it tsgo may ignore
///   the trigger context entirely, so completions stop being trigger-aware.
/// - `textDocument.completion.completionItemKind.valueSet` (`1..=25`) — the
///   completion parser [`parse_completion_item`] maps the full standard
///   `CompletionItemKind` range generically. The LSP DEFAULT value set when this
///   field is omitted is `Text..Reference` (1..=18), so Class (7) — the kind
///   component-tag completions depend on — is INSIDE the default range and is
///   preserved regardless. The valueSet is advertised to stop tsgo DOWNGRADING the
///   UPPER standard kinds 19..=25 (Folder, EnumMember, Constant, Struct, Event,
///   Operator, TypeParameter), which fall OUTSIDE the default range. The `valueSet`
///   is EXACTLY the parser's range `1..=25` (no over-claim past it).
///
/// NOT advertised — `completionItem.dataSupport`: there is NO `dataSupport`
/// capability in the LSP spec. `CompletionItem.data` is a transparent
/// passthrough — the server stamps it on each item and the client MUST echo the
/// same blob back on `completionItem/resolve` regardless of any advertised
/// capability. Both resolve sites here ([`TsgoTypeProvider::get_completion_details`]
/// and [`TsgoTypeProvider::resolve_completion`]) replay the item's opaque `data`
/// verbatim, and tsgo's resolve handler reads `params.Data` unconditionally (it
/// embeds the file name there to re-locate the language service); tsgo never reads
/// a `dataSupport` flag, so advertising it would be a meaningless non-spec field.
///
/// Completion fidelity: the parser DOES read `insertTextFormat` (snippet vs
/// plain), `commitCharacters`, `filterText`, `preselect`, and `labelDetails`, so
/// the matching client capabilities (`snippetSupport`, `commitCharactersSupport`,
/// `preselectSupport`, `labelDetailsSupport`) ARE advertised below — a server
/// only emits those item fields when the client claims support for them
/// (`filterText` needs no capability flag). The resolve handlers fold back the
/// STANDARD resolve property `labelDetails` in addition to `detail`/
/// `documentation`/`additionalTextEdits`, so `resolveSupport.properties` lists
/// those four. `command` is NOT a standard resolve property and is NOT advertised
/// (it is still folded opportunistically if the server returns one).
///
/// Intentionally NOT advertised (no handler fulfills them — over-claim would be a
/// silent no-op or worse): `documentSymbol`, `foldingRange`, `callHierarchy`,
/// `typeHierarchy`, `selectionRange`, `linkedEditingRange`, and `workspace/symbol`
/// (this provider issues none of those requests); completion `insertReplaceSupport`
/// (the completion parser maps a single `textEdit` range, not an insert/replace
/// pair); and `dataSupport` (not a real LSP capability — `data` is a
/// spec-transparent passthrough, see above).
fn build_client_capabilities() -> serde_json::Value {
    serde_json::json!({
        "textDocument": {
            // PUSH channel: `publishDiagnostics.tagSupport` IS the spec-defined
            // diagnostic-tag capability (`PublishDiagnosticsClientCapabilities`).
            // `relatedInformation: true` is the spec-defined gate for the secondary
            // "see declaration here" spans (`PublishDiagnosticsClientCapabilities.
            // relatedInformation`): a server only attaches `Diagnostic.
            // relatedInformation` when the client advertises it, so without this tsgo
            // silently strips the related spans `parse_lsp_diagnostic` is ready to
            // consume (the same silent-degradation class as the tag/completion gates).
            "publishDiagnostics": {
                "tagSupport": { "valueSet": [1, 2] },
                "relatedInformation": true
            },
            // PULL channel: `diagnostic.tagSupport` is NOT defined by LSP 3.17
            // (`DiagnosticClientCapabilities` has no `tagSupport`). It is a NON-SPEC
            // field retained for compatibility — tsgo may read it as a private
            // extension to gate pull-diagnostic tags — and is intentionally kept.
            // `diagnostic.relatedInformation` is likewise NOT an LSP 3.17 field
            // (`DiagnosticClientCapabilities` has no `relatedInformation` member); it
            // is retained alongside the tag flag as a private-extension hint so tsgo
            // may gate pull-channel related spans the same way as the push channel.
            "diagnostic": {
                "tagSupport": { "valueSet": [1, 2] },
                "relatedInformation": true
            },
            "completion": {
                // `get_completions` ALWAYS sends `CompletionParams.context` (the
                // trigger kind/character). Per LSP 3.17 a server only honours that
                // field when the client advertises `contextSupport: true`; without
                // it tsgo may ignore the trigger context and stop being trigger-aware.
                "contextSupport": true,
                // The completion parser (`parse_completion_item`) maps the full
                // standard `CompletionItemKind` range (1..=25) generically. The LSP
                // default value set (when omitted) is `Text..Reference` (1..=18), so
                // Class = 7 — on which component-tag completions depend — is INSIDE
                // that default range and preserved regardless. The valueSet stops tsgo
                // DOWNGRADING the UPPER kinds 19..=25 (outside the default range) to
                // `Text`. Advertise EXACTLY the parser's range 1..=25.
                "completionItemKind": {
                    "valueSet": [
                        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
                        19, 20, 21, 22, 23, 24, 25
                    ]
                },
                "completionItem": {
                    // A server only attaches these item fields when the client
                    // advertises support; the completion parser reads each one,
                    // so claim them here. `snippetSupport` gates `insertTextFormat`,
                    // `commitCharactersSupport` gates `commitCharacters`,
                    // `preselectSupport` gates `preselect`, `labelDetailsSupport`
                    // gates `labelDetails`. `filterText` needs no capability flag —
                    // a server may always send it.
                    "snippetSupport": true,
                    "commitCharactersSupport": true,
                    "preselectSupport": true,
                    "labelDetailsSupport": true,
                    // Only the STANDARD resolve properties this provider folds back
                    // are listed; listing any other invites discarded work.
                    // `labelDetails` IS a standard resolve property; `command` is
                    // NOT — advertising resolve-support for it would over-claim, so
                    // it is omitted (resolve_completion still folds a `command`
                    // opportunistically if the server happens to return one). `data`
                    // rides every resolve round-trip transparently per the LSP spec
                    // — there is no `dataSupport` capability to advertise.
                    "resolveSupport": {
                        "properties": [
                            "documentation",
                            "detail",
                            "additionalTextEdits",
                            "labelDetails"
                        ]
                    }
                }
            },
            // `get_code_actions` pulls fixes via `textDocument/codeAction`. Without
            // `codeActionLiteralSupport` TSGO may degrade to command-only actions
            // whose edits arrive only on a follow-up `codeAction/resolve` — a
            // resolve round-trip this provider does not implement — so advertise
            // literal support to keep the INLINE `WorkspaceEdit` on the action.
            // The value set lists the kinds the handler actually requests in
            // `context.only`: `quickfix` (the gate TSGO's quickfix providers honor —
            // this block ships the TS6133 unused-declaration QUICKFIX surface only).
            // The `source.removeUnused` SOURCE action is deferred to the `source.*`
            // backlog and is NOT requested here. NO `resolveSupport`/`dataSupport` —
            // that would force the resolve path.
            "codeAction": {
                "codeActionLiteralSupport": {
                    "codeActionKind": {
                        "valueSet": ["quickfix"]
                    }
                }
            }
        }
    })
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
        let content = content.to_string();
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
                    deliver_document_sync(
                        &transport,
                        &versions,
                        &contents_cache,
                        &path_owned,
                        &content,
                        DocumentSyncIntent::Open,
                        ProviderPriority::Interactive,
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
        let content_owned = content.to_string();
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsgo_load_file",
                format!("path={} content_len={}", path_owned, content_owned.len()),
                async {
                    contents_cache
                        .lock()
                        .await
                        .insert(contents_key(&path_owned), content_owned.into());
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
        let content = content.to_string();
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let mode = deliver_document_sync(
                &transport,
                &versions,
                &contents_cache,
                &path_owned,
                &content,
                DocumentSyncIntent::Update,
                ProviderPriority::Interactive,
            )
            .await?;
            match mode {
                DocumentSyncMode::DidChange { version } => {
                    crate::type_runtime_trace_event!(
                        "tsgo_update_file_result",
                        format!("path={} mode=didChange version={}", path_owned, version),
                    );
                }
                DocumentSyncMode::DidOpen => {
                    crate::type_runtime_trace_event!(
                        "tsgo_update_file_result",
                        format!("path={} mode=didOpen version=1", path_owned),
                    );
                }
            }
            Ok(())
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
                    deliver_document_close(
                        &transport,
                        &versions,
                        &contents_cache,
                        &path_owned,
                        ProviderPriority::Interactive,
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
                match cache.get(&contents_key(&path_owned)) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => {
                        // FAIL CLOSED: never fabricate a `(0, byte-offset)`
                        // position for the engine (see `get_hover`).
                        tracing::warn!(
                            "TSGO get_completions: no cached contents for {path_owned} — \
                             failing closed instead of fabricating a position"
                        );
                        return Ok(CompletionResult {
                            items: Vec::new(),
                            is_incomplete: false,
                        });
                    }
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
            let (line, character) = {
                let cache = contents_cache.lock().await;
                match cache.get(&contents_key(&path_owned)) {
                    Some(c) => offset_to_position(c, offset),
                    None => {
                        // FAIL CLOSED: without the synced content there is no
                        // valid position to convert to. A fabricated
                        // `(0, byte-offset)` coordinate is a malformed request
                        // the engine may not survive — never send one.
                        tracing::warn!(
                            "TSGO get_hover: no cached contents for {path_owned} — \n                             failing closed instead of fabricating a position"
                        );
                        return Ok(None);
                    }
                }
            };
            crate::type_runtime_trace_scope_async!(
                "tsgo_get_hover",
                format!(
                    "path={} uri={} offset={} line={} character={} content_cache_hit=true",
                    path_owned, uri, offset, line, character,
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
        let path_owned = path.to_string();
        let diagnostics_cache = Arc::clone(&self.diagnostics_cache);
        Box::pin(async move {
            // Use pull diagnostics (textDocument/diagnostic) — TSGO supports this
            // model rather than push (publishDiagnostics). Pull is synchronous:
            // we send a request and get the diagnostics back directly.
            match self.get_diagnostics_strict(&path_owned).await {
                Ok(diags) => {
                    tracing::debug!(
                        "get_diagnostics: pull returned {} diagnostics for {}",
                        diags.len(),
                        path_owned
                    );
                    Ok(diags)
                }
                Err(e) => {
                    // Pull diagnostics failed — fall back to push diagnostics cache.
                    tracing::debug!(
                        "get_diagnostics: pull failed ({e}), falling back to cache for {}",
                        path_owned
                    );
                    let cache_key = normalize_file_uri(&Self::path_to_uri(&path_owned));
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
                match cache.get(&contents_key(&path_owned)) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch)
                    }
                    None => {
                        // FAIL CLOSED: never fabricate a `(0, byte-offset)`
                        // position for the engine (see `get_hover`).
                        tracing::warn!(
                            "TSGO get_definition: no cached contents for {path_owned} — \
                             failing closed instead of fabricating a position"
                        );
                        return Ok(Vec::new());
                    }
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
                        cache
                            .get(&contents_key(&path_owned))
                            .map(|text| text.as_ref())
                    } else {
                        cache
                            .get(&contents_key(&target_path))
                            .map(|text| text.as_ref())
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
                match cache.get(&contents_key(&path_owned)) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch)
                    }
                    None => {
                        // FAIL CLOSED: never fabricate a `(0, byte-offset)`
                        // position for the engine (see `get_hover`).
                        tracing::warn!(
                            "TSGO get_type_definition: no cached contents for {path_owned} — \
                             failing closed instead of fabricating a position"
                        );
                        return Ok(Vec::new());
                    }
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
                        cache
                            .get(&contents_key(&path_owned))
                            .map(|text| text.as_ref())
                    } else {
                        cache
                            .get(&contents_key(&target_path))
                            .map(|text| text.as_ref())
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
            let (line, character) = {
                let cache = contents_cache.lock().await;
                match cache.get(&contents_key(&path_owned)) {
                    Some(c) => offset_to_position(c, offset),
                    None => {
                        // FAIL CLOSED: never fabricate a `(0, byte-offset)`
                        // position for the engine (see `get_hover`).
                        tracing::warn!(
                            "TSGO get_references: no cached contents for {path_owned} — \
                             failing closed instead of fabricating a position"
                        );
                        return Ok(Vec::new());
                    }
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
            // References are cross-file: each location's byte offsets must be computed against
            // THAT location's own file, not the queried file. Look up each target's content (disk
            // fallback inside `parse_lsp_location`), exactly as `get_definition` does — reusing the
            // queried file's single snapshot for every location packs cross-file offsets against
            // the WRONG file.
            let cache = contents_cache.lock().await;
            Ok(parse_lsp_locations_per_target(&locations, |target_path| {
                cache.get(target_path).map(|text| text.as_ref())
            }))
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
            let (line, character) = {
                let cache = contents_cache.lock().await;
                match cache.get(&contents_key(&path_owned)) {
                    Some(c) => offset_to_position(c, offset),
                    None => {
                        // FAIL CLOSED: never fabricate a `(0, byte-offset)`
                        // position for the engine (see `get_hover`).
                        tracing::warn!(
                            "TSGO get_rename_locations: no cached contents for {path_owned} — \
                             failing closed instead of fabricating a position"
                        );
                        return Ok(Vec::new());
                    }
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

            // Cross-file rename: convert each edit's range against ITS OWN target file's content
            // (disk fallback inside the parser), never the queried file's single snapshot — a
            // line-0 edit in the wrong file CORRUPTS it. Snapshot ONLY this workspace edit's target
            // files and RELEASE the async mutex before parsing, so the per-target blocking disk
            // fallback never runs under the lock (a multi-file rename would otherwise stall the
            // provider). Scanning the response bounds the snapshot to the touched files.
            let target_paths = crate::contents_snapshot::lsp_workspace_edit_target_paths(&result);
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                crate::contents_snapshot::targeted_contents_snapshot(&guard, &target_paths)
            };
            let mut locations = Vec::new();
            parse_workspace_edit_locations(
                &result,
                &|target_path| cache_snapshot.get(target_path).map(|text| text.as_ref()),
                &mut locations,
            );
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
                match cache.get(&contents_key(&path_owned)) {
                    Some(c) => offset_to_position(c, offset),
                    None => {
                        // FAIL CLOSED: never fabricate a `(0, byte-offset)`
                        // position for the engine (see `get_hover`).
                        tracing::warn!(
                            "TSGO get_signature_help: no cached contents for {path_owned} — \
                             failing closed instead of fabricating a position"
                        );
                        return Ok(None);
                    }
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
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let diagnostics = diagnostics.to_vec();
        Box::pin(async move {
            // TSGO's quickfix path requires a non-empty `context.diagnostics` whose
            // codes are INTEGERS (it skips string-coded diagnostics). With nothing
            // to act on, skip the round-trip.
            if diagnostics.is_empty() {
                return Ok(vec![]);
            }
            let (start_line, start_char, end_line, end_char, context_diagnostics) = {
                let cache = contents_cache.lock().await;
                let content = cache.get(&contents_key(&path_owned));
                let to_pos = |off: u32| match content {
                    Some(c) => offset_to_position(c, off),
                    None => (0, off),
                };
                let (sl, sc) = to_pos(start_offset);
                let (el, ec) = to_pos(end_offset);
                // Synthesize the LSP `Diagnostic` array TSGO matches fixes against:
                // each diagnostic's TSX byte range mapped to a line/character range
                // via the SAME `offset_to_position` used for the request range, plus
                // its INTEGER error code.
                let context_diagnostics: Vec<serde_json::Value> = diagnostics
                    .iter()
                    .map(|d| {
                        let (dsl, dsc) = to_pos(d.start);
                        let (del, dec) = to_pos(d.end);
                        serde_json::json!({
                            "range": {
                                "start": { "line": dsl, "character": dsc },
                                "end": { "line": del, "character": dec },
                            },
                            "code": d.code,
                            "severity": 1,
                            "source": "ts",
                            "message": "",
                        })
                    })
                    .collect();
                (sl, sc, el, ec, context_diagnostics)
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
                        "context": {
                            "diagnostics": context_diagnostics,
                            // `quickfix` is the gate TSGO's quickfix providers honor;
                            // this block requests the TS6133 unused-declaration
                            // QUICKFIX surface only. The `source.removeUnused` SOURCE
                            // action is deferred to the `source.*` backlog — not
                            // requested here. (When tsgo ports the per-diagnostic
                            // remove-unused codefix it returns under `quickfix`.)
                            "only": ["quickfix"],
                        },
                    }),
                )
                .await?;

            let items = result.as_array().cloned().unwrap_or_default();
            // Cross-file code-action edits: resolve each edit's range against ITS OWN target file's
            // content (disk fallback inside the parser), never the queried file's single snapshot.
            // Snapshot ONLY the files these actions target and RELEASE the async mutex before
            // parsing, so the per-target blocking disk fallback never runs under the lock (a fix-all
            // could stall the provider). Scanning the responses bounds the snapshot to touched files.
            let mut target_paths: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            for item in &items {
                target_paths.extend(crate::contents_snapshot::lsp_code_action_target_paths(item));
            }
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                crate::contents_snapshot::targeted_contents_snapshot(&guard, &target_paths)
            };
            Ok(items
                .iter()
                .filter_map(|item| {
                    parse_code_action(item, &|target_path| {
                        cache_snapshot.get(target_path).map(|text| text.as_ref())
                    })
                })
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
                cache.get(&contents_key(&path_owned)).cloned()
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
                match cache.get(&contents_key(&path_owned)) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => {
                        // FAIL CLOSED: never fabricate a `(0, byte-offset)`
                        // position for the engine (see `get_hover`).
                        tracing::warn!(
                            "TSGO get_document_highlights: no cached contents for {path_owned} — \
                             failing closed instead of fabricating a position"
                        );
                        return Ok(Vec::new());
                    }
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
                match cache.get(&contents_key(&path_owned)) {
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

            // Parse additionalTextEdits from the response. Edits may be absent —
            // a resolve can still enrich the item with detail/documentation/
            // labelDetails/command, so an empty edit list is NOT a reason to
            // return `None` (review: previously it was, dropping every non-edit
            // enrichment).
            let edits = result
                .get("additionalTextEdits")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let content_snapshot = {
                let cache = contents_cache.lock().await;
                cache.get(&contents_key(&path_owned)).cloned()
            };

            let additional_text_edits: Vec<ResolvedTextEdit> = edits
                .iter()
                .filter_map(|edit| parse_additional_text_edit(edit, content_snapshot.as_deref()))
                .collect();

            // The resolve response may also carry the lazy detail/documentation
            // and a refined `labelDetails` (a STANDARD resolve property we
            // advertise). A post-accept `command` is folded OPPORTUNISTICALLY: we
            // do NOT advertise resolve-support for `command` (it is not a standard
            // resolve property), but if the server returns one anyway, passing it
            // through is harmless — fold them all.
            let (detail, documentation) = extract_resolve_detail_and_documentation(&result);
            let label_details = result.get("labelDetails").and_then(parse_label_details);
            let command = parse_lsp_command(result.get("command"));

            // Return a result when ANY enrichment is present; otherwise `None`
            // (nothing to resolve) so the caller treats "no enrichment" uniformly.
            if additional_text_edits.is_empty()
                && detail.is_none()
                && documentation.is_none()
                && label_details.is_none()
                && command.is_none()
            {
                Ok(None)
            } else {
                Ok(Some(CompletionResolveResult {
                    additional_text_edits,
                    detail,
                    documentation,
                    label_details,
                    command,
                }))
            }
        })
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        let transport = Arc::clone(&self.transport);
        let child = self.child.as_ref();
        // Declare teardown intent BEFORE any teardown traffic: the child's exit
        // (stdout EOF) and any in-flight request timeouts are the REQUESTED
        // teardown, never a crash to report/restart from.
        self.teardown_intent.store(true, Ordering::SeqCst);
        Box::pin(async move {
            let Some(child) = child else {
                // Non-owning editor attach: close only this local feature bridge.
                // Never send shutdown/exit onto the editor-owned connection. The
                // Shutdown signal rides the UNBOUNDED control lane so a crashed /
                // wedged writer with a full interactive lane cannot block teardown.
                let _ = transport.control_tx.send(StdinMessage::Shutdown);
                return Ok(());
            };
            // Best-effort: try shutdown request + exit notification with overall 3s timeout.
            // If TSGO is unresponsive, we don't hang — the child has kill_on_drop.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                let _ = transport.request("shutdown", serde_json::Value::Null).await;
                let _ = transport.notify("exit", serde_json::Value::Null).await;
            })
            .await;
            // Signal the writer task to stop via the UNBOUNDED control lane — never
            // a bounded lane send that a wedged writer could park on.
            let _ = transport.control_tx.send(StdinMessage::Shutdown);

            let mut child = child
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            let Some(mut child) = child.take() else {
                // Idempotent/concurrent shutdown: another caller already owns teardown.
                return Ok(());
            };

            match tokio::time::timeout(std::time::Duration::from_secs(1), child.wait()).await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(error)) => {
                    let _ = child.start_kill();
                    let _ =
                        tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await;
                    Err(TypeProviderError::new(format!(
                        "failed to reap managed TSGO child: {error}"
                    )))
                }
                Err(_) => {
                    let kill_error = child.start_kill().err();
                    match tokio::time::timeout(std::time::Duration::from_secs(2), child.wait())
                        .await
                    {
                        Ok(Ok(_)) => Ok(()),
                        Ok(Err(error)) => Err(TypeProviderError::new(format!(
                            "failed to reap killed managed TSGO child: {error}{}",
                            kill_error.map_or_else(String::new, |kill| {
                                format!(" (kill also failed: {kill})")
                            })
                        ))),
                        Err(_) => Err(TypeProviderError::new(
                            kill_error.map_or_else(
                                || "timed out reaping killed managed TSGO child".to_string(),
                                |kill| {
                                    format!(
                                        "failed to kill managed TSGO child ({kill}) and timed out reaping it"
                                    )
                                },
                            ),
                        )),
                    }
                }
            }
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
        self.child.as_ref().and_then(|child| {
            child
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .and_then(Child::id)
        })
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
                    let content = contents_cache
                        .lock()
                        .await
                        .get(&contents_key(&path_owned))
                        .cloned();
                    Ok(items
                        .iter()
                        .filter_map(|d| {
                            parse_lsp_diagnostic(d, content.as_deref(), Some(path_owned.as_str()))
                        })
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
/// Parse a workspace edit's rename locations, resolving EACH edit's byte offsets against the
/// content of the file that edit targets.
///
/// A rename's edits are keyed by target URI (`changes: { [uri]: … }` or `documentChanges`), so a
/// cross-file rename touches several files. `content_for(target_path)` hands back each target's own
/// content; converting every edit against the queried file's single snapshot would pack cross-file
/// edit offsets against the WRONG file — and a rename edit at a wrong (line-0) offset CORRUPTS the
/// file. Mirrors the per-target content lookup `get_references` / `get_definition` use.
fn parse_workspace_edit_locations<'a>(
    result: &serde_json::Value,
    content_for: &impl Fn(&str) -> Option<&'a str>,
    locations: &mut Vec<RenameLocation>,
) {
    // Handle `changes: { [uri]: TextEdit[] }` format
    if let Some(changes) = result.get("changes").and_then(|v| v.as_object()) {
        for (change_uri, edits) in changes {
            if let Some(arr) = edits.as_array() {
                for edit in arr {
                    if let Some(loc) = parse_rename_edit(change_uri, edit, content_for) {
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
                    if let Some(loc) = parse_rename_edit(dc_uri, edit, content_for) {
                        locations.push(loc);
                    }
                }
            }
        }
    }
}

fn parse_rename_edit<'a>(
    uri: &str,
    edit: &serde_json::Value,
    content_for: &impl Fn(&str) -> Option<&'a str>,
) -> Option<RenameLocation> {
    let range = edit.get("range")?;
    // Canonical filesystem-path ID, matching `TypeLocation.path` and the tsserver provider — NOT
    // the raw `file://` URI (which would split file identity vs the documents/VFS layer on
    // Windows). The same canonical path keys the per-target content lookup.
    let path = uri_to_file_path(uri);
    // Resolve each rename edit's range against ITS OWN file content, with a per-target disk fallback
    // for a cache miss. FAIL CLOSED via the STRICT converter: a rename is a WRITE edit, so a total
    // cache+disk miss or an out-of-range position DROPS the location (returns None) rather than
    // packing a line-0 / clamped offset that CORRUPTS the file. The caller collects via push-if-Some,
    // so a dropped location skips only that span.
    let (start, end) = parse_range_to_offsets_strict_with_disk_fallback(range, &path, content_for)?;
    Some(RenameLocation { path, start, end })
}

/// Parse an LSP `ParameterInformation.label`, which is EITHER a JSON string OR a
/// two-element array of unsigned integers (`[start, end)` UTF-16 offsets into the
/// enclosing signature label).
///
/// `signature_label_utf16_len` is the UTF-16 code-unit length of the ENCLOSING
/// signature label; the offset form is bounds-checked against it.
///
/// Fail-closed (returns `None` so the parameter is dropped, NEVER rendered with a
/// fabricated label or a wrong/truncated/out-of-bounds bold span) for any of:
/// - a shape that is neither a string nor a 2-element array;
/// - an offset element that is not a `u64` (e.g. negative, fractional) or that
///   exceeds `u32::MAX` (`u32::try_from` overflow — a truncating `as u32` would
///   silently fabricate a wrong span);
/// - an empty or inverted span (`start >= end`);
/// - an out-of-bounds span (`end > signature_label_utf16_len`).
///
/// An offset span that is truncated, inverted, or out of bounds would bold the
/// WRONG run of the signature label (worse than no offsets), so it is rejected
/// rather than emitted.
fn parse_lsp_parameter_label(
    value: &serde_json::Value,
    signature_label_utf16_len: u32,
) -> Option<ParameterLabelKind> {
    if let Some(s) = value.as_str() {
        return Some(ParameterLabelKind::Simple(s.to_string()));
    }
    if let Some(arr) = value.as_array() {
        if arr.len() == 2 {
            // Checked: a value beyond u32::MAX must NOT truncate into a wrong
            // offset — drop the offset form instead (`as u32` would fabricate).
            let start = u32::try_from(arr[0].as_u64()?).ok()?;
            let end = u32::try_from(arr[1].as_u64()?).ok()?;
            // Reject empty/inverted spans and out-of-bounds spans: either would
            // bold the wrong span of the label.
            if start >= end || end > signature_label_utf16_len {
                return None;
            }
            return Some(ParameterLabelKind::Offsets(start, end));
        }
    }
    None
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
                    // UTF-16 length of THIS signature label; the offset-form param
                    // labels below bounds-check their `end` against it.
                    let label_utf16_len = label.encode_utf16().count() as u32;
                    let documentation = sig.get("documentation").and_then(extract_markup_string);
                    let parameters = sig
                        .get("parameters")
                        .and_then(|v| v.as_array())
                        .map(|params| {
                            params
                                .iter()
                                .filter_map(|p| {
                                    // LSP `ParameterInformation.label` is EITHER a
                                    // string OR a `[start, end)` UTF-16 offset pair
                                    // into the signature label. Parse whichever the
                                    // server sent; fail-closed (skip) on neither —
                                    // never fabricate offsets.
                                    let plabel = parse_lsp_parameter_label(
                                        p.get("label")?,
                                        label_utf16_len,
                                    )?;
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
                    // LSP `SignatureInformation.activeParameter` (optional,
                    // per-signature). Carried when present; checked (an out-of-range
                    // index → `None`, never a truncated wrong index).
                    let active_parameter = sig
                        .get("activeParameter")
                        .and_then(|v| v.as_u64())
                        .and_then(|v| u32::try_from(v).ok());
                    Some(SignatureInfo {
                        label,
                        documentation,
                        parameters,
                        active_parameter,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    SignatureHelp {
        signatures,
        // Checked: an out-of-range index becomes `None`, never a truncated value.
        active_signature: result
            .get("activeSignature")
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok()),
        active_parameter: result
            .get("activeParameter")
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok()),
    }
}

/// Parse a CodeAction from a JSON response.
/// Parse a code action, resolving EACH edit's byte offsets against the content of the file that
/// edit targets. A code action's edits are keyed by target URI, so a cross-file quick-fix /
/// refactor edits several files; `content_for(target_path)` supplies each target's own content.
/// Converting every edit against the queried file's single snapshot packs cross-file edit offsets
/// against the WRONG file — the same one-snapshot hazard fixed for references / rename.
fn parse_code_action<'a>(
    item: &serde_json::Value,
    content_for: &impl Fn(&str) -> Option<&'a str>,
) -> Option<TypeCodeAction> {
    let title = item.get("title")?.as_str()?.to_string();
    let kind = item.get("kind").and_then(|v| v.as_str()).map(String::from);

    let mut edits = Vec::new();
    if let Some(edit) = item.get("edit") {
        if let Some(changes) = edit.get("changes").and_then(|v| v.as_object()) {
            for (change_uri, text_edits) in changes {
                if let Some(arr) = text_edits.as_array() {
                    for te in arr {
                        if let Some(ce) = parse_text_edit_to_code_edit(change_uri, te, content_for)
                        {
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
                        if let Some(ce) = parse_text_edit_to_code_edit(dc_uri, te, content_for) {
                            edits.push(ce);
                        }
                    }
                }
            }
        }
    }

    // An edit-less action is not actionable — drop it, mirroring
    // `parse_tsserver_code_action`, so a no-op action never leaves the parse
    // boundary (every edit may have failed closed on unresolvable content).
    if edits.is_empty() {
        return None;
    }

    Some(TypeCodeAction { title, kind, edits })
}

/// Parse one completion `additionalTextEdit` (e.g. an auto-import insertion) into a resolved
/// byte-offset edit against the file's content.
///
/// FAIL CLOSED: an `additionalTextEdit` is a WRITE edit. On a content miss the edit is DROPPED (no
/// `pack_position` line-0 sentinel); the range converts through the CHECKED converter so an
/// out-of-range position drops rather than clamping to EOF, and an inverted span drops too. A
/// `line`/`character` exceeding `u32::MAX` drops here via the checked `u32::try_from` — never a
/// silent `as u32` truncation that would wrap a huge value into an in-range offset and land the
/// WRITE at the wrong location. The caller collects via `filter_map`, so a dropped edit skips only
/// itself.
fn parse_additional_text_edit(
    edit: &serde_json::Value,
    content: Option<&str>,
) -> Option<ResolvedTextEdit> {
    let range = edit.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    let sl = u32::try_from(start.get("line")?.as_u64()?).ok()?;
    let sc = u32::try_from(start.get("character")?.as_u64()?).ok()?;
    let el = u32::try_from(end.get("line")?.as_u64()?).ok()?;
    let ec = u32::try_from(end.get("character")?.as_u64()?).ok()?;
    let new_text = edit.get("newText")?.as_str()?.to_string();

    let c = content?;
    let start_offset = position_to_offset_checked(c, sl, sc)?;
    let end_offset = position_to_offset_checked(c, el, ec)?;
    if start_offset > end_offset {
        return None;
    }

    Some(ResolvedTextEdit {
        start: start_offset,
        end: end_offset,
        new_text,
    })
}

fn parse_text_edit_to_code_edit<'a>(
    uri: &str,
    te: &serde_json::Value,
    content_for: &impl Fn(&str) -> Option<&'a str>,
) -> Option<TypeCodeEdit> {
    let range = te.get("range")?;
    let new_text = te.get("newText")?.as_str()?.to_string();
    // Canonical filesystem-path ID (see `parse_rename_edit`), not the raw URI; keys the content.
    let path = uri_to_file_path(uri);
    // Per-target content with a disk fallback for a cache miss. FAIL CLOSED via the STRICT converter:
    // a total cache+disk miss or an out-of-range position DROPS the edit (returns None) rather than
    // packing a line-0 / clamped offset that the merge layer would apply at the WRONG location and
    // corrupt the file. The caller collects via push-if-Some, so a dropped edit skips only itself.
    let (start, end) = parse_range_to_offsets_strict_with_disk_fallback(range, &path, content_for)?;
    Some(TypeCodeEdit {
        path,
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

/// Like [`parse_range_to_offsets`], but FAIL CLOSED for EDIT paths: resolves the target content
/// (cache → disk) and, when content is unavailable, returns `None` (NO `pack_position` sentinel).
/// With content present it converts through the CHECKED [`position_to_offset_checked`], so an
/// out-of-range position DROPS instead of clamping to EOF, and an inverted `start > end` span drops
/// too.
///
/// Edit-producing parsers (`parse_text_edit_to_code_edit`, `parse_rename_edit`) route through this
/// so a total cache+disk miss or an out-of-range position never packs a line-0 / clamped offset that
/// the merge layer would apply as a corrupting WRITE. Navigation-only callers keep the lenient
/// `parse_range_to_offsets` (a packed sentinel is a tolerable display miss).
fn parse_range_to_offsets_strict_with_disk_fallback<'a>(
    range: &serde_json::Value,
    path: &str,
    content_for: &impl Fn(&str) -> Option<&'a str>,
) -> Option<(u32, u32)> {
    let start = range.get("start")?;
    let end = range.get("end")?;
    let sl = u32::try_from(start.get("line")?.as_u64()?).ok()?;
    let sc = u32::try_from(start.get("character")?.as_u64()?).ok()?;
    let el = u32::try_from(end.get("line")?.as_u64()?).ok()?;
    let ec = u32::try_from(end.get("character")?.as_u64()?).ok()?;

    let disk_content;
    let content = match content_for(path) {
        Some(content) => Some(content),
        None => {
            disk_content = std::fs::read_to_string(path).ok();
            disk_content.as_deref()
        }
    };
    // FAIL CLOSED on a total content miss — never pack a line-0 offset for a WRITE edit.
    let c = content?;
    let s = position_to_offset_checked(c, sl, sc)?;
    let e = position_to_offset_checked(c, el, ec)?;
    if s > e {
        return None;
    }
    Some((s, e))
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
/// 2. **Workspace `node_modules`** — the rc `@typescript/typescript-*` binary
///    installed as a workspace dependency (flat-npm OR pnpm layout). This is the
///    common real-project case (a project that pins `typescript@>=7` in
///    `package.json`) that the npm/npx cache misses.
/// 3. **npm/npx cache** — the rc native binary under the npm or npx cache.
///
/// `workspace_root` is the directory whose `node_modules` is searched in tier 2;
/// pass `None` (or a root without a matching `node_modules`) to skip straight to
/// the cache tier. Discovery is rc-only: there is no `tsgo`-on-`PATH` lookup and
/// no `.bin/tsgo` shim probe (a global `tsgo` is the retired native-preview
/// engine). Returns the existing [`TsgoBinaryLookupError`] (cache
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

/// Find the rc tsgo engine binary from an explicit override or the npm/npx
/// cache.
///
/// Checks (in order):
/// 1. The [`TSGO_BINARY_ENV`] (`VERTER_TSGO_BIN`) override, when it names an
///    existing file — the dev/baseline/oracle-gen callers' escape hatch when
///    the engine lives outside the workspace `node_modules`.
/// 2. The rc native binary from the npm/npx cache
///    (`@typescript/typescript-{platform}/lib/tsc`).
///
/// This is the rc-only PATH/cache tier of [`find_tsgo_binary_canonical`]
/// (tier 3); production should call the canonical entry point so the workspace
/// `node_modules` tier is honored first. There is intentionally NO `tsgo`-on-`PATH`
/// lookup and NO `.bin/tsgo` shim probe: a global `tsgo` is the retired
/// native-preview engine, so resolving one would silently launch the legacy
/// engine. Discovery fails closed instead.
pub fn find_tsgo_binary() -> Result<String, TsgoBinaryLookupError> {
    if let Some(path) = tsgo_binary_env_override() {
        tracing::debug!("TSGO discovery: using {TSGO_BINARY_ENV} override at {path}");
        return Ok(path);
    }

    let cache_roots = collect_npm_cache_roots(
        npm_config_cache_from_env(),
        npm_config_get_cache(),
        default_npm_cache_root(),
    );
    tracing::debug!("TSGO discovery: cache roots = {:?}", cache_roots);

    let result = find_tsgo_binary_in(&cache_roots);
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
/// the platform-specific rc `@typescript/typescript-{plat}-{arch}` binary
/// directly under `<node_modules>`:
///
/// - flat npm: `<node_modules>/@typescript/typescript-{plat}/lib/tsc[.exe]`
/// - pnpm:     `<node_modules>/.pnpm/@typescript+typescript-{plat}@*/node_modules/@typescript/typescript-{plat}/lib/tsc[.exe]`
///
/// Platform-aware (reuses [`tsgo_native_binary_rel_paths`]); returns `None` when
/// no binary is present. Paths are built with `Path::join`, never string
/// concatenation, so it is portable across macOS / Windows / Linux.
pub fn find_tsgo_binary_under_node_modules(node_modules: &std::path::Path) -> Option<String> {
    // Flat npm layout: <node_modules>/@typescript/typescript-*/lib/tsc[.exe].
    // `flat_npm_tsgo_candidate_paths` produces the rc `tsc` candidates (the sole
    // engine source); the first existing candidate wins.
    for candidate in flat_npm_tsgo_candidate_paths(node_modules) {
        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }

    // pnpm layout: <node_modules>/.pnpm/<pkg>@<ver>/node_modules/@typescript/typescript-*/lib/tsc[.exe].
    // The rc `typescript` package is the sole engine SOURCE; mtime ordering
    // breaks ties between multiple installed rc store entries.
    let pnpm_dir = node_modules.join(".pnpm");
    if let Ok(entries) = std::fs::read_dir(&pnpm_dir) {
        let store_dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();

        for source in TSGO_ENGINE_SOURCES {
            let mut dirs: Vec<PathBuf> = store_dirs
                .iter()
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(source.pnpm_store_prefix))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            // Prefer the most recently modified store entry (newest install) of
            // THIS source.
            dirs.sort_by_key(|b| std::cmp::Reverse(entry_modified(b)));
            for dir in dirs {
                for candidate in pnpm_store_tsgo_candidate_paths(&dir) {
                    if candidate.exists() {
                        return Some(candidate.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    None
}

/// Build the flat-npm tsgo candidate paths under a `node_modules` directory.
///
/// Pure path construction (no filesystem access) so the layout math is unit
/// testable on every platform: `<node_modules>/@typescript/typescript-{plat}-{arch}/lib/tsc[.exe]`.
/// Built with `Path::join` (never string concatenation) for portability.
fn flat_npm_tsgo_candidate_paths(node_modules: &std::path::Path) -> Vec<PathBuf> {
    tsgo_native_binary_rel_paths()
        .into_iter()
        .map(|rel| {
            // `rel` is rooted at "node_modules/…"; strip that prefix to join
            // under the given node_modules dir.
            let rel_under_nm = rel
                .strip_prefix("node_modules/")
                .map(str::to_owned)
                .unwrap_or(rel);
            node_modules.join(rel_under_nm)
        })
        .collect()
}

/// Build the pnpm-store tsgo candidate paths under a single pnpm store entry
/// (`<node_modules>/.pnpm/@typescript+typescript-{plat}@{ver}`).
///
/// Pure path construction (no filesystem access): the store entry nests a real
/// `node_modules/@typescript/typescript-{plat}-{arch}/lib/tsc[.exe]`, so
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

/// Resolve the rc tsgo engine binary from the npm/npx cache roots.
///
/// rc-only: scans each cache root's `_npx/*` entries (newest install first) for
/// the rc `@typescript/typescript-{plat}/lib/tsc[.exe]` native binary. There is
/// intentionally no `.bin/tsgo` shim probe — a `tsgo`-named shim is the retired
/// native-preview engine. Returns the checked-locations error when no rc binary
/// is present.
fn find_tsgo_binary_in(cache_roots: &[PathBuf]) -> Result<String, TsgoBinaryLookupError> {
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

    Err(TsgoBinaryLookupError::new(checked_locations))
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

/// The TS≥7 tsgo-engine binary source — the installed `typescript@>=7`
/// package's native binary. Mirrors the gate's discovery
/// (`tools/tsgo-api-gate/run-gate.mjs`): the published `typescript@7.x` (e.g.
/// `7.0.2`) ships the typescript-go engine as `tsc` (renamed from `tsgo`) in
/// `@typescript/typescript-{plat}-{arch}`. This is the SOLE engine source.
const TSGO_ENGINE_SOURCES: &[TsgoEngineSource] = &[TsgoEngineSource {
    scope_package_prefix: "@typescript/typescript-",
    pnpm_store_prefix: "@typescript+typescript-",
    binary_stem: "tsc",
}];

/// One TS≥7 tsgo-engine binary source (package family + binary name).
#[derive(Clone, Copy)]
struct TsgoEngineSource {
    /// The scoped platform-package name prefix, e.g. `@typescript/typescript-`
    /// (the `{plat}-{arch}` suffix is appended per target).
    scope_package_prefix: &'static str,
    /// The pnpm-store entry prefix, e.g. `@typescript+typescript-`.
    pnpm_store_prefix: &'static str,
    /// The binary file stem (no extension): `tsc` for the rc engine.
    binary_stem: &'static str,
}

/// The `{plat}-{arch}` platform-package suffixes, current platform first so its
/// binary is preferred within each source. Plain data — no filesystem.
fn tsgo_platform_arch_suffixes() -> Vec<&'static str> {
    let mut suffixes = Vec::new();

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    suffixes.push("win32-x64");
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    suffixes.push("win32-arm64");
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    suffixes.push("linux-x64");
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    suffixes.push("linux-arm64");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    suffixes.push("darwin-x64");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    suffixes.push("darwin-arm64");

    // Append the remaining platforms (cross-compilation / test stability); the
    // current-platform suffix (if any) stays first via the dedup below.
    for s in [
        "win32-x64",
        "win32-arm64",
        "linux-x64",
        "linux-arm64",
        "darwin-x64",
        "darwin-arm64",
    ] {
        if !suffixes.contains(&s) {
            suffixes.push(s);
        }
    }

    suffixes
}

/// The relative path under `<node_modules>` to a source's platform binary,
/// e.g. `node_modules/@typescript/typescript-win32-x64/lib/tsc.exe`. The `.exe`
/// suffix is added on Windows. Built as an owned `String` (each source/platform
/// combination is distinct), portable across OSes.
fn tsgo_source_rel_path(source: &TsgoEngineSource, plat_arch: &str) -> String {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    format!(
        "node_modules/{}{}/lib/{}{}",
        source.scope_package_prefix, plat_arch, source.binary_stem, ext
    )
}

/// The TS≥7 native-binary relative paths under `<node_modules>`: the rc
/// `@typescript/typescript-*` `tsc` binary (the sole engine source), current
/// platform first.
fn tsgo_native_binary_rel_paths() -> Vec<String> {
    let suffixes = tsgo_platform_arch_suffixes();
    let mut rel_paths = Vec::new();
    for source in TSGO_ENGINE_SOURCES {
        for plat_arch in &suffixes {
            let rel = tsgo_source_rel_path(source, plat_arch);
            if !rel_paths.contains(&rel) {
                rel_paths.push(rel);
            }
        }
    }
    rel_paths
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
    // Windows `\` separators normalize to `/` so the same root reached via either
    // separator dedups; backslash is a valid filename char on Unix, so that part
    // stays Windows-only. Case folds on a case-insensitive filesystem (Windows /
    // default macOS) through the single shared FS-identity policy so case-variant
    // roots dedup, and is preserved on a case-sensitive one (Linux).
    let value = path.to_string_lossy();
    let separator_normalized = if cfg!(windows) {
        value.replace('\\', "/")
    } else {
        value.into_owned()
    };
    if verter_span::path::fs_is_case_insensitive() {
        separator_normalized.to_ascii_lowercase()
    } else {
        separator_normalized
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
        // Seed resolvable content keyed by the CANONICAL path so the fail-closed rename
        // location survives; the assertion under test is the canonical path, not the raw URI.
        let content = "ab";
        let content_for = |p: &str| -> Option<&str> { (p == "d:/proj/App.vue").then_some(content) };
        let loc = parse_rename_edit("file:///D:/proj/App.vue", &edit_json(), &content_for).unwrap();
        assert_eq!(loc.path, "d:/proj/App.vue");
        assert_ne!(loc.path, "file:///D:/proj/App.vue");
        assert!(!loc.path.starts_with("file://"));
    }

    /// Per-target cross-file RENAME IPC: each rename edit's line:col range is converted against ITS
    /// OWN file's content. The cache-miss target falls back to a per-target DISK read — never the
    /// queried file's snapshot — so the byte offset lands on the real symbol, not line 0.
    ///
    /// Discriminating: the symbol sits on LINE 2 (`character` 0 of that line) of the target file,
    /// so the correct offset is well past 0. Resolving against the queried-file snapshot (or
    /// packing line:col) would yield a different, wrong offset; the test asserts the EXACT offset
    /// computed from the target's own content.
    #[test]
    fn parse_rename_edit_resolves_each_target_against_its_own_file_disk_fallback() {
        use super::{position_to_offset, uri_to_file_path};

        // Target file content: the renamed symbol is on line 2 (0-based), not line 0.
        let target_src = "// header line 0\nconst pad = 1;\nexport const renamed = 2;\n";
        let want_off = target_src.find("renamed").expect("symbol present") as u32;
        let (want_line, want_char) = super::offset_to_position(target_src, want_off);
        assert_eq!(want_line, 2, "fixture precondition: symbol on line 2");

        // Write the target to a real temp file, derive its canonical path + matching file:// URI.
        let dir = std::env::temp_dir().join(format!(
            "verter_tsgo_rename_pertarget_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("target.ts");
        std::fs::write(&file, target_src).unwrap();
        let canonical = uri_to_file_path(&format!(
            "file:///{}",
            file.to_string_lossy().replace('\\', "/")
        ));
        let uri = super::path_to_file_uri_string(&canonical);

        let edit = serde_json::json!({
            "range": {
                "start": { "line": want_line, "character": want_char },
                "end": { "line": want_line, "character": want_char + "renamed".len() as u32 },
            }
        });

        // `content_for` is a CACHE MISS for this path → forces the per-target disk fallback.
        let loc = parse_rename_edit(&uri, &edit, &|_p: &str| None)
            .expect("rename edit resolves through the per-target disk fallback");

        let want_end =
            position_to_offset(target_src, want_line, want_char + "renamed".len() as u32);
        assert_eq!(
            (loc.start, loc.end),
            (want_off, want_end),
            "the rename edit must resolve against the TARGET file's own content (disk fallback), \
             not pack a line-0 offset — got start={} end={}, want start={want_off} end={want_end}",
            loc.start,
            loc.end,
        );
        // Discriminating negative: a cache miss must resolve via disk, never pack a line:col
        // sentinel `pack_position(line, char)`; assert we did NOT get that.
        assert_ne!(
            loc.start,
            super::pack_position(want_line, want_char),
            "must NOT be the packed line:col fallback (that is the corrupting line-0 path)"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Twin of the rename per-target test for CODE ACTIONS: a code-action text edit's range is
    /// resolved against ITS OWN target file with the per-target disk fallback on a cache miss.
    #[test]
    fn parse_text_edit_resolves_each_target_against_its_own_file_disk_fallback() {
        use super::{position_to_offset, uri_to_file_path};

        let target_src = "import a from 'x';\nconst y = 1;\nexport const fixme = 3;\n";
        let want_off = target_src.find("fixme").expect("symbol present") as u32;
        let (want_line, want_char) = super::offset_to_position(target_src, want_off);
        assert_eq!(want_line, 2, "fixture precondition: symbol on line 2");

        let dir = std::env::temp_dir().join(format!(
            "verter_tsgo_codeaction_pertarget_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("target.ts");
        std::fs::write(&file, target_src).unwrap();
        let canonical = uri_to_file_path(&format!(
            "file:///{}",
            file.to_string_lossy().replace('\\', "/")
        ));
        let uri = super::path_to_file_uri_string(&canonical);

        let te = serde_json::json!({
            "range": {
                "start": { "line": want_line, "character": want_char },
                "end": { "line": want_line, "character": want_char + "fixme".len() as u32 },
            },
            "newText": "fixed"
        });

        let edit = parse_text_edit_to_code_edit(&uri, &te, &|_p: &str| None)
            .expect("code-action edit resolves through the per-target disk fallback");

        let want_end = position_to_offset(target_src, want_line, want_char + "fixme".len() as u32);
        assert_eq!(
            (edit.start, edit.end),
            (want_off, want_end),
            "the code-action edit must resolve against the TARGET file's own content (disk \
             fallback), not pack a line-0 offset",
        );
        assert_ne!(
            edit.start,
            super::pack_position(want_line, want_char),
            "must NOT be the packed line:col fallback"
        );

        let _ = std::fs::remove_dir_all(&dir);
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
        // Seed resolvable content keyed by the CANONICAL path so the fail-closed edit survives;
        // the assertion under test is that the stored path is canonical, not the raw `file://` URI.
        let content = "ab";
        let content_for = |p: &str| -> Option<&str> { (p == "d:/proj/App.vue").then_some(content) };
        let edit =
            parse_text_edit_to_code_edit("file:///D:/proj/App.vue", &te, &content_for).unwrap();
        assert_eq!(edit.path, "d:/proj/App.vue");
        assert_ne!(edit.path, "file:///D:/proj/App.vue");
        assert!(!edit.path.starts_with("file://"));
    }

    /// A URI whose canonical path is guaranteed absent on disk: built under the OS temp dir with a
    /// process- and time-unique segment, then removed so neither the file nor its parent exists.
    /// Returned in the same `file://` form production resolves through.
    fn absent_target_uri(tag: &str) -> String {
        use super::{path_to_file_uri_string, uri_to_file_path};
        let dir = std::env::temp_dir().join(format!(
            "verter_tsgo_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // Ensure absence: remove the directory tree if a prior run left it behind.
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("gone.ts");
        let canonical = uri_to_file_path(&format!(
            "file:///{}",
            file.to_string_lossy().replace('\\', "/")
        ));
        path_to_file_uri_string(&canonical)
    }

    /// A code-action text edit whose target is absent from the cache AND unreadable on disk must be
    /// DROPPED (returns `None`). Fails if the converter emits a packed line:col offset on a cache +
    /// disk miss — that offset would corrupt the WRONG file.
    #[test]
    fn parse_text_edit_to_code_edit_drops_when_content_unavailable() {
        let te = serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 1 }
            },
            "newText": "x"
        });
        let uri = absent_target_uri("code_edit_gone");
        let edit = parse_text_edit_to_code_edit(&uri, &te, &|_p| None);
        assert!(
            edit.is_none(),
            "a code-action edit whose content is unavailable must be DROPPED (fail-closed), never \
             packed: {edit:?}"
        );
    }

    /// A rename edit whose target is absent from the cache AND unreadable on disk must be DROPPED —
    /// a rename is a WRITE edit, so a packed line:col offset would corrupt the file.
    #[test]
    fn parse_rename_edit_drops_when_content_unavailable() {
        let uri = absent_target_uri("rename_gone");
        let loc = parse_rename_edit(&uri, &edit_json(), &|_p| None);
        assert!(
            loc.is_none(),
            "a rename edit whose content is unavailable must be DROPPED (fail-closed), never \
             packed: {loc:?}"
        );
    }

    /// A code-action edit whose content IS available but whose position is OUT OF RANGE (past EOF)
    /// is DROPPED, not clamped to a content-length offset — a clamped WRITE corrupts the file.
    #[test]
    fn parse_text_edit_to_code_edit_drops_out_of_range_position() {
        let te = serde_json::json!({
            "range": {
                // Line 999 is far past the 1-line content → the codec would clamp to EOF.
                "start": { "line": 999, "character": 0 },
                "end": { "line": 999, "character": 3 }
            },
            "newText": "boom"
        });
        let content = "short";
        let content_for = |p: &str| -> Option<&str> { (p == "d:/proj/oob.ts").then_some(content) };
        let edit = parse_text_edit_to_code_edit("file:///D:/proj/oob.ts", &te, &content_for);
        assert!(
            edit.is_none(),
            "an out-of-range code-action edit must be DROPPED, never clamped to EOF: {edit:?}"
        );
    }

    /// A `line`/`character` that exceeds `u32::MAX` must DROP the edit. The danger is a SILENT
    /// `as u32` truncation: `u32::MAX as u64 + 1` wraps to `0`, an in-range (line 0 / char 0)
    /// position the checked converter would ACCEPT — so the edit would land at the wrong offset.
    /// Content is valid and the wrapped position is in range, so the ONLY reason to drop is the
    /// overflow itself; that is what makes this discriminating.
    #[test]
    fn parse_text_edit_to_code_edit_drops_on_position_overflow() {
        // u32::MAX + 1 → truncates to 0 (a VALID line 0 / char 0) under a lossy `as u32`.
        let overflow = u32::MAX as u64 + 1;
        let te = serde_json::json!({
            "range": {
                "start": { "line": overflow, "character": 0 },
                "end": { "line": 0, "character": 1 }
            },
            "newText": "boom"
        });
        let content = "ab";
        let content_for = |p: &str| -> Option<&str> { (p == "d:/proj/ovf.ts").then_some(content) };
        let edit = parse_text_edit_to_code_edit("file:///D:/proj/ovf.ts", &te, &content_for);
        assert!(
            edit.is_none(),
            "a u64>u32::MAX position must be DROPPED, never truncated into an in-range offset: \
             {edit:?}"
        );

        // POSITIVE CONTROL: an in-range position with the SAME content still produces the correct
        // edit — the overflow guard must not change in-range behavior.
        let te_ok = serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 1 }
            },
            "newText": "x"
        });
        let ok = parse_text_edit_to_code_edit("file:///D:/proj/ovf.ts", &te_ok, &content_for)
            .expect("an in-range edit must still be produced");
        assert_eq!((ok.start, ok.end), (0, 1), "in-range offsets unchanged");
    }

    /// A rename span whose `character` exceeds `u32::MAX` must DROP the location. `u32::MAX + 1`
    /// truncates to `0` (a valid char 0 on line 0) under a lossy `as u32`, so the checked converter
    /// alone cannot catch it — the truncation must fail closed BEFORE the converter runs.
    #[test]
    fn parse_rename_edit_drops_on_position_overflow() {
        let overflow = u32::MAX as u64 + 1;
        let edit = serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": overflow }
            }
        });
        let content = "ab";
        let content_for = |p: &str| -> Option<&str> { (p == "d:/proj/ovf.ts").then_some(content) };
        let loc = parse_rename_edit("file:///D:/proj/ovf.ts", &edit, &content_for);
        assert!(
            loc.is_none(),
            "a u64>u32::MAX rename position must be DROPPED, never truncated: {loc:?}"
        );

        // POSITIVE CONTROL: the in-range rename span still resolves to the correct offsets.
        let edit_ok = serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 1 }
            }
        });
        let ok = parse_rename_edit("file:///D:/proj/ovf.ts", &edit_ok, &content_for)
            .expect("an in-range rename span must still resolve");
        assert_eq!((ok.start, ok.end), (0, 1), "in-range offsets unchanged");
    }
}

// Transport-level tests that use runtime-local types live in the sibling
// `ipc_tests.rs`. Tests that depend on LSP-internal types (PositionMapper,
// uri_to_canonical_id, merge) or on `verter_session` compilation stay in
// `verter_lsp`.
#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;
