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
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use verter_scheduler::request_context::{
    CacheEventKind, OpaqueContextGuard, OpaqueRequestContext, RequestContextLike, TlsUninstall,
};

use crate::component_meta_audit::accumulator::RequestFootprintAccumulator;
pub use crate::request_budget::RequestBudget;

/// Return the request-scoped projection budget carried by the active
/// [`RequestContext`].
#[must_use]
pub fn current_request_budget() -> Option<Arc<RequestBudget>> {
    current_request_context().map(|ctx| Arc::clone(&ctx.projection_budget))
}

use crate::semantic_query::{PartialReasonSet, ResultCompleteness};

thread_local! {
    /// Per-thread stack of per-COLD-COMPUTE completeness accumulators.
    ///
    /// A cold compute that admits a result into a SHARED semantic cache
    /// (`MaterializeStructureDb`, reused across consumers via R7
    /// cross-owner reuse) pushes a scope ([`ColdComputeCompletenessScope`])
    /// for the duration of its single-threaded compute. The compute's
    /// contributing child reads fold their partiality into the top scope
    /// via [`observe_component_meta_read_suppress`] /
    /// [`mark_request_materialization_cache_suppress`]; the admission gate
    /// reads the scope's completeness so the ENTRY carries its OWN
    /// completeness — NOT a request-global proxy that would let one
    /// consumer's partial poison a sibling consumer's complete entry.
    ///
    /// Single-threaded by construction: each singleflight cold compute
    /// runs start-to-finish on the winning flight's thread (the same
    /// model as the materialiser's `MATERIALIZE_IN_FLIGHT` / depth TLS),
    /// so the per-thread stack matches the compute's call tree. A nested
    /// compute bubbles its completeness into its parent on scope drop.
    static COLD_COMPUTE_COMPLETENESS: RefCell<Vec<ResultCompleteness>> =
        const { RefCell::new(Vec::new()) };
}

/// RAII scope tracking the completeness of ONE cold compute that admits
/// into a shared semantic cache. While held, partiality observed via
/// [`observe_component_meta_read_suppress`] /
/// [`mark_request_materialization_cache_suppress`] folds into THIS scope;
/// the admission gate reads [`current_cold_compute_completeness`]. On
/// drop the scope's completeness bubbles into the enclosing scope (if
/// any) so a nested compute taints its parent. A no-op-safe stack: with
/// no scope active the fold/read helpers degrade to request-level only.
#[must_use]
pub struct ColdComputeCompletenessScope {
    /// When `true` (the default every [`Self::enter`] caller gets),
    /// [`Drop`] merges this scope's accumulated partiality into the
    /// enclosing scope — a nested compute taints its parent. When `false`
    /// (set by [`Self::discard`]) the scope is popped WITHOUT bubbling.
    bubble: bool,
}

impl ColdComputeCompletenessScope {
    /// Enter a per-cold-compute completeness scope, seeded `Complete`.
    pub fn enter() -> Self {
        COLD_COMPUTE_COMPLETENESS.with(|s| s.borrow_mut().push(ResultCompleteness::Complete));
        Self { bubble: true }
    }

    /// Pop this scope WITHOUT bubbling its accumulated partiality into the
    /// enclosing scope.
    ///
    /// The DEFAULT drop bubbles a nested compute's partiality into its
    /// parent. A caller that propagates the completeness by another route —
    /// e.g. the fallthrough request executor, which carries the FINAL
    /// attempt's completeness out via `RequestRunResult.completeness` and
    /// folds it once at the surface boundary (`fold_result_completeness`) —
    /// uses this to retire a per-attempt scope (a discarded completion-fence
    /// retry, or the held scope on a cache-served-final path) so its
    /// partiality neither double-propagates nor over-suppresses a later
    /// complete result under the enclosing scope.
    pub fn discard(mut self) {
        self.bubble = false;
        // `self` drops here: the cleared `bubble` flag makes the `Drop` impl
        // pop this scope without merging into the parent.
    }
}

impl Drop for ColdComputeCompletenessScope {
    fn drop(&mut self) {
        COLD_COMPUTE_COMPLETENESS.with(|s| {
            let mut stack = s.borrow_mut();
            if let Some(child) = stack.pop() {
                if self.bubble {
                    if let Some(parent) = stack.last_mut() {
                        *parent = parent.merge(child);
                    }
                }
            }
        });
    }
}

/// Fold a partial completeness into the active per-cold-compute scope (if
/// any). No-op when no scope is active.
fn fold_cold_compute_completeness(completeness: ResultCompleteness) {
    COLD_COMPUTE_COMPLETENESS.with(|s| {
        if let Some(top) = s.borrow_mut().last_mut() {
            *top = top.merge(completeness);
        }
    });
}

/// The completeness accumulated by the active per-cold-compute scope.
/// `Complete` when no scope is active. The shared-semantic-cache
/// admission gate keys on `is_partial()` of this value so each entry
/// carries its OWN completeness (per-result/scoped, not a
/// request-global proxy).
#[must_use]
pub fn current_cold_compute_completeness() -> ResultCompleteness {
    COLD_COMPUTE_COMPLETENESS
        .with(|s| s.borrow().last().copied())
        .unwrap_or(ResultCompleteness::Complete)
}

/// Snapshot the request-result completeness signal as a boolean.
///
/// This is the REQUEST-level (cross-thread) partiality accumulator. For
/// the component-meta entry point request-scope IS result-scope (one
/// request resolves one component's meta), so this is the per-result
/// completeness for `synthesis_should_suppress`. The cross-consumer
/// poisoning hazard for SHARED semantic caches is handled separately and
/// precisely by the per-cold-compute scope above
/// ([`current_cold_compute_completeness`]). Returns `false` when no
/// `RequestContext` is installed.
#[must_use]
pub fn current_materialization_cache_suppress() -> bool {
    current_request_context()
        .map(|ctx| ctx.materialization_cache_suppress.load(Ordering::Relaxed))
        .unwrap_or(false)
}

/// Mark the request-result completeness signal partial, and fold the
/// partiality into the active per-cold-compute scope (if any). Sticky for
/// the request's lifetime. No-op when no `RequestContext` is installed
/// (the scope fold is still attempted — it is `RequestContext`-independent).
pub fn mark_request_materialization_cache_suppress() {
    if let Some(ctx) = current_request_context() {
        ctx.materialization_cache_suppress
            .store(true, Ordering::Relaxed);
    }
    fold_cold_compute_completeness(ResultCompleteness::partial(PartialReasonSet::PROPAGATED));
}

/// Fold a JOINED result completeness into the active suppress state — the
/// follower's no-poison fence in the singleflight rendezvous.
///
/// A singleflight FOLLOWER that coalesces onto a leader's lane receives the
/// leader's value AND its [`ResultCompleteness`]; this folds the EXACT
/// partial reason set into the follower's active per-cold-compute scope
/// (rather than blanket-re-marking a generic `PROPAGATED`, which would lose
/// the reason class) AND raises the request-scoped sticky suppress flag, so
/// the follower returns with its suppress state already partial BEFORE it
/// reaches any warm-admission site (`store_node`, the fallthrough result
/// cache, the owner / payload promotion). A `Complete` join is a no-op — the
/// generic-query rendezvous stays byte-identical.
pub fn fold_result_completeness(joined: ResultCompleteness) {
    if !joined.is_partial() {
        return;
    }
    if let Some(ctx) = current_request_context() {
        ctx.materialization_cache_suppress
            .store(true, Ordering::Relaxed);
    }
    fold_cold_compute_completeness(joined);
}

/// Class-fix helper for the component-meta read/materialize path: observe a
/// completed dispatch read and propagate PARTIAL-result suppression onto
/// the request-scoped sticky flag.
///
/// EVERY `dispatch.execute_read(...)` in the component-meta path
/// (projectors, the macro-payload substrate, dispatch helpers, graph
/// predicates, the slot-binding graph, and `component_meta_materialize`)
/// must route its result through this helper. A budget exhaustion / fatal
/// `QueryError` (`BudgetExceeded` / `UnstableState`) / same-path recursion
/// / walker fatal produces a PARTIAL value: such a read MUST suppress the
/// whole component-meta result's warm promotion (else a subsequent
/// identical request replays the poisoned partial instead of
/// cold-recomputing against the fresh budget).
///
/// CRITICAL distinction (the A2 signal split): the warm gate keys on
/// [`crate::semantic_query::CacheRead::result_is_partial`], NOT on
/// `cache_suppress`. `cache_suppress` is ALSO set when a perfectly VALID
/// complete result is merely not memo-publishable (a torn / unrootable
/// self-root, a tracer signature overflow, a `ReturnOnly`
/// cross-owner-reuse admission; see `project_semantic_dispatch::mod`'s
/// admission arms). Those are benign non-cacheability, NOT partial results
/// — keying the warm gate on `cache_suppress` would wrongly refuse to warm
/// a complete component-meta result (e.g. a carrier-stopped open `Pick`
/// whose valid shell rode a non-cacheable sub-read). Equally, a value-kind
/// gate (`matches!(value, Error | Recursive)`) is INSUFFICIENT: a
/// budget-tripped partial can surface as a COMPLETE `QueryResult::Value`
/// (a `ProjectPath` shallow-walking an `InstantiationRef` whose nested
/// `Instantiate` trips the budget — the walker catches the error,
/// contributes no surface, and `build_project_path` returns `Value` with
/// `result_is_partial=true`). The explicit `result_is_partial` field is
/// the sole correct authority.
#[inline]
pub fn observe_component_meta_read_suppress<T>(read: &crate::semantic_query::CacheRead<T>) {
    if read.result_is_partial {
        mark_request_materialization_cache_suppress();
    }
}

