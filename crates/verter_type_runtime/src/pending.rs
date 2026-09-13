//! Request bookkeeping shared by the external-TypeScript transports.
//!
//! Both transports speak a request/response protocol over a child process's
//! stdio with a monotonic per-session request id, so the mechanics of "who is
//! waiting for which id", "how long has this engine owed us an answer" and "how
//! many unanswered bounds in a row mean the child is wedged" are identical.
//! Only the wire framing, the cancellation channel and the error payload differ,
//! and those stay with their protocol.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use tokio::sync::{oneshot, Notify};

#[derive(Default)]
struct PendingRequestState {
    map: HashMap<i64, oneshot::Sender<serde_json::Value>>,
    closed: bool,
    /// Start of the current non-empty interval. A provider that was idle for a
    /// long time must receive the full silence allowance when new work arrives.
    pending_since: Option<Instant>,
}

/// In-flight requests awaiting a response, keyed by the protocol's request id.
///
/// A `std::sync::Mutex`, not an async one: every critical section is a single
/// map operation with no await inside it, and a synchronous lock is what lets a
/// dropped request registration clean up. A cancelled request is dropped, not
/// awaited to completion, so cleanup that could only run on an async path would
/// never run at all.
#[derive(Default)]
pub(crate) struct PendingRequestTable {
    state: StdMutex<PendingRequestState>,
}

impl PendingRequestTable {
    /// Register a request only while the reader is alive. The closed check and
    /// insertion share one lock with [`Self::close_and_fail_with`], closing the
    /// EOF race where a request could be inserted immediately after the reader
    /// drained the map and then wait forever for a dead process.
    pub(crate) fn insert(&self, id: i64, tx: oneshot::Sender<serde_json::Value>) -> bool {
        let mut state = self.lock();
        if state.closed {
            return false;
        }
        if state.map.is_empty() {
            state.pending_since = Some(Instant::now());
        }
        state.map.insert(id, tx);
        true
    }

    pub(crate) fn take(&self, id: i64) -> Option<oneshot::Sender<serde_json::Value>> {
        let mut state = self.lock();
        let sender = state.map.remove(&id);
        if state.map.is_empty() {
            state.pending_since = None;
        }
        sender
    }

    pub(crate) fn pending_since(&self) -> Option<Instant> {
        self.lock().pending_since
    }

