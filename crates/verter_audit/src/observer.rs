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
use crate::scheduler::SchedulerAudit;

/// Compact event tag emitted through [`AuditObserver::record_event`].
///
/// Producers prefer the dedicated `record_*` methods over the generic
/// `record_event`; this enum carries counter-style attributions for
/// events without a structured payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditEvent {
    /// One inflight-aborted retry observed in the cold-resolver loop.
    InflightAbortedRetry,
    /// One cold-abort sweep tick.
    ColdAbortSwept,
    /// One `CodeTransform` operation entry observed during compile.
    /// Producers in `verter_compiler::code_transform` emit this at each
    /// public op entry so the session-side observer can populate
    /// [`crate::payloads::compile::CompilePayload::code_transform_ops`]
    /// without bypassing the `CodeTransform` API surface (see CLAUDE.md
    /// §"CodeTransform Is the Single Source of Truth").
    CompileCodeTransformOp,
    /// One invocation of the cross-file external-type frontier closure
    /// (route-graph BFS in `verter_session::host_resolve::frontier_engine`).
    /// Producer increments at the entry of
    /// `run_external_type_frontier_closure_with_view`.
    FrontierClosureInvocation,
    /// Subset of [`Self::FrontierClosureInvocation`] whose final target
    /// was `None` (broken import chain — no exported symbol resolved).
    /// Producer increments AFTER the closure returns `Ok((_, None, _))`.
    FrontierClosureTargetNone,
    /// Subset of [`Self::FrontierClosureTargetNone`] where the
    /// `(owner_canonical, type_name)` pair already returned `None`
    /// earlier in the same audited request. Producer side checks a
    /// per-request set; the FIRST `None` for a pair bumps only
    /// `FrontierClosureTargetNone`, subsequent `None`s bump BOTH.
    FrontierClosureRedundantTargetNonePair,
    /// One warm hit on a host-owned "this `(owner, type)` resolved to
    /// `None`" entry in the resolved-external-type cache. Always `0`
    /// today (the cache only carries positive entries) — non-zero
    /// values indicate negative caching has landed.
    ResolvedExternalTypeCacheNegativeHit,
    /// One miss on a host-owned negative entry in the
    /// resolved-external-type cache. Proxies "we re-walked the
    /// frontier because there was no negative entry to short-circuit".
    /// Producer increments when the cache lookup returns no entry AND
    /// the closure subsequently returned `target = None`.
    ResolvedExternalTypeCacheNegativeMiss,
    /// One cold (cache-miss) import-route resolution that returned a
    /// positive target. Producer at the type-import lookup site.
    ResolveImportColdPositive,
    /// One cold import-route resolution that returned `None` (no
    /// known target — workspace resolver said the specifier is
    /// unresolvable from this owner).
    ResolveImportColdNegative,
    /// One warm import-route resolution served from the host cache /
    /// `IndexedReady.import_routes` snapshot with a positive target.
    ResolveImportWarmPositive,
    /// One warm import-route resolution served from cache with a
    /// negative (known-miss) target.
    ResolveImportWarmNegative,
    /// One import-route lookup that returned an entry the helper
    /// classified as `import_route_is_known_miss` (the route was
    /// previously recorded as unresolvable and is being served from
    /// the known-miss sidecar). Producer at `authoritative_import_route`.
    KnownMissRouteServed,
    /// One known-miss entry that the validator REVALIDATED as still
    /// missing in the current `content_generation` (no new file
    /// appeared to satisfy it). Producer at the
    /// `cached_import_route_resolution` validator branch.
    KnownMissRouteRevalidated,
    /// One known-miss entry that the validator RECOMPUTED because the
    /// `content_generation` advanced past the recorded value (a new
    /// candidate file may now satisfy it). Producer at the
    /// `cached_import_route_resolution` validator branch.
    KnownMissRouteRecomputed,
    /// One cold resolution of an imported registry symbol (cache miss
    /// in `ImportedRegistryDb`). Producer at the cooperative-admission
    /// closure entry in
    /// `ComponentMetaQueryEngine::resolve_imported_registry_symbol`.
    ImportedRegistryCold,
    /// One warm hit on `ImportedRegistryDb` peek. Producer at the
    /// `host_db.peek(...)` branch.
    ImportedRegistryWarm,
    /// One imported-registry resolution that returned `None` (the
    /// symbol could not be resolved at all from the owner). Producer
    /// at the cold closure's negative return.
    ImportedRegistryNegative,
    /// One cold (cache-miss) imported-type-root resolution. Producer
    /// at the `ImportedRootDb::get_or_resolve_returning_facts` closure
    /// entry — the closure body runs only on cache miss.
    ImportedRootCold,
    /// One warm-cache hit on imported-type-root resolution. Producer
    /// at the post-cache-lookup branch where the returned `Some` came
    /// from cache (the closure did not run).
    ImportedRootWarm,
    /// One barrel-export hop traversed during route-frontier
    /// resolution (`export { X } from 'Y'` re-export chain).
    /// Producer at the `route_shallow_state` barrel-edge expansion.
    RouteDbBarrelStep,
    /// One `export *` wildcard fan-out expansion observed during
    /// route-frontier resolution. Producer at the wildcard-route
    /// fanout fuse check site (one per source the wildcard enumerates).
    RouteDbWildcardFanout,
    /// One cold (cache-miss) prepared-decl bundle materialization.
    /// Producer at the singleflight-leader closure entry in
    /// `prepared_decl_bundle_with_store_view`.
    PreparedDeclBundleCold,
    /// One warm prepared-decl bundle cache hit. Producer at the
    /// fast-path `get_if_valid_self_rooted` success branch.
    PreparedDeclBundleWarm,
    /// One prepared-decl bundle warm-read rejection where the cache
    /// `DashMap` carried no entry for the canonical at all. Bumped at
    /// the fast-path validator in `prepared_decl_bundle_with_store_view`
    /// when [`crate::current_observer`] reports an attribution sink.
    PreparedDeclBundleRejectEntryMissing,
    /// One prepared-decl bundle warm-read rejection where the cache
    /// entry's self-root `FileWholeHash` canonical is untracked by the
    /// view (`whole_hashes.get(canonical)` returned `None`).
    PreparedDeclBundleRejectSelfRootUntracked,
    /// One prepared-decl bundle warm-read rejection where the cache
    /// entry's self-root `FileWholeHash` is tracked by the view but
    /// the stored hash differs from `whole_hashes[canonical]`.
    PreparedDeclBundleRejectSelfRootHashMismatch,
    /// One prepared-decl bundle warm-read rejection where the cache
    /// entry's `ImportRoute` `DerivedFactHash` is missing from the
    /// view's `derived_hashes` map (no live ImportRoute snapshot for
    /// this canonical).
    PreparedDeclBundleRejectImportRouteAbsent,
    /// One prepared-decl bundle warm-read rejection where the cache
    /// entry's `ImportRoute` `DerivedFactHash` exists in the view's
    /// `derived_hashes` but the stored hash differs from the live
    /// snapshot.
    PreparedDeclBundleRejectImportRouteMismatch,
    /// One prepared-decl bundle warm-read rejection that did not match
    /// any of the four attributed predicates above. Must stay 0 in
    /// steady state — a non-zero count means the bundle is admitting
    /// fact variants the per-rejection attribution does not yet cover
    /// and the diagnosis is incomplete.
    PreparedDeclBundleRejectOther,
    /// Focused semantic-query counters
    /// --------------------------------
    /// These attribute each `ProjectSemanticDispatch::execute` call by
    /// `SemanticQueryKey` variant + cold-vs-warm so an investigator
    /// can attribute pathological-fixture cost (e.g.
    /// ChatMessages.vue's >30s timeout) to a specific query kind.
    /// Cold = the cold-build closure ran. Warm = the cache served the
    /// result without running the closure. Bumped at the
    /// `execute_cooperative` dispatch in
    /// `ProjectSemanticDispatch::execute_via_cold_build_helper`.
    /// One cold dispatch of a `SemanticQueryKey::TypeOf` query.
    SemanticQueryTypeOfCold,
    /// One warm dispatch of a `SemanticQueryKey::TypeOf` query.
    SemanticQueryTypeOfWarm,
    /// One cold dispatch of a `SemanticQueryKey::Instantiate` query.
    SemanticQueryInstantiateCold,
    /// One warm dispatch of a `SemanticQueryKey::Instantiate` query.
    SemanticQueryInstantiateWarm,
    /// One cold dispatch of a `SemanticQueryKey::Conditional` query.
    SemanticQueryConditionalCold,
    /// One warm dispatch of a `SemanticQueryKey::Conditional` query.
    SemanticQueryConditionalWarm,
    /// One cold dispatch of a `SemanticQueryKey::MappedType` query.
    SemanticQueryMappedTypeCold,
    /// One warm dispatch of a `SemanticQueryKey::MappedType` query.
    SemanticQueryMappedTypeWarm,
    /// One cold dispatch of a `SemanticQueryKey::IndexedAccess` query.
    SemanticQueryIndexedAccessCold,
    /// One warm dispatch of a `SemanticQueryKey::IndexedAccess` query.
    SemanticQueryIndexedAccessWarm,
    /// One cold dispatch of a `SemanticQueryKey::KeyOf` query.
    SemanticQueryKeyOfCold,
    /// One warm dispatch of a `SemanticQueryKey::KeyOf` query.
    SemanticQueryKeyOfWarm,
    /// One cold dispatch of a `SemanticQueryKey::ProjectPath` /
    /// `ProjectMember` query.
    SemanticQueryProjectPathCold,
    /// One warm dispatch of a `SemanticQueryKey::ProjectPath` /
    /// `ProjectMember` query.
    SemanticQueryProjectPathWarm,
    /// One call to
    /// `ProjectSemanticDispatch::substitute_semantic_type_param`.
    /// Bumped at the entry of substitute (NOT the recursive
    /// `substitute_with_change_tracking`) so the count is per top-level
    /// substitution. Pairs with the cache-hit counter below to
    /// distinguish a fresh substitute walk from a memo hit.
    SubstituteTopLevelCall,
    /// One hit on the `substitute_memo_get` fast path in
    /// `substitute_semantic_type_param`. Bumped when the memo collapsed
    /// an identical `(value_expr, parameter_node, arg)` triple.
    SubstituteMemoHit,
    /// One return of `(node, false)` from
    /// `substitute_with_change_tracking`'s `SemanticNodeData::TypeOf`
    /// arm — the substitute did NOT descend into the TypeOf, treating
    /// it as opaque. This is the site for the "opaque TypeOf returns"
    /// counter.
    SubstituteTypeOfOpaque,
    /// One return from `substitute_with_change_tracking`'s
    /// `Conditional` arm that descended into the conditional branches
    /// (a non-identity recursive walk). Distinguishes
    /// "Conditional touched" from "Conditional rebuilt".
    SubstituteConditionalDescend,
    /// One return from `substitute_with_change_tracking`'s
    /// `MappedType` arm that descended into the mapper's
    /// constraint / source / value_expr (a non-identity recursive
    /// walk). This is the site for the "Mapped descents" counter.
    SubstituteMappedTypeDescend,
    /// One call to `build_typeof` (the `typeof`-rooted declaration
    /// lookup at `build.rs:162`). Flagged as a primary cost suspect:
    /// the HIGH-confidence direction.
    BuildTypeofCall,
    /// One `build_typeof` call where the value-root scope returned
    /// `None` from `ensure_indexed_ready` (the prepared-value miss
    /// the brief flags at `build.rs:162`).
    BuildTypeofPreparedValueMiss,
    /// Focused mapped-member materialization counters
    /// -----------------------------------------------
    /// Per-K mapped-member materialization is the measured
    /// hot path: `build_mapped_type` (`build.rs:1968`) +
    /// `synthesise_mapped_surface` (`walk.rs:2550`) iterate
    /// enumerated keys and call into
    /// `materialize_mapped_member_value_for_key` (plain Expanded
    /// path, `build.rs:2113`) or
    /// `materialize_selected_key_mapped_value` (selected-key
    /// publication path, `build.rs:2207`). Both helpers substitute
    /// the mapper binder + evaluate. The instrumentation splits the
    /// two helpers and counts unique vs repeated identity tuples to
    /// confirm Hypothesis A (a typed mapped-member materialization
    /// cache will collapse the K-loop cross product).
    /// One call to `materialize_mapped_member_value_for_key` whose
    /// identity tuple `(mapper.value_expr, mapper.parameter_node,
    /// key_name, mode, demand)` was seen for the FIRST time in the
    /// active request — would NOT be served by a cache (cold).
    /// Producer at the helper's entry in
    /// `materialize_mapped_member_value_for_key`.
    MappedMemberPlainUnique,
    /// One call to `materialize_mapped_member_value_for_key` whose
    /// identity tuple was already seen in the active request —
    /// WOULD be served by a typed mapped-member cache (repeat).
    MappedMemberPlainRepeated,
    /// One call to `materialize_selected_key_mapped_value` /
    /// `materialize_selected_key_mapped_value_with_node` whose
    /// identity tuple was seen for the FIRST time in the active
    /// request — would NOT be served by a cache (cold).
    MappedMemberSelectedKeyUnique,
    /// One call to `materialize_selected_key_mapped_value` /
    /// `materialize_selected_key_mapped_value_with_node` whose
    /// identity tuple was already seen in the active request —
    /// WOULD be served by a typed mapped-member cache (repeat).
    MappedMemberSelectedKeyRepeated,
    /// One call to `prepared_decl_bundle` from
    /// `SessionDispatchHost::scope_payload_for_base`
    /// (`mod.rs:1661`) — the four `DispatchHost` trait callbacks
    /// (`resolve_prepared_type_decl`, `root_identity`,
    /// `utility_source`, `bare_ref_origin`) all route through this
    /// helper. Dominant warm-read attribution for the K-loop hot path.
    PreparedDeclBundleCallsiteScopePayload,
    /// One call to `prepared_decl_bundle` from `build_instantiate`
    /// (`build.rs:495`) — the per-instantiation scope-payload
    /// fetch.
    PreparedDeclBundleCallsiteBuildInstantiate,
    /// One call to `prepared_decl_bundle` from any other site
    /// (residual; must be small for the attribution to be useful).
    PreparedDeclBundleCallsiteOther,
    /// One observed collision where the SAME mapper source AST
    /// (`(canonical_id, whole_hash, display_name)` triple) was
    /// interned at DIFFERENT `mapped_binder_ordinal` values within
    /// a single request — the mapper-identity-instability signal
    /// codex flagged. Non-zero count means the per-dispatcher
    /// counter is destabilising mapper identity across dispatch
    /// instances, preventing the typed cache from collapsing what
    /// SHOULD be cache hits.
    MappedBinderOrdinalCollision,
    /// Focused recursive-substitution counters
    /// ----------------------------------------
    /// Key insight: the recursive helper at
    /// `substitute.rs:99-104`
    /// (`substitute_with_change_tracking`) BYPASSES the top-level
    /// `substitute_memo` even though `(node, parameter_node, arg)`
    /// is a complete identity. Measurement refuted Hypothesis A (the
    /// per-K mapped-member helpers are NOT the bottleneck — they
    /// run 0-1 times). The true cost is
    /// `substitute_with_change_tracking` rebuilding `Mapped` and
    /// `Conditional` nodes (`substitute.rs:408-467` and
    /// `:482-520`) from substituted sub-trees that ARE structurally
    /// identical across calls but carry NEW `SemanticNodeId`s.
    /// These counters classify the recursive entries to confirm
    /// the bottleneck shape BEFORE wiring a recursive memo, and
    /// quantify the memo's hit-rate AFTER.
    /// One recursive-helper entry whose `(node, parameter_node,
    /// arg)` triple was FIRST-SEEN in the active request. Producer
    /// at `substitute_with_change_tracking` entry; before the
    /// recursive memo lands a high `_unique` count is expected.
    /// After the recursive memo lands `_unique` measures the
    /// distinct triple count for the request — the lower bound on
    /// the work the memo could not have collapsed.
    RecursiveSubstituteUnique,
    /// One recursive-helper entry whose `(node, parameter_node,
    /// arg)` triple was already seen in the active request — the
    /// recursive memo SHOULD short-circuit this entry. The memo's
    /// effectiveness on the active request is
    /// `_repeated / (_unique + _repeated)`. Producer at
    /// `substitute_with_change_tracking` entry.
    RecursiveSubstituteRepeated,
    /// One rebuild of a `Mapped` semantic node in
    /// `substitute_with_change_tracking`
    /// (`substitute.rs:408-467`) after one or more descendant
    /// sub-trees actually changed. Distinguishes the "Mapped
    /// rebuilt" hot path from the upstream "Mapped descended"
    /// (`SubstituteMappedTypeDescend`) which counts every visit
    /// regardless of rebuild. Producer at the rebuild branch
    /// `(self.graph().intern_preserving_scope(..., Mapped { ... }))`.
    SubstituteMappedRebuild,
    /// One rebuild of a `Conditional` semantic node in
    /// `substitute_with_change_tracking`
    /// (`substitute.rs:482-520`) after one or more of the
    /// `check`/`extends`/`true_branch_ref`/`false_branch_ref`
    /// sub-trees actually changed. Distinguishes the "Conditional
    /// rebuilt" hot path from the upstream "Conditional descended"
    /// (`SubstituteConditionalDescend`). Producer at the rebuild
    /// branch.
    SubstituteConditionalRebuild,
    /// One hit on the RECURSIVE-helper hash-cons memo (the
    /// memo lookup at the entry of
    /// `substitute_with_change_tracking`, not the top-level
    /// `SubstituteMemoHit` which counts hits at the public
    /// surface). When the recursive memo is engaged this counter
    /// reports its hit count; before the memo wires this counter
    /// stays at 0. Pairs with `RecursiveSubstituteRepeated` —
    /// a non-zero gap between `_repeated` and
    /// `RecursiveSubstituteMemoHit` indicates repeated triples
    /// that the memo failed to serve (e.g. evicted under FIFO
    /// pressure).
    RecursiveSubstituteMemoHit,
    /// One call to a public
    /// [`ImportedMacroSurface`] projection accessor (`resolve_root`,
    /// `project_named_member`, or `enumerate_member_names`).
    ///
    /// Producer: every public dispatch accessor on
    /// `verter_session::resolver_core::ImportedMacroSurface` bumps
    /// this counter exactly once at entry, before any dispatch
    /// work. The counter is the observability hook that lets
    /// later analyses verify whether consumers reach the typed-IR
    /// bridge or a parallel resolution rail. Enumeration
    /// (`enumerate_member_names`) contributes to the same
    /// bridge-demand counter rather than a separate
    /// enumeration-only counter — a finer enumerate-vs-project
    /// split is a future refinement wired alongside consumer
    /// adoption, not yet present.
    ///
    /// Until a consumer adopts the bridge in production the
    /// counter stays at 0 across audited production requests.
    /// The discriminator tests in
    /// `crates/verter_session/tests/imported_macro_surface_bridge.rs`
    /// drive the counter explicitly through hermetic fixtures —
    /// that is the only context where a non-zero value is
    /// expected today.
    ImportedMacroSurfaceProjection,
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

    /// Record one scheduler dispatch fact for the current request.
    ///
    /// Called by the scheduler at every dispatch site that runs an
    /// audited stage. The first call wins on the per-request slot;
    /// subsequent calls bump the dispatch counter on the previously
    /// captured [`SchedulerAudit`]. The session-side `RequestContext`
    /// implements this; the substrate's [`crate::noop::NoOpObserver`]
    /// leaves it as a default no-op.
    fn record_scheduler_dispatch(&self, _audit: SchedulerAudit) {}
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

/// Clear the calling thread's observer slot and capture the prior
/// value for restoration on drop. The empty-slot mirror of
/// [`install_observer`].
///
/// Used by the scheduler's cooperative inline-execute path when the
/// dispatched job has no `winner_ctx`: the inline branch runs on the
/// CALLING worker's thread, so without a clear of the substrate
/// observer slot the outer request's `AuditObserver` would still be
/// visible to lower crates emitting through [`current_observer`],
/// and the inner stage's events would be misattributed to the outer
/// request. Stack-safe — drop restores whatever value the slot held
/// before the clear.
#[must_use]
pub fn clear_observer() -> ObserverGuard {
    let prev = CURRENT_OBSERVER.with(|slot| slot.replace(None));
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