/// RAII guard scoping the slot-binding synthesis phase on the active
/// request context. While held, [`RequestContext::synthesis_active_depth`]
/// is `> 0`, so [`crate::project_semantic_dispatch`]'s `build_instantiate`
/// attributes any `Expanded` Instantiate to synthesis (bumping
/// [`RequestContext::synthesis_expanded_instantiate_calls`]).
///
/// Re-entrant: nested synthesis frames raise/lower the depth so a balanced
/// stack always restores the pre-entry depth. A no-op when no
/// `RequestContext` is installed (the synthetic test-fixture path).
#[must_use]
pub struct SynthesisScopeGuard {
    ctx: Option<Arc<RequestContext>>,
}

impl SynthesisScopeGuard {
    /// Enter the synthesis phase: raise the active depth on the current
    /// request context (if any).
    pub fn enter() -> Self {
        let ctx = current_request_context();
        if let Some(ctx) = ctx.as_ref() {
            ctx.synthesis_active_depth.fetch_add(1, Ordering::Relaxed);
        }
        Self { ctx }
    }
}

impl Drop for SynthesisScopeGuard {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.as_ref() {
            ctx.synthesis_active_depth.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// Bump the synthesis-attributable `Expanded` Instantiate counter on the
/// active request context IFF the synthesis phase is currently active
/// ([`RequestContext::synthesis_active_depth`] `> 0`). Called from
/// `build_instantiate` alongside the request-wide
/// [`RequestContext::expanded_instantiate_calls`] bump. No-op when no
/// context is installed or synthesis is not active.
pub fn note_expanded_instantiate_for_synthesis_scope() {
    if let Some(ctx) = current_request_context() {
        if ctx.synthesis_active_depth.load(Ordering::Relaxed) > 0 {
            ctx.synthesis_expanded_instantiate_calls
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Per-cache hit/miss attribution counter pair. Bumped at the
/// get/insert boundary of each cache via
/// [`current_request_context`] so the counts attribute exactly to
/// the request that performed the lookup. Concurrent requests each
/// see their own context — no host-global delta misattribution
/// under concurrency (the joiner-accounting contract: per-request
/// hits/misses attribute exactly even under concurrent dedup-join).
#[derive(Debug, Default)]
pub struct HitMiss {
    /// Hits observed on this cache layer during the request.
    pub hits: AtomicU64,
    /// Misses observed on this cache layer during the request.
    pub misses: AtomicU64,
}

impl HitMiss {
    /// Snapshot the current hit/miss values with relaxed ordering.
    /// The request is single-threaded at finalisation, so relaxed
    /// reads observe every prior bump on the same context.
    #[must_use]
    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }
}

/// Per-cache hit/miss attribution. One field per cache layer that
/// participates in the request's per-cache observability surface.
/// Each field is bumped at the get/insert boundary of its cache
/// when a [`RequestContext`] is currently installed in TLS. The
/// host-global counters in
/// [`crate::project_type_store::ProjectTypeStoreCounters`] remain
/// unchanged — they observe live entries / stale sweeps
/// cross-request, while these per-cache counters observe
/// per-request hit/miss attribution.
#[derive(Debug, Default)]
pub struct PerRequestCacheCounters {
    /// `FileArtifactStore` — canonical post-parse artifact cache.
    pub indexed: HitMiss,
    /// `AnalysisReadyDb` — analysis-stage artifact cache.
    pub analysis: HitMiss,
    /// `OwnerImportSurfaceDb` — owner direct-import surface cache.
    pub owner_import: HitMiss,
    /// `ComponentMetaResultDb` — final component-meta result cache.
    pub component_meta: HitMiss,
    /// `RouteDb` — host-backed resolver route cache.
    pub route_db: HitMiss,
    /// `RefCycleResultDb` — transitive-cycle result cache for
    /// parameterized generic helpers.
    pub ref_cycle: HitMiss,
    /// `IntrinsicRegistry` — intrinsic dispatch lookup cache.
    pub intrinsic_registry: HitMiss,
    /// `SemanticGraphStore` — semantic-query memo / graph cache.
    pub semantic_graph: HitMiss,
    /// `MaterializeStructureDb` — structural materialisation cache.
    pub materialize_structure: HitMiss,
    /// `ShapeCacheDb` — universal shape cache, TypeExpr
    /// subject. Counter retained under the legacy name to preserve
    /// audit-harness JSON schema compatibility.
    pub materialize_memo: HitMiss,
    /// `ShapeCacheDb` — universal shape cache,
    /// SemanticNode subject. Counter retained under the legacy name
    /// to preserve audit-harness JSON schema compatibility.
    pub member_shape_cache: HitMiss,
    /// Always-zero counter for the removed prepared-surface walker DB.
    /// Retained under the legacy name to preserve audit-harness JSON
    /// schema compatibility.
    pub prepared_surface: HitMiss,
    /// Always-zero counter for the removed prepared-member walker DB.
    /// Retained under the legacy name to preserve audit-harness JSON
    /// schema compatibility.
    pub prepared_member: HitMiss,
    /// Rule-compliance diagnostic counters. Empirical instrumentation
    /// that quantifies the bypass surfaces identified as the
    /// residual perf-gap suspects: per-request
    /// `HostStoreView::from_host` builds, bare-host
    /// `ComponentMetaQueryEngine::new(...)` constructions, and
    /// `ResolverContext::resolver_store_view()` warm-hit validator
    /// rebuilds. These counters are production-on (atomic, ~ns of
    /// cost vs. the µs–ms work they observe) and snapshot into
    /// [`verter_audit::store::BypassDiagnostics`] at request close.
    pub bypass_diagnostics: BypassDiagnosticCounters,
}

/// Diagnostic counters that quantify the rule-compliance bypass
/// surfaces. Bumped from the production code paths identified as the
/// residual perf-gap suspects; snapshotted into
/// [`verter_audit::store::BypassDiagnostics`] at request close so
/// each component-meta request emits its own delta (no host-global
/// accumulation, no cross-request leakage).
///
/// Atomic increments are negligible (~ns) compared with the µs–ms
/// of work each bump observes, so the counters stay on in production
/// builds — that is the only state in which the bench corpus surfaces
/// the bypass leverage these counters measure.
#[derive(Debug, Default)]
pub struct BypassDiagnosticCounters {
    /// Number of `HostStoreView::from_host` invocations on the
    /// current request. The per-request hoist
    /// expects this to drop to a small constant; counts >1 reveal
    /// resolver-tier carriers that still build their own owned view
    /// instead of borrowing the request-bound view.
    pub host_store_view_from_host_builds: AtomicU64,
    /// Number of `ComponentMetaQueryEngine::new(ctx)` constructions
    /// on the current request where `ctx.is_request_bound()` returned
    /// `false` (i.e. the engine was bound to a bare `&VerterHost`
    /// rather than a `HostResolverContext` / `SessionResolverContext`).
    /// Final-state invariant: `0` — every production engine is bound
    /// to a request-bound ctx.
    pub bare_engine_constructions: AtomicU64,
    /// Number of `ResolverContext::resolver_store_view()` calls on
    /// the current request. Each call rebuilds a full owned
    /// `HostStoreView` via `HostStoreView::from_host`; warm-hit
    /// validator paths in `fact_signature_helpers` now consult the
    /// borrowed `store_view()` directly, so this counter trends to 0
    /// in production.
    pub resolver_store_view_calls: AtomicU64,
}

impl BypassDiagnosticCounters {
    /// Snapshot the counter values with relaxed ordering. The
    /// request is single-threaded at finalisation so relaxed reads
    /// observe every prior bump.
    #[must_use]
    pub fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.host_store_view_from_host_builds
                .load(Ordering::Relaxed),
            self.bare_engine_constructions.load(Ordering::Relaxed),
            self.resolver_store_view_calls.load(Ordering::Relaxed),
        )
    }
}

/// Bump the per-request `resolver_store_view_calls` diagnostic
/// counter when a request context is installed. The bump is a noop
/// outside an audited request (synthesised tests, non-audited
/// callers). Called from every
/// `impl ResolverContext::resolver_store_view()` so the counter
/// observes all trait-dispatched owned-view rebuilds — the
/// warm-hit validator path was the dominant consumer until the
/// borrowed-view substitution landed.
#[inline]
pub fn bump_resolver_store_view_call() {
    if let Some(ctx) = current_request_context() {
        ctx.cache_counters
            .bypass_diagnostics
            .resolver_store_view_calls
            .fetch_add(1, Ordering::Relaxed);
    }
}

/// Bump the per-request `bare_engine_constructions` diagnostic
/// counter when a request context is installed. Called from
/// `ComponentMetaQueryEngine::new` whenever the ctx fails the
/// `is_request_bound()` predicate (i.e. the engine is being bound
/// to the bare `impl ResolverContext for VerterHost` rail).
#[inline]
pub fn bump_bare_engine_construction() {
    if let Some(ctx) = current_request_context() {
        ctx.cache_counters
            .bypass_diagnostics
            .bare_engine_constructions
            .fetch_add(1, Ordering::Relaxed);
    }
}

/// Per-request state. Held as `Arc<RequestContext>` and wrapped into
/// [`OpaqueRequestContext`] when handed to the scheduler. Also implements
/// [`verter_audit::AuditObserver`] so the same `Arc` plants into the
/// substrate's TLS slot via [`RequestContextGuard::install`].
#[derive(Debug)]
pub struct RequestContext {
    /// Monotonic request id. Non-zero by construction.
    pub request_id: u64,
    /// Per-request trace identifier — a stable string token that
    /// propagates through tracing spans the request opens. Wired into
    /// the tracing instrumentation so log scrapers can correlate
    /// dispatch / memo / walker events under one request. Generated
    /// fresh per construction.
    pub trace_id: String,
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
    /// Whether per-file timing capture is enabled for this request.
    /// Mirrors `HostConfig::audit_timing_capture`. When `true`, the
    /// workspace and executor stages wrap their reads / parses / lowers
    /// in `Instant::now()` and the resulting `*_ns` values flow into
    /// `FileAudit::read_ms` / `parse_ms` / `lower_ms` for entries this
    /// request triggered.
    pub timing_capture: bool,
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
    /// Per-request projection-operation fuse used by semantic dispatch.
    /// Stored on the existing request context so scheduler worker TLS
    /// propagation carries the same budget state as audit/cache counters.
    pub projection_budget: Arc<RequestBudget>,
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
    /// Per-context aggregate — total wall-clock spent waiting on
    /// lock acquisitions during the audited window, in nanoseconds.
    /// Populated only when `audit_timing_capture` is `true`; the
    /// production lock-acquisition helpers short-circuit before
    /// `Instant::now()` when timing is off so this counter stays
    /// at `0`. Surfaces as
    /// [`verter_audit::WaitAudit::lock_wait_ns`].
    pub lock_wait_ns: AtomicU64,
    /// Per-context aggregate — total number of lock acquisitions
    /// (cross-cache) observed for the audited request. Bumped once
    /// per acquisition through the session-side helpers regardless
    /// of which shard / canonical owned the mutex. Surfaces as
    /// [`verter_audit::WaitAudit::lock_acquisitions`].
    pub lock_acquisitions: AtomicU64,
    /// Per-context aggregate — total scheduler queue dwell time for
    /// the audited request, in nanoseconds. Accumulates across
    /// every dispatch the request observes (initial + retries).
    /// Always `0` when no scheduler dispatch is observed (e.g. WASM,
    /// fast paths). Surfaces as
    /// [`verter_audit::WaitAudit::queue_wait_ns`].
    pub queue_wait_ns: AtomicU64,
    /// Per-context counter — times a `dep_signature` was merged into
    /// the materialiser's `local_fence`.
    pub dep_signature_merges: AtomicU64,
    /// Per-context counter — subset of `dep_signature_merges` that
    /// hit an existing intern bucket.
    pub dep_signature_intern_hits: AtomicU64,
    /// Per-cache hit/miss attribution for the cache layers
    /// participating in the request. Bumped at the get/insert
    /// boundary of each cache. Snapshotted at request close into
    /// the audit record's
    /// [`verter_audit::store::CacheLayerBreakdown`] field.
    pub cache_counters: PerRequestCacheCounters,
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
    /// Per-request peak process RSS slot. The host-owned sampler
    /// thread (see
    /// [`crate::host_audit_runtime::HostAuditRuntime`]) ticks every
    /// 50 ms while the matching
    /// [`crate::host_audit_runtime::AuditRequestRegistration`] is in
    /// the active-request registry; on each tick it calls
    /// `current_process_rss()` and writes
    /// `fetch_max(current_rss)` here. At finalize time, the audit
    /// builder snapshots the load and surfaces it as
    /// `RequestAuditRecord::memory.process_rss_peak_bytes`. Stays at
    /// `0` when:
    ///   * `HostConfig::audit_timing_capture` is disabled,
    ///   * the target is `wasm32` (sampler is gated off there), or
    ///   * the registration is `Noop` (filtered kind).
    pub process_rss_peak_bytes: AtomicU64,