    /// How many requests are in flight. The leak surface: a request abandoned
    /// without releasing its slot shows up here and nowhere else.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().map.len()
    }

    /// Take one arbitrary in-flight sender. Tests that need to answer "whatever
    /// request the transport just issued" do not know its id, so they cannot use
    /// [`Self::take`]; this keeps them off the inner map.
    #[cfg(test)]
    pub(crate) fn take_any(&self) -> Option<oneshot::Sender<serde_json::Value>> {
        let mut state = self.lock();
        let id = *state.map.keys().next()?;
        let sender = state.map.remove(&id);
        if state.map.is_empty() {
            state.pending_since = None;
        }
        sender
    }

    /// Fail every in-flight request with the caller's protocol error so callers
    /// return immediately instead of waiting indefinitely, and reject later
    /// registrations on this dead transport. Process death is sticky for this
    /// transport; the resilient owner creates a fresh one on restart.
    pub(crate) fn close_and_fail_with(&self, error: impl Fn() -> serde_json::Value) {
        let drained: Vec<_> = {
            let mut state = self.lock();
            state.closed = true;
            state.pending_since = None;
            state.map.drain().collect()
        };
        for (_id, tx) in drained {
            let _ = tx.send(error());
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PendingRequestState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Evidence about whether the child engine is still turning its loop.
///
/// Two facts, kept together because each is only meaningful next to the other:
/// when the child last emitted ANY protocol output, and how many successive
/// fully-bounded hops it answered with nothing at all.
pub(crate) struct EngineLiveness {
    /// When the read loop last got ANY output from the child — a response OR an
    /// event. A child that is emitting is working, however slowly; a WEDGED
    /// child emits nothing at all.
    last_output_at: StdMutex<Instant>,
    /// Successive unanswered full-bound hops. Reset outright by any response.
    consecutive_failures: AtomicU32,
    /// When the last strike was charged, so hops already in flight then are not
    /// counted as independent evidence. Cleared with the counter.
    last_strike_at: StdMutex<Option<Instant>>,
}

impl Default for EngineLiveness {
    fn default() -> Self {
        Self::since(Instant::now())
    }
}

impl EngineLiveness {
    pub(crate) fn since(last_output_at: Instant) -> Self {
        Self {
            last_output_at: StdMutex::new(last_output_at),
            consecutive_failures: AtomicU32::new(0),
            last_strike_at: StdMutex::new(None),
        }
    }

    /// Stamp output from the child. The read loop calls this for every line it
    /// reads, which is what lets hang detection tell a busy engine from a
    /// wedged one.
    pub(crate) fn note_output(&self) {
        self.set_last_output_at(Instant::now());
    }

    pub(crate) fn set_last_output_at(&self, at: Instant) {
        *self
            .last_output_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = at;
    }

    pub(crate) fn last_output_at(&self) -> Instant {
        *self
            .last_output_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether the child produced nothing at all while a hop issued at
    /// `issued_at` was running. A hop is evidence of a WEDGE only under that
    /// condition: a busy engine keeps emitting while it works.
    pub(crate) fn was_silent_during(&self, issued_at: Instant) -> bool {
        self.last_output_at() <= issued_at
    }

    /// Charge one unanswered hop toward hang detection, returning the resulting
    /// strike count when this hop was independent evidence.
    ///
    /// CONSECUTIVE MEANS SEQUENTIAL IN TIME. A hop that was already in flight
    /// when the previous strike was charged observed the SAME window of
    /// silence, so it is not independent evidence. The LSP fans out — hover,
    /// definition, completion, references and a background diagnostics pull are
    /// routinely in flight at the same instant on the same bound — so charging
    /// each of them separately would reach the threshold after a SINGLE bound's
    /// worth of silence and restart an engine that is merely busy building a
    /// cold program. The restart discards that program, so the next wave is
    /// cold too: a self-sustaining restart loop. `issued_at` is when this hop's
    /// bound started; only a hop issued at or after the last strike advances the
    /// count.
    pub(crate) fn charge_strike(&self, issued_at: Instant) -> Option<u32> {
        if !self.was_silent_during(issued_at) {
            return None;
        }
        let mut last = self
            .last_strike_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.is_some_and(|at| issued_at < at) {
            return None;
        }
        *last = Some(Instant::now());
        Some(self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Clear hang-detection state after proof the engine is alive and answering.
    pub(crate) fn clear_strikes(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        *self
            .last_strike_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    #[cfg(test)]
    pub(crate) fn strikes(&self) -> u32 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }
}

/// The pending-work half of the silence watchdog's input.
///
/// Implemented by each transport's pending state so the watchdog can hold a
/// `Weak` to it: when the transport is dropped the watchdog observes the dead
/// weak reference and ends, rather than outliving its session.
pub(crate) trait PendingWorkClock: Send + Sync + 'static {
    /// Start of the current non-empty in-flight interval, if any.
    fn pending_since(&self) -> Option<Instant>;
}

impl PendingWorkClock for PendingRequestTable {
    fn pending_since(&self) -> Option<Instant> {
        PendingRequestTable::pending_since(self)
    }
}

/// Provider-health watchdog, not a request timeout.
///
/// Feature requests remain pending until response or client cancellation. A
/// restart is requested only when the child has pending work and emits no
/// protocol output whatsoever for the absolute silence cap — a slow engine that
/// keeps emitting progress remains healthy.
///
/// `suppress_while` disarms the signal for a transport that reports a
/// deliberate teardown; a transport without that concept passes `None`.
pub(crate) async fn watch_engine_silence<P: PendingWorkClock>(
    pending: std::sync::Weak<P>,
    liveness: Arc<EngineLiveness>,
    crash_notify: Arc<Notify>,
    suppress_while: Option<Arc<std::sync::atomic::AtomicBool>>,
    poll: Duration,
    silence_cap: Duration,
    engine: &'static str,
) {
    loop {
        tokio::time::sleep(poll).await;
        let Some(pending) = pending.upgrade() else {
            return;
        };
        if suppress_while
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
        {
            continue;
        }
        let Some(pending_since) = pending.pending_since() else {
            continue;
        };
        let silent_for = std::cmp::max(liveness.last_output_at(), pending_since).elapsed();
        if silent_for < silence_cap {
            continue;
        }
        tracing::error!(
            "{engine} emitted no output for {silent_for:?} while requests were pending; restarting"
        );
        crash_notify.notify_waiters();
        return;
    }
}
