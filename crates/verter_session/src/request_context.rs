#![deny(missing_docs)]
//! Session-side request context + per-context counters + TLS guards.
//!
//! `RequestContext` is the per-request state that rides along
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

use parking_lot::Mutex;
use verter_scheduler::request_context::{
    CacheEventKind, OpaqueContextGuard, OpaqueRequestContext, RequestContextLike, TlsUninstall,
};

use crate::component_meta_audit::accumulator::RequestFootprintAccumulator;

/// Request-scoped fuse state promoted to
/// host-owned + thread-local accessor.
///
/// Tracks the per-request projection-op count that the legacy engine's
/// `FuseBudgets::projection_op_count` rail used to terminate utility-
/// shape recursion (`Partial<T>` / `Pick<T,K>` / etc.) before the call
/// stack exhausts. The CAP is constructor-time on `HostConfig`
/// (`HostConfig::projection_op_budget`) per §0.6.5 stack-depth
/// discipline. The COUNTER is per-request — held here in TLS so
/// dispatch-side helpers (which today do NOT have an engine instance
/// to consult) can observe and increment the same fuse the engine's
/// `check_projection_op_count` observes.
///
/// Constructed at the dispatch entry point from `HostConfig::projection_op_budget`
/// (or its 2000-default when the field is `0`). Threaded via the
/// existing `RequestContextLike::install_tls` bridge — NO new TLS
/// axis (per the §5.13a.1.2 brief: "Threaded through dispatch's
/// `Instantiate` and `MaterializeSurface` paths via the existing
/// `RequestContextLike::install_tls` bridge … NO new TLS axis;
/// reuse the existing one").
#[derive(Debug)]
pub struct RequestBudget {
    /// Projection-operation budget for the request. Sourced from
    /// `HostConfig::projection_op_budget`.
    pub projection_op_budget: usize,
    /// Per-request counter — incremented by the legacy engine's
    /// `FuseState::check_projection_op_count` (during the 5m migration
    /// window; the engine retains a parallel counter inside
    /// `FuseState`) AND by the dispatch-side helper. The value seen
    /// by the dispatch helper is the single-host-instance projection
    /// counter for the current request.
    pub projection_ops_executed: Cell<usize>,
}

impl RequestBudget {
    /// Construct a new per-request budget with a zeroed counter and
    /// the supplied cap.
    ///
    /// The returned `Arc<RequestBudget>` is request-scoped and lives
    /// behind the request-context TLS axis; it never crosses a thread
    /// boundary, so the `!Sync` `Cell<usize>` counter is safe even
    /// though `Arc::new` triggers the
    /// [`clippy::arc_with_non_send_sync`] lint. Switching to `Rc`
    /// would lose the structural compatibility with the rest of the
    /// resolver-core API surface (which threads `Arc<…>` everywhere
    /// else), so the canonical fix is the explicit allow with this
    /// rationale anchor.
    #[must_use]
    #[allow(
        clippy::arc_with_non_send_sync,
        reason = "request-scoped, TLS-pinned; Arc retained for resolver-core API symmetry"
    )]
    pub fn new(projection_op_budget: usize) -> Arc<Self> {
        Arc::new(Self {
            projection_op_budget,
            projection_ops_executed: Cell::new(0),
        })
    }

    /// Equivalent of the legacy engine's
    /// `FuseState::check_projection_op_count`: increments the counter
    /// and returns `true` when the budget has been exceeded (caller
    /// should bail). The cap is the constructor-time value passed to
    /// `RequestBudget::new`. When the cap is `0`, the legacy default
    /// of 2000 is used as a fall-back so existing tests that
    /// constructed a `HostConfig` without setting the field continue
    /// to observe the documented cap.
    pub fn check_projection_op_count(&self) -> bool {
        let current = self.projection_ops_executed.get().saturating_add(1);
        self.projection_ops_executed.set(current);
        let cap = if self.projection_op_budget == 0 {
            2000
        } else {
            self.projection_op_budget
        };
        current > cap
    }

    /// Read-only view of the counter (test-only — used by §5.D.4 to
    /// observe budget-exceeded sentinel behavior).
    #[cfg(test)]
    pub(crate) fn projection_ops_executed_count(&self) -> usize {
        self.projection_ops_executed.get()
    }
}