    // ─────── Type-resolution counters ───────
    //
    // Populated by [`crate::project_semantic_dispatch::ProjectSemanticDispatch::execute`]
    // and the navigator hop drivers. Snapshotted at request finalisation
    // into [`verter_audit::TypeResolutionPayload`] for
    // [`verter_audit::RequestKind::TypeResolution`] requests.
    /// Number of resolver hops taken — every dispatch through
    /// `SemanticQueryApi::execute` against the active context bumps
    /// this once.
    pub type_resolution_hops: AtomicU64,
    /// Number of `Navigate` hops — intermediate path-projection hops
    /// that walked through a member without expanding it.
    pub type_resolution_navigations: AtomicU64,
    /// Number of `Expanded` / `Shallow` hops that allocated new
    /// semantic nodes. Cache hits do NOT bump this counter.
    pub type_resolution_expansions: AtomicU64,
    /// Number of conditional-type branch decisions resolved (open
    /// distributions + closed branch reductions).
    pub type_resolution_conditional_decisions: AtomicU64,
    /// Number of `ref_root_reaches_transitive_cycle_node` cache hits
    /// observed during the request.
    pub type_resolution_ref_root_cycle_hits: AtomicU64,
    /// Total projection ops executed against the projection-op
    /// budget (`SemanticQueryKey::ProjectPath` invocations).
    pub type_resolution_projection_ops: AtomicU64,
    /// Maximum walker depth observed during the request — the
    /// `fetch_max`-monotonic high-water mark for navigator and
    /// dispatch recursion.
    pub type_resolution_depth_high_water: AtomicU16,
    /// Set to `true` when the depth budget
    /// (`verter_audit::WALKER_DEPTH_CAP`) was exceeded during the
    /// request.
    pub type_resolution_recursion_limit_reached: AtomicBool,
    /// Per-request `SemanticQueryKey` dispatch trace: bit `i` is set once a key
    /// with [`SemanticQueryKeyTag::bit_index`](crate::semantic_query::SemanticQueryKeyTag::bit_index)
    /// `i` dispatches through the shared
    /// `ProjectSemanticDispatch::execute_via_cold_build_helper` cold-build choke
    /// point. Both the `SemanticQueryApi::execute` trait method and the
    /// dep-signature-preserving `execute_read` subquery entry funnel through that
    /// helper, so the mask records EVERY `SemanticQueryKey` variant dispatched
    /// anywhere during the audited request — including nested reducer
    /// sub-dispatches that enter only via `execute_read` (e.g.
    /// `NormalizeIntersection`, `ProjectPath`), not just the top-level
    /// `execute`-entered subset. (This is distinct from the focused cold/warm
    /// `semantic_query_*` counters, which attribute cost for only the hot-path
    /// subset.) Surfaced verbatim on
    /// [`verter_audit::TypeResolutionPayload::semantic_query_dispatch_mask`] so a
    /// consumer can recover which query families a resolution actually touched.
    pub type_resolution_dispatched_query_tags: AtomicU32,
    /// Per-context accumulator for compile-phase wall-clock — parse
    /// phase. Stored as fixed-point microseconds (`f64` ms × 1_000`)
    /// so the atomic counter can `fetch_add` cheaply; finalisation
    /// converts back to milliseconds.
    pub compile_parse_us: AtomicU64,
    /// Per-context accumulator for compile-phase wall-clock — script
    /// (transform) phase. Same fixed-point-microseconds encoding as
    /// [`Self::compile_parse_us`].
    pub compile_transform_us: AtomicU64,
    /// Per-context accumulator for compile-phase wall-clock — codegen
    /// phase (template / IDE). Same fixed-point-microseconds encoding
    /// as [`Self::compile_parse_us`].
    pub compile_codegen_us: AtomicU64,
    /// Per-context accumulator for compile-phase wall-clock — CSS
    /// analysis phase. Same fixed-point-microseconds encoding as
    /// [`Self::compile_parse_us`].
    pub compile_css_analysis_us: AtomicU64,
    /// Per-context accumulator for compile-phase wall-clock — sourcemap
    /// generation. Same fixed-point-microseconds encoding as
    /// [`Self::compile_parse_us`].
    pub compile_sourcemap_us: AtomicU64,
    /// Per-context counter — number of `CodeTransform` operations
    /// observed during the compile request. Bumped from
    /// [`verter_audit::AuditEvent::CompileCodeTransformOp`].
    pub compile_code_transform_ops: AtomicU64,

    // ─────── Slot-binding-synthesis attribution counters ───────
    //
    // Per-request partitioning of host-global counters that would
    // otherwise leak across dispatch sites under workspace-parallel
    // test execution. The host-global atomics keep their existing
    // semantics; the per-request mirrors below let attribution tests
    // assert per-request synthesis events without false positives
    // from peer dispatches.
    /// Expanded-mode `Instantiate` (`context.projection_reduction.mode ==
    /// Expanded`) dispatches observed against
    /// the active request. Mirrors host-global
    /// `SLOT_BINDING_EXPANDED_INSTANTIATE_CALLS`; surfaces on
    /// [`verter_audit::ComponentMetaPayload::expanded_instantiate_calls`].
    ///
    /// Request-WIDE: bumped on EVERY `Expanded` Instantiate in the request,
    /// including the canonical macro-surface PRODUCER phase (which legitimately
    /// expands imported macro roots). NOT a synthesis-purity signal — see
    /// [`Self::synthesis_expanded_instantiate_calls`] for the synthesis-scoped
    /// counter the slot-binding eagerness guard asserts.
    pub expanded_instantiate_calls: AtomicU64,
    /// Re-entrant depth of the slot-binding synthesis phase
    /// ([`crate::meta_resolve::slot_binding_graph::resolve_slot_bindings_graph_native`]).
    /// Raised on synthesis entry, lowered on exit (a `Drop` guard restores
    /// it even on early return). `> 0` means an `Instantiate` dispatch is
    /// attributable to slot-binding synthesis.
    pub synthesis_active_depth: AtomicU64,
    /// Expanded-mode `Instantiate` (`context.projection_reduction.mode ==
    /// Expanded`) dispatches observed WHILE
    /// [`Self::synthesis_active_depth`] `> 0` — the synthesis-attributable
    /// subset of [`Self::expanded_instantiate_calls`]. The slot-binding
    /// eagerness guard `enrich_does_not_eagerly_instantiate_carrier`
    /// asserts this is ZERO: synthesis must drive the carrier walk in
    /// `Navigate` / `Skeleton`, never the giant-tree `Expanded` body mode.
    /// Surfaces on
    /// [`verter_audit::ComponentMetaPayload::synthesis_expanded_instantiate_calls`].
    pub synthesis_expanded_instantiate_calls: AtomicU64,
    /// `MemoEntry` insertions published into the `SemanticGraphStore`
    /// warm map during this request (bumped at `warm_publish_one`);
    /// surfaces on
    /// [`verter_audit::ComponentMetaPayload::memo_insertions`].
    pub memo_insertions: AtomicU64,
    /// `cache_runtime::singleflight` builds that landed with
    /// `cache_suppress=true` and had their warm-publish skipped;
    /// surfaces on
    /// [`verter_audit::ComponentMetaPayload::memo_publish_suppressed`].
    /// Discriminating signal for the no-poison gate.
    pub memo_publish_suppressed: AtomicU64,
    /// Sticky flag raised by reducer / materializer paths on every
    /// `cache_suppress=true` semantic read. OR-folded into
    /// `synthesis_should_suppress` so the final-result
    /// `ComponentMetaResultDb` refuses to admit projection-budget
    /// partials. See `current_materialization_cache_suppress` and
    /// `mark_request_materialization_cache_suppress`.
    pub materialization_cache_suppress: AtomicBool,

