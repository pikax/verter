//! tsserver transport layer: newline-delimited JSON over stdio.
//!
//! Spawns `node tsserver.js` as a child process and communicates using
//! the tsserver protocol (NOT LSP Content-Length framing):
//!
//! Request:  `{"seq":N,"type":"request","command":"...","arguments":{...}}\n`
//! Response: `{"seq":N,"type":"response","command":"...","request_seq":N,"success":true,"body":{...}}\n`
//! Event:    `{"seq":N,"type":"event","event":"...","body":{...}}\n`

use std::collections::{BTreeSet, HashMap, HashSet};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

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

/// In-flight requests awaiting a response, keyed by tsserver sequence number.
///
/// A `std::sync::Mutex`, not an async one: every critical section is a single
/// map operation with no await inside it, and a synchronous lock is what lets
/// [`TsserverPendingRequest::drop`] clean up. A cancelled request is dropped,
/// not awaited to completion, so cleanup that could only run on an async path
/// would never run at all.
#[derive(Default)]
struct TsserverPendingState {
    map: HashMap<i64, oneshot::Sender<serde_json::Value>>,
    closed: bool,
    /// Start of the current non-empty interval. A provider that was idle for a
    /// long time must receive a full silence allowance after new work arrives.
    pending_since: Option<std::time::Instant>,
}

#[derive(Default)]
struct TsserverPendingRequests {
    state: StdMutex<TsserverPendingState>,
    /// User-facing requests currently in flight. Background diagnostics may
    /// enter tsserver only while this is zero.
    interactive_in_flight: AtomicU32,
    /// Wakes background diagnostics when the last interactive request exits.
    interactive_idle: Notify,
    /// Sequence number of the single active background request, or zero.
    background_seq: AtomicI64,
    /// Advanced whenever interactive traffic arrives. A background request
    /// uses this to distinguish preemption from an ordinary provider failure.
    background_preemption_epoch: AtomicU64,
    /// Diagnostics are single-flight so they cannot build a background queue in
    /// front of later user requests on tsserver's one JavaScript thread.
    background_gate: Mutex<()>,
    /// Synchronous configured-project builds currently bracketed by tsserver's
    /// `projectLoadingStart` / `projectLoadingFinish` events.
    project_loads_in_flight: AtomicI64,
}

impl TsserverPendingRequests {
    /// Atomically reject registrations after stdout has closed. Sharing this
    /// lock with [`Self::drain_with_crash_error`] prevents an EOF/request race
    /// from stranding an unbounded production request on a dead process.
    fn insert(&self, seq: i64, tx: oneshot::Sender<serde_json::Value>) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.closed {
            return false;
        }
        if state.map.is_empty() {
            state.pending_since = Some(std::time::Instant::now());
        }
        state.map.insert(seq, tx);
        true
    }

    fn take(&self, seq: i64) -> Option<oneshot::Sender<serde_json::Value>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let sender = state.map.remove(&seq);
        if state.map.is_empty() {
            state.pending_since = None;
        }
        sender
    }

    fn pending_since(&self) -> Option<std::time::Instant> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending_since
    }

    /// How many requests are in flight. The leak surface: a request abandoned
    /// without releasing its slot shows up here and nowhere else.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .map
            .len()
    }

    /// Take one arbitrary in-flight sender. Tests that need to answer "whatever
    /// request the transport just issued" do not know its sequence number, so
    /// they cannot use [`Self::take`]; this keeps them off the inner map.
    #[cfg(test)]
    fn take_any(&self) -> Option<oneshot::Sender<serde_json::Value>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let seq = *state.map.keys().next()?;
        let sender = state.map.remove(&seq);
        if state.map.is_empty() {
            state.pending_since = None;
        }
        sender
    }

    /// Fail every in-flight request so callers return immediately instead of
    /// waiting indefinitely, and reject later registrations on this dead
    /// transport. A provider restart creates a new pending state.
    fn drain_with_crash_error(&self) {
        let drained: Vec<_> = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.closed = true;
            state.pending_since = None;
            state.map.drain().collect()
        };
        for (_seq, tx) in drained {
            let _ = tx.send(serde_json::json!({
                "success": false,
                "message": "tsserver process crashed"
            }));
        }
    }
}

/// Admission guard for a user-facing tsserver request.
///
/// Entering the guard preempts the one active diagnostics request through
/// tsserver's out-of-band cancellation pipe. Dropping the guard admits deferred
/// diagnostics after all interactive work has drained.
struct TsserverInteractiveRequest {
    pending: Arc<TsserverPendingRequests>,
}

impl Drop for TsserverInteractiveRequest {
    fn drop(&mut self) {
        if self
            .pending
            .interactive_in_flight
            .fetch_sub(1, Ordering::AcqRel)
            == 1
        {
            self.pending.interactive_idle.notify_waiters();
        }
    }
}

/// Clears the active-background marker on every exit path.
struct TsserverBackgroundRequest {
    pending: Arc<TsserverPendingRequests>,
    seq: i64,
}

