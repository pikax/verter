#![deny(missing_docs)]
//! [`AuditObserver`] trait + the [`current_observer`] TLS accessor.
//!
//! Lower crates emit through this trait; they never reach into
//! `verter_session` or `verter_scheduler` for context. The session
//! layer's concrete `RequestContext` implements `AuditObserver`, and
//! `RequestContextGuard::install` populates the substrate's TLS slot
//! alongside its own bookkeeping.

use std::cell::RefCell;
use std::sync::Arc;

use crate::origin_graph::VfsLayer;

/// Compact event tag emitted through [`AuditObserver::record_event`].
///
/// Producers prefer the dedicated `record_*` methods over the generic
/// `record_event`; this enum carries counter-style attributions for
/// events without a structured payload — used today by the
/// inflight-abort retry mirror and the cold-abort sweep tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditEvent {
    /// One inflight-aborted retry observed in the cold-resolver loop.
    InflightAbortedRetry,
    /// One cold-abort sweep tick.
    ColdAbortSwept,
}

/// Trait implemented by anything wanting to receive audit events.
///
/// The default implementations are no-ops so producers only override
/// the methods they care about. The session-side `RequestContext`
/// provides full implementations; the [`crate::noop::NoOpObserver`]
/// and trivial test fakes leave them defaulted.
pub trait AuditObserver: Send + Sync {
    /// Counter-style attribution for events without structured
    /// payload. Producers that already have a typed signal (file
    /// read, lock acquisition, …) should call the dedicated method
    /// instead.
    fn record_event(&self, _event: AuditEvent) {}

    /// Record one cache layer hit / miss decision. The substrate
    /// keeps the layer name as a `&'static str` to avoid allocating
    /// on the hot path; the session-side implementation matches on
    /// the literal name.
    fn record_cache_event(&self, _layer: &'static str, _hit: bool) {}

    /// Record that the request observed a workspace file read at the
    /// given canonical id.
    fn record_file(
        &self,
        _canonical_id: &str,
        _layer: VfsLayer,
        _bytes_read: u64,
        _cache_hit: bool,
    ) {
    }

    /// Record one lock acquisition with the given wall-clock cost.
    fn record_lock_acquisition(&self, _lock_name: &'static str, _wait_ns: u64) {}

    /// Record a phase boundary timing. Producers call this at the end
    /// of a named phase with the elapsed milliseconds.
    fn record_phase_timing(&self, _phase: &'static str, _elapsed_ms: f64) {}
}

thread_local! {
    /// Owned slot for the current thread's [`AuditObserver`]. Installed
    /// either by [`crate::noop::install_noop_observer`] (for filtered
    /// requests) or by the session-side `RequestContextGuard::install`
    /// path (for active requests).
    static CURRENT_OBSERVER: RefCell<Option<Arc<dyn AuditObserver>>> =
        const { RefCell::new(None) };
}

/// Return a clone of the currently installed observer, or `None`
/// when no observer has been planted on this thread.
///
/// Cost: ~3 ns on miss, ~5 ns on hit (the cost is one TLS load + one
/// `Arc::clone` on the success path). Same order of magnitude as the
/// scheduler's existing `current_request_id()`.
#[must_use]
pub fn current_observer() -> Option<Arc<dyn AuditObserver>> {
    CURRENT_OBSERVER.with(|slot| slot.borrow().as_ref().map(Arc::clone))
}

/// Install `observer` as the active observer on the calling thread,
/// returning an RAII guard that restores the previous slot on drop.
///
/// Public so the session-side `RequestContextGuard::install` can plant
/// an `Arc<RequestContext>` (which implements [`AuditObserver`])
/// without going through `install_noop_observer`. Stack-safe — drop
/// restores whatever value the slot held before the install.
#[must_use]
pub fn install_observer(observer: Arc<dyn AuditObserver>) -> ObserverGuard {
    let prev = CURRENT_OBSERVER.with(|slot| slot.replace(Some(observer)));
    ObserverGuard { prev }
}

/// RAII guard returned by [`install_observer`] (and by
/// [`crate::noop::install_noop_observer`]). Restores the previous
/// observer on drop.
pub struct ObserverGuard {
    prev: Option<Arc<dyn AuditObserver>>,
}

impl Drop for ObserverGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        CURRENT_OBSERVER.with(|slot| {
            slot.replace(prev);
        });
    }
}