    // ─────── Resolver / import-route hot-path counters ───────
    //
    // Populated by producer-side emits via
    // `verter_audit::current_observer().record_event(...)`. Snapshotted
    // at request finalisation into
    // [`verter_audit::ResolverHotPathCounters`] on the
    // [`verter_audit::RequestFootprintAudit::resolver_hot_path`] field.
    /// Total invocations of `run_external_type_frontier_closure_with_view`.
    pub frontier_closure_invocations_total: AtomicU64,
    /// Subset of [`Self::frontier_closure_invocations_total`] whose
    /// frontier returned `target = None`.
    pub frontier_closure_invocations_target_none: AtomicU64,
    /// Subset of [`Self::frontier_closure_invocations_target_none`]
    /// for `(owner, type_name)` pairs already observed in the request.
    /// Discriminating signal for the "cross-request negative-resolution
    /// caching defect" hypothesis.
    pub frontier_closure_redundant_target_none_pairs: AtomicU64,
    /// Per-request set tracking the `(owner_canonical, type_name)`
    /// pairs that have already emitted a `None` from the frontier
    /// closure during this request. Used by the producer to discriminate
    /// "first None for pair" (bumps only `target_none`) from "subsequent
    /// None for same pair" (bumps both `target_none` AND
    /// `redundant_target_none_pairs`).
    pub frontier_target_none_pairs_seen:
        Mutex<rustc_hash::FxHashSet<(std::string::String, std::string::String)>>,
    /// Warm hits on a host-owned negative entry in the
    /// resolved-external-type cache.
    pub resolved_external_type_cache_negative_hits: AtomicU64,
    /// Misses on a host-owned negative entry — the cache had no
    /// "known None" entry to short-circuit.
    pub resolved_external_type_cache_negative_misses: AtomicU64,
    /// Cold import-route resolutions that returned a positive target.
    pub resolve_import_cold_positive: AtomicU64,
    /// Cold import-route resolutions that returned `None`.
    pub resolve_import_cold_negative: AtomicU64,
    /// Warm import-route resolutions served with a positive target.
    pub resolve_import_warm_positive: AtomicU64,
    /// Warm import-route resolutions served with a known-miss target.
    pub resolve_import_warm_negative: AtomicU64,
    /// Import-route lookups classified as `import_route_is_known_miss`.
    pub known_miss_route_served: AtomicU64,
    /// Known-miss entries revalidated as still missing.
    pub known_miss_route_revalidated: AtomicU64,
    /// Known-miss entries recomputed because the `content_generation`
    /// advanced.
    pub known_miss_route_recomputed: AtomicU64,
    /// Cold imported-registry-symbol resolutions.
    pub imported_registry_cold: AtomicU64,
    /// Warm imported-registry-symbol resolutions (`peek` hit).
    pub imported_registry_warm: AtomicU64,
    /// Imported-registry-symbol resolutions that returned `None`.
    pub imported_registry_negative: AtomicU64,
    /// Cold imported-type-root resolutions (closure body ran).
    pub imported_root_cold: AtomicU64,
    /// Warm imported-type-root resolutions (cached value reused).
    pub imported_root_warm: AtomicU64,
    /// Barrel-export hops traversed during route-frontier resolution.
    pub route_db_barrel_steps: AtomicU64,
    /// `export *` wildcard fan-out expansions observed.
    pub route_db_wildcard_fanout: AtomicU64,
    /// Cold prepared-decl bundle materializations.
    pub prepared_decl_bundle_cold: AtomicU64,
    /// Warm prepared-decl bundle cache hits.
    pub prepared_decl_bundle_warm: AtomicU64,
    /// Bundle warm-read rejection — no `DashMap` entry for the canonical.
    pub prepared_decl_bundle_reject_entry_missing: AtomicU64,
    /// Bundle warm-read rejection — self-root canonical not tracked by view.
    pub prepared_decl_bundle_reject_self_root_untracked: AtomicU64,
    /// Bundle warm-read rejection — self-root tracked, hash differs.
    pub prepared_decl_bundle_reject_self_root_hash_mismatch: AtomicU64,
    /// Bundle warm-read rejection — `ImportRoute` snapshot absent.
    pub prepared_decl_bundle_reject_import_route_absent: AtomicU64,
    /// Bundle warm-read rejection — `ImportRoute` snapshot differs.
    pub prepared_decl_bundle_reject_import_route_mismatch: AtomicU64,
    /// Bundle warm-read rejection — unattributed (must stay 0).
    pub prepared_decl_bundle_reject_other: AtomicU64,
    // Focused semantic-query counters -----------------------------------
    // Each is per-request, bumped from `ProjectSemanticDispatch`. The
    // mining surface in `ResolverHotPathCounters` carries the `u32`
    // snapshot.
    /// Cold dispatches of `SemanticQueryKey::TypeOf`.
    pub semantic_query_typeof_cold: AtomicU64,
    /// Warm dispatches of `SemanticQueryKey::TypeOf`.
    pub semantic_query_typeof_warm: AtomicU64,
    /// Cold dispatches of `SemanticQueryKey::Instantiate`.
    pub semantic_query_instantiate_cold: AtomicU64,
    /// Warm dispatches of `SemanticQueryKey::Instantiate`.
    pub semantic_query_instantiate_warm: AtomicU64,
    /// Cold dispatches of `SemanticQueryKey::Conditional`.
    pub semantic_query_conditional_cold: AtomicU64,
    /// Warm dispatches of `SemanticQueryKey::Conditional`.
    pub semantic_query_conditional_warm: AtomicU64,
    /// Cold dispatches of `SemanticQueryKey::MappedType`.
    pub semantic_query_mapped_type_cold: AtomicU64,
    /// Warm dispatches of `SemanticQueryKey::MappedType`.
    pub semantic_query_mapped_type_warm: AtomicU64,
    /// Cold dispatches of `SemanticQueryKey::IndexedAccess`.
    pub semantic_query_indexed_access_cold: AtomicU64,
    /// Warm dispatches of `SemanticQueryKey::IndexedAccess`.
    pub semantic_query_indexed_access_warm: AtomicU64,
    /// Cold dispatches of `SemanticQueryKey::KeyOf`.
    pub semantic_query_keyof_cold: AtomicU64,
    /// Warm dispatches of `SemanticQueryKey::KeyOf`.
    pub semantic_query_keyof_warm: AtomicU64,
    /// Cold dispatches of `SemanticQueryKey::ProjectPath` /
    /// `ProjectMember`.
    pub semantic_query_project_path_cold: AtomicU64,
    /// Warm dispatches of `SemanticQueryKey::ProjectPath` /
    /// `ProjectMember`.
    pub semantic_query_project_path_warm: AtomicU64,
    /// Top-level `substitute_semantic_type_param` calls.
    pub substitute_top_level_calls: AtomicU64,
    /// Hits on the `substitute_memo_get` fast path.
    pub substitute_memo_hits: AtomicU64,
    /// `TypeOf` opaque returns from
    /// `substitute_with_change_tracking`.
    pub substitute_typeof_opaque: AtomicU64,
    /// `Conditional` descents in
    /// `substitute_with_change_tracking`.
    pub substitute_conditional_descend: AtomicU64,
    /// `MappedType` descents in
    /// `substitute_with_change_tracking`.
    pub substitute_mapped_type_descend: AtomicU64,
    /// Calls to `build_typeof`.
    pub build_typeof_calls: AtomicU64,
    /// `build_typeof` calls where `ensure_indexed_ready_serve` returned
    /// `None`.
    pub build_typeof_prepared_value_misses: AtomicU64,
    // Mapped-member materialization counters
    // --------------------------------------
    /// Calls to `materialize_mapped_member_value_for_key` whose
    /// identity tuple is FIRST-SEEN in the active request.
    pub mapped_member_plain_unique: AtomicU64,
    /// Calls to `materialize_mapped_member_value_for_key` whose
    /// identity tuple was already seen in the active request.
    pub mapped_member_plain_repeated: AtomicU64,
    /// Calls to `materialize_selected_key_mapped_value*` whose
    /// identity tuple is FIRST-SEEN in the active request.
    pub mapped_member_selected_key_unique: AtomicU64,
    /// Calls to `materialize_selected_key_mapped_value*` whose
    /// identity tuple was already seen in the active request.
    pub mapped_member_selected_key_repeated: AtomicU64,
    /// `prepared_decl_bundle` callsite attribution: from
    /// `SessionDispatchHost::scope_payload_for_base`.
    pub prepared_decl_bundle_callsite_scope_payload: AtomicU64,
    /// `prepared_decl_bundle` callsite attribution: from
    /// `build_instantiate`.
    pub prepared_decl_bundle_callsite_build_instantiate: AtomicU64,
    /// `prepared_decl_bundle` callsite attribution: residual sites.
    pub prepared_decl_bundle_callsite_other: AtomicU64,
    /// Mapper-binder-ordinal collisions: the same mapper source
    /// triple interned at different ordinals within one request.
    pub mapped_binder_ordinal_collision: AtomicU64,
    // Recursive-substitution counters
    // -------------------------------
    // Key insight: the recursive helper
    // `substitute_with_change_tracking` (substitute.rs:99-104)
    // BYPASSES the top-level `substitute_memo` even though
    // `(node, parameter_node, arg)` is a complete identity. These
    // counters classify recursive entries so the implementer can
    // confirm Path A (high repeat rate → wire memo) vs Path B (high
    // unique rate → parameterized generic-body cache at
    // build.rs:631-640 + lower.rs:140-215). After the recursive
    // memo wires, `_repeated` measures saved work and
    // `RecursiveSubstituteMemoHit` measures the memo's actual hit
    // count (which may differ under FIFO eviction).
    /// Recursive `substitute_with_change_tracking` entries whose
    /// `(node, parameter_node, arg)` triple is FIRST-SEEN in the
    /// active request.
    pub recursive_substitute_unique: AtomicU64,
    /// Recursive `substitute_with_change_tracking` entries whose
    /// `(node, parameter_node, arg)` triple was already seen in
    /// the active request — the recursive memo SHOULD short-circuit.
    pub recursive_substitute_repeated: AtomicU64,
    /// `Mapped`-arm rebuilds in `substitute_with_change_tracking`
    /// after at least one descendant sub-tree changed.
    pub substitute_mapped_rebuild: AtomicU64,
    /// `Conditional`-arm rebuilds in `substitute_with_change_tracking`
    /// after at least one descendant sub-tree changed.
    pub substitute_conditional_rebuild: AtomicU64,
    /// Recursive-helper hash-cons memo hits (at the
    /// `substitute_with_change_tracking` entry, not the public
    /// surface).
    pub recursive_substitute_memo_hits: AtomicU64,
    /// Typed-IR macro-surface projection accessor invocations.
    /// Bumped exactly once per public `resolve_root`,
    /// `project_named_member`, or `enumerate_member_names` call.
    /// The counter
    /// is the empirical hook that lets later analyses confirm
    /// consumers reach the typed-IR bridge rather than a parallel
    /// rail. Until a consumer adopts the bridge in production the
    /// counter stays at 0 in audited production requests; the
    /// hermetic discriminators in
    /// `tests/imported_macro_surface_bridge.rs` drive it
    /// explicitly.
    pub imported_macro_surface_projection: AtomicU64,
    /// Per-request observation set for recursive substitution
    /// identity triples — used by the unique/repeated classifier
    /// at the recursive helper's entry. The set is per-request so
    /// the classification resets between component-meta queries.
    /// Held briefly per recursive entry, not held across recursion.
    pub recursive_substitute_seen:
        parking_lot::Mutex<rustc_hash::FxHashSet<RecursiveSubstituteIdentity>>,
    /// Per-request observation set for mapped-member identity tuples
    /// — used by the unique/repeated classifier in
    /// `record_mapped_member_materialization_classify`. Lock-free
    /// ABI: a `parking_lot::Mutex` guards the small `FxHashSet` and
    /// is held briefly per per-K call. The set is per-request so the
    /// classification resets between component-meta queries.
    pub mapped_member_seen: parking_lot::Mutex<rustc_hash::FxHashSet<MappedMemberIdentity>>,
    /// Per-request observation set for mapper-source triples →
    /// observed ordinals, used by the binder-ordinal collision
    /// classifier in
    /// `record_mapper_binder_ordinal_for_classification`.
    pub mapper_source_ordinals:
        parking_lot::Mutex<rustc_hash::FxHashMap<MapperSourceIdentity, u16>>,
}

/// Identity tuple for the mapped-member materialization
/// unique/repeated classifier. Pairs the mapper binder + value
/// expression + key + reduction context — what a typed cache key
/// for the materialization result would look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MappedMemberIdentity {
    /// `mapper.parameter_node` — the binder SemanticNodeId.
    pub parameter_node: u64,
    /// `mapper.value_expr` — the substitution target SemanticNodeId.
    pub value_expr: u64,
    /// The enumerated key literal node id (interned literal-string /
    /// literal-number) the helper is being called with.
    pub key_node: u64,
    /// Compact encoding of the reduction context:
    /// `(mode_tag << 1) | demand_bit`.
    pub context_bits: u32,
    /// `0` = plain `materialize_mapped_member_value_for_key`,
    /// `1` = selected-key variant.
    pub variant: u8,
}

/// Identity tuple for the mapper-binder-ordinal collision
/// classifier. The source triple is what `lower.rs` uses to
/// construct the binder's `DeclIdentity`; if the same triple
/// produces two different `param_index` ordinals across calls,
/// downstream `SemanticNodeData::TypeParam` interns hash to two
/// distinct SemanticNodeIds — and the typed cache misses on what
/// SHOULD be a hit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapperSourceIdentity {
    /// The mapper-binder's declaring file canonical (`""` for
    /// `NodeScopeId::Global`).
    pub canonical_id: Arc<str>,
    /// The declaring file's whole-hash.
    pub whole_hash: u64,
    /// The mapper-parameter's display name (`"K"`, `"P"`, etc.).
    pub display_name: Arc<str>,
}

