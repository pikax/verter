//! Canonical export routing facts.
//!
//! Replaces frontier wildcard resolution and export-graph-style routing state.
//! Answers `(module, exported_name) -> defining module + defining symbol | stable miss`.
//!
//! Barrel files get a `BarrelRouteSurface` built lazily on first query — all
//! wildcard specifiers are resolved once. Individual `(barrel, name)` lookups
//! then read the surface in O(1). Route misses are cached as `RouteResult::Miss`.
//!
//! Module-augmentation stitching is owned by `ProjectSemanticDispatch`. The
//! `effective_export_sets` table remains only as the validation backing for the
//! legacy `FactKey::EffectiveExportSet` fact shape; there is no production cold
//! publisher for it. New semantic results observe
//! `ModuleAugmentationIndexShape` and contributor facts directly.
//!
//! Concurrent cold requests for the same barrel surface or route key coalesce
//! via singleflight.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::facts::registry::SymbolSpace;

use crate::file_artifact_store::ProjectIdentity;
#[cfg(any(test, feature = "test-support"))]
use crate::resolver_core::PermissiveStoreView;
use crate::resolver_core::{
    FactVersionRef, ResolverContext, SingleflightGroup, SingleflightRole, SingleflightRunResult,
    StoreView, ValidatedFactCache,
};
use crate::types::Hash16;

mod effective_export_set;

/// Substrate version for the route/barrel resolution algorithm. A bump
/// invalidates every `RouteNameKey` / `BarrelSurfaceKey` slot by changing
/// the key (the entries are still validated value-side by their fact
/// signature; the version is the coarse "the resolver itself changed
/// shape" rail, mirroring `RESOLVED_IMPORT_FACTS_RESOLVER_VERSION`).
///
/// Bumped 1 → 2: resolved routes now retain the exact lexical owner of the
/// defining declaration. A version-1 result cannot distinguish same-name
/// module and instance declarations.
pub const ROUTE_DB_RESOLVER_VERSION: u32 = 2;

/// Query-identity key for a single named-export route lookup
/// `(provider, exported_name)` (R5 query-identity family, R6 content-free,
/// R21 split-env).
///
/// The route resolution is resolve-domain: it depends on `resolve_env_hash`
/// (module resolution / paths / conditions) and `lib_env_hash` (module
/// augmentations stitch into the visible surface), keyed under the owning
/// `project_identity` so a route resolved in project A never satisfies a
/// lookup in project B. `symbol_space` discriminates the type/value
/// namespace. NO content/version hash lives on the key — route freshness
/// rides the value-side `ValidatedFactCache` fact signature, revalidated
/// against the live `StoreView` on every warm hit. `parse_env_hash` /
/// `type_env_hash` do NOT key a route surface (R21 scoping).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteNameKey {
    pub provider_canonical: Arc<str>,
    pub exported_name: Arc<str>,
    pub symbol_space: SymbolSpace,
    pub project_identity: ProjectIdentity,
    pub resolve_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub resolver_version: u32,
}

impl RouteNameKey {
    /// Build a route key, stamping the current [`ROUTE_DB_RESOLVER_VERSION`].
    /// The env axes (`project_identity`, `resolve_env_hash`, `lib_env_hash`)
    /// are sourced by the caller from the host
    /// (`host_view_project_identity_for` / `host_view_env_hashes_for`) so a
    /// single builder serves both the warm lookup and the cold publish.
    #[must_use]
    pub fn new(
        provider_canonical: impl Into<Arc<str>>,
        exported_name: impl Into<Arc<str>>,
        symbol_space: SymbolSpace,
        project_identity: ProjectIdentity,
        resolve_env_hash: Hash16,
        lib_env_hash: Hash16,
    ) -> Self {
        Self {
            provider_canonical: provider_canonical.into(),
            exported_name: exported_name.into(),
            symbol_space,
            project_identity,
            resolve_env_hash,
            lib_env_hash,
            resolver_version: ROUTE_DB_RESOLVER_VERSION,
        }
    }
}

/// Query-identity key for a barrel file's pre-resolved wildcard route
/// surface (R5 query-identity family, R6 content-free, R21 split-env). Same
/// resolve/lib env discipline as [`RouteNameKey`]; a barrel surface is a
/// whole-file surface, so it carries no `symbol_space`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BarrelSurfaceKey {
    pub barrel_canonical: Arc<str>,
    pub project_identity: ProjectIdentity,
    pub resolve_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub resolver_version: u32,
}

impl BarrelSurfaceKey {
    /// Build a barrel-surface key, stamping the current
    /// [`ROUTE_DB_RESOLVER_VERSION`].
    #[must_use]
    pub fn new(
        barrel_canonical: impl Into<Arc<str>>,
        project_identity: ProjectIdentity,
        resolve_env_hash: Hash16,
        lib_env_hash: Hash16,
    ) -> Self {
        Self {
            barrel_canonical: barrel_canonical.into(),
            project_identity,
            resolve_env_hash,
            lib_env_hash,
            resolver_version: ROUTE_DB_RESOLVER_VERSION,
        }
    }
}

pub(crate) use effective_export_set::build_module_augmentation_index_shape_fact_key;
pub use effective_export_set::{
    EffectiveExportEntry, EffectiveExportSetEntry, EffectiveExportSetKey, EffectiveExportSetScope,
};

/// Result of resolving a named export route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteResult {
    /// Route resolved to a defining file and symbol.
    Resolved {
        defining_canonical: String,
        defining_owner: verter_type_expr::TopLevelOwnerId,
        defining_symbol: String,
    },
    /// Stable miss — symbol is not exported by this provider.
    Miss,
}

impl RouteResult {
    pub fn is_miss(&self) -> bool {
        matches!(self, RouteResult::Miss)
    }

    pub fn resolved(&self) -> Option<(&str, verter_type_expr::TopLevelOwnerId, &str)> {
        match self {
            RouteResult::Resolved {
                defining_canonical,
                defining_owner,
                defining_symbol,
            } => Some((defining_canonical, *defining_owner, defining_symbol)),
            RouteResult::Miss => None,
        }
    }
}

