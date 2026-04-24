#![deny(missing_docs)]
//! Session-side request context + per-context counters + TLS guards.
//!
//! Plan §2.2. `RequestContext` is the per-request state that rides along
//! one `get_component_meta_with_resolution` call: request id, canonical,
//! footprint capture flag, the audit accumulator (if capturing), and the
//! per-context atomic cache-event counters. Per-context counters kill
//! the `is_approximate` story — they are exact even under concurrent
//! audits because each request's context isolates its own events.
//!
//! TLS installation is stack-safe:
//!
//! - `RequestContextGuard::install` uses `RefCell::replace` (never
//!   `borrow_mut`) so a nested install cannot panic on an
//!   already-occupied slot.
//! - Accessors (`current_request_context`, `current_accumulator`) take
//!   a short borrow, clone the `Arc`, and return — the borrow is
//!   released before the clone escapes.
//! - `Drop` restores the previous slot unconditionally via `take` +
//!   `replace`, both of which are non-panicking.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use verter_scheduler::request_context::{
    CacheEventKind, OpaqueContextGuard, OpaqueRequestContext, RequestContextLike, TlsUninstall,
};

use crate::component_meta_audit::accumulator::RequestFootprintAccumulator;

/// Per-request state. Held as `Arc<RequestContext>` and wrapped into
/// [`OpaqueRequestContext`] when handed to the scheduler.
#[derive(Debug)]
pub struct RequestContext {
    /// Monotonic request id. Non-zero by construction.
    pub request_id: u64,
    /// Canonical id the request resolves for.
    pub canonical_id: Arc<str>,
    /// Whether the request is capturing its semantic footprint. When
    /// `true`, `audit_accumulator` is populated.
    pub footprint_capture: bool,
    /// The per-request footprint accumulator (opt-in).
    pub audit_accumulator: Option<Arc<RequestFootprintAccumulator>>,
    /// Per-context cold-build counter. Populated by
    /// `execute_cooperative` calling `ctx.record_cache_event(Miss |
    /// ColdBuild)`. Exact per-request even under concurrent audits
    /// because each request's context isolates its own events
    /// (plan §1.4 — kills the `is_approximate` field).
    pub cold_builds: AtomicU64,
    /// Per-context warm-hit counter. Fired on `Hit`.
    pub warm_hits: AtomicU64,
    /// Per-context joined-wait counter. Fired on `JoinedWait`
    /// (a peer picked up an in-flight artifact before this request
    /// could start from cold).
    pub joined_waits: AtomicU64,
    /// Per-context sentinel counter. Fired on `Sentinel` — placeholder
    /// entries that collapse to a real artifact later.
    pub sentinels: AtomicU64,
    /// Per-context in-flight-abort-retry counter. Fired on
    /// `InflightAbortedRetry` — a retry loop after an in-flight
    /// slot was aborted by a newer generation.
    pub inflight_aborted_retries: AtomicU64,
    /// Per-context cold-abort-swept counter. Fired on
    /// `ColdAbortSwept` — a cold entry reaped during generation
    /// reconciliation.
    pub cold_aborts_swept: AtomicU64,
}

impl RequestContext {
    /// Construct a new per-request context with zeroed counters.
    pub fn new(
        request_id: u64,
        canonical_id: Arc<str>,
        footprint_capture: bool,
        audit_accumulator: Option<Arc<RequestFootprintAccumulator>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            request_id,
            canonical_id,
            footprint_capture,
            audit_accumulator,
            cold_builds: AtomicU64::new(0),
            warm_hits: AtomicU64::new(0),
            joined_waits: AtomicU64::new(0),
            sentinels: AtomicU64::new(0),
            inflight_aborted_retries: AtomicU64::new(0),
            cold_aborts_swept: AtomicU64::new(0),
        })
    }
}

impl RequestContextLike for RequestContext {
    fn request_id(&self) -> u64 {
        self.request_id
    }
    fn capture_enabled(&self) -> bool {
        self.footprint_capture
    }
    fn on_dedup_joiner(
        &self,
        _canonical_id: Arc<str>,
        _winner_request_id: u64,
        _winner_audited: bool,
    ) {
        // Commit 4 wires this into the accumulator's
        // `push_shared_load_reuse`. Before the footprint miner is
        // hooked, the callback is a no-op — the observability surface
        // is not yet consuming these events. Plan §2.7.
        if let Some(acc) = self.audit_accumulator.as_ref() {
            acc.push_shared_load_reuse(_canonical_id, _winner_request_id, _winner_audited);
        }
    }
    fn record_cache_event(&self, event: CacheEventKind) {
        let counter = match event {
            CacheEventKind::Hit => &self.warm_hits,
            CacheEventKind::Miss => &self.cold_builds,
            CacheEventKind::JoinedWait => &self.joined_waits,
            CacheEventKind::Sentinel => &self.sentinels,
            CacheEventKind::ColdBuild => &self.cold_builds,
            CacheEventKind::InflightAbortedRetry => &self.inflight_aborted_retries,
            CacheEventKind::ColdAbortSwept => &self.cold_aborts_swept,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }
    fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
        let guard = RequestContextGuard::install(self);
        Box::new(GuardUninstaller { _guard: guard })
    }
}