/// Identity tuple for the recursive-substitution
/// unique/repeated classifier. The triple matches the existing
/// top-level `substitute_memo` key composition
/// (`(value_expr, parameter_node, arg)`) so the classifier output
/// directly predicts the recursive-memo's hit rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecursiveSubstituteIdentity {
    /// The `SemanticNodeId` of the recursive substitution input
    /// (`node` parameter of `substitute_with_change_tracking`).
    pub node: u64,
    /// The binder SemanticNodeId (`parameter_node` parameter).
    pub parameter_node: u64,
    /// The substitution argument SemanticNodeId (`arg` parameter).
    pub arg: u64,
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
        Self::with_kind_and_timing(
            request_id,
            canonical_id,
            kind,
            footprint_capture,
            false,
            audit_accumulator,
        )
    }

    /// Construct a new per-request context with explicit kind AND
    /// per-file timing-capture flag. Used by the audited-request
    /// entry-points that thread `HostConfig::audit_timing_capture`
    /// through to producers.
    pub fn with_kind_and_timing(
        request_id: u64,
        canonical_id: Arc<str>,
        kind: verter_audit::RequestKind,
        footprint_capture: bool,
        timing_capture: bool,
        audit_accumulator: Option<Arc<RequestFootprintAccumulator>>,
    ) -> Arc<Self> {
        Self::with_kind_timing_and_projection_budget(
            request_id,
            canonical_id,
            kind,
            footprint_capture,
            timing_capture,
            audit_accumulator,
            0,
        )
    }

    /// Construct a new per-request context with an explicit
    /// projection-operation budget. Component-meta entry points thread
    /// `HostConfig::projection_op_budget` through this constructor so
    /// semantic dispatch sees the same fuse on the main thread and on
    /// scheduler workers.
    pub fn with_kind_timing_and_projection_budget(
        request_id: u64,
        canonical_id: Arc<str>,
        kind: verter_audit::RequestKind,
        footprint_capture: bool,
        timing_capture: bool,
        audit_accumulator: Option<Arc<RequestFootprintAccumulator>>,
        projection_op_budget: usize,
    ) -> Arc<Self> {
        // Sniff the scheduler's TLS slot for an enclosing parent
        // request. When a sub-request is created inside another
        // audited request's TLS context (either on the same thread or
        // after `install_tls` propagated the parent into a worker),
        // the new context records the parent's id so the audit record
        // surfaces parent / child correlation. `None` when no
        // enclosing context is installed.
        let parent_request_id = verter_scheduler::request_context::current_request_id();
        // Per-request trace_id — uuid v4 string, generated once per
        // request. Propagates through tracing spans the request opens
        // so structured-event consumers (the dispatch / memo / walker
        // instrumentation in this crate) can correlate emitted events
        // back to the originating request.
        let trace_id = uuid::Uuid::new_v4().to_string();
        Arc::new(Self {
            request_id,
            trace_id,
            canonical_id,
            kind,
            footprint_capture,
            timing_capture,
            audit_accumulator,
            audit_registration: std::sync::OnceLock::new(),
            cold_builds: AtomicU64::new(0),
            warm_hits: AtomicU64::new(0),
            joined_waits: AtomicU64::new(0),
            sentinels: AtomicU64::new(0),
            inflight_aborted_retries: AtomicU64::new(0),
            cold_aborts_swept: AtomicU64::new(0),
            materialize_structure_calls: AtomicU64::new(0),
            projection_budget: RequestBudget::new(projection_op_budget),
            materialize_structure_cache_hits: AtomicU64::new(0),
            node_arena_lock_acquisitions: AtomicU64::new(0),
            family_map_lock_acquisitions: AtomicU64::new(0),
            lock_wait_ns: AtomicU64::new(0),
            lock_acquisitions: AtomicU64::new(0),
            queue_wait_ns: AtomicU64::new(0),
            dep_signature_merges: AtomicU64::new(0),
            dep_signature_intern_hits: AtomicU64::new(0),
            cache_counters: PerRequestCacheCounters::default(),
            parent_request_id,
            scheduler_audit: Mutex::new(None),
            process_rss_peak_bytes: AtomicU64::new(0),
            type_resolution_hops: AtomicU64::new(0),
            type_resolution_navigations: AtomicU64::new(0),
            type_resolution_expansions: AtomicU64::new(0),
            type_resolution_conditional_decisions: AtomicU64::new(0),
            type_resolution_ref_root_cycle_hits: AtomicU64::new(0),
            type_resolution_projection_ops: AtomicU64::new(0),
            type_resolution_depth_high_water: AtomicU16::new(0),
            type_resolution_recursion_limit_reached: AtomicBool::new(false),
            type_resolution_dispatched_query_tags: AtomicU32::new(0),
            compile_parse_us: AtomicU64::new(0),
            compile_transform_us: AtomicU64::new(0),
            compile_codegen_us: AtomicU64::new(0),
            compile_css_analysis_us: AtomicU64::new(0),
            compile_sourcemap_us: AtomicU64::new(0),
            compile_code_transform_ops: AtomicU64::new(0),
            expanded_instantiate_calls: AtomicU64::new(0),
            synthesis_active_depth: AtomicU64::new(0),
            synthesis_expanded_instantiate_calls: AtomicU64::new(0),
            memo_insertions: AtomicU64::new(0),
            memo_publish_suppressed: AtomicU64::new(0),
            materialization_cache_suppress: AtomicBool::new(false),
            frontier_closure_invocations_total: AtomicU64::new(0),
            frontier_closure_invocations_target_none: AtomicU64::new(0),
            frontier_closure_redundant_target_none_pairs: AtomicU64::new(0),
            frontier_target_none_pairs_seen: Mutex::new(rustc_hash::FxHashSet::default()),
            resolved_external_type_cache_negative_hits: AtomicU64::new(0),
            resolved_external_type_cache_negative_misses: AtomicU64::new(0),
            resolve_import_cold_positive: AtomicU64::new(0),
            resolve_import_cold_negative: AtomicU64::new(0),
            resolve_import_warm_positive: AtomicU64::new(0),
            resolve_import_warm_negative: AtomicU64::new(0),
            known_miss_route_served: AtomicU64::new(0),
            known_miss_route_revalidated: AtomicU64::new(0),
            known_miss_route_recomputed: AtomicU64::new(0),
            imported_registry_cold: AtomicU64::new(0),
            imported_registry_warm: AtomicU64::new(0),
            imported_registry_negative: AtomicU64::new(0),
            imported_root_cold: AtomicU64::new(0),
            imported_root_warm: AtomicU64::new(0),
            route_db_barrel_steps: AtomicU64::new(0),
            route_db_wildcard_fanout: AtomicU64::new(0),
            prepared_decl_bundle_cold: AtomicU64::new(0),
            prepared_decl_bundle_warm: AtomicU64::new(0),
            prepared_decl_bundle_reject_entry_missing: AtomicU64::new(0),
            prepared_decl_bundle_reject_self_root_untracked: AtomicU64::new(0),
            prepared_decl_bundle_reject_self_root_hash_mismatch: AtomicU64::new(0),
            prepared_decl_bundle_reject_import_route_absent: AtomicU64::new(0),
            prepared_decl_bundle_reject_import_route_mismatch: AtomicU64::new(0),
            prepared_decl_bundle_reject_other: AtomicU64::new(0),
            semantic_query_typeof_cold: AtomicU64::new(0),
            semantic_query_typeof_warm: AtomicU64::new(0),
            semantic_query_instantiate_cold: AtomicU64::new(0),
            semantic_query_instantiate_warm: AtomicU64::new(0),
            semantic_query_conditional_cold: AtomicU64::new(0),
            semantic_query_conditional_warm: AtomicU64::new(0),
            semantic_query_mapped_type_cold: AtomicU64::new(0),
            semantic_query_mapped_type_warm: AtomicU64::new(0),
            semantic_query_indexed_access_cold: AtomicU64::new(0),
            semantic_query_indexed_access_warm: AtomicU64::new(0),
            semantic_query_keyof_cold: AtomicU64::new(0),
            semantic_query_keyof_warm: AtomicU64::new(0),
            semantic_query_project_path_cold: AtomicU64::new(0),
            semantic_query_project_path_warm: AtomicU64::new(0),
            substitute_top_level_calls: AtomicU64::new(0),
            substitute_memo_hits: AtomicU64::new(0),
            substitute_typeof_opaque: AtomicU64::new(0),
            substitute_conditional_descend: AtomicU64::new(0),
            substitute_mapped_type_descend: AtomicU64::new(0),
            build_typeof_calls: AtomicU64::new(0),
            build_typeof_prepared_value_misses: AtomicU64::new(0),
            mapped_member_plain_unique: AtomicU64::new(0),
            mapped_member_plain_repeated: AtomicU64::new(0),
            mapped_member_selected_key_unique: AtomicU64::new(0),
            mapped_member_selected_key_repeated: AtomicU64::new(0),
            prepared_decl_bundle_callsite_scope_payload: AtomicU64::new(0),
            prepared_decl_bundle_callsite_build_instantiate: AtomicU64::new(0),
            prepared_decl_bundle_callsite_other: AtomicU64::new(0),
            mapped_binder_ordinal_collision: AtomicU64::new(0),
            recursive_substitute_unique: AtomicU64::new(0),
            recursive_substitute_repeated: AtomicU64::new(0),
            substitute_mapped_rebuild: AtomicU64::new(0),
            substitute_conditional_rebuild: AtomicU64::new(0),
            recursive_substitute_memo_hits: AtomicU64::new(0),
            imported_macro_surface_projection: AtomicU64::new(0),
            recursive_substitute_seen: parking_lot::Mutex::new(rustc_hash::FxHashSet::default()),
            mapped_member_seen: parking_lot::Mutex::new(rustc_hash::FxHashSet::default()),
            mapper_source_ordinals: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
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

    /// Bump the `type_resolution_hops` counter by one. Called by the
    /// shared dispatcher (`ProjectSemanticDispatch::execute`) on every
    /// dispatched query — the per-request snapshot at finalization
    /// time surfaces as
    /// [`verter_audit::TypeResolutionPayload::hops`].
    pub fn bump_type_resolution_hop(&self, mode: crate::semantic_query::ProjectionMode) {
        self.type_resolution_hops.fetch_add(1, Ordering::Relaxed);
        match mode {
            crate::semantic_query::ProjectionMode::Navigate => {
                self.type_resolution_navigations
                    .fetch_add(1, Ordering::Relaxed);
            }
            crate::semantic_query::ProjectionMode::Expanded
            | crate::semantic_query::ProjectionMode::Shallow => {
                self.type_resolution_expansions
                    .fetch_add(1, Ordering::Relaxed);
            }
            crate::semantic_query::ProjectionMode::Identity
            | crate::semantic_query::ProjectionMode::Skeleton => {}
        }
    }

    /// Bump the conditional-branch decision counter — fired when an
    /// open conditional distributes the remaining path or a closed
    /// conditional reduces immediately.
    pub fn bump_type_resolution_conditional_decision(&self) {
        self.type_resolution_conditional_decisions
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Bump the `ref_root_reaches_transitive_cycle_node` cache-hit
    /// counter.
    pub fn bump_type_resolution_ref_root_cycle_hit(&self) {
        self.type_resolution_ref_root_cycle_hits
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Bump the `ProjectPath` projection-op counter by one — fired
    /// once per dispatched `SemanticQueryKey::ProjectPath`.
    pub fn bump_type_resolution_projection_op(&self) {
        self.type_resolution_projection_ops
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record that a `SemanticQueryKey` with `tag` dispatched through the shared
    /// `ProjectSemanticDispatch::execute_via_cold_build_helper` cold-build choke
    /// point. Called from the top of that helper (before key canonicalisation),
    /// which both `execute` and `execute_read` route through — so nested
    /// `execute_read`-only sub-dispatches are recorded too. Idempotent per tag —
    /// sets the tag's [`bit_index`](crate::semantic_query::SemanticQueryKeyTag::bit_index)
    /// bit in the per-request dispatch mask.
    pub fn record_dispatched_query_tag(&self, tag: crate::semantic_query::SemanticQueryKeyTag) {
        self.type_resolution_dispatched_query_tags
            .fetch_or(1 << tag.bit_index(), Ordering::Relaxed);
    }

    /// The accumulated `SemanticQueryKey` dispatch mask for this request — bit
    /// `i` set iff a tag with `bit_index() == i` dispatched at least once. Decode
    /// with [`SemanticQueryKeyTag::decode_dispatch_mask`](crate::semantic_query::SemanticQueryKeyTag::decode_dispatch_mask).
    #[must_use]
    pub fn type_resolution_dispatched_query_tags_mask(&self) -> u32 {
        self.type_resolution_dispatched_query_tags
            .load(Ordering::Relaxed)
    }

    /// Mark that the cross-file external-type frontier closure
    /// resolved `(owner_canonical, type_name)` to `target = None`
    /// during this request. ALWAYS bumps
    /// `frontier_closure_invocations_target_none`; bumps
    /// `frontier_closure_redundant_target_none_pairs` ONLY when the
    /// pair has already been observed earlier in the request — the
    /// dominant signal for the "cross-request negative-resolution
    /// caching defect" hypothesis.
    ///
    /// Producer side — the cross-file resolver in
    /// `host_resolve::external_type_resolution` — calls this after
    /// the frontier returns `Ok((_, None, _))`. Cheap on the hot
    /// path: one `Mutex` lock + one `FxHashSet::insert` + at most one
    /// `fetch_add`.
    pub fn observe_frontier_target_none_for_pair(&self, owner: &str, type_name: &str) {
        self.frontier_closure_invocations_target_none
            .fetch_add(1, Ordering::Relaxed);
        let key = (owner.to_string(), type_name.to_string());
        let inserted = self.frontier_target_none_pairs_seen.lock().insert(key);
        if !inserted {
            self.frontier_closure_redundant_target_none_pairs
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Update the per-request walker depth high-water mark. Cheap on
    /// the hot path: one `fetch_max` per recursive entry — saturates
    /// at `u16::MAX` if a depth value somehow exceeds the cap. When
    /// `depth >= verter_audit::WALKER_DEPTH_CAP`, also sets the
    /// `type_resolution_recursion_limit_reached` latch — this is the
    /// signal the audit consumer surfaces to discriminate
    /// pathological-recursion paths.
    pub fn observe_type_resolution_depth(&self, depth: u16) {
        // fetch_max is monotonic — observers may race with each other
        // safely. Relaxed is sufficient since the snapshot happens
        // single-threaded at finalisation.
        self.type_resolution_depth_high_water
            .fetch_max(depth, Ordering::Relaxed);
        if depth >= verter_audit::WALKER_DEPTH_CAP {
            self.type_resolution_recursion_limit_reached
                .store(true, Ordering::Relaxed);
        }
    }

    /// Classify one mapped-member materialization call as unique or
    /// repeated based on the identity tuple
    /// `(mapper.value_expr, mapper.parameter_node, key_node, context,
    /// variant)`. Bumps the appropriate `mapped_member_*_unique` /
    /// `mapped_member_*_repeated` counter via
    /// [`verter_audit::current_observer`] — paired so an
    /// investigator can compute the unique/repeated ratio that
    /// determines whether a typed mapped-member cache would close
    /// the K-loop cross product.
    ///
    /// Cheap on the hot path: one `Mutex` lock + one
    /// `FxHashSet::insert` + one `AuditEvent` emission. The set
    /// is per-request so classification resets between
    /// component-meta queries.
    pub fn classify_mapped_member_materialization(&self, identity: MappedMemberIdentity) {
        let inserted = self.mapped_member_seen.lock().insert(identity);
        let event = match (identity.variant, inserted) {
            (0, true) => verter_audit::AuditEvent::MappedMemberPlainUnique,
            (0, false) => verter_audit::AuditEvent::MappedMemberPlainRepeated,
            (_, true) => verter_audit::AuditEvent::MappedMemberSelectedKeyUnique,
            (_, false) => verter_audit::AuditEvent::MappedMemberSelectedKeyRepeated,
        };
        if let Some(obs) = verter_audit::current_observer() {
            obs.record_event(event);
        }
    }

    /// Classify one mapped-binder-ordinal assignment. Records the
    /// `(canonical_id, whole_hash, display_name)` triple → ordinal
    /// mapping; if the same triple has already been assigned a
    /// DIFFERENT ordinal earlier in this request, bumps
    /// `mapped_binder_ordinal_collision` (mapper-identity-instability).
    ///
    /// Mapper-identity-stability gate. A non-zero count
    /// confirms the mapper-identity-instability concern at
    /// `lower.rs:976` — the same mapper source triple receiving
    /// different ordinals from different dispatcher instances will
    /// hash to distinct `SemanticNodeData::TypeParam` interns and
    /// the typed mapped-member cache will MISS on what should be a
    /// HIT.
    pub fn classify_mapper_binder_ordinal(&self, identity: MapperSourceIdentity, ordinal: u16) {
        let mut map = self.mapper_source_ordinals.lock();
        match map.get(&identity) {
            Some(&existing) if existing == ordinal => {
                // Same triple, same ordinal — stable. No-op.
            }
            Some(_) => {
                // Same triple, DIFFERENT ordinal. Mapper-identity
                // instability — record the collision but leave the
                // first-seen ordinal in place so subsequent
                // observations still detect drift.
                drop(map);
                if let Some(obs) = verter_audit::current_observer() {
                    obs.record_event(verter_audit::AuditEvent::MappedBinderOrdinalCollision);
                }
            }
            None => {
                map.insert(identity, ordinal);
            }
        }
    }

    /// Classify one recursive `substitute_with_change_tracking`
    /// entry as unique-or-repeated. The identity tuple
    /// `(node, parameter_node, arg)` matches the existing top-level
    /// `substitute_memo` key composition so the classifier output
    /// is a direct predictor of the recursive memo's hit rate when
    /// engaged.
    ///
    /// Cheap on the hot path: one `Mutex` lock + one
    /// `FxHashSet::insert` + one `AuditEvent` emission. The set is
    /// per-request so classification resets between component-meta
    /// queries.
    pub fn classify_recursive_substitute(&self, identity: RecursiveSubstituteIdentity) {
        let inserted = self.recursive_substitute_seen.lock().insert(identity);
        let event = if inserted {
            verter_audit::AuditEvent::RecursiveSubstituteUnique
        } else {
            verter_audit::AuditEvent::RecursiveSubstituteRepeated
        };
        if let Some(obs) = verter_audit::current_observer() {
            obs.record_event(event);
        }
    }
}

impl RequestContextLike for RequestContext {
    fn request_id(&self) -> u64 {
        self.request_id
    }
    fn capture_enabled(&self) -> bool {
        self.footprint_capture
    }
    fn timing_enabled(&self) -> bool {
        self.timing_capture
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
            verter_audit::AuditEvent::CompileCodeTransformOp => {
                self.compile_code_transform_ops
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::FrontierClosureInvocation => {
                self.frontier_closure_invocations_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::FrontierClosureTargetNone => {
                self.frontier_closure_invocations_target_none
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::FrontierClosureRedundantTargetNonePair => {
                self.frontier_closure_redundant_target_none_pairs
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ResolvedExternalTypeCacheNegativeHit => {
                self.resolved_external_type_cache_negative_hits
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ResolvedExternalTypeCacheNegativeMiss => {
                self.resolved_external_type_cache_negative_misses
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ResolveImportColdPositive => {
                self.resolve_import_cold_positive
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ResolveImportColdNegative => {
                self.resolve_import_cold_negative
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ResolveImportWarmPositive => {
                self.resolve_import_warm_positive
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ResolveImportWarmNegative => {
                self.resolve_import_warm_negative
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::KnownMissRouteServed => {
                self.known_miss_route_served.fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::KnownMissRouteRevalidated => {
                self.known_miss_route_revalidated
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::KnownMissRouteRecomputed => {
                self.known_miss_route_recomputed
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ImportedRegistryCold => {
                self.imported_registry_cold.fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ImportedRegistryWarm => {
                self.imported_registry_warm.fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ImportedRegistryNegative => {
                self.imported_registry_negative
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ImportedRootCold => {
                self.imported_root_cold.fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ImportedRootWarm => {
                self.imported_root_warm.fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::RouteDbBarrelStep => {
                self.route_db_barrel_steps.fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::RouteDbWildcardFanout => {
                self.route_db_wildcard_fanout
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleCold => {
                self.prepared_decl_bundle_cold
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleWarm => {
                self.prepared_decl_bundle_warm
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleRejectEntryMissing => {
                self.prepared_decl_bundle_reject_entry_missing
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleRejectSelfRootUntracked => {
                self.prepared_decl_bundle_reject_self_root_untracked
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleRejectSelfRootHashMismatch => {
                self.prepared_decl_bundle_reject_self_root_hash_mismatch
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleRejectImportRouteAbsent => {
                self.prepared_decl_bundle_reject_import_route_absent
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleRejectImportRouteMismatch => {
                self.prepared_decl_bundle_reject_import_route_mismatch
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleRejectOther => {
                self.prepared_decl_bundle_reject_other
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryTypeOfCold => {
                self.semantic_query_typeof_cold
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryTypeOfWarm => {
                self.semantic_query_typeof_warm
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryInstantiateCold => {
                self.semantic_query_instantiate_cold
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryInstantiateWarm => {
                self.semantic_query_instantiate_warm
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryConditionalCold => {
                self.semantic_query_conditional_cold
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryConditionalWarm => {
                self.semantic_query_conditional_warm
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryMappedTypeCold => {
                self.semantic_query_mapped_type_cold
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryMappedTypeWarm => {
                self.semantic_query_mapped_type_warm
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryIndexedAccessCold => {
                self.semantic_query_indexed_access_cold
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryIndexedAccessWarm => {
                self.semantic_query_indexed_access_warm
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryKeyOfCold => {
                self.semantic_query_keyof_cold
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryKeyOfWarm => {
                self.semantic_query_keyof_warm
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryProjectPathCold => {
                self.semantic_query_project_path_cold
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SemanticQueryProjectPathWarm => {
                self.semantic_query_project_path_warm
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SubstituteTopLevelCall => {
                self.substitute_top_level_calls
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SubstituteMemoHit => {
                self.substitute_memo_hits.fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SubstituteTypeOfOpaque => {
                self.substitute_typeof_opaque
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SubstituteConditionalDescend => {
                self.substitute_conditional_descend
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SubstituteMappedTypeDescend => {
                self.substitute_mapped_type_descend
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::BuildTypeofCall => {
                self.build_typeof_calls.fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::BuildTypeofPreparedValueMiss => {
                self.build_typeof_prepared_value_misses
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::MappedMemberPlainUnique => {
                self.mapped_member_plain_unique
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::MappedMemberPlainRepeated => {
                self.mapped_member_plain_repeated
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::MappedMemberSelectedKeyUnique => {
                self.mapped_member_selected_key_unique
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::MappedMemberSelectedKeyRepeated => {
                self.mapped_member_selected_key_repeated
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleCallsiteScopePayload => {
                self.prepared_decl_bundle_callsite_scope_payload
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleCallsiteBuildInstantiate => {
                self.prepared_decl_bundle_callsite_build_instantiate
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::PreparedDeclBundleCallsiteOther => {
                self.prepared_decl_bundle_callsite_other
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::MappedBinderOrdinalCollision => {
                self.mapped_binder_ordinal_collision
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::RecursiveSubstituteUnique => {
                self.recursive_substitute_unique
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::RecursiveSubstituteRepeated => {
                self.recursive_substitute_repeated
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SubstituteMappedRebuild => {
                self.substitute_mapped_rebuild
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::SubstituteConditionalRebuild => {
                self.substitute_conditional_rebuild
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::RecursiveSubstituteMemoHit => {
                self.recursive_substitute_memo_hits
                    .fetch_add(1, Ordering::Relaxed);
            }
            verter_audit::AuditEvent::ImportedMacroSurfaceProjection => {
                self.imported_macro_surface_projection
                    .fetch_add(1, Ordering::Relaxed);
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

    fn record_lock_acquisition(&self, lock_name: &'static str, wait_ns: u64) {
        // Per-cache totals: keep the legacy named-counter behaviour so
        // existing snapshot consumers (component-meta payload counters)
        // remain populated. Unknown names skip the per-cache bump but
        // still flow into the cross-cache aggregates below.
        match lock_name {
            "node_arena" => {
                self.node_arena_lock_acquisitions
                    .fetch_add(1, Ordering::Relaxed);
            }
            "family_map" => {
                self.family_map_lock_acquisitions
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        // Cross-cache aggregates surfaced via `WaitAudit`. The cumulative
        // wait counter is incremented unconditionally on the assumption
        // the producer already gated its `Instant::now()` capture: when
        // the timing flag is off, `wait_ns == 0` and the `fetch_add`
        // is a no-op-equivalent. The acquisition-count aggregate is
        // bumped on every lock acquisition regardless of timing-capture
        // because the count itself is independent of duration.
        self.lock_wait_ns.fetch_add(wait_ns, Ordering::Relaxed);
        self.lock_acquisitions.fetch_add(1, Ordering::Relaxed);
    }

    fn record_phase_timing(&self, phase: &'static str, elapsed_ms: f64) {
        // Compile-phase boundary timings flow into the per-request
        // accumulators that
        // [`crate::VerterHost::compile_with_audit`] reads when
        // assembling the `CompilePayload`. Other phase names are
        // currently unmodeled — the trait method intentionally
        // returns silently so producers may emit through
        // `current_observer()` without session-side coupling for
        // signals not yet plumbed.
        let micros = (elapsed_ms * 1_000.0).max(0.0) as u64;
        let counter = match phase {
            "compile.parse" => &self.compile_parse_us,
            "compile.transform" => &self.compile_transform_us,
            "compile.codegen" => &self.compile_codegen_us,
            "compile.css_analysis" => &self.compile_css_analysis_us,
            "compile.sourcemap" => &self.compile_sourcemap_us,
            _ => return,
        };
        counter.fetch_add(micros, Ordering::Relaxed);
    }

    fn record_scheduler_dispatch(&self, audit: verter_audit::SchedulerAudit) {
        // Accumulate this dispatch's queue-dwell into the per-request
        // wait-aggregate so `WaitAudit::queue_wait_ns` reflects the
        // full contention cost across all dispatches the request
        // observes (initial + retries). The session-side
        // `RequestContext` is the canonical owner of the per-request
        // scheduler audit; `AuditBuilder::finish` consults the slot
        // and the wait aggregate when building the
        // [`verter_audit::RequestAuditRecord`].
        let dwell_ns = (audit.queue_dwell_ms * 1_000_000.0).max(0.0) as u64;
        self.queue_wait_ns.fetch_add(dwell_ns, Ordering::Relaxed);
        // First dispatch wins on the slot; subsequent dispatches bump
        // the dispatch counter so retries / re-enqueues are visible
        // without overwriting the first-dispatch facts (worker thread
        // id, depths, dwell).
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

/// RAII guard that clears the session-side TLS slots
/// (`CURRENT_REQUEST_CONTEXT` and `CURRENT_ACCUMULATOR`) plus the
/// `verter_audit` substrate's `current_observer()` slot, restoring
/// every prior value on drop. The empty-slot mirror of
/// [`RequestContextGuard::install`] for the substrate + session
/// slots; the scheduler-side opaque slot is cleared by the
/// scheduler's `OpaqueContextGuard::clear`.
///
/// Returned via the registered `ClearTlsHook` and held inside the
/// scheduler's `AllSlotsClearGuard` so the cooperative inline-execute
/// path can clear ALL of the install_tls slots symmetrically when
/// the dispatched job has no `winner_ctx`.
struct SessionAndAuditClearGuard {
    prev_context: Option<Arc<RequestContext>>,
    prev_accumulator: Option<Arc<RequestFootprintAccumulator>>,
    // Drops after the two session slots, restoring the audit observer
    // last so the substrate's slot empties first (mirrors the install
    // direction in `RequestContextGuard::install`).
    _audit_observer_guard: verter_audit::observer::ObserverGuard,
}

impl SessionAndAuditClearGuard {
    fn clear_all() -> Self {
        // Clear the session-side slots; capture prior values for
        // restoration.
        let prev_context = CURRENT_REQUEST_CONTEXT.with(|c| c.replace(None));
        let prev_accumulator = CURRENT_ACCUMULATOR.with(|c| c.replace(None));
        // Clear the audit observer substrate slot.
        let audit_observer_guard = verter_audit::observer::clear_observer();
        Self {
            prev_context,
            prev_accumulator,
            _audit_observer_guard: audit_observer_guard,
        }
    }
}

impl Drop for SessionAndAuditClearGuard {
    fn drop(&mut self) {
        let prev_ctx = self.prev_context.take();
        let prev_acc = self.prev_accumulator.take();
        CURRENT_ACCUMULATOR.with(|c| {
            c.replace(prev_acc);
        });
        CURRENT_REQUEST_CONTEXT.with(|c| {
            c.replace(prev_ctx);
        });
        // `_audit_observer_guard` drops after the two session slots
        // (field-order drop), restoring the audit observer last.
    }
}

impl TlsUninstall for SessionAndAuditClearGuard {
    fn uninstall(self: Box<Self>) {
        // Guard drops via Self's Drop when the box drops.
    }
}

/// Concrete `ClearTlsHook` for the scheduler's substrate-level
/// registry. Returns a boxed `SessionAndAuditClearGuard` whose
/// `Drop` restores the session and audit substrate TLS slots.
///
/// Used by the cooperative inline-execute None-winner_ctx path:
/// the scheduler invokes the registered hook to clear every TLS
/// slot the session's install_tls would have planted, and stores
/// the returned handle so drop restores all of them.
fn clear_session_and_audit_tls_hook() -> Box<dyn TlsUninstall + Send> {
    Box::new(SessionAndAuditClearGuard::clear_all())
}

/// Register the session-side cross-crate "clear TLS" hook with the
/// scheduler's substrate. Idempotent — repeat calls observe that the
/// hook is already registered and silently no-op (handy for test
/// crates that set the host up multiple times).
///
/// Called by the host (`VerterHost::new` / equivalent) at startup so
/// the scheduler's cooperative inline-execute path can clear ALL
/// install_tls slots symmetrically when no `winner_ctx` is supplied
/// — without the hook only the scheduler-side opaque slot would
/// clear, and the outer request's session + audit TLS would bleed
/// into the inner stage.
pub fn install_clear_tls_hook() {
    let _ = verter_scheduler::request_context::register_clear_tls_hook(
        clear_session_and_audit_tls_hook,
    );
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
    fn bump_bare_engine_construction_increments_on_installed_context() {
        // Discriminating: a bare-host `ResolverContext::is_request_bound()`
        // returns false → calling `bump_bare_engine_construction()`
        // under an installed RequestContext must increment the counter.
        // Without an installed context the bump is a noop.
        let ctx = make_ctx(101, false);
        let (_, bare_before, _) = ctx.cache_counters.bypass_diagnostics.snapshot();
        assert_eq!(bare_before, 0, "fresh context starts at 0");
        {
            let _g = RequestContextGuard::install(Arc::clone(&ctx));
            bump_bare_engine_construction();
            bump_bare_engine_construction();
        }
        let (_, bare_after, _) = ctx.cache_counters.bypass_diagnostics.snapshot();
        assert_eq!(
            bare_after, 2,
            "counter must increment twice under an installed context"
        );
        // Outside the guard the bump is a noop.
        bump_bare_engine_construction();
        let (_, bare_outside, _) = ctx.cache_counters.bypass_diagnostics.snapshot();
        assert_eq!(
            bare_outside, 2,
            "bump outside an installed context must be a noop"
        );
    }

    #[test]
    fn bump_resolver_store_view_call_increments_on_installed_context() {
        let ctx = make_ctx(102, false);
        let (_, _, calls_before) = ctx.cache_counters.bypass_diagnostics.snapshot();
        assert_eq!(calls_before, 0, "fresh context starts at 0");
        {
            let _g = RequestContextGuard::install(Arc::clone(&ctx));
            bump_resolver_store_view_call();
            bump_resolver_store_view_call();
            bump_resolver_store_view_call();
        }
        let (_, _, calls_after) = ctx.cache_counters.bypass_diagnostics.snapshot();
        assert_eq!(
            calls_after, 3,
            "counter must increment three times under an installed context"
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

    /// The session-registered "clear all install_tls slots" hook
    /// bridges the scheduler-side inline-execute None-`winner_ctx`
    /// clear path to the session + audit substrate TLS, keeping
    /// every install_tls slot in lock-step on Drop. While the
    /// scheduler-side `AllSlotsClearGuard` is held the hook must
    /// zero `CURRENT_REQUEST_CONTEXT`, `CURRENT_ACCUMULATOR`, and
    /// `verter_audit::current_observer()`; on Drop it must restore
    /// all three.
    ///
    /// The scheduler's per-crate `OpaqueContextGuard::clear` covers
    /// the scheduler opaque slot directly. The session and audit
    /// slots live in `verter_session` / `verter_audit` respectively
    /// and require this registered cross-crate hook to fire — the
    /// scheduler crate has no direct dependency on either crate.
    ///
    /// Discriminator: without the hook (or with the hook returning
    /// a no-op guard), `current_request_context().is_some()` and
    /// `verter_audit::current_observer().is_some()` would still
    /// hold INSIDE the `AllSlotsClearGuard` scope — the test
    /// asserts both are None.
    #[test]
    fn install_clear_tls_hook_clears_session_and_audit_slots_during_all_slots_clear_guard() {
        // Register the hook (idempotent — a previous test or host
        // construction may have already done so).
        install_clear_tls_hook();

        let ctx = make_ctx(42, true);
        let g = RequestContextGuard::install(Arc::clone(&ctx));
        assert!(
            current_request_context().is_some(),
            "outer install must populate session TLS",
        );
        assert!(
            verter_audit::current_observer().is_some(),
            "outer install must populate audit substrate TLS",
        );

        {
            // Clear-all guard: every install_tls slot must be empty
            // for the lifetime of this scope.
            let _clear = verter_scheduler::request_context::AllSlotsClearGuard::clear_all();
            assert!(
                current_request_context().is_none(),
                "AllSlotsClearGuard must clear session request-context TLS",
            );
            assert!(
                current_accumulator().is_none(),
                "AllSlotsClearGuard must clear session accumulator TLS",
            );
            assert!(
                verter_audit::current_observer().is_none(),
                "AllSlotsClearGuard must clear audit observer substrate TLS",
            );
        }

        // Drop restores all three slots.
        assert!(
            current_request_context().is_some(),
            "drop must restore session request-context TLS",
        );
        assert!(
            verter_audit::current_observer().is_some(),
            "drop must restore audit observer substrate TLS",
        );
        drop(g);
    }
}