/// Pre-resolved wildcard route surface for a barrel file.
///
/// Maps each wildcard `source_specifier` to its resolved `canonical_id`.
/// Built lazily on first barrel query, then reused for all subsequent queries.
///
/// Version rooting lives in `fact_dep_signature` (a sorted, deduplicated
/// list of `FactVersionRef` entries the producer observed while
/// computing the surface). Concurrent file versions of the same
/// `barrel_canonical` coexist as distinct candidates inside the
/// multi-candidate `ValidatedFactCache` slot — each candidate's
/// signature validates against the current `StoreView`.
#[derive(Debug, Clone)]
pub struct BarrelRouteSurface {
    /// The barrel canonical this surface was built for.
    pub barrel_canonical: String,
    /// specifier → canonical_id
    pub wildcard_edges: FxHashMap<String, String>,
    /// Fact dependencies recorded while the surface was built — the
    /// validation signature for this candidate. Multi-candidate cache
    /// slots store one signature per candidate so concurrent file
    /// versions or overlay variants coexist without overwriting each
    /// other.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

/// Cold route-resolve flight value: the resolved route plus its
/// shared-store admission status BY VALUE.
///
/// Retention on the route singleflight mirrors admission: only an
/// ADMITTED resolve (non-empty fact signature, persisted into
/// [`RouteDb::routes`]) is retained as a joinable rendezvous for the
/// burst. An UNADMITTED resolve — the empty-fact-signature carrier: a
/// frontier walk that consumed a fenced (ReturnOnly) serve, or an
/// unrootable-wildcard negative-cache resolve — is served to the
/// leader's own caller only. A burst member that adopted it would
/// receive a possibly-superseded route with neither a fact signature
/// to bubble into its outer tracer nor any ReturnOnly signal on its
/// own request; the by-value `admitted` bit is what lets a committed
/// follower detect that and re-resolve against fresh state.
#[derive(Debug)]
struct RouteFlightOutcome {
    route: Arc<RouteResult>,
    admitted: bool,
}

/// Shared DB for canonical export routing facts.
#[derive(Debug)]
pub struct RouteDb {
    /// [`RouteNameKey`] → route result. The key carries the split env
    /// axes (R21) so a route resolved under one project/env never
    /// satisfies a lookup under another; value-side fact validation
    /// carries content freshness (R6).
    routes: ValidatedFactCache<RouteNameKey, RouteResult>,
    route_singleflight: SingleflightGroup<RouteNameKey, RouteFlightOutcome, ()>,
    /// [`BarrelSurfaceKey`] → full wildcard route surface (lazy, built once).
    barrel_surfaces: ValidatedFactCache<BarrelSurfaceKey, BarrelRouteSurface>,
    barrel_singleflight: SingleflightGroup<BarrelSurfaceKey, Arc<BarrelRouteSurface>, ()>,
    /// Per-provider effective export surface (post-augmentation
    /// stitching) keyed by `(provider, project_identity,
    /// resolve_env_hash, lib_env_hash, session_scope)` (R15 + R21 + R29).
    /// `session_scope` is the CONTENT-FREE [`EffectiveExportSetScope`]
    /// (R6); the overlay-set content fingerprint is matched on the value,
    /// never in this key.
    effective_export_sets: ValidatedFactCache<EffectiveExportSetKey, EffectiveExportSetEntry>,
    /// Test-only provenance counter — bumped each time
    /// [`Self::get_or_resolve_route_observing_facts`] returns through
    /// the warm-hit branch (validated cache lookup succeeded). Pairs
    /// with the cold + coalesced counters so tests can discriminate
    /// which branch satisfied a consumer call.
    #[cfg(any(test, feature = "test-support"))]
    route_warm_fact_bubble_emissions: std::sync::atomic::AtomicU64,
    /// Test-only provenance counter — bumped when
    /// [`Self::get_or_resolve_route_observing_facts`] returned through
    /// the singleflight leader branch (this thread won the cold
    /// resolve and admitted the entry). The freshly-stored facts are
    /// re-read from the validated cache before this counter advances.
    #[cfg(any(test, feature = "test-support"))]
    route_cold_fact_bubble_emissions: std::sync::atomic::AtomicU64,
    /// Test-only provenance counter — bumped when
    /// [`Self::get_or_resolve_route_observing_facts`] returned through
    /// the singleflight follower branch (another thread won the
    /// cold resolve, this thread joined and re-read the just-admitted
    /// facts). Discriminates the coalesced-join path from leader.
    #[cfg(any(test, feature = "test-support"))]
    route_coalesced_fact_bubble_emissions: std::sync::atomic::AtomicU64,
}

impl RouteDb {
    pub fn new() -> Self {
        Self {
            routes: ValidatedFactCache::default(),
            route_singleflight: SingleflightGroup::default(),
            barrel_surfaces: ValidatedFactCache::default(),
            barrel_singleflight: SingleflightGroup::default(),
            effective_export_sets: ValidatedFactCache::default(),
            #[cfg(any(test, feature = "test-support"))]
            route_warm_fact_bubble_emissions: std::sync::atomic::AtomicU64::new(0),
            #[cfg(any(test, feature = "test-support"))]
            route_cold_fact_bubble_emissions: std::sync::atomic::AtomicU64::new(0),
            #[cfg(any(test, feature = "test-support"))]
            route_coalesced_fact_bubble_emissions: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Snapshot the test-only warm fact-bubble emission counter.
    /// Returns the current value with relaxed ordering. Exposed under
    /// `cfg(any(test, feature = "test-support"))` so integration tests in
    /// `tests/` (which compile without `cfg(test)`) can read it.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn route_warm_fact_bubble_emissions(&self) -> u64 {
        self.route_warm_fact_bubble_emissions
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Snapshot the test-only cold fact-bubble emission counter.
    /// Returns the current value with relaxed ordering.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn route_cold_fact_bubble_emissions(&self) -> u64 {
        self.route_cold_fact_bubble_emissions
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Snapshot the test-only coalesced fact-bubble emission counter.
    /// Returns the current value with relaxed ordering.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn route_coalesced_fact_bubble_emissions(&self) -> u64 {
        self.route_coalesced_fact_bubble_emissions
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    // -----------------------------------------------------------------------
    // Route lookups
    // -----------------------------------------------------------------------

    /// Look up a cached route for `key` if valid in the view.
    pub fn get_route<V: StoreView>(
        &self,
        key: &RouteNameKey,
        view: &V,
    ) -> Option<Arc<RouteResult>> {
        let result = self.routes.get_if_valid(key, view);
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .route_db
                    .hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .route_db
                    .misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        result
    }

    /// Permissive route lookup without store-view validation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_route_any(&self, key: &RouteNameKey) -> Option<Arc<RouteResult>> {
        let result = self.routes.get_if_valid(key, &PermissiveStoreView);
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .route_db
                    .hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .route_db
                    .misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        result
    }

    /// Look up or materialize a route for `key` with fact validation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_resolve_route_with_facts<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        host: &crate::VerterHost,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        self.get_or_resolve_route_with_facts_with_context(key, view, host, resolve)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn get_or_resolve_route_with_facts_with_context<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        ctx: &dyn ResolverContext,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        let tracer_host = ctx.host_for_fact_tracer_install();
        crate::fact_signature_helpers::with_cacheability_scope(tracer_host, |probe| {
            self.get_or_resolve_route_with_facts_in_scope(key, view, probe, resolve)
        })
        .0
    }