thread_local! {
    pub(crate) static CURRENT_REQUEST_CONTEXT:
        RefCell<Option<Arc<RequestContext>>> = const { RefCell::new(None) };
    pub(crate) static CURRENT_ACCUMULATOR:
        RefCell<Option<Arc<RequestFootprintAccumulator>>> = const { RefCell::new(None) };
    pub(crate) static NESTED_AUDIT_GUARD: Cell<bool> = const { Cell::new(false) };
    pub(crate) static REQUESTS_CREATED_IN_CURRENT_AUDITED_RUN:
        Cell<u32> = const { Cell::new(0) };
}

/// RAII guard that installs a `RequestContext` (and its accumulator,
/// if present) into TLS and restores the previous slots on drop. Both
/// the `CURRENT_REQUEST_CONTEXT` and `CURRENT_ACCUMULATOR` TLS slots
/// also plant the scheduler's `OpaqueRequestContext` so worker-thread
/// code that reads `verter_scheduler::request_context::current_request_id()`
/// observes this request's id.
///
/// Stack-safe: `RefCell::replace` never panics on an already-occupied
/// slot; `Drop` uses `take` + `replace` which also never panic.
pub struct RequestContextGuard {
    prev_context: Option<Arc<RequestContext>>,
    prev_accumulator: Option<Arc<RequestFootprintAccumulator>>,
    // Installs the opaque context into scheduler TLS so workers see
    // `current_request_id()` return the right value.
    _opaque_guard: OpaqueContextGuard,
}

impl RequestContextGuard {
    /// Install `ctx` as both the session-side `CURRENT_REQUEST_CONTEXT`
    /// (together with the accumulator TLS) and the scheduler's
    /// opaque TLS slot, so worker threads see `current_request_id()`
    /// return the right value. The returned guard restores every
    /// prior TLS value on drop.
    pub fn install(ctx: Arc<RequestContext>) -> Self {
        let acc = ctx.audit_accumulator.clone();
        let opaque = OpaqueRequestContext(Arc::clone(&ctx) as Arc<dyn RequestContextLike>);
        let opaque_guard = OpaqueContextGuard::install(opaque);
        let prev_context = CURRENT_REQUEST_CONTEXT.with(|c| c.replace(Some(ctx)));
        let prev_accumulator = CURRENT_ACCUMULATOR.with(|c| c.replace(acc));
        Self {
            prev_context,
            prev_accumulator,
            _opaque_guard: opaque_guard,
        }
    }
}

impl Drop for RequestContextGuard {
    fn drop(&mut self) {
        // Non-panicking restore: `take` + `replace` never panic.
        let prev_acc = self.prev_accumulator.take();
        let prev_ctx = self.prev_context.take();
        CURRENT_ACCUMULATOR.with(|c| {
            c.replace(prev_acc);
        });
        CURRENT_REQUEST_CONTEXT.with(|c| {
            c.replace(prev_ctx);
        });
        // `_opaque_guard` drops after this, restoring the scheduler's
        // TLS to whatever it held before our install.
    }
}

struct GuardUninstaller {
    #[allow(dead_code)]
    _guard: RequestContextGuard,
}

impl TlsUninstall for GuardUninstaller {
    fn uninstall(self: Box<Self>) {
        // Guard drops via field drop when Self drops.
    }
}

/// Return a clone of the currently installed `RequestContext`, or
/// `None` when no context is installed. Takes a short borrow, clones
/// the Arc, releases the borrow before the clone escapes — no
/// RefCell borrow is held across user code.
#[must_use]
pub fn current_request_context() -> Option<Arc<RequestContext>> {
    CURRENT_REQUEST_CONTEXT.with(|c| c.borrow().as_ref().map(Arc::clone))
}

/// Return a clone of the currently installed accumulator, or `None`.
/// Same Arc-clone-out-of-borrow pattern as `current_request_context`.
#[must_use]
pub fn current_accumulator() -> Option<Arc<RequestFootprintAccumulator>> {
    CURRENT_ACCUMULATOR.with(|c| c.borrow().as_ref().map(Arc::clone))
}