thread_local! {
    /// Request-scoped fuse state TLS slot.
    /// Installed by `RequestBudgetGuard::install` and read by the
    /// dispatch-side helpers (`current_request_budget`).
    static CURRENT_REQUEST_BUDGET: RefCell<Option<Arc<RequestBudget>>> =
        const { RefCell::new(None) };
}

/// RAII guard that installs a `RequestBudget` into TLS and restores
/// the previous slot on drop. Stack-safe: `RefCell::replace` never
/// panics on an already-occupied slot; `Drop` uses `take` + `replace`
/// which also never panic.
pub struct RequestBudgetGuard {
    prev: Option<Arc<RequestBudget>>,
}

impl RequestBudgetGuard {
    /// Install `budget` into TLS, returning a guard that restores the
    /// previous slot on drop.
    #[must_use]
    pub fn install(budget: Arc<RequestBudget>) -> Self {
        let prev = CURRENT_REQUEST_BUDGET.with(|c| c.replace(Some(budget)));
        Self { prev }
    }
}

impl Drop for RequestBudgetGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        CURRENT_REQUEST_BUDGET.with(|c| {
            c.replace(prev);
        });
    }
}

/// Return a clone of the currently installed `RequestBudget`, or
/// `None` when no budget is installed (e.g. callers outside an
/// audited request — the engine path independently observes its
/// `FuseState`).
#[must_use]
pub fn current_request_budget() -> Option<Arc<RequestBudget>> {
    CURRENT_REQUEST_BUDGET.with(|c| c.borrow().as_ref().map(Arc::clone))
}

/// Per-request state. Held as `Arc<RequestContext>` and wrapped into
/// [`OpaqueRequestContext`] when handed to the scheduler. Also implements
/// [`verter_audit::AuditObserver`] so the same `Arc` plants into the
/// substrate's TLS slot via [`RequestContextGuard::install`].
#[derive(Debug)]
pub struct RequestContext {
    /// Monotonic request id. Non-zero by construction.
    pub request_id: u64,
    /// Canonical id the request resolves for.
    pub canonical_id: Arc<str>,
    /// Audit-side request kind. Defaults to
    /// [`verter_audit::RequestKind::ComponentMeta`] for the component-meta
    /// entry-point. Other producer surfaces pass their own kind through
    /// [`Self::with_kind`] when wiring an audited request through this
    /// context.
    pub kind: verter_audit::RequestKind,
    /// Whether the request is capturing its semantic footprint. When
    /// `true`, `audit_accumulator` is populated.
    pub footprint_capture: bool,
    /// The per-request footprint accumulator (opt-in).
    pub audit_accumulator: Option<Arc<RequestFootprintAccumulator>>,
    /// Audit-record registration handle planted by the public audited
    /// entry-point. The entry-point constructs an
    /// [`crate::host_audit_runtime::AuditRequestRegistration`] **before**
    /// installing this context into TLS, hands it to
    /// [`Self::install_audit_registration`], and the inner resolver
    /// path finalises through it instead of calling
    /// [`crate::VerterHost::finalize_request_audit_record`] directly. `None`
    /// when no audited entry-point is in scope (rare — direct callers
    /// of `resolve_component_meta` outside the audited path).
    pub audit_registration:
        std::sync::OnceLock<Arc<crate::host_audit_runtime::AuditRequestRegistration>>,
    /// Per-context cold-build counter. Populated by
    /// `execute_cooperative` calling `ctx.record_cache_event(Miss |
    /// ColdBuild)`. Exact per-request even under concurrent audits
    /// because each request's context isolates its own events
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
    /// Per-context counter — total
    /// `materialize_component_meta_structure` invocations observed
    /// during the request.
    pub materialize_structure_calls: AtomicU64,
    /// Per-context counter — subset of `materialize_structure_calls`
    /// satisfied by the materialiser's `MaterializeStructureDb` peek.
    ///
    pub materialize_structure_cache_hits: AtomicU64,
    /// Per-context counter — lock acquisitions on the per-scope
    /// `NodeArena` dedup index.
    pub node_arena_lock_acquisitions: AtomicU64,
    /// Per-context counter — lock acquisitions on the family-map
    /// dep-signature reverse index.
    pub family_map_lock_acquisitions: AtomicU64,
    /// Per-context counter — times a `dep_signature` was merged into
    /// the materialiser's `local_fence`.
    pub dep_signature_merges: AtomicU64,
    /// Per-context counter — subset of `dep_signature_merges` that
    /// hit an existing intern bucket.
    pub dep_signature_intern_hits: AtomicU64,
    /// Optional parent-request id captured at construction time. When
    /// the scheduler's TLS context (via
    /// [`verter_scheduler::request_context::current_request_id`]) is
    /// `Some(parent)` at construction, this slot stores `parent` so
    /// the audit record's `parent_request_id` field is populated.
    /// `None` when the request has no parent (top-level audited
    /// entry-point with no enclosing TLS context).
    pub parent_request_id: Option<u64>,
    /// Scheduler-side attribution for this request. Populated by
    /// [`Self::record_scheduler_dispatch`] (called via the audit
    /// observer trait by scheduler workers at dispatch time). The
    /// first dispatch wins on the per-request capture; subsequent
    /// dispatches increment `dispatch_count` on the captured value.
    pub scheduler_audit: Mutex<Option<verter_audit::SchedulerAudit>>,
}