impl Drop for TsserverBackgroundRequest {
    fn drop(&mut self) {
        let _ = self.pending.background_seq.compare_exchange(
            self.seq,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// tsserver's per-request cancellation channel.
///
/// tsserver has no in-band cancel message. What it has is
/// `--cancellationPipeName <prefix>*`: while it executes request N it polls for
/// the existence of the file `<prefix>N` and throws `OperationCanceledException`
/// out of the language-service call the moment it appears. A queued request is
/// covered too — the name is bound when the request is dequeued, so the very
/// first poll of an already-cancelled request sees the file.
///
/// The cancel being a file create is exactly the property this transport needs:
/// it bypasses the stdin queue entirely, and a request is usually being
/// cancelled precisely BECAUSE that queue is not draining. It is the structural
/// equivalent of the tsgo transport's unbounded control lane.
///
/// Sequence numbers are unique and monotonic within a session, so a cancellation
/// can only ever apply to the request that minted it: a file written after
/// tsserver has already answered that seq names work it will never run again.
struct TsserverCancellation {
    /// Session-private directory holding the cancellation files.
    dir: std::path::PathBuf,
    /// The exact string tsserver concatenates the request id onto. Built once so
    /// the path this side writes is byte-identical to the one tsserver stats.
    prefix: String,
    /// Files written but not yet acknowledged by the exact request sequence.
    /// tsserver does not unlink them, so the writer owns cleanup.
    written: StdMutex<HashMap<i64, std::path::PathBuf>>,
}

impl TsserverCancellation {
    /// Create the session's cancellation directory.
    ///
    /// `None` when it cannot be created or cannot be named to tsserver. Provider
    /// startup treats that as a failed transport invariant: running tsserver
    /// without out-of-band cancellation would make its interactive lane
    /// untrustworthy.
    fn create() -> Option<Self> {
        static NONCE: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "verter-tsserver-cancel-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).ok()?;
        let prefix = dir.join("c").to_str()?.to_string();
        // tsserver rejects the whole template when the prefix itself contains a
        // `*`, which would leave the session silently un-cancellable.
        if prefix.contains('*') {
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        Some(Self {
            dir,
            prefix,
            written: StdMutex::new(HashMap::new()),
        })
    }

    /// The `--cancellationPipeName` argument for this session.
    fn pipe_name_arg(&self) -> String {
        format!("{}*", self.prefix)
    }

    /// Tell tsserver to stop working on `seq`. Retention lasts until the exact
    /// response/requestCompleted acknowledgement or session teardown.
    fn cancel(&self, seq: i64) {
        let path = std::path::PathBuf::from(format!("{}{seq}", self.prefix));
        if std::fs::File::create(&path).is_err() {
            return;
        }
        let mut written = self.written.lock().unwrap();
        written.insert(seq, path);
    }

    /// Remove cancellation evidence only after the engine acknowledges the
    /// exact sequence. Requests are unbounded, so age/count are not proof that
    /// a frame has left tsserver's stdin queue.
    fn acknowledge(&self, seq: i64) {
        if let Some(path) = self.written.lock().unwrap().remove(&seq) {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for TsserverCancellation {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// One in-flight request's registration, released on drop.
///
/// The caller's future can be dropped at any await point — the request deadline
/// elapsing upstream is the normal case, not an exotic one. Dropping it must
/// leave nothing behind:
///
/// * the pending-map entry goes, or a provider that never answers leaks an entry
///   per abandoned request for the life of the session;
/// * the cancellation goes out, or tsserver keeps computing an answer no one
///   will read — and on one JavaScript thread that abandoned work sits directly
///   in front of every request that replaced it.
struct TsserverPendingRequest {
    seq: i64,
    pending: Arc<TsserverPendingRequests>,
    cancellation: Option<Arc<TsserverCancellation>>,
    /// Cleared once the response is in hand — a completed request must not
    /// cancel a seq the engine has already answered.
    armed: bool,
}

impl TsserverPendingRequest {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TsserverPendingRequest {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.pending.take(self.seq);
        if let Some(cancellation) = &self.cancellation {
            cancellation.cancel(self.seq);
        }
    }
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
    pending: Arc<TsserverPendingRequests>,
    next_seq: AtomicI64,
    /// Counts consecutive request timeouts. Reset to 0 on any successful response.
    /// When this reaches `HANG_THRESHOLD`, fires `crash_notify` to trigger a restart
    /// via the `ResilientProvider` crash-recovery machinery — a wedged-but-alive
    /// tsserver (accepts requests, never responds) must be detected and restarted,
    /// not silently time out every request for the rest of the session.
    consecutive_failures: AtomicU32,
    /// When the last hang strike was charged, so hops that were already in flight
    /// then are not counted as independent evidence. See
    /// [`TsserverTransport::note_hang_failure`]. Cleared with the counter.
    last_strike_at: StdMutex<Option<std::time::Instant>>,
    /// When the read loop last got ANY output from the child — a response OR an
    /// event. Shared with the read loop. A child that is emitting is working,
    /// however slowly; a WEDGED child emits nothing at all.
    last_message_at: Arc<StdMutex<std::time::Instant>>,
    /// Shared with `ResilientProvider` — signaled when the provider appears hung.
    crash_notify: Option<Arc<Notify>>,
    /// Singleflight + cooldown stamp for `reloadProjects` membership recovery.
    /// Under a hover/diagnostics storm, dozens of concurrent cold-miss retries would
    /// each fire `reloadProjects` (a full all-projects rebuild), saturating tsserver.
    /// The stamp coalesces those to at most one reload per cooldown window.
    membership_recovery: Mutex<Option<std::time::Instant>>,
    /// Per-request cancellation channel for this session. `None` when the
    /// session could not create its cancellation directory, in which case an
    /// abandoned request still releases its slot but the engine keeps working.
    cancellation: Option<Arc<TsserverCancellation>>,
}

/// Number of consecutive request timeouts before the transport signals a hang.
/// Mirrors the tsgo transport's `HANG_THRESHOLD`: when reached, `crash_notify` is
/// fired so the `ResilientProvider` restarts the wedged process (kill, backoff,
/// re-spawn, replay desired state) instead of timing out forever.
const HANG_THRESHOLD: u32 = 3;

/// A lost `projectLoadingFinish` must not disable wedge recovery forever. A
/// healthy large-project build may be silent, but a child silent beyond this
/// backstop is considered dead even while a load marker remains active.
const LOADING_WEDGE_SILENCE_CAP: std::time::Duration = std::time::Duration::from_secs(120);

const SILENCE_WATCHDOG_POLL: std::time::Duration = std::time::Duration::from_secs(5);

/// Provider-health watchdog, deliberately separate from request completion.
/// Requests have no latency timeout. Only a process with pending work and no
/// response/event output at all for the absolute silence cap is restarted; a
/// slow engine that emits project-loading progress remains healthy.
async fn watch_tsserver_silence(
    pending: std::sync::Weak<TsserverPendingRequests>,
    last_message_at: Arc<StdMutex<std::time::Instant>>,
    crash_notify: Arc<Notify>,
    poll: std::time::Duration,
    silence_cap: std::time::Duration,
) {
    loop {
        tokio::time::sleep(poll).await;
        let Some(pending) = pending.upgrade() else {
            return;
        };
        let Some(pending_since) = pending.pending_since() else {
            continue;
        };
        let last_message = *last_message_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let silent_for = std::cmp::max(last_message, pending_since).elapsed();
        if silent_for < silence_cap {
            continue;
        }
        tracing::error!(
            "tsserver emitted no output for {silent_for:?} while requests were pending; restarting"
        );
        crash_notify.notify_waiters();
        return;
    }
}

/// Quiet window before background engine work is admitted. This mirrors editor
/// idle scheduling: bursts of hover/completion/navigation finish first instead
/// of repeatedly starting and cancelling the same diagnostics or graph refresh.
const BACKGROUND_IDLE_GRACE: std::time::Duration = std::time::Duration::from_millis(150);

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
    fn preempt_background_request(&self) {
        self.pending
            .background_preemption_epoch
            .fetch_add(1, Ordering::AcqRel);

        let background_seq = self.pending.background_seq.swap(0, Ordering::AcqRel);
        if background_seq != 0 {
            if let Some(cancellation) = &self.cancellation {
                cancellation.cancel(background_seq);
            }
            // Wake the background future immediately; the out-of-band file is
            // what stops tsserver itself. A later engine response for this seq
            // is intentionally ignored because no caller still owns it.
            if let Some(tx) = self.pending.take(background_seq) {
                let _ = tx.send(serde_json::json!({
                    "success": true,
                    "body": { "canceled": true }
                }));
            }
        }
    }

    fn begin_interactive_request(&self) -> TsserverInteractiveRequest {
        self.pending
            .interactive_in_flight
            .fetch_add(1, Ordering::AcqRel);
        self.preempt_background_request();

        TsserverInteractiveRequest {
            pending: Arc::clone(&self.pending),
        }
    }

    async fn wait_for_interactive_idle(&self) {
        loop {
            let notified = self.pending.interactive_idle.notified();
            if self.pending.interactive_in_flight.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }

    /// Charge one round-trip failure toward hang detection, firing the restart
    /// notification once [`HANG_THRESHOLD`] consecutive failures accumulate.
    ///
    /// Only a hop that ran to its FULL configured bound reaches here. A hop the
    /// caller's own deadline cut short is not evidence of anything: a cold
    /// project legitimately takes longer than a 1.5s hover budget, and charging
    /// those restarts a healthy engine mid-program-build. The restart throws away
    /// the program, which makes the next requests cold too, which charges three
    /// more — a loop in which the engine never gets far enough to answer, and
    /// requests come back fast and EMPTY instead of slow and correct.
    ///
    /// CONSECUTIVE MEANS SEQUENTIAL IN TIME. A hop that was already in flight when
    /// the previous strike was charged observed the SAME window of silence, so it
    /// is not independent evidence. The LSP fans out — hover, definition,
    /// completion, references and a background diagnostics pull are routinely in
    /// flight at the same instant on the same bound — so charging each of them
    /// separately reaches the threshold after a SINGLE bound's worth of silence
    /// and restarts an engine that is merely busy. `issued_at` is when this hop's
    /// bound started; only a hop issued at or after the last strike advances the
    /// count, so reaching [`HANG_THRESHOLD`] takes that many successive windows in
    /// which the engine answered nothing at all.
    /// A hop is evidence of a WEDGE only if the child produced nothing at all
    /// while it ran.
    ///
    /// A response resets the counter outright, but a busy tsserver also emits
    /// EVENTS while it works — `projectLoadingStart`/`Finish`, diagnostics,
    /// telemetry, `requestCompleted`. Those prove the child's loop is turning.
    /// Without this gate, a request whose answer the caller does not even use —
    /// `notify_carrier_changed` awaits `projectInfo` purely as an ORDERING FENCE
    /// and discards the body — becomes proof that the engine is hung, and three
    /// carrier changes against a cold project destroy it.
    fn child_was_silent_during(&self, issued_at: std::time::Instant) -> bool {
        let last = *self
            .last_message_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        last <= issued_at
    }

    fn note_hang_failure(&self, command: &str, issued_at: std::time::Instant) {
        if !self.child_was_silent_during(issued_at) {
            return;
        }
        if self.pending.project_loads_in_flight.load(Ordering::Relaxed) > 0 {
            let last = *self
                .last_message_at
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if last.elapsed() < LOADING_WEDGE_SILENCE_CAP {
                return;
            }
        }
        let count = {
            let mut last = self
                .last_strike_at
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if last.is_some_and(|at| issued_at < at) {
                return;
            }
            *last = Some(std::time::Instant::now());
            self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1
        };
        if count >= HANG_THRESHOLD {
            tracing::error!(
                "tsserver appears hung ({count} successive unanswered full-bound hops, \
                 latest '{command}') — triggering restart"
            );
            if let Some(notify) = &self.crash_notify {
                notify.notify_waiters();
            }
        }
    }

    /// Clear hang-detection state after proof the engine is alive and answering.
    fn clear_hang_evidence(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        *self
            .last_strike_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    fn finish_response(
        &self,
        command: &str,
        seq: i64,
        value: serde_json::Value,
    ) -> Result<serde_json::Value, TypeProviderError> {
        self.clear_hang_evidence();
        if let Some(false) = value.get("success").and_then(|flag| flag.as_bool()) {
            let message = value
                .get("message")
                .and_then(|message| message.as_str())
                .unwrap_or("unknown error");
            crate::type_runtime_trace_event!(
                "tsserver_transport_request_error",
                format!("command={} seq={} message={}", command, seq, message),
            );
            return Err(TypeProviderError::new(message));
        }
        if value
            .get("body")
            .and_then(|body| body.get("canceled"))
            .and_then(|flag| flag.as_bool())
            == Some(true)
        {
            crate::type_runtime_trace_event!(
                "tsserver_transport_request_error",
                format!("command={} seq={} message=canceled", command, seq),
            );
            return Err(TypeProviderError::new(format!(
                "request '{command}' was canceled at the engine"
            )));
        }
        crate::type_runtime_trace_event!(
            "tsserver_transport_request_result",
            format!(
                "command={} seq={} body_kind={}",
                command,
                seq,
                value
                    .get("body")
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
        Ok(value
            .get("body")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    /// Send a tsserver request and wait for the response.
    async fn request(
        &self,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, TypeProviderError> {
        let _interactive = self.begin_interactive_request();
        self.request_inner(command, arguments, None, None).await
    }

    /// Run a cancellable, single-flight background request only while the
    /// interactive lane is idle. If user traffic arrives, tsserver is cancelled
    /// out of band and the background request retries after the lane drains.
    async fn request_background(
        &self,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, TypeProviderError> {
        let requests = [(command, arguments)];
        let mut responses = self.request_background_batch(&requests).await?;
        Ok(responses
            .pop()
            .expect("a one-frame background batch returns one response"))
    }

    /// Admit an ordered background transaction under one editor-idle quiet
    /// window. Every frame remains independently cancellable; if interactive
    /// traffic preempts any frame, the whole idempotent transaction restarts
    /// after the interactive lane drains.
    async fn request_background_batch(
        &self,
        requests: &[(&str, serde_json::Value)],
    ) -> Result<Vec<serde_json::Value>, TypeProviderError> {
        self.request_background_batch_results(requests)
            .await?
            .into_iter()
            .collect()
    }

    /// Variant used by diagnostics, where the semantic pass is authoritative
    /// but syntactic and suggestion failures degrade independently. Admission
    /// and preemption are transaction-wide; ordinary command errors remain
    /// frame-local so the later diagnostic categories are still collected.
    async fn request_background_batch_results(
        &self,
        requests: &[(&str, serde_json::Value)],
    ) -> Result<Vec<Result<serde_json::Value, TypeProviderError>>, TypeProviderError> {
        self.request_background_batch_results_with_preemption(requests, true)
            .await
    }

    async fn request_background_batch_results_once(
        &self,
        requests: &[(&str, serde_json::Value)],
    ) -> Result<Vec<Result<serde_json::Value, TypeProviderError>>, TypeProviderError> {
        self.request_background_batch_results_with_preemption(requests, false)
            .await
    }

    async fn request_background_batch_results_with_preemption(
        &self,
        requests: &[(&str, serde_json::Value)],
        retry_after_preemption: bool,
    ) -> Result<Vec<Result<serde_json::Value, TypeProviderError>>, TypeProviderError> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        if self.cancellation.is_none() {
            return Err(TypeProviderError::new(
                "tsserver background work requires an out-of-band cancellation channel",
            ));
        }

        let _gate = self.pending.background_gate.lock().await;
        loop {
            self.wait_for_interactive_idle().await;
            let epoch = self
                .pending
                .background_preemption_epoch
                .load(Ordering::Acquire);
            tokio::time::sleep(BACKGROUND_IDLE_GRACE).await;
            if self.pending.interactive_in_flight.load(Ordering::Acquire) != 0
                || self
                    .pending
                    .background_preemption_epoch
                    .load(Ordering::Acquire)
                    != epoch
            {
                continue;
            }
            let mut responses = Vec::with_capacity(requests.len());
            let mut preempted = false;
            for (command, arguments) in requests {
                match self
                    .request_inner(command, arguments.clone(), None, Some(epoch))
                    .await
                {
                    Ok(response) => responses.push(Ok(response)),
                    Err(error)
                        if (error.message.contains("preempted")
                            || error.message.contains("canceled"))
                            && self
                                .pending
                                .background_preemption_epoch
                                .load(Ordering::Acquire)
                                != epoch =>
                    {
                        preempted = true;
                        break;
                    }
                    Err(error) => responses.push(Err(error)),
                }
            }
            if preempted {
                if retry_after_preemption {
                    continue;
                }
                return Err(TypeProviderError::new(
                    "tsserver background transaction preempted",
                ));
            }
            return Ok(responses);
        }
    }

    async fn request_interactive_batch(
        &self,
        requests: &[(&str, serde_json::Value)],
    ) -> Result<Vec<serde_json::Value>, TypeProviderError> {
        let _interactive = self.begin_interactive_request();
        let mut responses = Vec::with_capacity(requests.len());
        for (command, arguments) in requests {
            responses.push(
                self.request_inner(command, arguments.clone(), None, None)
                    .await?,
            );
        }
        Ok(responses)
    }

    /// Send a tsserver request with a custom configured response timeout. Split
    /// from [`TsserverTransport::request`] so tests can exercise the timeout /
    /// hang detection path without waiting the full production timeout.
    ///
    /// `configured` is an upper bound, not the bound: the hop actually issued is
    /// the tighter of `configured` and what the ambient request deadline has
    /// left (see [`crate::deadline::hop_budget`]). tsserver is one JavaScript
    /// thread, so a hop that outlives the request that asked for it is pure
    /// queue contention — and, worse, a hop bound that never wins the race
    /// against the caller's 1.5-6s deadline means the failure branch below never
    /// executes: the pending entry is never released and the engine is never told
    /// to stop. Hang detection is the exception — see [`Self::note_hang_failure`].
    #[cfg(test)]
    async fn request_with_timeout(
        &self,
        command: &str,
        arguments: serde_json::Value,
        configured: std::time::Duration,
    ) -> Result<serde_json::Value, TypeProviderError> {
        let _interactive = self.begin_interactive_request();
        self.request_inner(command, arguments, Some(configured), None)
            .await
    }

    async fn request_inner(
        &self,
        command: &str,
        arguments: serde_json::Value,
        timeout: Option<std::time::Duration>,
        background_epoch: Option<u64>,
    ) -> Result<serde_json::Value, TypeProviderError> {
        let Some(configured) = timeout else {
            return self
                .request_unbounded_inner(command, arguments, background_epoch)
                .await;
        };
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
                if !self.pending.insert(seq, tx) {
                    return Err(TypeProviderError::new("tsserver process is not available"));
                }
                // Armed from the instant the seq is registered: every exit from
                // here on — return, error, or the caller's future being dropped
                // mid-await — releases the registration and tells the engine to
                // stop through the same path.
                let mut registration = TsserverPendingRequest {
                    seq,
                    pending: Arc::clone(&self.pending),
                    cancellation: self.cancellation.clone(),
                    armed: true,
                };

                let _background_request = if let Some(epoch) = background_epoch {
                    self.pending.background_seq.store(seq, Ordering::Release);
                    let slot = TsserverBackgroundRequest {
                        pending: Arc::clone(&self.pending),
                        seq,
                    };
                    // Close both admission races: interactive traffic may have
                    // arrived after the idle check, or may have entered and
                    // exited before this seq became visible.
                    if self.pending.interactive_in_flight.load(Ordering::Acquire) != 0
                        || self
                            .pending
                            .background_preemption_epoch
                            .load(Ordering::Acquire)
                            != epoch
                    {
                        registration.disarm();
                        self.pending.take(seq);
                        return Err(TypeProviderError::new(
                            "tsserver background request preempted by interactive traffic",
                        ));
                    }
                    Some(slot)
                } else {
                    None
                };

                // The enqueue and the response wait SHARE one deadline, so the
                // whole round-trip is bounded. An unbounded `send().await` on a
                // full lane parks the request BEFORE the response bound even
                // starts, and without charging anything toward hang detection.
                let hop = crate::deadline::hop_budget(configured);
                // Whether the engine actually got the bound it was promised. A
                // shortened hop expiring is the CALLER running out of patience,
                // not the engine failing to answer.
                let full_bound = hop >= configured;
                let issued_at = std::time::Instant::now();
                let deadline = tokio::time::Instant::now() + hop;

                // tsserver uses newline-delimited JSON (no Content-Length framing)
                let frame = format!("{body}\n");
                let send_budget = deadline.saturating_duration_since(tokio::time::Instant::now());
                match self
                    .stdin_tx
                    .send_timeout(TsserverStdinMessage::Frame(frame.into_bytes()), send_budget)
                    .await
                {
                    Ok(()) => {}
                    Err(mpsc::error::SendTimeoutError::Closed(_)) => {
                        // The frame never reached the engine, so there is nothing
                        // to cancel — release the slot without naming a seq the
                        // engine has never seen.
                        registration.disarm();
                        self.pending.take(seq);
                        return Err(TypeProviderError::new("stdin writer closed"));
                    }
                    Err(mpsc::error::SendTimeoutError::Timeout(_)) => {
                        registration.disarm();
                        self.pending.take(seq);
                        if full_bound {
                            self.note_hang_failure(command, issued_at);
                        }
                        crate::type_runtime_trace_event!(
                            "tsserver_transport_request_error",
                            format!(
                                "command={} seq={} message=stdin-enqueue-timeout",
                                command, seq
                            ),
                        );
                        return Err(TypeProviderError::new(format!(
                            "request '{command}' stdin enqueue timed out after {hop:?}"
                        )));
                    }
                }

                let rx_budget = deadline.saturating_duration_since(tokio::time::Instant::now());
                let result = tokio::time::timeout(rx_budget, rx).await;
                match result {
                    Ok(Ok(val)) => {
                        // Answered: the read loop already took the entry, and an
                        // engine that has replied must not be told to cancel.
                        registration.disarm();
                        // Any response (even a tsserver-level error) proves the process
                        // is alive and answering — reset the hang detector.
                        self.clear_hang_evidence();
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
                        // A cancelled request answers `success: true` with a
                        // `{ canceled: true }` body — a success-shaped envelope
                        // carrying no result. Every feature parser reads the body
                        // as an array and falls back to empty, so passing it on
                        // would turn "the engine stopped early" into "there are
                        // no results here": a silently wrong answer in place of a
                        // visible failure.
                        if val
                            .get("body")
                            .and_then(|body| body.get("canceled"))
                            .and_then(|flag| flag.as_bool())
                            == Some(true)
                        {
                            crate::type_runtime_trace_event!(
                                "tsserver_transport_request_error",
                                format!("command={} seq={} message=canceled", command, seq),
                            );
                            return Err(TypeProviderError::new(format!(
                                "request '{command}' was canceled at the engine"
                            )));
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
                        // The sender was dropped (drained on crash), so the seq is
                        // already gone and the engine is not running the work.
                        registration.disarm();
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
                        // Timed out with the request live at the engine. Leave the
                        // registration ARMED: dropping it is what releases the
                        // pending entry and cancels the engine's work, on this
                        // path and on the caller-dropped path alike.
                        if full_bound {
                            self.note_hang_failure(command, issued_at);
                        }
                        crate::type_runtime_trace_event!(
                            "tsserver_transport_request_error",
                            format!("command={} seq={} message=timeout", command, seq),
                        );
                        Err(TypeProviderError::new(format!(
                            "request '{command}' timed out after {hop:?}"
                        )))
                    }
                }
            }
        )
        .await
    }

    /// Production request path. There is deliberately no latency timeout: a
    /// cold configured project is valid work, not an empty result. Dropping the
    /// caller future still removes the pending entry and sends tsserver's
    /// out-of-band cancellation file through [`TsserverPendingRequest::drop`].
    async fn request_unbounded_inner(
        &self,
        command: &str,
        arguments: serde_json::Value,
        background_epoch: Option<u64>,
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
                let message = serde_json::json!({
                    "seq": seq,
                    "type": "request",
                    "command": command,
                    "arguments": arguments,
                });
                let body = serde_json::to_string(&message)
                    .map_err(|error| TypeProviderError::new(format!("serialize error: {error}")))?;

                let (tx, rx) = oneshot::channel();
                if !self.pending.insert(seq, tx) {
                    return Err(TypeProviderError::new("tsserver process is not available"));
                }
                let mut registration = TsserverPendingRequest {
                    seq,
                    pending: Arc::clone(&self.pending),
                    cancellation: self.cancellation.clone(),
                    armed: true,
                };

                let _background_request = if let Some(epoch) = background_epoch {
                    self.pending.background_seq.store(seq, Ordering::Release);
                    let slot = TsserverBackgroundRequest {
                        pending: Arc::clone(&self.pending),
                        seq,
                    };
                    if self.pending.interactive_in_flight.load(Ordering::Acquire) != 0
                        || self
                            .pending
                            .background_preemption_epoch
                            .load(Ordering::Acquire)
                            != epoch
                    {
                        registration.disarm();
                        self.pending.take(seq);
                        return Err(TypeProviderError::new(
                            "tsserver background request preempted by interactive traffic",
                        ));
                    }
                    Some(slot)
                } else {
                    None
                };

                let frame = format!("{body}\n");
                if self
                    .stdin_tx
                    .send(TsserverStdinMessage::Frame(frame.into_bytes()))
                    .await
                    .is_err()
                {
                    registration.disarm();
                    self.pending.take(seq);
                    return Err(TypeProviderError::new("stdin writer closed"));
                }

                let value = match rx.await {
                    Ok(value) => value,
                    Err(_) => {
                        registration.disarm();
                        crate::type_runtime_trace_event!(
                            "tsserver_transport_request_error",
                            format!(
                                "command={} seq={} message=response channel closed",
                                command, seq
                            ),
                        );
                        return Err(TypeProviderError::new("response channel closed"));
                    }
                };
                registration.disarm();
                self.finish_response(command, seq, value)
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

/// Read loop for tsserver stdout.
///
/// tsserver can send responses in two formats:
/// 1. Content-Length framed (modern tsserver default)
/// 2. Newline-delimited JSON
///
/// We handle the Content-Length format since modern tsserver uses it for responses.
async fn read_loop(
    stdout: tokio::process::ChildStdout,
    pending: Arc<TsserverPendingRequests>,
    cancellation: Arc<TsserverCancellation>,
    crash_notify: Option<Arc<Notify>>,
    last_message_at: Arc<StdMutex<std::time::Instant>>,
) {
    let mut reader = BufReader::new(stdout);
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(0) => {
                // EOF — process exited
                pending.drain_with_crash_error();
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
                // ANY output from the child proves its loop is turning. Hang
                // detection reads this to tell a busy engine from a wedged one:
                // a busy tsserver keeps emitting events while it builds; a
                // wedged one goes silent.
                *last_message_at
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = std::time::Instant::now();

                // Check if this is a Content-Length header (modern tsserver)
                if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
                    if let Ok(len) = len_str.trim().parse::<usize>() {
                        // Read the blank line
                        line_buf.clear();
                        if reader.read_line(&mut line_buf).await.is_err() {
                            pending.drain_with_crash_error();
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
                            pending.drain_with_crash_error();
                            if let Some(notify) = &crash_notify {
                                notify.notify_waiters();
                            }
                            return;
                        }
                        if let Ok(msg) = serde_json::from_slice::<serde_json::Value>(&body) {
                            handle_message(&msg, &pending, Some(&cancellation));
                        }
                    }
                    continue;
                }

                // Try to parse as JSON directly (newline-delimited mode)
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    handle_message(&msg, &pending, Some(&cancellation));
                }
            }
            Err(_) => {
                pending.drain_with_crash_error();
                if let Some(notify) = &crash_notify {
                    notify.notify_waiters();
                }
                return;
            }
        }
    }
}

/// Handle a parsed tsserver message (response or event).
fn handle_message(
    msg: &serde_json::Value,
    pending: &TsserverPendingRequests,
    cancellation: Option<&TsserverCancellation>,
) {
    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "response" => {
            if let Some(request_seq) = msg.get("request_seq").and_then(|v| v.as_i64()) {
                if let Some(cancellation) = cancellation {
                    cancellation.acknowledge(request_seq);
                }
                if let Some(tx) = pending.take(request_seq) {
                    let _ = tx.send(msg.clone());
                }
            }
        }
        "event" => {
            let event_name = msg.get("event").and_then(|v| v.as_str()).unwrap_or("");
            if event_name == "requestCompleted" {
                let request_seq = msg
                    .get("body")
                    .and_then(|body| body.get("request_seq").or_else(|| body.get("requestSeq")))
                    .and_then(|value| value.as_i64());
                if let (Some(cancellation), Some(request_seq)) = (cancellation, request_seq) {
                    cancellation.acknowledge(request_seq);
                }
            }
            if event_name == "projectLoadingStart" {
                pending
                    .project_loads_in_flight
                    .fetch_add(1, Ordering::Relaxed);
            } else if event_name == "projectLoadingFinish" {
                let _ = pending.project_loads_in_flight.fetch_update(
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                    |value| Some((value - 1).max(0)),
                );
            }
            // `semanticDiag` / `syntaxDiag` / `suggestionDiag` events carry a
            // file but no ScriptInfo version in supported TypeScript protocol
            // versions. They are therefore health/progress signals only here,
            // never a diagnostics authority. Diagnostics are pulled
            // synchronously below and fenced by the exact authored document
            // version before LSP publication.
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

/// tsserver's genuine no-hover answer: the engine ANSWERED `quickinfo` with
/// `success: false` because the position carries no quickinfo (whitespace,
/// punctuation, an unloaded inferred file). This is an empty RESULT, not a
/// failure — the only quickinfo error that may surface as `Ok(None)`.
const TSSERVER_NO_CONTENT: &str = "No content available";

/// Whether a `quickinfo` transport error is the engine's genuine no-content
/// answer rather than a provider failure (crash, closed transport, timeout).
/// NARROW by construction: every other error must propagate as `Err` so the
/// caller's recovery engages instead of the client seeing a silent empty
/// hover from a dead provider.
fn tsserver_error_is_no_content(error: &TypeProviderError) -> bool {
    error.message.contains(TSSERVER_NO_CONTENT)
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

/// Convert a byte offset to tsserver's zero-based absolute UTF-16 offset.
///
/// `encodedSemanticClassifications-full` and `provideInlayHints` take numeric
/// absolute offsets rather than the line/offset objects used by most tsserver
/// commands. Invalid byte offsets fail closed instead of being rounded onto a
/// neighboring character.
pub fn byte_offset_to_tsserver_absolute_offset(content: &str, offset: u32) -> Option<u32> {
    let end = usize::try_from(offset).ok()?;
    let prefix = content.get(..end)?;
    u32::try_from(prefix.encode_utf16().count()).ok()
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

/// Parse one tsserver-family `provideInlayHints` entry into the provider
/// contract's byte-offset representation.
///
/// Both the managed-tsserver and extension-hosted decoders consume this shared
/// owner. A missing content snapshot or malformed/out-of-range UTF-16 position
/// drops the hint; packed line/column sentinels are forbidden because carrier
/// sourcemap merging interprets [`InlayHint::position`] as a byte offset.
pub fn parse_tsserver_inlay_hint(
    hint: &serde_json::Value,
    content: Option<&str>,
) -> Option<InlayHint> {
    let text = hint.get("text")?.as_str()?.to_string();
    let pos = hint.get("position")?;
    let line = u32::try_from(pos.get("line")?.as_u64()?).ok()?;
    let offset = u32::try_from(pos.get("offset")?.as_u64()?).ok()?;
    let position = tsserver_pos_to_byte_offset_checked(content?, line, offset)?;

    let kind = match hint.get("kind").and_then(|value| value.as_str()) {
        Some("Type") => Some(InlayHintKind::Type),
        Some("Parameter") => Some(InlayHintKind::Parameter),
        _ => None,
    };

    Some(InlayHint {
        position,
        label: text,
        kind,
        padding_left: hint
            .get("whitespaceBefore")
            .and_then(|value| value.as_bool()),
        padding_right: hint
            .get("whitespaceAfter")
            .and_then(|value| value.as_bool()),
    })
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
    CarrierSource,
}

/// Last successful synchronous diagnostic pull for one exact local content
/// generation. A transport failure may reuse it only while that same generation
/// is still current; edits and close/reopen cycles draw a fresh global stamp.
#[derive(Clone)]
struct CachedDiagnostics {
    content_generation: u64,
    diagnostics: Vec<TypeDiagnostic>,
}

fn cached_diagnostics_for_generation(
    cached: Option<&CachedDiagnostics>,
    current_generation: Option<u64>,
) -> Vec<TypeDiagnostic> {
    cached
        .filter(|cached| current_generation == Some(cached.content_generation))
        .map(|cached| cached.diagnostics.clone())
        .unwrap_or_default()
}

/// A `TypeProvider` backed by a tsserver process (`node tsserver.js`).
pub struct TsserverTypeProvider {
    transport: Arc<TsserverTransport>,
    /// tsserver child process. Killed on drop.
    child: Child,
    /// Process-tree handle armed immediately after spawn, before Node can be
    /// treated as a live provider. It also registers the tree with the LSP
    /// client-lifetime monitor for abrupt editor death.
    tree: verter_tsgo_api::process::TreeKill,
    /// Cached file contents for position conversion.
    contents: Arc<Mutex<HashMap<String, Arc<str>>>>,
    /// Files that have been sent to tsserver via `open` command, tagged by
    /// [`OpenKind`] so a resync replays a source WITH content but a carrier
    /// companion CONTENTLESSLY. Used by `update_file` to decide between `open` vs
    /// `updateOpen`. `load_file` adds to `contents` but NOT to `opened_files`.
    opened_files: Arc<Mutex<HashMap<String, OpenKind>>>,
    /// Last successful synchronous diagnostics pull, fenced by local content
    /// generation. Unversioned tsserver diagnostic events never enter it.
    diagnostics_cache: Arc<Mutex<HashMap<String, CachedDiagnostics>>>,
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
    /// IDE companion path -> authored framework source identity. Public APIs
    /// continue to use companion paths for source-map offsets; tsserver queries
    /// the source identity whose host snapshot contains those generated bytes.
    carrier_sources: Arc<parking_lot::RwLock<HashMap<String, String>>>,
    /// Reverse source -> IDE companion edge for remapping provider response paths
    /// back into the generated/source-map domain expected by the LSP.
    carrier_companions: Arc<parking_lot::RwLock<HashMap<String, String>>>,
    /// Internal LSP providers normalize managed authored Program identities back
    /// to companion coordinates for the Rust source-map layer. On the editor's
    /// direct plugin surface the plugin is already the sole mapper, so source
    /// identities must pass through unchanged.
    normalize_response_paths_to_companions: bool,
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
    /// ScriptInfos whose manifest identity changed and to refresh external roots.
    carrier_store_refresh_generation: AtomicU64,
    /// Explicit active authored-source working set for the internal LSP tsserver.
    ///
    /// The publish store may contain every workspace carrier; only companions in
    /// this set are eligible to become eager configured-project roots. The plugin
    /// creates closed, host-backed ScriptInfos for them, so carrier activation
    /// never uses a protocol `open` and never competes for content ownership.
    active_carrier_sources: Arc<parking_lot::RwLock<BTreeSet<String>>>,
    /// One contentless authored-source bootstrap per configured project. The
    /// internal tsserver has no editor-owned `.ts` open lifecycle, so one source
    /// must cause the configured project to instantiate and invoke the plugin's
    /// `getExternalFiles`; every other framework source remains plugin-owned.
    project_bootstraps: Arc<parking_lot::RwLock<HashMap<String, String>>>,
    /// Coalesces store/plugin invalidation into one idle refresh. Publication
    /// callers await only its lightweight configure fence; Program construction
    /// remains lazy and interactive requests can preempt the background lane.
    carrier_refresh: Arc<TsserverCarrierRefresh>,
}

#[derive(Default)]
struct TsserverCarrierRefresh {
    requested_generation: AtomicU64,
    urgent_generation: AtomicU64,
    applied_generation: AtomicU64,
    running: AtomicBool,
    completion: Notify,
    failure: StdMutex<Option<(u64, TypeProviderError)>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CarrierRefreshPriority {
    Background,
    Interactive,
}

impl Drop for TsserverTypeProvider {
    fn drop(&mut self) {
        self.tree.kill_tree();
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
                    "includePackageJsonAutoImports": "on",
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
        verter_span::path::simplify_verbatim_path_str(plugin_path).into_owned(),
        "--allowLocalPluginLoads".to_string(),
    ]
}

/// Build the EXACT command a tsserver session is spawned from: program, every
/// argument, every environment mutation. [`TsserverTypeProvider::spawn`] adds
/// only the stdio wiring and the process-tree configuration on top, so this is
/// the single argument-construction site for the tsserver lane — cold spawn,
/// resilient respawn, and the test harness all reach tsserver through it.
///
/// Every path-valued input crosses the exec boundary here and is therefore run
/// through [`verter_span::path::simplify_verbatim_path_str`]. Whether an
/// UNSIMPLIFIABLE value is fatal depends on WHO consumes it and through WHICH
/// API — an `fs` call accepts the `\\?\` prefix (that is what the prefix is FOR),
/// a path algebra that predates it does not:
///
/// - `tsserver_path` (argv[1]) — node `resolveMainPath` → `Module._findPath` →
///   `toRealPath`. Node's OWN path handling, `\\?\`-unaware: it degenerates to
///   `lstat('D:')` and throws `EISDIR` before tsserver initialises. **FATAL —
///   refuse.**
/// - `plugin_path` (`--pluginProbeLocations`) — TypeScript's
///   `importServicePluginSync` does `combinePaths(dir, "node_modules")` +
///   `resolvePath` + `normalizeSlashes`, then `host.require` →
///   `resolveJSModule` → `nodeModuleNameResolverWorker` (Node10, ancestor
///   walk). **Simplified, never refused** — see the trace below.
/// - `node_path` (the program) — consumed by `CreateProcessW` as
///   `lpApplicationName`, a Win32 file API that accepts the extended-length
///   form. `discovery::find_node` also never canonicalizes (it joins `PATH`
///   entries), so it cannot produce one. **Simplified defensively, never
///   refused** — refusing here would kill a session the OS would have started.
/// - `carrier_store_dir` (`VERTER_CARRIER_STORE_DIR`) — the plugin uses it only
///   as `path.join(storeDir, …)` + `fs.readFileSync` (`carrierStore.ts:202`,
///   `:570`, `:591`). No `require`, no module resolution; node's `fs` accepts
///   `\\?\`, which is precisely its documented long-path mechanism.
///   **Simplified, never refused.**
///
/// ### The probe path, traced end to end
///
/// Two review passes reached opposite conclusions here, so the trace is recorded
/// rather than the verdict. Against the pinned TypeScript 6.0.3 in
/// `node_modules`:
///
/// 1. `importServicePluginSync` (`typescript.js:187996`) computes
///    `normalizeSlashes(host.resolvePath(combinePaths(initialDir, "node_modules")))`.
///    `normalizeSlashes` (`:8852`) rewrites `\` to `/`, so a verbatim
///    `\\?\D:\ext` becomes `//?/D:/ext`.
/// 2. TypeScript's path algebra UNDERSTANDS that spelling as absolute:
///    `getEncodedRootLength` (`:8749`) sees `//`, scans for the next `/` from
///    index 2, and returns 4 — the root is `//?/`. `getDirectoryPath` (`:8791`)
///    walks ancestors correctly and stops at that root, so
///    `forEachAncestorDirectory` (`:9154`) terminates rather than escaping. The
///    claim that TypeScript mangles the prefix is WRONG: it preserves it.
/// 3. But `host.resolvePath` is node's `_path.resolve` (`:8332`), which
///    backslashes the path, and step 1's `normalizeSlashes` forward-slashes it
///    again. So every `fileExists`/`directoryExists` probe and the final
///    `require` run on the FORWARD-slash `//?/…` spelling.
/// 4. Whether Win32 honours THAT spelling is not decidable from this repository.
///    Node's `path.win32.toNamespacedPath` returns its input unchanged once it
///    sees `?` at index 2, so the forward-slash form is what reaches libuv — and
///    the `\\?\` prefix is documented to disable separator normalization, which
///    is precisely what would stop those `/` characters being separators.
///
/// TypeScript preserves the prefix (step 2); the OS-level outcome (step 4) is
/// genuinely unknown here. **The decision does not depend on resolving that**,
/// because the consequences are asymmetric: if the plugin CAN load, refusing
/// kills a session that would have worked; if it CANNOT, the plugin is absent
/// but tsserver still serves every plain `.ts` file, and refusing turns a
/// degraded session into no session at all. Both branches are worse than not
/// refusing. That is the structural difference from the main script, where node
/// provably exits and there is no session left to degrade.
///
/// The probe path is therefore simplified best-effort and NEVER refused. The
/// residual — an unrepresentable probe path may silently load no plugin — is a
/// VISIBILITY problem whose remedy is a plugin-load status surface, not a
/// refusal.
///
/// The one fatal value comes from `Path::canonicalize()`, which is load-bearing
/// (it resolves the pnpm package symlink so tsserver's script-relative
/// default-lib lookup lands beside the real `.pnpm/typescript@…` install) and
/// which on Windows returns `\\?\D:\…`. The canonical path is kept for
/// filesystem work and identity; only the value handed to `exec` is simplified.
/// `discovery::validate_tsserver_candidate` refuses an unrepresentable install
/// as a candidate rejection, which is where the user-visible message comes from;
/// the check repeated here is the fail-closed backstop for every other caller.
///
/// `cancellation_pipe_name` is deliberately NOT simplified: it is a `*` glob
/// TEMPLATE, not a path, and it is built from `std::env::temp_dir()`, which is
/// never an extended-length path. Running a path simplifier over a glob would be
/// a category error (the `*` makes its last component non-representable, so the
/// transform would refuse anyway) and would silently disarm cancellation if it
/// ever did rewrite it.
fn build_tsserver_command(
    node_path: &str,
    tsserver_path: &str,
    cancellation_pipe_name: &str,
    plugin_path: Option<&str>,
    carrier_store_dir: Option<&str>,
    plugin_response_remap: bool,
) -> Result<tokio::process::Command, TypeProviderError> {
    use verter_span::path::simplify_verbatim_path_str;

    if let Some(refusal) = verter_span::path::verbatim_refusal(tsserver_path) {
        return Err(TypeProviderError::new(format!(
            "refusing to launch node against the extended-length path {tsserver_path}: {refusal}. \
             Node cannot parse the `\\\\?\\` prefix and would exit before tsserver starts. \
             Install TypeScript at a path Windows can name normally, or point the \
             `typescript.tsdk` setting at one."
        )));
    }

    let mut cmd = tokio::process::Command::new(simplify_verbatim_path_str(node_path).as_ref());

    // Remove VS Code/Electron debug env vars to prevent tsserver from
    // opening a debugger port during F5 sessions.
    for var in CHILD_PROCESS_ENV_DENYLIST {
        cmd.env_remove(var);
    }

    cmd.arg(simplify_verbatim_path_str(tsserver_path).as_ref())
        .arg("--useSyntaxServer=false")
        .arg("--disableAutomaticTypingAcquisition");

    // Per-request cancellation is a transport invariant. Without it an
    // abandoned background diagnostic keeps the single JavaScript thread
    // busy ahead of every user request that replaced it.
    cmd.arg("--cancellationPipeName")
        .arg(cancellation_pipe_name);

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
        cmd.env(
            "VERTER_CARRIER_STORE_DIR",
            simplify_verbatim_path_str(store_dir).as_ref(),
        );
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

    Ok(cmd)
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
        // Per-request cancellation is a transport invariant. Without it an
        // abandoned background diagnostic keeps the single JavaScript thread
        // busy ahead of every user request that replaced it. Failing provider
        // startup is safer than advertising an interactive lane we cannot honor.
        let cancellation = Arc::new(TsserverCancellation::create().ok_or_else(|| {
            TypeProviderError::new(
                "tsserver cancellation directory unavailable; refusing an unpreemptible session",
            )
        })?);

        let mut cmd = build_tsserver_command(
            node_path,
            tsserver_path,
            &cancellation.pipe_name_arg(),
            plugin_path,
            carrier_store_dir,
            plugin_response_remap,
        )?;

        let child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        verter_tsgo_api::process::configure_tree_spawn(child);
        let mut child = child
            .spawn()
            .map_err(|e| TypeProviderError::new(format!("failed to spawn tsserver: {e}")))?;
        let tree = verter_tsgo_api::process::TreeKill::arm(child.id().unwrap_or(0));

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TypeProviderError::new("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TypeProviderError::new("no stdout"))?;
        let stderr = child.stderr.take();

        let pending = Arc::new(TsserverPendingRequests::default());

        // Use a channel + dedicated writer task instead of Arc<Mutex<ChildStdin>>
        // to eliminate contention between concurrent request() and command_no_response() calls.
        let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
        tokio::spawn(tsserver_stdin_writer_loop(stdin, stdin_rx));

        let last_message_at = Arc::new(StdMutex::new(std::time::Instant::now()));

        let transport = Arc::new(TsserverTransport {
            stdin_tx: stdin_tx.clone(),
            pending: Arc::clone(&pending),
            next_seq: AtomicI64::new(1),
            consecutive_failures: AtomicU32::new(0),
            last_strike_at: StdMutex::new(None),
            last_message_at: Arc::clone(&last_message_at),
            crash_notify: crash_notify.clone(),
            membership_recovery: Mutex::new(None),
            cancellation: Some(Arc::clone(&cancellation)),
        });
        if let Some(notify) = crash_notify.as_ref() {
            tokio::spawn(watch_tsserver_silence(
                Arc::downgrade(&pending),
                Arc::clone(&last_message_at),
                Arc::clone(notify),
                SILENCE_WATCHDOG_POLL,
                LOADING_WEDGE_SILENCE_CAP,
            ));
        }

        let diagnostics_cache: Arc<Mutex<HashMap<String, CachedDiagnostics>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let contents_cache: Arc<Mutex<HashMap<String, Arc<str>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Start the read loop
        tokio::spawn(read_loop(
            stdout,
            pending,
            cancellation,
            crash_notify,
            last_message_at,
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
            tree,
            contents: contents_cache,
            opened_files: Arc::new(Mutex::new(HashMap::new())),
            diagnostics_cache,
            workspace_root: ws_root,
            project_roots: Arc::new(parking_lot::RwLock::new(Vec::new())),
            carrier_projects: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            carrier_sources: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            carrier_companions: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            normalize_response_paths_to_companions: !plugin_response_remap,
            content_generations: Arc::new(ContentGenerations::default()),
            carrier_store_refresh_generation: AtomicU64::new(0),
            active_carrier_sources: Arc::new(parking_lot::RwLock::new(BTreeSet::new())),
            project_bootstraps: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            carrier_refresh: Arc::new(TsserverCarrierRefresh::default()),
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

    /// Translate the LSP's generated companion identity to the authored source
    /// identity used by the managed tsserver Program.
    fn query_file_for(&self, file: &str) -> String {
        self.carrier_sources
            .read()
            .get(file)
            .cloned()
            .unwrap_or_else(|| file.to_string())
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

/// Map a tsserver Program source identity back to the generated companion
/// identity consumed by the LSP sourcemap layer. TypeScript only ever sees the
/// authored `.vue`/`.svelte` path; the companion remains a private Verter API
/// boundary for byte-offset mapping.
fn remap_carrier_response_path(
    path: &str,
    carrier_companions: &parking_lot::RwLock<HashMap<String, String>>,
    normalize_to_companion: bool,
) -> String {
    let path = verter_span::path::canonicalize_path(path);
    if !normalize_to_companion {
        return path;
    }
    carrier_companions
        .read()
        .get(&path)
        .cloned()
        .unwrap_or(path)
}

async fn notify_carriers_changed_inner(
    transport: Arc<TsserverTransport>,
    files: Vec<String>,
    refresh_generation: u64,
    active_carrier_sources: Vec<String>,
    priority: CarrierRefreshPriority,
) -> Result<(), TypeProviderError> {
    let Some(fence_file) = files.first().cloned() else {
        return Ok(());
    };

    crate::type_runtime_trace_event!(
        "tsserver_carrier_working_set",
        format!(
            "fence_file={} active_count={} active={}",
            fence_file,
            active_carrier_sources.len(),
            active_carrier_sources.join("|")
        ),
    );

    // The official Volar/Svelte model lets the configured project consume
    // external roots lazily; forcing `projectInfo` here eagerly rebuilds the
    // same program once per carrier and monopolizes tsserver's single JavaScript
    // thread. Configure the plugin, then send one no-op tsserver configure only
    // AFTER its response. The second response is a constant-cost host-turn
    // fence: the plugin's `setImmediate` graph reconciliation runs between the
    // two input turns, so a following diagnostic/hover cannot observe the new
    // store manifest with the old configured-project roots.
    let requests = [
        (
            "configurePlugin",
            serde_json::json!({
                "pluginName": "@verter/typescript-plugin",
                "configuration": {
                    "carrierStoreRefreshToken": refresh_generation,
                    "activeCarrierSources": active_carrier_sources,
                }
            }),
        ),
        ("configure", serde_json::json!({})),
    ];
    match priority {
        CarrierRefreshPriority::Background => {
            transport
                .request_background_batch_results_once(&requests)
                .await?
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
        }
        CarrierRefreshPriority::Interactive => {
            transport.request_interactive_batch(&requests).await?;
        }
    }
    Ok(())
}

fn schedule_carrier_refresh(
    transport: Arc<TsserverTransport>,
    active_sources: Arc<parking_lot::RwLock<BTreeSet<String>>>,
    refresh: Arc<TsserverCarrierRefresh>,
    generation: u64,
    changed_file: String,
    priority: CarrierRefreshPriority,
) {
    refresh
        .requested_generation
        .fetch_max(generation, Ordering::AcqRel);
    if priority == CarrierRefreshPriority::Interactive {
        refresh
            .urgent_generation
            .fetch_max(generation, Ordering::AcqRel);
        transport.preempt_background_request();
    }
    if refresh
        .running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    tokio::spawn(async move {
        loop {
            let target = refresh.requested_generation.load(Ordering::Acquire);
            let priority = if refresh.urgent_generation.load(Ordering::Acquire)
                > refresh.applied_generation.load(Ordering::Acquire)
            {
                CarrierRefreshPriority::Interactive
            } else {
                CarrierRefreshPriority::Background
            };
            let active = active_sources.read().iter().cloned().collect();
            if let Err(error) = notify_carriers_changed_inner(
                Arc::clone(&transport),
                vec![changed_file.clone()],
                target,
                active,
                priority,
            )
            .await
            {
                if error.message.contains("background transaction preempted") {
                    // Every background refresh is idempotent and represents a
                    // still-unapplied requested generation. Ordinary hover or
                    // completion also preempts this lane, without advancing
                    // `urgent_generation`; retry after the interactive-idle
                    // admission instead of permanently dropping membership.
                    continue;
                }
                tracing::warn!(
                    "failed to refresh tsserver carrier source membership: {}",
                    error.message
                );
                *refresh
                    .failure
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((target, error));
                refresh.completion.notify_waiters();
                refresh.running.store(false, Ordering::Release);
                if refresh.requested_generation.load(Ordering::Acquire) > target
                    && refresh
                        .running
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                {
                    continue;
                }
                return;
            }
            refresh.applied_generation.store(target, Ordering::Release);
            if priority == CarrierRefreshPriority::Interactive {
                let _ = refresh.urgent_generation.fetch_update(
                    Ordering::AcqRel,
                    Ordering::Acquire,
                    |generation| (generation <= target).then_some(0),
                );
            }
            {
                let mut failure = refresh
                    .failure
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if failure
                    .as_ref()
                    .is_some_and(|(generation, _)| *generation <= target)
                {
                    *failure = None;
                }
            }
            refresh.completion.notify_waiters();

            if refresh.requested_generation.load(Ordering::Acquire)
                == refresh.applied_generation.load(Ordering::Acquire)
            {
                refresh.running.store(false, Ordering::Release);
                if refresh.requested_generation.load(Ordering::Acquire)
                    == refresh.applied_generation.load(Ordering::Acquire)
                    || refresh
                        .running
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_err()
                {
                    return;
                }
            }
        }
    });
}

async fn wait_for_carrier_refresh(
    refresh: &TsserverCarrierRefresh,
    generation: u64,
) -> Result<(), TypeProviderError> {
    loop {
        let notified = refresh.completion.notified();
        if refresh.applied_generation.load(Ordering::Acquire) >= generation {
            return Ok(());
        }
        if let Some((failed_generation, error)) = refresh
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            if *failed_generation >= generation {
                return Err(error.clone());
            }
        }
        notified.await;
    }
}

/// Activate one already-published IDE carrier without recompiling it. Workspace-
/// scan carriers remain closed store-backed roots. An editor-active source is
/// opened contentlessly; a demand-discovered project frontier opens only its one
/// configured-project bootstrap and lets the plugin own all remaining roots.
/// Generated snapshots stay lazy in the carrier store on both paths.
struct PublishedCarrierActivation {
    transport: Arc<TsserverTransport>,
    opened_files: Arc<Mutex<HashMap<String, OpenKind>>>,
    carrier_projects: Arc<parking_lot::RwLock<HashMap<String, String>>>,
    active_sources: Arc<parking_lot::RwLock<BTreeSet<String>>>,
    project_bootstraps: Arc<parking_lot::RwLock<HashMap<String, String>>>,
}

async fn activate_published_carrier_inner(
    activation: PublishedCarrierActivation,
    source: String,
    companion: String,
    project_file_name: String,
    script_kind_name: &'static str,
    keep_source_open: bool,
) -> Result<bool, TypeProviderError> {
    activation
        .carrier_projects
        .write()
        .insert(source.clone(), project_file_name.clone());
    let newly_active = activation.active_sources.write().insert(source.clone());

    // Unlike VS Code's editor-owned tsserver, the internal LSP process receives
    // no ordinary TypeScript didOpen event that would instantiate the configured
    // project and its global plugin. Reserve one bootstrap per project. Once the
    // plugin exists, its `getExternalFiles` hook admits READY authored framework
    // sources as closed, host-backed ScriptInfos, matching Volar/Svelte. The
    // editor-active source remains open so the configured project has a durable
    // owner. The plugin patches the managed source ScriptInfo's protocol
    // coordinates to its versioned generated snapshot without transferring that
    // snapshot over the protocol.
    let reserved_bootstrap = {
        let mut bootstraps = activation.project_bootstraps.write();
        if bootstraps.contains_key(&project_file_name) {
            false
        } else {
            bootstraps.insert(project_file_name.clone(), source.clone());
            true
        }
    };
    let project_root = project_file_name
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_else(|| project_file_name.clone());
    // A demand-discovered workspace-symbol frontier needs the source only as a
    // plugin external root. Once one source has bootstrapped the configured
    // project, every other member is admitted by the batched plugin config and
    // must not emit a protocol `open`. An actual editor activation keeps its
    // authored source contentlessly open.
    if !reserved_bootstrap && !keep_source_open {
        return Ok(newly_active);
    }
    if reserved_bootstrap {
        // The global plugin is instantiated as part of configured-project
        // creation. A transient generated-path root creates that project without
        // transferring bytes. It is closed after the authored source has become
        // the durable editor-active root.
        if let Err(error) = open_carrier_source_contentless(
            &activation.transport,
            &activation.opened_files,
            &activation.carrier_projects,
            &companion,
            script_kind_name,
            &project_root,
        )
        .await
        {
            let mut bootstraps = activation.project_bootstraps.write();
            if bootstraps.get(&project_file_name) == Some(&source) {
                bootstraps.remove(&project_file_name);
            }
            drop(bootstraps);
            activation.active_sources.write().remove(&source);
            activation.carrier_projects.write().remove(&source);
            return Err(error);
        }
    }

    if let Err(error) = open_carrier_source_contentless(
        &activation.transport,
        &activation.opened_files,
        &activation.carrier_projects,
        &source,
        script_kind_name,
        &project_root,
    )
    .await
    {
        if reserved_bootstrap {
            activation.opened_files.lock().await.remove(&companion);
            let _ = activation
                .transport
                .command_no_response("close", serde_json::json!({ "file": companion }))
                .await;
            let mut bootstraps = activation.project_bootstraps.write();
            if bootstraps.get(&project_file_name) == Some(&source) {
                bootstraps.remove(&project_file_name);
            }
        }
        activation.active_sources.write().remove(&source);
        return Err(error);
    }

    if !reserved_bootstrap {
        return Ok(newly_active);
    }
    activation.opened_files.lock().await.remove(&companion);
    if let Err(error) = activation
        .transport
        .command_no_response("close", serde_json::json!({ "file": companion }))
        .await
    {
        {
            let mut bootstraps = activation.project_bootstraps.write();
            if bootstraps.get(&project_file_name) == Some(&source) {
                bootstraps.remove(&project_file_name);
            }
        }
        activation.active_sources.write().remove(&source);
        activation.carrier_projects.write().remove(&source);
        activation.opened_files.lock().await.remove(&source);
        return Err(error);
    }
    Ok(newly_active)
}

struct CarrierMetadataCaches<'a> {
    contents: &'a Mutex<HashMap<String, Arc<str>>>,
    generations: &'a ContentGenerations,
    projects: &'a parking_lot::RwLock<HashMap<String, String>>,
    sources: &'a parking_lot::RwLock<HashMap<String, String>>,
    companions: &'a parking_lot::RwLock<HashMap<String, String>>,
}

async fn cache_carrier_metadata(
    caches: CarrierMetadataCaches<'_>,
    file: &str,
    content: Arc<str>,
    source: &str,
    project_file_name: &str,
) -> bool {
    let is_ide_companion = file.ends_with(".tsx") || file.ends_with(".jsx");
    let mut content_changed = store_content_if_changed_bump_generation(
        caches.contents,
        caches.generations,
        file,
        Arc::clone(&content),
    )
    .await;
    let project_changed = caches
        .projects
        .write()
        .insert(file.to_string(), project_file_name.to_string())
        .as_deref()
        != Some(project_file_name);
    if is_ide_companion {
        let source_content_changed = store_content_if_changed_bump_generation(
            caches.contents,
            caches.generations,
            source,
            Arc::clone(&content),
        )
        .await;
        content_changed |= source_content_changed;
        caches
            .projects
            .write()
            .insert(source.to_string(), project_file_name.to_string());
        caches
            .companions
            .write()
            .insert(source.to_string(), file.to_string());
        let source_changed = caches
            .sources
            .write()
            .insert(file.to_string(), source.to_string())
            .as_deref()
            != Some(source);
        content_changed |= source_changed;
    }
    content_changed || project_changed
}

fn carrier_metadata_requires_refresh(
    metadata_changed: bool,
    active_sources: &BTreeSet<String>,
    source: &str,
    companion: &str,
) -> bool {
    metadata_changed
        && (companion.ends_with(".tsx") || companion.ends_with(".jsx"))
        && active_sources.contains(source)
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
                    // Read the old document's exact UTF-16 end BEFORE inserting
                    // new content. A line-only sentinel is wrong for a document
                    // without a trailing newline and can leave an old suffix in
                    // ScriptInfo, splitting its line table from the Program.
                    let old_end = {
                        let cache = contents_cache.lock().await;
                        cache
                            .get(&file)
                            .map(|c| byte_offset_to_tsserver_pos(c, c.len() as u32))
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
                        if let Some((end_line, end_offset)) = old_end {
                            tracing::debug!(
                                "tsserver update_file: updateOpen for {file} (end={end_line}:{end_offset})"
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
                                                "end": { "line": end_line, "offset": end_offset },
                                                "newText": content,
                                            }]
                                        }]
                                    }),
                                )
                                .await?;
                            crate::type_runtime_trace_event!(
                                "tsserver_update_file_result",
                                format!(
                                    "file={} mode=update_open old_end={}:{}",
                                    file, end_line, end_offset
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
        let carrier_companions = Arc::clone(&self.carrier_companions);
        let active_sources = Arc::clone(&self.active_carrier_sources);
        let project_bootstraps = Arc::clone(&self.project_bootstraps);
        let carrier_refresh = Arc::clone(&self.carrier_refresh);
        let refresh_generation = self
            .carrier_store_refresh_generation
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        Box::pin(async move {
            crate::type_runtime_trace_scope_async!(
                "tsserver_close_file",
                format!("file={}", file),
                async {
                    let carrier_source = carrier_sources.read().get(&file).cloned();
                    if let Some(source) = carrier_source.as_ref() {
                        if let Some(project) = carrier_projects.read().get(source).cloned() {
                            let mut bootstraps = project_bootstraps.write();
                            if bootstraps.get(&project) == Some(source) {
                                bootstraps.remove(&project);
                            }
                        }
                        carrier_sources.write().remove(&file);
                        carrier_companions.write().remove(source);
                        active_sources.write().remove(source);
                        carrier_projects.write().remove(source);
                        if opened_files.lock().await.remove(source) == Some(OpenKind::CarrierSource)
                        {
                            transport
                                .command_no_response("close", serde_json::json!({ "file": source }))
                                .await?;
                        }
                        forget_content(&contents_cache, &content_generations, source).await;
                        schedule_carrier_refresh(
                            Arc::clone(&transport),
                            Arc::clone(&active_sources),
                            Arc::clone(&carrier_refresh),
                            refresh_generation,
                            source.clone(),
                            CarrierRefreshPriority::Interactive,
                        );
                    }
                    forget_content(&contents_cache, &content_generations, &file).await;
                    opened_files.lock().await.remove(&file);
                    // Retract the carrier→project routing for a closed companion so
                    // it no longer injects `projectFileName` (a closed companion is
                    // no longer a member; a stale route would target a project the
                    // companion left). A no-op for a real `.ts`/`.tsx` file (never in
                    // the carrier map).
                    carrier_projects.write().remove(&file);
                    if carrier_source.is_none() {
                        transport
                            .command_no_response("close", serde_json::json!({ "file": file }))
                            .await?;
                    }
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
        let refresh_generation = self
            .carrier_store_refresh_generation
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        schedule_carrier_refresh(
            Arc::clone(&self.transport),
            Arc::clone(&self.active_carrier_sources),
            Arc::clone(&self.carrier_refresh),
            refresh_generation,
            file,
            CarrierRefreshPriority::Background,
        );
        let refresh = Arc::clone(&self.carrier_refresh);
        Box::pin(async move { wait_for_carrier_refresh(&refresh, refresh_generation).await })
    }

    fn notify_carriers_changed<'a>(
        &'a self,
        companion_paths: &'a [String],
    ) -> ProviderFuture<'a, ()> {
        let mut files: Vec<String> = companion_paths
            .iter()
            .map(|path| Self::normalize_path(path))
            .collect();
        files.sort_unstable();
        files.dedup();
        let refresh_generation = self
            .carrier_store_refresh_generation
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        if let Some(file) = files.into_iter().next() {
            schedule_carrier_refresh(
                Arc::clone(&self.transport),
                Arc::clone(&self.active_carrier_sources),
                Arc::clone(&self.carrier_refresh),
                refresh_generation,
                file,
                CarrierRefreshPriority::Background,
            );
            let refresh = Arc::clone(&self.carrier_refresh);
            return Box::pin(async move {
                wait_for_carrier_refresh(&refresh, refresh_generation).await
            });
        }
        Box::pin(async { Ok(()) })
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
        let carrier_companions = Arc::clone(&self.carrier_companions);
        let transport = Arc::clone(&self.transport);
        let opened_files = Arc::clone(&self.opened_files);
        let active_sources = Arc::clone(&self.active_carrier_sources);
        let project_bootstraps = Arc::clone(&self.project_bootstraps);
        let carrier_refresh = Arc::clone(&self.carrier_refresh);
        let refresh_generation = self
            .carrier_store_refresh_generation
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        let script_kind_name = if file.ends_with(".jsx") { "JSX" } else { "TSX" };
        Box::pin(async move {
            // Hydrate the LOCAL position-conversion content for the companion —
            // the generated bytes also back the managed process's single active
            // source ScriptInfo. Closed workspace carriers remain lazy store-
            // backed roots; this active snapshot keeps ScriptInfo's protocol
            // line table identical to the Program's plugin projection.
            let _metadata_changed = cache_carrier_metadata(
                CarrierMetadataCaches {
                    contents: &contents_cache,
                    generations: &content_generations,
                    projects: &carrier_projects,
                    sources: &carrier_sources,
                    companions: &carrier_companions,
                },
                &file,
                Arc::clone(&content),
                &source,
                &project_file_name,
            )
            .await;
            // Record the owning configured project so carrier queries route there
            // via `projectFileName` (the companion is an EXTERNAL `getExternalFiles`
            // member, so its default-project resolution is otherwise undecided —
            // `ensureDefaultProjectForFile` would throw `No Project` for a virtual
            // companion on no real-disk path).
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
            activate_published_carrier_inner(
                PublishedCarrierActivation {
                    transport: Arc::clone(&transport),
                    opened_files: Arc::clone(&opened_files),
                    carrier_projects,
                    active_sources: Arc::clone(&active_sources),
                    project_bootstraps,
                },
                source.clone(),
                file.clone(),
                project_file_name,
                script_kind_name,
                true,
            )
            .await?;

            schedule_carrier_refresh(
                Arc::clone(&transport),
                active_sources,
                Arc::clone(&carrier_refresh),
                refresh_generation,
                file,
                CarrierRefreshPriority::Interactive,
            );
            wait_for_carrier_refresh(&carrier_refresh, refresh_generation).await?;

            Ok(())
        })
    }

    fn activate_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        project_file_name: &str,
        script_kind: crate::traits::CarrierScriptKind,
    ) -> ProviderFuture<'_, ()> {
        let source = Self::normalize_path(source_path);
        let companion = Self::normalize_path(companion_path);
        let project_file_name = Self::normalize_path(project_file_name);
        let transport = Arc::clone(&self.transport);
        let opened_files = Arc::clone(&self.opened_files);
        let carrier_projects = Arc::clone(&self.carrier_projects);
        let active_sources = Arc::clone(&self.active_carrier_sources);
        let project_bootstraps = Arc::clone(&self.project_bootstraps);
        let refresh = Arc::clone(&self.carrier_refresh);
        let refresh_generation = &self.carrier_store_refresh_generation;
        Box::pin(async move {
            let newly_active = activate_published_carrier_inner(
                PublishedCarrierActivation {
                    transport: Arc::clone(&transport),
                    opened_files,
                    carrier_projects,
                    active_sources: Arc::clone(&active_sources),
                    project_bootstraps,
                },
                source,
                companion.clone(),
                project_file_name,
                script_kind.tsserver_name(),
                true,
            )
            .await?;
            if newly_active {
                let generation = refresh_generation.fetch_add(1, Ordering::Relaxed) + 1;
                schedule_carrier_refresh(
                    transport,
                    active_sources,
                    Arc::clone(&refresh),
                    generation,
                    companion,
                    CarrierRefreshPriority::Interactive,
                );
                wait_for_carrier_refresh(&refresh, generation).await?;
            }
            Ok(())
        })
    }

    fn activate_carrier_members<'a>(
        &'a self,
        members: &'a [crate::traits::CarrierActivation],
    ) -> ProviderFuture<'a, ()> {
        let mut members = members.to_vec();
        members.sort_unstable_by(|left, right| {
            left.source_path
                .cmp(&right.source_path)
                .then_with(|| left.companion_path.cmp(&right.companion_path))
        });
        members.dedup_by(|left, right| {
            left.source_path == right.source_path && left.companion_path == right.companion_path
        });
        let transport = Arc::clone(&self.transport);
        let opened_files = Arc::clone(&self.opened_files);
        let carrier_projects = Arc::clone(&self.carrier_projects);
        let active_sources = Arc::clone(&self.active_carrier_sources);
        let project_bootstraps = Arc::clone(&self.project_bootstraps);
        let refresh = Arc::clone(&self.carrier_refresh);
        let refresh_generation = &self.carrier_store_refresh_generation;
        Box::pin(async move {
            let mut changed_file = None;
            let mut activation_error = None;
            for member in members {
                let source = Self::normalize_path(&member.source_path);
                let companion = Self::normalize_path(&member.companion_path);
                let project_file_name = Self::normalize_path(&member.project_file_name);
                match activate_published_carrier_inner(
                    PublishedCarrierActivation {
                        transport: Arc::clone(&transport),
                        opened_files: Arc::clone(&opened_files),
                        carrier_projects: Arc::clone(&carrier_projects),
                        active_sources: Arc::clone(&active_sources),
                        project_bootstraps: Arc::clone(&project_bootstraps),
                    },
                    source,
                    companion.clone(),
                    project_file_name,
                    member.script_kind.tsserver_name(),
                    false,
                )
                .await
                {
                    Ok(true) => {
                        changed_file.get_or_insert(companion);
                    }
                    Ok(false) => {}
                    Err(error) => {
                        activation_error = Some(error);
                        break;
                    }
                }
            }

            // Admit the complete successfully-opened frontier with one plugin
            // configuration transaction. Even when a later activation failed,
            // publish the earlier durable working-set changes before returning
            // that error so provider and desired state cannot diverge.
            if let Some(changed_file) = changed_file {
                let generation = refresh_generation.fetch_add(1, Ordering::Relaxed) + 1;
                schedule_carrier_refresh(
                    transport,
                    active_sources,
                    Arc::clone(&refresh),
                    generation,
                    changed_file,
                    CarrierRefreshPriority::Interactive,
                );
                wait_for_carrier_refresh(&refresh, generation).await?;
            }
            if let Some(error) = activation_error {
                return Err(error);
            }
            Ok(())
        })
    }

    fn register_carrier_metadata(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        let source = Self::normalize_path(source_path);
        let file = Self::normalize_path(companion_path);
        let project_file_name = Self::normalize_path(project_file_name);
        let content: Arc<str> = Arc::from(content);
        let contents_cache = Arc::clone(&self.contents);
        let content_generations = Arc::clone(&self.content_generations);
        let carrier_projects = Arc::clone(&self.carrier_projects);
        let carrier_sources = Arc::clone(&self.carrier_sources);
        let carrier_companions = Arc::clone(&self.carrier_companions);
        let active_sources = Arc::clone(&self.active_carrier_sources);
        let transport = Arc::clone(&self.transport);
        let refresh = Arc::clone(&self.carrier_refresh);
        let refresh_generation = &self.carrier_store_refresh_generation;
        Box::pin(async move {
            let metadata_changed = cache_carrier_metadata(
                CarrierMetadataCaches {
                    contents: &contents_cache,
                    generations: &content_generations,
                    projects: &carrier_projects,
                    sources: &carrier_sources,
                    companions: &carrier_companions,
                },
                &file,
                content,
                &source,
                &project_file_name,
            )
            .await;
            if carrier_metadata_requires_refresh(
                metadata_changed,
                &active_sources.read(),
                &source,
                &file,
            ) {
                let generation = refresh_generation.fetch_add(1, Ordering::Relaxed) + 1;
                schedule_carrier_refresh(
                    transport,
                    active_sources,
                    Arc::clone(&refresh),
                    generation,
                    file,
                    CarrierRefreshPriority::Interactive,
                );
                wait_for_carrier_refresh(&refresh, generation).await?;
            }
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
        let query_file = self.query_file_for(&file);
        let trigger = trigger_character.map(|s| s.to_string());
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let project_file_name = self.project_file_name_for(&query_file);
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
                    "file": query_file,
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
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let project_file_name = self.project_file_name_for(&query_file);
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
                    // `getExternalFiles`) and re-issue `quickinfo`. Recovery is
                    // attempt-bounded, never time-bounded: a slow valid project
                    // is awaited, while a structurally absent source cannot spin.
                    let mut recovery_attempts = 0_u8;
                    let result = loop {
                        let r = transport
                            .request(
                                "quickinfo",
                                inject_project_file_name(
                                    serde_json::json!({
                                        "file": query_file,
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
                                    && recovery_attempts < 2 =>
                            {
                                recovery_attempts += 1;
                                recover_companion_membership(&transport).await;
                                tokio::task::yield_now().await;
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
                            // "No content available." is the engine's genuine
                            // no-hover ANSWER (a position with no quickinfo): an
                            // empty result, not a failure. Every other error —
                            // a crashed process, a closed transport, a timeout —
                            // is a provider FAILURE and must surface as one so
                            // the caller's resync-and-retry recovery engages;
                            // collapsing it to `Ok(None)` here made a dead
                            // provider indistinguishable from "no hover at this
                            // position" (hover silently stops serving).
                            if tsserver_error_is_no_content(&e) {
                                tracing::debug!(
                                    "tsserver quickinfo: no content for {file} at {line}:{col}"
                                );
                                crate::type_runtime_trace_event!(
                                    "tsserver_get_hover_result",
                                    format!("file={} no_content=true", file),
                                );
                                Ok(None)
                            } else {
                                tracing::warn!("tsserver quickinfo error for {file}: {e}");
                                crate::type_runtime_trace_event!(
                                    "tsserver_get_hover_result",
                                    format!("file={} error={}", file, e),
                                );
                                Err(e)
                            }
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
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let project_file_name = self.project_file_name_for(&query_file);
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
                            inject_project_file_name(
                                serde_json::json!({
                                    "file": query_file,
                                    "line": line,
                                    "offset": col,
                                    "entryNames": entry_names,
                                }),
                                &project_file_name,
                            ),
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
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let project_file_name = self.project_file_name_for(&query_file);
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
                    inject_project_file_name(
                        serde_json::json!({
                            "file": query_file,
                            "line": line,
                            "offset": col,
                            "entryNames": [entry],
                        }),
                        &project_file_name,
                    ),
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
                // Managed carriers are queried under their authored source
                // identity while the host serves generated companion bytes.
                // TypeScript therefore names the SOURCE in codeActions, even
                // though the offsets belong to the generated snapshot cached
                // under both identities. Match that wire target here; the LSP
                // envelope still routes the resulting byte edits through the
                // companion path and its authoritative source map.
                &query_file,
                &cache_snapshot,
            ))
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let file = Self::normalize_path(path);
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let diagnostics_cache = Arc::clone(&self.diagnostics_cache);
        let content_generations = Arc::clone(&self.content_generations);
        let carrier_companions = Arc::clone(&self.carrier_companions);
        let normalize_response_paths = self.normalize_response_paths_to_companions;
        // Route a carrier companion's diagnostic passes to its OWNING configured
        // project (so `semanticDiagnosticsSync` type-checks it where
        // `getExternalFiles` admitted it, not a fresh inferred project that returns
        // empty). `None` for a non-carrier file (its default project is correct).
        let project_file_name = self.project_file_name_for(&query_file);
        let diagnostic_file = query_file;
        Box::pin(async move {
            let (content, request_generation) = {
                let cache = contents_cache.lock().await;
                let generation = content_generations.map.lock().get(&file).copied();
                (cache.get(&file).cloned(), generation)
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
            // the semantic pass, bounded by recovery attempts rather than elapsed
            // time (never a busy-spin). The recovery fires ONLY on this cold miss,
            // so a warm pull never pays it. Only this error is retried: a genuine
            // module-not-found arrives in the SUCCESS body (so it never reaches the
            // error path) and timeouts / closed channels are distinct terminal
            // strings that fall straight through.
            let mut recovery_attempts = 0_u8;
            let (semantic_result, syntactic_result, suggestion_result) = loop {
                let requests = [
                    (
                        "semanticDiagnosticsSync",
                        inject_project_file_name(
                            serde_json::json!({ "file": diagnostic_file.clone() }),
                            &project_file_name,
                        ),
                    ),
                    (
                        "syntacticDiagnosticsSync",
                        inject_project_file_name(
                            serde_json::json!({ "file": diagnostic_file.clone() }),
                            &project_file_name,
                        ),
                    ),
                    (
                        "suggestionDiagnosticsSync",
                        inject_project_file_name(
                            serde_json::json!({ "file": diagnostic_file.clone() }),
                            &project_file_name,
                        ),
                    ),
                ];
                let (semantic, syntactic, suggestion) =
                    match transport.request_background_batch_results(&requests).await {
                        Ok(results) => {
                            let mut results = results.into_iter();
                            (
                                results.next().expect("diagnostic batch has semantic frame"),
                                results.next(),
                                results.next(),
                            )
                        }
                        Err(error) => (Err(error), None, None),
                    };
                match &semantic {
                    Err(error)
                        if tsserver_diag_error_is_companion_not_ready(&error.message)
                            && recovery_attempts < 2 =>
                    {
                        recovery_attempts += 1;
                        recover_companion_membership(&transport).await;
                        tokio::task::yield_now().await;
                    }
                    _ => break (semantic, syntactic, suggestion),
                }
            };

            match semantic_result {
                Ok(semantic_body) => {
                    let semantic = parse_tsserver_diagnostics_body(
                        &semantic_body,
                        content.as_deref(),
                        Some(file.as_str()),
                    );

                    let syntactic = syntactic_result
                        .and_then(Result::ok)
                        .map(|body| {
                            parse_tsserver_diagnostics_body(
                                &body,
                                content.as_deref(),
                                Some(file.as_str()),
                            )
                        })
                        .unwrap_or_default();

                    let suggestion = suggestion_result
                        .and_then(Result::ok)
                        .map(|body| {
                            parse_tsserver_diagnostics_body(
                                &body,
                                content.as_deref(),
                                Some(file.as_str()),
                            )
                        })
                        .unwrap_or_default();

                    let mut diags = merge_diagnostic_sets(semantic, syntactic, suggestion);
                    for diagnostic in &mut diags {
                        for related in &mut diagnostic.related_information {
                            related.path = remap_carrier_response_path(
                                &related.path,
                                &carrier_companions,
                                normalize_response_paths,
                            );
                        }
                    }
                    // Cache only if the file remained on the exact content
                    // generation that initiated the pull. The caller applies
                    // its own authored-document version fence before publish;
                    // this guard prevents a later transport failure from
                    // reviving a response that raced an edit or close/reopen.
                    let current_generation = content_generations.map.lock().get(&file).copied();
                    if let Some(content_generation) = request_generation {
                        if current_generation == Some(content_generation) {
                            diagnostics_cache.lock().await.insert(
                                file.clone(),
                                CachedDiagnostics {
                                    content_generation,
                                    diagnostics: diags.clone(),
                                },
                            );
                        }
                    }
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
                    // A transport failure may reuse only a last-good pull from
                    // this exact local content generation. tsserver diagnostic
                    // events carry no version and are intentionally never cached.
                    let current_generation = content_generations.map.lock().get(&file).copied();
                    let cache = diagnostics_cache.lock().await;
                    Ok(cached_diagnostics_for_generation(
                        cache.get(&file),
                        current_generation,
                    ))
                }
            }
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(path);
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let carrier_companions = Arc::clone(&self.carrier_companions);
        let normalize_response_paths = self.normalize_response_paths_to_companions;
        let project_file_name = self.project_file_name_for(&query_file);
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
                            "file": query_file,
                            "line": line,
                            "offset": col,
                        }),
                        &project_file_name,
                    ),
                )
                .await?;

            let mut locs: Vec<TypeLocation> = {
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
            for location in &mut locs {
                location.path = remap_carrier_response_path(
                    &location.path,
                    &carrier_companions,
                    normalize_response_paths,
                );
            }

            Ok(locs)
        })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(path);
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let carrier_companions = Arc::clone(&self.carrier_companions);
        let normalize_response_paths = self.normalize_response_paths_to_companions;
        let project_file_name = self.project_file_name_for(&query_file);
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
                            "file": query_file,
                            "line": line,
                            "offset": col,
                        }),
                        &project_file_name,
                    ),
                )
                .await?;

            let mut locs: Vec<TypeLocation> = {
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
            for location in &mut locs {
                location.path = remap_carrier_response_path(
                    &location.path,
                    &carrier_companions,
                    normalize_response_paths,
                );
            }

            Ok(locs)
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(path);
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let carrier_companions = Arc::clone(&self.carrier_companions);
        let normalize_response_paths = self.normalize_response_paths_to_companions;
        let project_file_name = self.project_file_name_for(&query_file);
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
                            "file": query_file,
                            "line": line,
                            "offset": col,
                        }),
                        &project_file_name,
                    ),
                )
                .await?;

            let mut locs: Vec<TypeLocation> = {
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
            for location in &mut locs {
                location.path = remap_carrier_response_path(
                    &location.path,
                    &carrier_companions,
                    normalize_response_paths,
                );
            }

            Ok(locs)
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let file = Self::normalize_path(path);
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let carrier_companions = Arc::clone(&self.carrier_companions);
        let normalize_response_paths = self.normalize_response_paths_to_companions;
        let project_file_name = self.project_file_name_for(&query_file);
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
                    inject_project_file_name(
                        serde_json::json!({
                            "file": query_file,
                            "line": line,
                            "offset": col,
                            "findInComments": false,
                            "findInStrings": false,
                        }),
                        &project_file_name,
                    ),
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
            let mut locs: Vec<RenameLocation> = {
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
            for location in &mut locs {
                location.path = remap_carrier_response_path(
                    &location.path,
                    &carrier_companions,
                    normalize_response_paths,
                );
            }

            Ok(locs)
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let file = Self::normalize_path(path);
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let project_file_name = self.project_file_name_for(&query_file);
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
                    inject_project_file_name(
                        serde_json::json!({
                            "file": query_file,
                            "line": line,
                            "offset": col,
                        }),
                        &project_file_name,
                    ),
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
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let carrier_companions = Arc::clone(&self.carrier_companions);
        let normalize_response_paths = self.normalize_response_paths_to_companions;
        let project_file_name = self.project_file_name_for(&query_file);
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
                    inject_project_file_name(
                        serde_json::json!({
                            "file": query_file,
                            "startLine": sl,
                            "startOffset": sc,
                            "endLine": el,
                            "endOffset": ec,
                            "errorCodes": error_codes,
                        }),
                        &project_file_name,
                    ),
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
                    .request(
                        "getCombinedCodeFix",
                        inject_project_file_name(
                            combined_code_fix_args(&query_file, fix_id),
                            &project_file_name,
                        ),
                    )
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
            for action in &mut actions {
                for edit in &mut action.edits {
                    edit.path = remap_carrier_response_path(
                        &edit.path,
                        &carrier_companions,
                        normalize_response_paths,
                    );
                }
            }
            Ok(actions)
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let file = Self::normalize_path(path);
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let project_file_name = self.project_file_name_for(&query_file);
        Box::pin(async move {
            let content = {
                let cache = contents_cache.lock().await;
                cache.get(&file).cloned()
            };
            let Some(content) = content else {
                // No cached content — nothing to get tokens for
                return Ok(vec![]);
            };
            // `EncodedSemanticClassificationsRequestArgs` takes NUMERIC
            // `start`/`length` — UTF-16 code-unit offsets — NOT the
            // line/offset objects most tsserver commands use. tsserver
            // answers a line/offset-shaped request with `success: true` and
            // ZERO spans (live-verified on TS 5.4/5.8/6.0), so the wrong
            // shape reads as an engine with no classifications rather than
            // an error.
            let utf16_length = content.encode_utf16().count() as u64;

            let result = transport
                .request_background(
                    "encodedSemanticClassifications-full",
                    inject_project_file_name(
                        serde_json::json!({
                            "file": query_file,
                            "start": 0,
                            "length": utf16_length,
                            "format": "2020",
                        }),
                        &project_file_name,
                    ),
                )
                .await;

            match result {
                Ok(body) => {
                    let spans = body
                        .get("spans")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();

                    // Spans come as [start, length, classification, ...]
                    // triplets whose classification is `"format": "2020"`
                    // packed. The shared owner decodes the packing, remaps
                    // both halves into Verter's published legend space
                    // (unmappable classifications drop their span), and
                    // converts the engine's UTF-16 span offsets to the byte
                    // offsets the SemanticToken contract requires.
                    Ok(crate::semantic_tokens::map_classified_spans_2020(
                        &spans,
                        Some(&content),
                    ))
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
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let project_file_name = self.project_file_name_for(&query_file);
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
                    inject_project_file_name(
                        serde_json::json!({
                            "file": query_file,
                            "line": line,
                            "offset": col,
                            "filesToSearch": [query_file],
                        }),
                        &project_file_name,
                    ),
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
        let query_file = self.query_file_for(&file);
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        let project_file_name = self.project_file_name_for(&query_file);
        Box::pin(async move {
            let (start, length, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => {
                        let Some(start) = byte_offset_to_tsserver_absolute_offset(c, start_offset)
                        else {
                            return Ok(vec![]);
                        };
                        let Some(end) = byte_offset_to_tsserver_absolute_offset(c, end_offset)
                        else {
                            return Ok(vec![]);
                        };
                        let Some(length) = end.checked_sub(start) else {
                            return Ok(vec![]);
                        };
                        (start, length, Some(Arc::clone(c)))
                    }
                    None => return Ok(vec![]),
                }
            };

            let result = transport
                .request_background(
                    "provideInlayHints",
                    inject_project_file_name(
                        serde_json::json!({
                            "file": query_file,
                            "start": start,
                            "length": length,
                        }),
                        &project_file_name,
                    ),
                )
                .await;

            match result {
                Ok(body) => {
                    let hints = body
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|hint| {
                                    parse_tsserver_inlay_hint(hint, content_snapshot.as_deref())
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

/// Store carrier-local projection bytes only when their immutable snapshot
/// identity changed. Returning `false` lets repeated membership activation avoid
/// a plugin refresh while preserving the generation invariant for real updates.
async fn store_content_if_changed_bump_generation(
    contents: &Mutex<HashMap<String, Arc<str>>>,
    generations: &ContentGenerations,
    file: &str,
    content: Arc<str>,
) -> bool {
    let mut guard = contents.lock().await;
    if guard.get(file).is_some_and(|current| current == &content) {
        return false;
    }
    let next = generations.next_generation();
    guard.insert(file.to_string(), content);
    generations.map.lock().insert(file.to_string(), next);
    true
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

/// Open one active carrier identity without transferring generated bytes,
/// factored out so send-failure rollback is unit-testable against a bare
/// transport. Both transient bootstrap companions and durable authored
/// identities resolve lazily through the store-backed plugin.
///
/// Atomically marks the carrier identity opened (`opened_files`) and, only if newly
/// marked, issues its `open`. On a transport-send FAILURE it ROLLS BACK the
/// optimistic `opened_files` mark AND the `carrier_projects` routing entry, so a
/// later registration RE-ATTEMPTS the open instead of observing a phantom "already
/// opened" (`opened_now == false`) and skipping it forever — which would leave the
/// identity never a configured-project member (a phantom-registered carrier). The
/// atomic check-and-mark (not a check-then-mark) keeps two concurrent registrations
/// from both issuing the open.
async fn open_carrier_source_contentless(
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
        .insert(file.to_string(), OpenKind::CarrierSource)
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
    /// The captured active snapshot. Ordinary sources reopen with their bytes;
    /// managed carriers remain contentless and resolve through the plugin.
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
/// gate closes). A `CarrierSource` reopens contentlessly and routes to its own
/// configured project. Closed workspace carriers are not tracked here at all;
/// they remain lazy store roots.
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
            OpenKind::CarrierSource => {
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