/// Increment the thread-local audited-run request counter. Returns the
/// value AFTER the increment. Used by the harness to detect multiple
/// requests in a single `run_custom` closure.
pub fn increment_requests_created() -> u32 {
    REQUESTS_CREATED_IN_CURRENT_AUDITED_RUN.with(|cell| {
        let n = cell.get().saturating_add(1);
        cell.set(n);
        n
    })
}

/// Snapshot the current audited-run request counter.
pub fn requests_created_snapshot() -> u32 {
    REQUESTS_CREATED_IN_CURRENT_AUDITED_RUN.with(|cell| cell.get())
}

/// Reset the audited-run request counter to zero. Harness calls this
/// on entry to each audited run.
pub fn reset_requests_created() {
    REQUESTS_CREATED_IN_CURRENT_AUDITED_RUN.with(|cell| cell.set(0));
}

/// Mark the nested-audit guard; returns `true` when a nested audit is
/// about to run on the same thread (harness rejects this).
pub fn nested_audit_in_progress() -> bool {
    NESTED_AUDIT_GUARD.with(|cell| cell.get())
}

/// RAII guard that flips the `NESTED_AUDIT_GUARD` flag while alive.
pub struct NestedAuditGuard;

impl NestedAuditGuard {
    /// Try to enter a nested audit guard. Returns `Some(Self)` when no
    /// audit is in progress on this thread and the guard is installed;
    /// returns `None` when an audit is already active (the harness
    /// surfaces this as `NestedAuditNotSupported`).
    pub fn enter() -> Option<Self> {
        let already = NESTED_AUDIT_GUARD.with(|cell| {
            if cell.get() {
                true
            } else {
                cell.set(true);
                false
            }
        });
        if already {
            None
        } else {
            Some(Self)
        }
    }
}

impl Drop for NestedAuditGuard {
    fn drop(&mut self) {
        NESTED_AUDIT_GUARD.with(|cell| cell.set(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(id: u64, capture: bool) -> Arc<RequestContext> {
        RequestContext::new(id, Arc::from("/x.vue"), capture, None)
    }

    #[test]
    fn request_context_guard_clears_tls_on_normal_return() {
        assert!(current_request_context().is_none());
        let ctx = make_ctx(1, true);
        {
            let _g = RequestContextGuard::install(Arc::clone(&ctx));
            assert_eq!(current_request_context().unwrap().request_id, 1);
        }
        assert!(current_request_context().is_none());
    }

    #[test]
    fn request_context_guard_clears_tls_on_panic_unwind() {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let ctx = make_ctx(2, true);
        let r = catch_unwind(AssertUnwindSafe(|| {
            let _g = RequestContextGuard::install(Arc::clone(&ctx));
            assert_eq!(current_request_context().unwrap().request_id, 2);
            panic!("test panic");
        }));
        assert!(r.is_err());
        assert!(
            current_request_context().is_none(),
            "TLS slot must be cleared via the guard's Drop on unwind",
        );
    }

    #[test]
    fn request_context_guard_drop_uses_take_and_replace_never_panics() {
        // Nested install — outer guard's Drop must restore outer's
        // prior (None), not panic on the inner's live borrow.
        let a = make_ctx(10, false);
        let b = make_ctx(20, false);
        let g1 = RequestContextGuard::install(Arc::clone(&a));
        assert_eq!(current_request_context().unwrap().request_id, 10);
        let g2 = RequestContextGuard::install(Arc::clone(&b));
        assert_eq!(current_request_context().unwrap().request_id, 20);
        drop(g2);
        assert_eq!(current_request_context().unwrap().request_id, 10);
        drop(g1);
        assert!(current_request_context().is_none());
    }

    #[test]
    fn current_accumulator_cloned_out_of_borrow_no_refcell_held_across_push() {
        // Install a context that HAS an accumulator; read via
        // current_accumulator; push a record while holding the Arc.
        // If the TLS borrow were held across the push, this would
        // panic on a RefCell re-borrow. It must succeed.
        let acc = Arc::new(RequestFootprintAccumulator::new());
        let ctx = RequestContext::new(3, Arc::from("/y.vue"), true, Some(Arc::clone(&acc)));
        let _g = RequestContextGuard::install(ctx);
        let held = current_accumulator().expect("accumulator present");
        held.push_shared_load_reuse(Arc::from("/a.vue"), 99, true);
        // Access again to confirm TLS remains usable after the push.
        let again = current_accumulator().expect("still present");
        assert!(Arc::ptr_eq(&held, &again));
    }
}