impl RequestContext {
    /// Construct a new per-request context with zeroed counters. The
    /// kind defaults to [`verter_audit::RequestKind::ComponentMeta`];
    /// callers wanting a different kind use [`Self::with_kind`].
    pub fn new(
        request_id: u64,
        canonical_id: Arc<str>,
        footprint_capture: bool,
        audit_accumulator: Option<Arc<RequestFootprintAccumulator>>,
    ) -> Arc<Self> {
        Self::with_kind(
            request_id,
            canonical_id,
            verter_audit::RequestKind::ComponentMeta,
            footprint_capture,
            audit_accumulator,
        )
    }

    /// Construct a new per-request context with an explicit
    /// [`verter_audit::RequestKind`]. Producer surfaces other than the
    /// component-meta entry-point pass their kind through this
    /// constructor; the `kind` is consumed by
    /// [`crate::host_audit_runtime::AuditRequestRegistration::new`] when
    /// the audit-config consumer filter decides whether to enter the
    /// active-request registry.
    pub fn with_kind(
        request_id: u64,
        canonical_id: Arc<str>,
        kind: verter_audit::RequestKind,
        footprint_capture: bool,
        audit_accumulator: Option<Arc<RequestFootprintAccumulator>>,
    ) -> Arc<Self> {
        // Sniff the scheduler's TLS slot for an enclosing parent
        // request. When a sub-request is created inside another
        // audited request's TLS context (either on the same thread or
        // after `install_tls` propagated the parent into a worker),
        // the new context records the parent's id so the audit record
        // surfaces parent / child correlation. `None` when no
        // enclosing context is installed.
        let parent_request_id = verter_scheduler::request_context::current_request_id();
        Arc::new(Self {
            request_id,
            canonical_id,
            kind,
            footprint_capture,
            audit_accumulator,
            audit_registration: std::sync::OnceLock::new(),
            cold_builds: AtomicU64::new(0),
            warm_hits: AtomicU64::new(0),
            joined_waits: AtomicU64::new(0),
            sentinels: AtomicU64::new(0),
            inflight_aborted_retries: AtomicU64::new(0),
            cold_aborts_swept: AtomicU64::new(0),
            materialize_structure_calls: AtomicU64::new(0),
            materialize_structure_cache_hits: AtomicU64::new(0),
            node_arena_lock_acquisitions: AtomicU64::new(0),
            family_map_lock_acquisitions: AtomicU64::new(0),
            dep_signature_merges: AtomicU64::new(0),
            dep_signature_intern_hits: AtomicU64::new(0),
            parent_request_id,
            scheduler_audit: Mutex::new(None),
        })
    }

    /// Plant the audit-record registration handle on this context.
    /// Called by the public audited entry-point after constructing the
    /// `AuditRequestRegistration`. The inner resolver path consults
    /// `self.audit_registration.get()` to decide whether to finalise
    /// through the registration or fall back to direct publication.
    /// Returns `Err` only on the rare race where two callers race for
    /// the same slot — the public entry-point is the sole writer in
    /// tree.
    pub fn install_audit_registration(
        &self,
        registration: Arc<crate::host_audit_runtime::AuditRequestRegistration>,
    ) -> Result<(), Arc<crate::host_audit_runtime::AuditRequestRegistration>> {
        self.audit_registration.set(registration)
    }