    #[cfg(any(test, feature = "test-support"))]
    fn get_or_resolve_route_with_facts_in_scope<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        if let Some(result) = self.routes.get_if_valid(&key, view) {
            return Some(result);
        }

        let run_result = self.resolve_route_singleflight_inner(key, view, probe, resolve)?;
        Some(Arc::clone(&run_result.value.route))
    }

    /// Shared singleflight orchestrator for the cold-path route resolve used
    /// by both [`Self::get_or_resolve_route_with_facts`] and
    /// [`Self::get_or_resolve_route_observing_facts`].
    ///
    /// Runs the caller's `resolve` closure under the [`Self::route_singleflight`]
    /// group so concurrent cold lookups for the same `(provider, exported_name)`
    /// key coalesce onto a single materialization. The closure inside the
    /// singleflight first re-checks the validated cache (to absorb races where
    /// another path warmed the entry between the caller's pre-check and
    /// admission), then invokes `resolve()`. On success, the entry is admitted
    /// to [`Self::routes`] under strict admission rules (non-empty fact
    /// signatures only — empty-signature resolves are the never-persisted
    /// carrier: a frontier walk that consumed a fenced (ReturnOnly) serve, or
    /// the negative-cache pattern surfaced from [`Self::get_or_resolve_route`]
    /// — and are returned to the caller without being persisted as a
    /// fact-validated cache hit).
    ///
    /// Retention mirrors admission (the bounded re-validation loop the
    /// IndexedReady and prepared-decl-bundle lanes use): an ADMITTED
    /// outcome is retained as a joinable rendezvous for the burst; an
    /// UNADMITTED outcome serves only the LEADER (ReturnOnly); a FOLLOWER
    /// receives the unadmitted outcome by value and re-runs `resolve`
    /// against fresh state on a fresh lane. Under sustained churn the
    /// bounded fallback adopts the last unadmitted outcome ReturnOnly.
    ///
    /// EVERY unadmitted outcome — leader-produced or follower-adopted —
    /// marks the non-cacheability rail of the thread it is served to.
    /// Both refusal reasons need it, for different halves of the same
    /// hazard:
    ///
    /// - `probe.non_cacheable()` (fenced serve / broken lease /
    ///   unrootable route): the reads that set it already fanned out to
    ///   every tracer on the LEADER's stack, so the leader's re-mark is a
    ///   harmless no-op — but an ADOPTING FOLLOWER never ran that walk,
    ///   and nothing has marked its tracers.
    /// - `facts.is_empty()`: the RESULT is unrootable and NO
    ///   non-cacheable read need have occurred at all, so NEITHER thread
    ///   is marked. `build_named_type_export_route_entry` hand-marks its
    ///   fenced and unrootable-wildcard exits, but its NORMAL exit
    ///   returns whatever the participant walk produced — EMPTY when no
    ///   participant yields a whole-hash or a route-surface hash. An
    ///   empty signature also FANS NOTHING, so an enclosing traced
    ///   compute observes no fact for the route, warm-admits a result
    ///   folding a route it cannot root, and revalidates against the live
    ///   view forever.
    ///
    /// Marking on `!admitted` — rather than per reason — is the
    /// structural floor: no unadmitted value leaves this funnel without
    /// marking the thread that receives it, whatever refused it and
    /// whichever producer supplied it. The producer-side empty-facts
    /// convention is a discipline; this is the floor that does not depend
    /// on a producer remembering it. The mark is cache non-admission
    /// only, never request partiality: the value served is VALID
    /// (Complete).
    ///
    /// Returns `Some(SingleflightRunResult { value, role, .. })` on success
    /// (callers that need to discriminate leader vs follower for provenance
    /// counter bumps inspect `role`), or `None` when the resolve closure
    /// returns `None`.
    fn resolve_route_singleflight_inner<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        resolve: F,
    ) -> Option<SingleflightRunResult<RouteFlightOutcome>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        let flight_body = || {
            if let Some(result) = self.routes.get_if_valid(&key, view) {
                return Ok(RouteFlightOutcome {
                    route: result,
                    admitted: true,
                });
            }
            match resolve() {
                Some((result, facts)) => {
                    let arc = Arc::new(result);
                    // Admission is TWO independent gates, both fail-closed:
                    //
                    // - a non-empty fact signature (an empty one gives a warm
                    //   read nothing to validate against);
                    // - the cacheability verdict of the scope enclosing this
                    //   resolve, sampled AFTER the walk ran. A fenced serve, a
                    //   broken decl-body lease, an unrootable route or an
                    //   unobservable contributor source env consumed anywhere in
                    //   the walk means the route's basis cannot be soundly
                    //   rooted — and three of those four are CONTENT-NEUTRAL, so
                    //   the entry would root on the LIVE hash and validate on
                    //   every warm read forever. The empty-facts convention is a
                    //   producer-side discipline; this gate is the structural
                    //   floor that does not depend on a producer remembering it.
                    //
                    // The route surface is still returned to the caller either
                    // way; only the persist is refused.
                    let admitted = !facts.is_empty() && !probe.non_cacheable();
                    if admitted {
                        self.routes.insert_arc_with_kind(
                            key.clone(),
                            arc.clone(),
                            facts,
                            "route_db.routes",
                        );
                    }
                    // R23 typed event: cold-path route admission.
                    // Fires once per `(provider, exported_name)`
                    // resolution. The `augmented` field is `false`
                    // for the bare-route resolution path; the
                    // post-augmentation-stitched
                    // `EffectiveExportSet` path emits its own
                    // `ExportRouteResolved` with `augmented: true`
                    // when consumers walk its entries.
                    emit_export_route_resolved_event(
                        &key.provider_canonical,
                        &key.exported_name,
                        arc.as_ref(),
                        /* augmented = */ false,
                    );
                    Ok(RouteFlightOutcome {
                        route: arc,
                        admitted,
                    })
                }
                None => Err(()),
            }
        };
        const MAX_FLIGHT_ATTEMPTS: usize = 3;
        let mut last_unadmitted: Option<SingleflightRunResult<RouteFlightOutcome>> = None;
        for _attempt in 0..MAX_FLIGHT_ATTEMPTS {
            let run_result = self
                .route_singleflight
                .run_retaining(key.clone(), view.compat_token(), flight_body, |outcome| {
                    outcome.admitted
                })
                .ok()?;
            if run_result.value.admitted {
                return Some(run_result);
            }
            if matches!(run_result.role, SingleflightRole::Leader) {
                // Unadmitted leader: serve its own caller, and carry the
                // non-cacheability onto that caller's rails.
                //
                // The mark is NOT redundant with "the resolve ran on this
                // thread". That reasoning covers only ONE of the two refusal
                // reasons. `admitted = !facts.is_empty() && !probe.non_cacheable()`:
                //
                // - `probe.non_cacheable()` — the walk consumed a fenced serve /
                //   broken lease / unrootable route. Each of those fanned out to
                //   EVERY tracer on this thread's stack at the point of the read,
                //   before the funnel ever sampled the probe. Re-marking here is a
                //   harmless no-op (the rail is a bool).
                // - `facts.is_empty()` — the RESULT is unrootable. NO non-cacheable
                //   read need have occurred: `build_named_type_export_route_entry`
                //   marks its fenced and unrootable-wildcard exits by hand, but its
                //   NORMAL exit returns whatever `append_route_participant_fact_versions`
                //   produced — and that is EMPTY when no participant yields either a
                //   whole-hash or a route-surface hash (an evicted provider with no
                //   resolvable surface). An empty signature FANS NOTHING, so the
                //   enclosing traced compute observes no fact for the route at all,
                //   warm-admits a result folding a route it cannot root, and
                //   revalidates against the live view forever — nothing moved.
                //
                // Marking on `!admitted` (rather than on the empty-facts reason
                // alone) is the structural floor: no unadmitted value leaves this
                // funnel without marking the thread that receives it, whatever
                // reason refused it and whichever producer supplied it — the
                // producer-side empty-facts convention is a discipline, this is the
                // floor that does not depend on a producer remembering it. This is a
                // VALID (Complete) route, NOT a partial result — cache non-admission
                // only, never request partiality.
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::UnrootableRoute,
                );
                return Some(run_result);
            }
            last_unadmitted = Some(run_result);
        }
        if last_unadmitted.is_some() {
            // Sustained-churn bounded fallback (FOLLOWER adoption): the
            // adopted route is unadmitted — fenced-derived or unrootable
            // — and this thread never ran the resolve that produced it.
            // Carry the non-cacheability by hand so an enclosing traced
            // cold compute refuses shared-cache admission of any result
            // folding a route it cannot root. This is a VALID (Complete)
            // adopted route, NOT a partial result — cache non-admission
            // only, never request partiality.
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::UnrootableRoute,
            );
        }
        last_unadmitted
    }

    /// **Test-only.** Strong-reference count of the in-flight route
    /// singleflight [`FlightState`] for `(provider, name)` under
    /// `view`'s compat token, or `0` if no flight is registered.
    ///
    /// A leader parked inside its resolve closure holds the leader-only
    /// baseline of 2 (its local `state` + the `flights` map entry); a
    /// follower that has joined and is committed to the condvar wait
    /// raises the count to 3. Tests poll this to deterministically
    /// observe follower admission onto the singleflight before releasing
    /// the leader — replacing the wall-clock sleep that previously raced
    /// the follower's registration.
    ///
    /// Exposed under `cfg(any(test, feature = "test-support"))` so integration
    /// tests in `tests/` (which compile without `cfg(test)`) can call it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_route_inflight_strong_count<V: StoreView + ?Sized>(
        &self,
        key: &RouteNameKey,
        view: &V,
    ) -> usize {
        self.route_singleflight
            .test_flight_strong_count(key, view.compat_token())
    }

    /// **Test-only.** Run `f` while a participation pin is held on the
    /// route singleflight lane for `key` — modelling a concurrent burst
    /// sibling whose in-flight claim keeps the lane alive across the
    /// leader's publish.
    ///
    /// That sibling pin is what makes the RETENTION decision observable
    /// at all. With no other pin on the lane, a lone follower's own
    /// unpin reaps the lane the instant it reads the leader's terminal,
    /// so its next bounded attempt re-elects a fresh leader whether the
    /// terminal was retained or discarded — the two are indistinguishable
    /// from the consumer loop. Holding a pin keeps a RETAINED
    /// `Done(unadmitted)` joinable across the claimant's bounded
    /// attempts, which is the state in which adopting an unrooted route
    /// becomes observable.
    ///
    /// Exposed under `cfg(any(test, feature = "test-support"))` so integration
    /// tests in `tests/` (which compile without `cfg(test)`) can reach
    /// the private singleflight group. The pin is released when `f`
    /// returns.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_with_pinned_route_lane<V, R>(
        &self,
        key: RouteNameKey,
        view: &V,
        f: impl FnOnce() -> R,
    ) -> R
    where
        V: StoreView + ?Sized,
    {
        let _pin = self
            .route_singleflight
            .participate(key, view.compat_token());
        f()
    }

    /// Look up a route and return both the result and its recorded
    /// fact-dep signature. Returns `None` on a cold miss or if the
    /// candidate's signature fails validation against `view`.
    ///
    /// Callers that need to bubble the route's dependencies into an
    /// active outer tracer scope (R28 fact-bubble-up) use this variant
    /// instead of [`Self::get_route`] so the facts are visible without a
    /// second round-trip through the cache.
    pub fn get_route_with_facts<V: StoreView + ?Sized>(
        &self,
        key: &RouteNameKey,
        view: &V,
    ) -> Option<(Arc<RouteResult>, Arc<[FactVersionRef]>)> {
        self.routes.get_if_valid_with_facts(key, view)
    }

    /// Look up or materialize a route, and bubble its fact-dep signature
    /// into any active tracer on the current thread.
    ///
    /// On a warm hit the cached fact-dep signature is fanned out via
    /// [`crate::fact_signature_helpers::observe_fact_signature`] before
    /// returning. On a cold miss the inner `resolve` closure is invoked
    /// inside a singleflight group; after resolution the freshly-stored
    /// facts are read back and also fanned out. When a concurrent thread
    /// joins the singleflight (follower role), the joiner's re-read picks
    /// up the leader's just-published facts and fans them into the joiner
    /// thread's outer tracer scope so cross-thread fact bubbling holds
    /// for coalesced consumers.
    ///
    /// This method is the consumer-facing entry-point: callers that need
    /// the route's fact-dep signature to participate in outer
    /// `with_fact_tracer` scopes use it instead of [`Self::get_route`]
    /// or [`Self::get_or_resolve_route_with_facts`] (the latter remain
    /// available as low-level primitives for intra-RouteDb code).
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_resolve_route_observing_facts<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        host: &crate::VerterHost,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        self.get_or_resolve_route_observing_facts_with_context(key, view, host, resolve)
    }

    pub(crate) fn get_or_resolve_route_observing_facts_with_context<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        ctx: &dyn ResolverContext,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        let tracer_host = ctx.host_for_fact_tracer_install();
        crate::fact_signature_helpers::with_cacheability_scope(tracer_host, |probe| {
            self.get_or_resolve_route_observing_facts_in_scope(key, view, probe, resolve)
        })
        .0
    }

    fn get_or_resolve_route_observing_facts_in_scope<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        // Warm-hit fast path: validated cache lookup with fact bubbling.
        if let Some((value, facts)) = self.get_route_with_facts(&key, view) {
            crate::fact_signature_helpers::observe_fact_signature(&facts);
            #[cfg(any(test, feature = "test-support"))]
            self.route_warm_fact_bubble_emissions
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(value);
        }

        // Cold path: delegate to the shared singleflight helper, then
        // observe the leader / follower role and bump the matching
        // provenance counter on the post-admission re-read.
        let run_result =
            self.resolve_route_singleflight_inner(key.clone(), view, probe, resolve)?;

        // Post-admission re-read: fan the just-stored facts into the
        // current thread's tracer stack. Leader: the closure ran here
        // and admitted; the re-read finds the freshly-stored entry.
        // Follower: another thread won the singleflight and admitted;
        // this thread's re-read picks up the admitted entry and the
        // bubble fans the leader's facts into this thread's outer
        // tracer scope.
        if let Some((_value, facts)) = self.get_route_with_facts(&key, view) {
            crate::fact_signature_helpers::observe_fact_signature(&facts);
            #[cfg(any(test, feature = "test-support"))]
            match run_result.role {
                SingleflightRole::Leader => {
                    self.route_cold_fact_bubble_emissions
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                SingleflightRole::Follower => {
                    self.route_coalesced_fact_bubble_emissions
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        Some(Arc::clone(&run_result.value.route))
    }

    /// Test-only: drive [`Self::get_or_resolve_route_with_facts`] the way a
    /// production producer does — inside a REAL cacheability tracer scope
    /// opened around the whole resolve.
    ///
    /// A `CacheabilityProbe` cannot be forged (private field, one constructor),
    /// so these wrappers are not an escape hatch around the admission contract:
    /// they ARE the contract, spelled for a test that has no surrounding
    /// producer. A test whose resolve consumes a non-cacheable read is refused
    /// admission here exactly as production is.
    #[cfg(test)]
    fn get_or_resolve_route_with_facts_probe_for_test<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        self.get_or_resolve_route_with_facts(key, view, test_host(), resolve)
    }

    /// Test-only traced sibling of [`Self::get_or_build_barrel_surface`].
    #[cfg(test)]
    fn get_or_build_barrel_surface_probe_for_test<V, F>(
        &self,
        key: BarrelSurfaceKey,
        view: &V,
        build: F,
    ) -> Option<Arc<BarrelRouteSurface>>
    where
        V: StoreView,
        F: FnOnce() -> Option<BarrelRouteSurface>,
    {
        self.get_or_build_barrel_surface(key, view, test_host(), build)
    }

    /// Insert a pre-resolved route. **Test-only**: the empty-facts variant
    /// admits entries that would warm under any [`StoreView`] — production
    /// paths must use [`Self::insert_route_with_facts`].
    #[cfg(test)]
    pub fn insert_route(&self, key: RouteNameKey, result: RouteResult) {
        self.routes.insert(key, result, Vec::new());
    }

    /// Insert a pre-resolved route with explicit fact validation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_route_with_facts(
        &self,
        key: RouteNameKey,
        result: RouteResult,
        facts: Vec<FactVersionRef>,
    ) {
        self.routes.insert(key, result, facts);
    }

    /// Evict all routes for a provider, across every env / project / symbol-
    /// space variant of the provider canonical (the typed keys carry those
    /// dims, so the eviction snapshot-filters by `provider_canonical` rather
    /// than removing a single bare-string key).
    pub fn evict_provider(&self, provider_canonical: &str) {
        let route_keys: Vec<_> = self
            .routes
            .snapshot_all()
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| key.provider_canonical.as_ref() == provider_canonical)
            .collect();
        for key in route_keys {
            self.routes.remove(&key);
        }

        // Barrel surfaces are now keyed by the typed `BarrelSurfaceKey`
        // (env dims included), so a bare-string `.remove` can no longer
        // address them — snapshot-filter by barrel canonical across every
        // env variant, mirroring the effective-export-set eviction below.
        let barrel_keys: Vec<_> = self
            .barrel_surfaces
            .snapshot_all()
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| key.barrel_canonical.as_ref() == provider_canonical)
            .collect();
        for key in barrel_keys {
            self.barrel_surfaces.remove(&key);
        }

        // Evict every effective-export-set candidate for this
        // provider across all `(project, resolve_env, lib_env)` keys.
        let effective_keys: Vec<_> = self
            .effective_export_sets
            .snapshot_all()
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| key.provider_canonical == provider_canonical)
            .collect();
        for key in effective_keys {
            self.effective_export_sets.remove(&key);
        }
    }

    // -----------------------------------------------------------------------
    // Barrel surface lookups
    // -----------------------------------------------------------------------

    /// Look up a cached barrel surface if valid in the view.
    pub fn get_barrel_surface<V: StoreView>(
        &self,
        key: &BarrelSurfaceKey,
        view: &V,
    ) -> Option<Arc<BarrelRouteSurface>> {
        self.barrel_surfaces.get_if_valid(key, view)
    }

    /// Look up or build a barrel surface.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_build_barrel_surface<V, F>(
        &self,
        key: BarrelSurfaceKey,
        view: &V,
        host: &crate::VerterHost,
        build: F,
    ) -> Option<Arc<BarrelRouteSurface>>
    where
        V: StoreView,
        F: FnOnce() -> Option<BarrelRouteSurface>,
    {
        crate::fact_signature_helpers::with_cacheability_scope(host, |probe| {
            self.get_or_build_barrel_surface_in_scope(key, view, probe, build)
        })
        .0
    }

    #[cfg(any(test, feature = "test-support"))]
    fn get_or_build_barrel_surface_in_scope<V, F>(
        &self,
        key: BarrelSurfaceKey,
        view: &V,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        build: F,
    ) -> Option<Arc<BarrelRouteSurface>>
    where
        V: StoreView,
        F: FnOnce() -> Option<BarrelRouteSurface>,
    {
        if let Some(surface) = self.barrel_surfaces.get_if_valid(&key, view) {
            return Some(surface);
        }

        let flight = self
            .barrel_singleflight
            .run(key.clone(), view.compat_token(), || {
                if let Some(surface) = self.barrel_surfaces.get_if_valid(&key, view) {
                    return Ok(surface);
                }
                match build() {
                    Some(surface) => {
                        let arc = Arc::new(surface);
                        let facts = self.barrel_validation_facts(&arc);
                        // Strict admission, TWO fail-closed gates: a non-empty
                        // fact-dep signature (an empty one gives a warm read
                        // nothing to validate against), AND the cacheability
                        // verdict of the enclosing scope, sampled after the
                        // build ran — a barrel surface built over a fenced
                        // serve / broken lease / unrootable route cannot be
                        // soundly rooted, and those reasons are content-neutral,
                        // so the entry would validate forever. The surface is
                        // still returned; only the persist is refused.
                        if !facts.is_empty() && !probe.non_cacheable() {
                            self.barrel_surfaces.insert_arc_with_kind(
                                key.clone(),
                                arc.clone(),
                                facts,
                                "route_db.barrel_surfaces",
                            );
                        }
                        Ok(arc)
                    }
                    None => Err(()),
                }
            });

        match flight {
            Ok(run_result) => Some((*run_result.value).clone()),
            Err(()) => None,
        }
    }

    /// Insert a pre-built barrel surface under `key`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_barrel_surface(&self, key: BarrelSurfaceKey, surface: BarrelRouteSurface) {
        let facts = self.barrel_validation_facts(&surface);
        self.barrel_surfaces.insert(key, surface, facts);
    }

    /// R20 instrumentation: total `signature_overflow_count` across
    /// every backing `ValidatedFactCache` on this `RouteDb`. A non-
    /// zero value means a producer flattened transitive facts where
    /// it should have folded a downstream materialiser's
    /// `semantic_hash`. The pre-canary + final canary both assert
    /// this stays at 0 over the steady-state loop.
    #[must_use]
    pub fn signature_overflow_count(&self) -> u64 {
        self.routes.signature_overflow_count()
            + self.barrel_surfaces.signature_overflow_count()
            + self.effective_export_sets.signature_overflow_count()
    }

    /// R20 instrumentation: total `admission_refused_count` across
    /// every backing `ValidatedFactCache` on this `RouteDb`.
    /// Producers that admit via the loose `insert_arc` path keep
    /// this counter at 0; only strict-mode admissions via
    /// `insert_arc_with_kind` advance it.
    #[must_use]
    pub fn admission_refused_count(&self) -> u64 {
        self.routes.admission_refused_count()
            + self.barrel_surfaces.admission_refused_count()
            + self.effective_export_sets.admission_refused_count()
    }

    // -----------------------------------------------------------------------
    // Clearing
    // -----------------------------------------------------------------------

    /// Clear all cached routes, barrel surfaces, and effective export
    /// sets.
    pub fn clear(&self) {
        self.routes.clear();
        self.route_singleflight.clear();
        self.barrel_surfaces.clear();
        self.barrel_singleflight.clear();
        self.effective_export_sets.clear();
        // Reset the test-only fact-bubble provenance counters so each
        // test sees a clean baseline after a host-wide clear.
        #[cfg(any(test, feature = "test-support"))]
        {
            self.route_warm_fact_bubble_emissions
                .store(0, std::sync::atomic::Ordering::Relaxed);
            self.route_cold_fact_bubble_emissions
                .store(0, std::sync::atomic::Ordering::Relaxed);
            self.route_coalesced_fact_bubble_emissions
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    // -----------------------------------------------------------------------
    // Fact construction
    // -----------------------------------------------------------------------

    /// Return the cached `fact_dep_signature` for a barrel surface as
    /// a fresh `Vec<FactVersionRef>` suitable for re-admission into a
    /// downstream `ValidatedFactCache`.
    ///
    /// Contract: the signature is already the
    /// validation oracle for the surface — it was finalised at
    /// admission time. This helper exists for callers that need to
    /// thread the existing signature into a higher-tier
    /// `insert_arc(..., facts)` call (the `ValidatedFactCache` API
    /// takes `Vec<FactVersionRef>`, not the immutable `Arc<[...]>`
    /// the candidate stores). For warm-hit observation onto the
    /// active tracer use `observe_borrowed_signature(...)` instead.
    #[cfg(any(test, feature = "test-support"))]
    fn barrel_validation_facts(&self, surface: &BarrelRouteSurface) -> Vec<FactVersionRef> {
        surface.fact_dep_signature.as_ref().to_vec()
    }
}

impl Default for RouteDb {
    fn default() -> Self {
        Self::new()
    }
}

/// Emit a typed
/// [`StructuredAuditEvent::ExportRouteResolved`] for the cold-path
/// route admission. Silent no-op when no audit accumulator is
/// installed on the active thread. `Miss` results never emit —
/// only resolved routes carry an attribution.
fn emit_export_route_resolved_event(
    provider_canonical: &str,
    exported_name: &str,
    result: &RouteResult,
    augmented: bool,
) {
    if let RouteResult::Resolved {
        defining_canonical,
        defining_symbol,
        ..
    } = result
    {
        crate::host_manage::push_structured_event(
            crate::component_meta_audit::StructuredAuditEvent::ExportRouteResolved {
                provider_canonical: Arc::<str>::from(provider_canonical),
                exported_name: Arc::<str>::from(exported_name),
                resolved_canonical: Arc::<str>::from(defining_canonical.as_str()),
                resolved_source_name: Arc::<str>::from(defining_symbol.as_str()),
                augmented,
            },
        );
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for RouteDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ResolverState, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.clear();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for RouteDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        // Routes are keyed on (resolver_owner_canonical, specifier);
        // a content edit on a provider canonical evicts every route
        // routed through that provider via `evict_provider`. Returns
        // 0 because the underlying primitive does not surface a count;
        // the cascade outcome is verified via the per-DB unit tests.
        self.evict_provider(canonical_id);
        0
    }
}

/// Resolver context used by in-crate cache unit tests. The DB owns the tracer
/// scope in tests and production alike.
#[cfg(test)]
fn test_host() -> &'static crate::VerterHost {
    static TEST_SCOPE_HOST: std::sync::OnceLock<crate::VerterHost> = std::sync::OnceLock::new();
    TEST_SCOPE_HOST
        .get_or_init(|| crate::VerterHost::new_standalone(crate::types::HostConfig::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::{FactVersionRef, StoreView, StoreViewCompatToken};

    #[derive(Debug)]
    struct TestView {
        token: StoreViewCompatToken,
    }

    impl TestView {
        fn accepting_all(token: u64) -> Self {
            Self {
                token: StoreViewCompatToken {
                    epoch: token,
                    session: None,
                    validity_fingerprint: 0,
                },
            }
        }
    }

    impl StoreView for TestView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }

        fn validates(&self, _fact: &FactVersionRef) -> bool {
            true // Accept all facts in tests.
        }
    }

    /// Build a zero-env route key. These unit tests exercise the cache /
    /// singleflight mechanics in isolation, not env discrimination (that
    /// is covered by the R21 guard in
    /// `tests/cases/g_cache/r6_r21_query_identity_keys.rs`), so a uniform zero
    /// env keeps the focus on route behaviour. Zero `ProjectIdentity` is
    /// permitted here because this is a `#[cfg(test)]` block.
    fn rk(provider: &str, name: &str) -> RouteNameKey {
        RouteNameKey::new(
            provider,
            name,
            SymbolSpace::Type,
            ProjectIdentity([0u8; 16]),
            [0u8; 16],
            [0u8; 16],
        )
    }

    /// Build a zero-env barrel-surface key (see [`rk`]).
    fn bk(barrel: &str) -> BarrelSurfaceKey {
        BarrelSurfaceKey::new(barrel, ProjectIdentity([0u8; 16]), [0u8; 16], [0u8; 16])
    }

    #[test]
    fn insert_and_get_route() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        db.insert_route(
            rk("index.ts", "Foo"),
            RouteResult::Resolved {
                defining_canonical: "foo.ts".to_owned(),
                defining_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                defining_symbol: "Foo".to_owned(),
            },
        );

        let result = db.get_route(&rk("index.ts", "Foo"), &view);
        assert!(result.is_some());
        let route = result.unwrap();
        assert!(
            matches!(&*route, RouteResult::Resolved { defining_canonical, .. } if defining_canonical == "foo.ts")
        );
    }

    #[test]
    fn route_resolver_v1_entry_is_rejected_by_v2_key() {
        const PREVIOUS_ROUTE_RESOLVER_VERSION: u32 = 1;
        assert_eq!(
            ROUTE_DB_RESOLVER_VERSION,
            PREVIOUS_ROUTE_RESOLVER_VERSION + 1
        );

        let db = RouteDb::new();
        let view = TestView::accepting_all(1);
        let current = rk("index.ts", "OwnerExact");
        let stale = RouteNameKey {
            resolver_version: PREVIOUS_ROUTE_RESOLVER_VERSION,
            ..current.clone()
        };
        db.insert_route(stale.clone(), RouteResult::Miss);

        assert!(db.get_route(&stale, &view).is_some());
        assert!(
            db.get_route(&current, &view).is_none(),
            "the owner-exact v2 key rejects a v1 route value"
        );

        db.insert_route(current.clone(), RouteResult::Miss);
        assert!(
            db.get_route(&current, &view).is_some(),
            "a v2 route value roundtrips under the current key"
        );
    }

    #[test]
    fn miss_is_cached() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        db.insert_route(rk("index.ts", "Missing"), RouteResult::Miss);

        let result = db.get_route(&rk("index.ts", "Missing"), &view);
        assert!(result.is_some());
        assert!(result.unwrap().is_miss());
    }

    #[test]
    fn get_or_resolve_route_caches_result() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);
        let call_count = std::sync::atomic::AtomicU32::new(0);

        // Strict admission contract: the zero-facts
        // `get_or_resolve_route` helper does NOT cache its result
        // because the empty signature would refuse strict admission.
        // Callers that want a cached entry must thread a non-empty
        // fact signature through `get_or_resolve_route_with_facts`,
        // as demonstrated below.
        let dummy_fact = FactVersionRef::FileWholeHash {
            canonical_id: "bar.ts".to_owned(),
            hash: [0u8; 16],
        };
        let result =
            db.get_or_resolve_route_with_facts_probe_for_test(rk("index.ts", "Bar"), &view, || {
                call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some((
                    RouteResult::Resolved {
                        defining_canonical: "bar.ts".to_owned(),
                        defining_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        defining_symbol: "Bar".to_owned(),
                    },
                    vec![dummy_fact.clone()],
                ))
            });
        assert!(result.is_some());

        // Second call should hit cache because we admitted with a
        // non-empty fact signature on the first pass.
        let result2 =
            db.get_or_resolve_route_with_facts_probe_for_test(rk("index.ts", "Bar"), &view, || {
                call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some((RouteResult::Miss, vec![dummy_fact.clone()]))
            });
        assert!(result2.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn get_or_resolve_route_with_empty_facts_does_not_cache() {
        // Strict-admission discrimination: a resolve that returns an EMPTY
        // fact signature — the exact shape `build_named_type_export_route_entry`
        // produces for a route it cannot root — must NOT admit a cache entry.
        // The second call re-invokes the resolver because the first skipped
        // admission.
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let resolve_unrootable = || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some((
                RouteResult::Resolved {
                    defining_canonical: "bar.ts".to_owned(),
                    defining_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    defining_symbol: "Bar".to_owned(),
                },
                Vec::new(),
            ))
        };

        let result = db.get_or_resolve_route_with_facts_probe_for_test(
            rk("index.ts", "Bar"),
            &view,
            resolve_unrootable,
        );
        assert!(
            result.is_some(),
            "refusal keeps the VALUE — an unrootable route is still served to its caller"
        );
        let result2 = db.get_or_resolve_route_with_facts_probe_for_test(
            rk("index.ts", "Bar"),
            &view,
            resolve_unrootable,
        );
        assert!(result2.is_some());
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "Empty-fact route resolves are not cached under strict \
             admission; the second call MUST re-invoke the resolver. \
             A non-empty fact signature is what opts a resolve into caching."
        );
    }

    /// ReturnOnly never publishes — route-singleflight rendezvous arm.
    /// A resolve the strict admission refused to persist (the
    /// empty-fact-signature carrier: the fenced-walk arm of
    /// `build_named_type_export_route_entry`, or an unrootable-wildcard
    /// negative-cache resolve) must NOT stay behind as a joinable
    /// `Done` rendezvous: a late claimant on the still-pinned lane
    /// would adopt a possibly-superseded route with neither a fact
    /// signature to bubble nor any ReturnOnly signal on its own
    /// request. Retention must mirror admission.
    #[test]
    fn unadmitted_route_resolve_is_not_retained_as_a_joinable_rendezvous() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);
        let key = rk("provider.ts", "Foo");

        // A burst sibling's participation pin keeps the lane alive past
        // the leader's completion — the window in which a late claimant
        // could join a retained `Done`.
        let _pin = db
            .route_singleflight
            .participate(key.clone(), view.compat_token());

        let resolves = std::sync::atomic::AtomicU32::new(0);
        let superseded = RouteResult::Resolved {
            defining_canonical: "superseded.ts".to_owned(),
            defining_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            defining_symbol: "Foo".to_owned(),
        };
        let live = RouteResult::Resolved {
            defining_canonical: "live.ts".to_owned(),
            defining_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            defining_symbol: "Foo".to_owned(),
        };
        let live_fact = FactVersionRef::FileWholeHash {
            canonical_id: "live.ts".to_owned(),
            hash: [7u8; 16],
        };

        // Call 1: the resolve returns the never-persisted empty-facts
        // shape (the carrier the fenced frontier walk produces). The
        // caller is still served its own result.
        let first = db.get_or_resolve_route_with_facts_probe_for_test(
            rk("provider.ts", "Foo"),
            &view,
            || {
                resolves.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some((superseded.clone(), Vec::new()))
            },
        );
        assert_eq!(
            first.as_deref(),
            Some(&superseded),
            "the leader's own caller is still served the unadmitted route",
        );

        // Call 2 (a late claimant on the pinned lane): must NOT adopt
        // the unadmitted result — it re-resolves cold against fresh
        // state and its admitted result serves warm afterwards.
        let second = db.get_or_resolve_route_with_facts_probe_for_test(
            rk("provider.ts", "Foo"),
            &view,
            || {
                resolves.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some((live.clone(), vec![live_fact.clone()]))
            },
        );
        assert_eq!(
            resolves.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "a late claimant must re-run its own resolve instead of \
             adopting the unadmitted (empty-facts) route as a retained \
             rendezvous",
        );
        assert_eq!(
            second.as_deref(),
            Some(&live),
            "the late claimant must return its own fresh resolve's route",
        );
        assert_eq!(
            db.get_route(&rk("provider.ts", "Foo"), &view).as_deref(),
            Some(&live),
            "the late claimant's admitted re-resolve must serve warm",
        );
    }

    #[test]
    fn barrel_surface_insert_and_get() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        let surface = BarrelRouteSurface {
            barrel_canonical: "barrel.ts".to_owned(),
            wildcard_edges: {
                let mut m = FxHashMap::default();
                m.insert("./foo".to_owned(), "foo.ts".to_owned());
                m.insert("./bar".to_owned(), "bar.ts".to_owned());
                m
            },
            fact_dep_signature: Arc::from(
                vec![
                    FactVersionRef::FileWholeHash {
                        canonical_id: "barrel.ts".to_owned(),
                        hash: [1; 16],
                    },
                    FactVersionRef::FileWholeHash {
                        canonical_id: "foo.ts".to_owned(),
                        hash: [2; 16],
                    },
                    FactVersionRef::FileWholeHash {
                        canonical_id: "bar.ts".to_owned(),
                        hash: [3; 16],
                    },
                ]
                .into_boxed_slice(),
            ),
        };

        db.insert_barrel_surface(bk("barrel.ts"), surface);

        let result = db.get_barrel_surface(&bk("barrel.ts"), &view);
        assert!(result.is_some());
        let s = result.unwrap();
        assert_eq!(s.wildcard_edges.len(), 2);
        assert_eq!(s.wildcard_edges.get("./foo").unwrap(), "foo.ts");
    }

    #[test]
    fn get_or_build_barrel_surface_caches() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let result = db.get_or_build_barrel_surface_probe_for_test(bk("barrel.ts"), &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(BarrelRouteSurface {
                barrel_canonical: "barrel.ts".to_owned(),
                wildcard_edges: FxHashMap::default(),
                fact_dep_signature: Arc::from(
                    vec![FactVersionRef::FileWholeHash {
                        canonical_id: "barrel.ts".to_owned(),
                        hash: [1; 16],
                    }]
                    .into_boxed_slice(),
                ),
            })
        });
        assert!(result.is_some());

        let result2 = db.get_or_build_barrel_surface_probe_for_test(bk("barrel.ts"), &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        });
        assert!(result2.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn clear_removes_all() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        db.insert_route(
            rk("a.ts", "X"),
            RouteResult::Resolved {
                defining_canonical: "x.ts".to_owned(),
                defining_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                defining_symbol: "X".to_owned(),
            },
        );
        db.insert_barrel_surface(
            bk("b.ts"),
            BarrelRouteSurface {
                barrel_canonical: "b.ts".to_owned(),
                wildcard_edges: FxHashMap::default(),
                fact_dep_signature: Arc::from(
                    vec![FactVersionRef::FileWholeHash {
                        canonical_id: "b.ts".to_owned(),
                        hash: [1; 16],
                    }]
                    .into_boxed_slice(),
                ),
            },
        );

        db.clear();

        assert!(db.get_route(&rk("a.ts", "X"), &view).is_none());
        assert!(db.get_barrel_surface(&bk("b.ts"), &view).is_none());
    }
}