    /// Audit-side request kind. Returns the kind set at construction
    /// time. Defaults to [`verter_audit::RequestKind::ComponentMeta`]
    /// when the request was constructed via [`Self::new`]; producers
    /// passing a different kind must use [`Self::with_kind`].
    #[must_use]
    pub fn kind(&self) -> verter_audit::RequestKind {
        self.kind.clone()
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
        // Wires this into the accumulator's `push_shared_load_reuse`.
        // Before the footprint miner is hooked, the callback is a no-op —
        // the observability surface is not yet consuming these events.
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

impl verter_audit::AuditObserver for RequestContext {
    /// Counter-style attribution. The session-side
    /// `RequestContext` keeps per-request atomics for each event tag;
    /// this method bumps the matching atomic so producers in lower
    /// crates can call `verter_audit::current_observer()` and emit
    /// without reaching into `verter_session`.
    fn record_event(&self, event: verter_audit::AuditEvent) {
        match event {
            verter_audit::AuditEvent::InflightAbortedRetry => {
                self.inflight_aborted_retries
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ColdAbortSwept => {
                self.cold_aborts_swept.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn record_cache_event(&self, layer: &'static str, hit: bool) {
        // Map the substrate's coarse hit/miss signal onto the
        // existing per-request cache-event counters. The layer
        // string is the canonical short name producers use today;
        // unknown layers are intentionally ignored — this method is
        // a counter mirror, not a generic observability bus.
        let counter = match (layer, hit) {
            ("warm", true) | ("hit", true) => &self.warm_hits,
            ("cold", false) | ("miss", false) | ("cold_build", _) => &self.cold_builds,
            ("joined_wait", _) => &self.joined_waits,
            ("sentinel", _) => &self.sentinels,
            _ => return,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn record_file(
        &self,
        _canonical_id: &str,
        _layer: verter_audit::origin_graph::VfsLayer,
        _bytes_read: u64,
        _cache_hit: bool,
    ) {
        // VFS reads are accumulated by the per-request
        // `RequestFootprintAccumulator` via the workspace's
        // `register_audit_sink` path. The `AuditObserver` bridge
        // does not duplicate that signal — leaving this method as a
        // typed no-op preserves the substrate API while routing
        // remains through the dedicated session-side sink.
    }

    fn record_lock_acquisition(&self, lock_name: &'static str, _wait_ns: u64) {
        // Map the canonical lock names producers report onto the
        // per-request lock-acquisition counters maintained on the
        // request context. Unknown names are intentionally ignored.
        let counter = match lock_name {
            "node_arena" => &self.node_arena_lock_acquisitions,
            "family_map" => &self.family_map_lock_acquisitions,
            _ => return,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn record_phase_timing(&self, _phase: &'static str, _elapsed_ms: f64) {
        // Phase timings are not yet aggregated into
        // `RequestContext` counters; the typed signal is consumed
        // by the request-finalisation step which assembles a
        // `RequestTimingAudit` from the audit-side accumulator.
        // The trait method is left intentionally empty so producers
        // may emit through `current_observer()` without any
        // session-side coupling.
    }

    fn record_scheduler_dispatch(&self, audit: verter_audit::SchedulerAudit) {
        // First dispatch wins on the slot; subsequent dispatches bump
        // the dispatch counter so retries / re-enqueues are visible
        // without overwriting the first-dispatch facts (worker thread
        // id, depths, dwell). The session-side `RequestContext` is
        // the canonical owner of the per-request scheduler audit;
        // `AuditBuilder::finish` consults this slot when building the
        // [`verter_audit::RequestAuditRecord`].
        let mut slot = self.scheduler_audit.lock();
        match slot.as_mut() {
            None => *slot = Some(audit),
            Some(existing) => {
                existing.dispatch_count = existing.dispatch_count.saturating_add(1);
            }
        }
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
/// observes this request's id. The same `Arc<RequestContext>` is also
/// planted as an `Arc<dyn verter_audit::AuditObserver>` into the
/// substrate's `current_observer()` TLS slot so producers in lower
/// crates emit through `verter_audit` without reaching into
/// `verter_session`.
///
/// Stack-safe: `RefCell::replace` never panics on an already-occupied
/// slot; `Drop` uses `take` + `replace` which also never panic.
pub struct RequestContextGuard {
    prev_context: Option<Arc<RequestContext>>,
    prev_accumulator: Option<Arc<RequestFootprintAccumulator>>,
    // Installs the opaque context into scheduler TLS so workers see
    // `current_request_id()` return the right value.
    _opaque_guard: OpaqueContextGuard,
    // Installs the same `RequestContext` (as `Arc<dyn AuditObserver>`)
    // into the `verter_audit` substrate's TLS slot so producers in
    // lower crates can emit through `verter_audit::current_observer()`
    // without reaching into `verter_session`. Drops in field order
    // (after the opaque guard) to leave the substrate slot empty
    // before the session-side TLS slot empties.
    _audit_observer_guard: verter_audit::observer::ObserverGuard,
}

impl RequestContextGuard {
    /// Install `ctx` as both the session-side `CURRENT_REQUEST_CONTEXT`
    /// (together with the accumulator TLS) and the scheduler's
    /// opaque TLS slot, so worker threads see `current_request_id()`
    /// return the right value. The same `ctx` is also planted as an
    /// `Arc<dyn verter_audit::AuditObserver>` into the substrate's
    /// TLS slot so producers in lower crates can emit through
    /// `verter_audit::current_observer()`. The returned guard restores
    /// every prior TLS value on drop.
    pub fn install(ctx: Arc<RequestContext>) -> Self {
        let acc = ctx.audit_accumulator.clone();
        let opaque = OpaqueRequestContext(Arc::clone(&ctx) as Arc<dyn RequestContextLike>);
        let opaque_guard = OpaqueContextGuard::install(opaque);
        // Plant the same `RequestContext` into the `verter_audit`
        // substrate's TLS slot. Producers in lower crates retrieve it
        // via `verter_audit::current_observer()` — they never reach
        // into `verter_session` for context.
        let audit_observer_guard = verter_audit::observer::install_observer(
            Arc::clone(&ctx) as Arc<dyn verter_audit::AuditObserver>
        );
        let prev_context = CURRENT_REQUEST_CONTEXT.with(|c| c.replace(Some(ctx)));
        let prev_accumulator = CURRENT_ACCUMULATOR.with(|c| c.replace(acc));
        Self {
            prev_context,
            prev_accumulator,
            _opaque_guard: opaque_guard,
            _audit_observer_guard: audit_observer_guard,
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
        // `_opaque_guard` drops next, restoring the scheduler's TLS to
        // whatever it held before our install. `_audit_observer_guard`
        // drops last, restoring the substrate's `current_observer()`
        // slot to its prior occupant.
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
    fn request_budget_check_increments_until_cap_then_returns_true() {
        let budget = RequestBudget::new(3);
        // Each call increments. The cap is 3 (a value of 3 is "at
        // budget" — equality returns false; the FOURTH call is over
        // budget — `>` returns true).
        assert!(!budget.check_projection_op_count(), "1st call (1 of 3)");
        assert!(!budget.check_projection_op_count(), "2nd call (2 of 3)");
        assert!(!budget.check_projection_op_count(), "3rd call (3 of 3)");
        assert!(budget.check_projection_op_count(), "4th call exceeds 3");
        // Discrimination: the counter has been incremented exactly 4
        // times. NEGATIVE assertion — counter must NOT have been reset
        // by the trip.
        assert_eq!(
            budget.projection_ops_executed_count(),
            4,
            "counter must persist past the trip; the trip should not silently reset"
        );
    }

    #[test]
    fn request_budget_zero_cap_falls_back_to_default_2000() {
        let budget = RequestBudget::new(0);
        // Increment 1999 times — within the 2000 fall-back cap.
        for _ in 0..1999 {
            assert!(!budget.check_projection_op_count());
        }
        // The 2000th call is exactly at cap (2000 > 2000 is false).
        assert!(!budget.check_projection_op_count(), "2000th call at cap");
        // The 2001st call exceeds the fall-back cap.
        assert!(
            budget.check_projection_op_count(),
            "2001st call exceeds default"
        );
    }

    #[test]
    fn request_budget_guard_clears_tls_on_normal_return() {
        assert!(current_request_budget().is_none());
        let budget = RequestBudget::new(123);
        {
            let _g = RequestBudgetGuard::install(Arc::clone(&budget));
            assert_eq!(current_request_budget().unwrap().projection_op_budget, 123);
        }
        // NEGATIVE assertion: TLS slot is empty after the guard drops.
        assert!(
            current_request_budget().is_none(),
            "RequestBudgetGuard must clear TLS on drop"
        );
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
