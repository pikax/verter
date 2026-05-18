//! Canonical export routing facts.
//!
//! Replaces frontier wildcard resolution and export-graph-style routing state.
//! Answers `(module, exported_name) -> defining module + defining symbol | stable miss`.
//!
//! Barrel files get a `BarrelRouteSurface` built lazily on first query — all
//! wildcard specifiers are resolved once. Individual `(barrel, name)` lookups
//! then read the surface in O(1). Route misses are cached as `RouteResult::Miss`.
//!
//! `EffectiveExportSet` cold-path computation stitches module augmentations
//! into the resolved export surface for a provider canonical. The
//! `effective_export_sets` sister table caches the post-augmentation
//! result keyed by `(provider, project_identity, resolve_env_hash,
//! lib_env_hash)` (R21 — route surface depends on libs because module
//! augmentations live in libs).
//!
//! Concurrent cold requests for the same barrel surface or route key coalesce
//! via singleflight.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::facts::registry::{InternedName, SymbolSpace};

use crate::file_artifact_store::{
    AugmentationTargetKey, AugmentationTargetKind, AugmenterEntry, AugmenterSet, FileArtifactKey,
    FileArtifactStore, ProjectIdentity,
};
use crate::resolver_core::{
    FactVersionRef, PermissiveStoreView, RouteSurfaceFactRef, SingleflightGroup, SingleflightRole,
    SingleflightRunResult, StoreView, ValidatedFactCache,
};
use crate::types::Hash16;

/// Result of resolving a named export route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteResult {
    /// Route resolved to a defining file and symbol.
    Resolved {
        defining_canonical: String,
        defining_symbol: String,
    },
    /// Stable miss — symbol is not exported by this provider.
    Miss,
}

impl RouteResult {
    pub fn is_miss(&self) -> bool {
        matches!(self, RouteResult::Miss)
    }

    pub fn resolved(&self) -> Option<(&str, &str)> {
        match self {
            RouteResult::Resolved {
                defining_canonical,
                defining_symbol,
            } => Some((defining_canonical, defining_symbol)),
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

/// Key for the per-provider effective export surface (R29 + R21).
///
/// Identifies one effective surface scoped to a `(provider, project,
/// resolve env, lib env)` quadruple. `lib_env_hash` enters this key
/// because module augmentations live in libs / ambient corpora and
/// can change which augmenters are visible — see R21 scoping rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectiveExportSetKey {
    /// Canonical id of the provider whose surface this is.
    pub provider_canonical: String,
    /// Project identity dimension (R21).
    pub project_identity: ProjectIdentity,
    /// Resolve-env hash dimension (R21).
    pub resolve_env_hash: Hash16,
    /// Lib-env hash dimension (R21).
    pub lib_env_hash: Hash16,
}

/// One contribution from an augmenter into a provider's effective
/// export surface.
///
/// Equality + hash are by `(augmented_name, space, contributor_canonical)`
/// so a downstream cache that hashes the entry can detect order-stable
/// changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectiveExportEntry {
    /// Augmented name added by the contributor.
    pub augmented_name: InternedName,
    /// Symbol space of the contribution.
    pub space: SymbolSpace,
    /// Canonical id of the file that emitted this augmentation.
    pub contributor_canonical: Arc<str>,
}

/// Cached effective export set after augmentation stitching (R29 +
/// G1).
///
/// `entries` is sorted by `(augmented_name, space, contributor_canonical)`
/// so the post-stitch surface is determinate under augmenter set
/// reordering. `fact_dep_signature` records the
/// `ModuleAugmentationIndexShape` fact for the queried target plus
/// per-augmenter file-version anchors — invalidating the consumer when
/// the augmenter set changes (G1) OR when one augmenter's content
/// changes.
#[derive(Debug, Clone)]
pub struct EffectiveExportSetEntry {
    /// Stitched effective contributions sorted by
    /// `(augmented_name, space, contributor_canonical)`.
    pub entries: Arc<[EffectiveExportEntry]>,
    /// Number of augmenters that contributed to this surface.
    pub augmenter_count: u32,
    /// Fingerprint of the augmenter set at stitch time. The
    /// downstream consumer's `fact_dep_signature` records a
    /// `RouteSurfaceFactRef::ModuleAugmentationIndexShape` carrying
    /// this hash as `expected_hash`.
    pub augmenter_set_fingerprint: Hash16,
    /// Fact-dep signature for this candidate. Multi-candidate cache
    /// slots store one signature per candidate.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

/// Shared DB for canonical export routing facts.
#[derive(Debug)]
pub struct RouteDb {
    /// `(provider_canonical, exported_name)` → route result.
    routes: ValidatedFactCache<(String, String), RouteResult>,
    route_singleflight: SingleflightGroup<(String, String), Arc<RouteResult>, ()>,
    /// `barrel_canonical` → full wildcard route surface (lazy, built once).
    barrel_surfaces: ValidatedFactCache<String, BarrelRouteSurface>,
    barrel_singleflight: SingleflightGroup<String, Arc<BarrelRouteSurface>, ()>,
    /// Per-provider effective export surface (post-augmentation
    /// stitching) keyed by `(provider, project_identity,
    /// resolve_env_hash, lib_env_hash)` (R15 + R21 + R29).
    effective_export_sets: ValidatedFactCache<EffectiveExportSetKey, EffectiveExportSetEntry>,
    effective_export_singleflight:
        SingleflightGroup<EffectiveExportSetKey, Arc<EffectiveExportSetEntry>, ()>,
    /// Test-only provenance counter — bumped each time
    /// [`Self::get_or_resolve_route_observing_facts`] returns through
    /// the warm-hit branch (validated cache lookup succeeded). Pairs
    /// with the cold + coalesced counters so tests can discriminate
    /// which branch satisfied a consumer call.
    #[cfg(any(test, debug_assertions))]
    route_warm_fact_bubble_emissions: std::sync::atomic::AtomicU64,
    /// Test-only provenance counter — bumped when
    /// [`Self::get_or_resolve_route_observing_facts`] returned through
    /// the singleflight leader branch (this thread won the cold
    /// resolve and admitted the entry). The freshly-stored facts are
    /// re-read from the validated cache before this counter advances.
    #[cfg(any(test, debug_assertions))]
    route_cold_fact_bubble_emissions: std::sync::atomic::AtomicU64,
    /// Test-only provenance counter — bumped when
    /// [`Self::get_or_resolve_route_observing_facts`] returned through
    /// the singleflight follower branch (another thread won the
    /// cold resolve, this thread joined and re-read the just-admitted
    /// facts). Discriminates the coalesced-join path from leader.
    #[cfg(any(test, debug_assertions))]
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
            effective_export_singleflight: SingleflightGroup::default(),
            #[cfg(any(test, debug_assertions))]
            route_warm_fact_bubble_emissions: std::sync::atomic::AtomicU64::new(0),
            #[cfg(any(test, debug_assertions))]
            route_cold_fact_bubble_emissions: std::sync::atomic::AtomicU64::new(0),
            #[cfg(any(test, debug_assertions))]
            route_coalesced_fact_bubble_emissions: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Snapshot the test-only warm fact-bubble emission counter.
    /// Returns the current value with relaxed ordering. Exposed under
    /// `cfg(any(test, debug_assertions))` so integration tests in
    /// `tests/` (which compile without `cfg(test)`) can read it.
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn route_warm_fact_bubble_emissions(&self) -> u64 {
        self.route_warm_fact_bubble_emissions
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Snapshot the test-only cold fact-bubble emission counter.
    /// Returns the current value with relaxed ordering.
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn route_cold_fact_bubble_emissions(&self) -> u64 {
        self.route_cold_fact_bubble_emissions
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Snapshot the test-only coalesced fact-bubble emission counter.
    /// Returns the current value with relaxed ordering.
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn route_coalesced_fact_bubble_emissions(&self) -> u64 {
        self.route_coalesced_fact_bubble_emissions
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    // -----------------------------------------------------------------------
    // Route lookups
    // -----------------------------------------------------------------------

    /// Look up a cached route for `(provider, name)` if valid in the view.
    pub fn get_route<V: StoreView>(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        view: &V,
    ) -> Option<Arc<RouteResult>> {
        let key = (provider_canonical.to_owned(), exported_name.to_owned());
        let result = self.routes.get_if_valid(&key, view);
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
    pub fn get_route_any(
        &self,
        provider_canonical: &str,
        exported_name: &str,
    ) -> Option<Arc<RouteResult>> {
        let key = (provider_canonical.to_owned(), exported_name.to_owned());
        let result = self.routes.get_if_valid(&key, &PermissiveStoreView);
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

    /// Look up or materialize a route for `(provider, name)`.
    pub fn get_or_resolve_route<V, F>(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        view: &V,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView,
        F: FnOnce() -> Option<RouteResult>,
    {
        self.get_or_resolve_route_with_facts(provider_canonical, exported_name, view, || {
            resolve().map(|result| (result, Vec::new()))
        })
    }

    /// Look up or materialize a route for `(provider, name)` with fact validation.
    pub fn get_or_resolve_route_with_facts<V, F>(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        view: &V,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView,
        F: FnOnce() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        let key = (provider_canonical.to_owned(), exported_name.to_owned());

        if let Some(result) = self.routes.get_if_valid(&key, view) {
            return Some(result);
        }

        let run_result = self.resolve_route_singleflight_inner(key, view, resolve)?;
        Some((*run_result.value).clone())
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
    /// signatures only — empty-signature resolves are the negative-cache
    /// pattern surfaced from [`Self::get_or_resolve_route`] and are returned
    /// to the caller without being persisted as a fact-validated cache hit).
    ///
    /// Returns `Some(SingleflightRunResult { value, role, .. })` on success
    /// (callers that need to discriminate leader vs follower for provenance
    /// counter bumps inspect `role`), or `None` when the resolve closure
    /// returns `None`.
    fn resolve_route_singleflight_inner<V, F>(
        &self,
        key: (String, String),
        view: &V,
        resolve: F,
    ) -> Option<SingleflightRunResult<Arc<RouteResult>>>
    where
        V: StoreView,
        F: FnOnce() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        self.route_singleflight
            .run(key.clone(), view.compat_token(), || {
                if let Some(result) = self.routes.get_if_valid(&key, view) {
                    return Ok(result);
                }
                match resolve() {
                    Some((result, facts)) => {
                        let arc = Arc::new(result);
                        // Strict admission. Routes resolved with
                        // non-empty fact signatures admit through the
                        // strict entry-point; empty-signature resolves
                        // are the negative-cache pattern
                        // (`get_or_resolve_route` passes `Vec::new()`)
                        // and are NOT admitted — the route surface is
                        // still returned to the caller, but the entry is
                        // not persisted as a fact-validated cache hit.
                        if !facts.is_empty() {
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
                            &key.0,
                            &key.1,
                            arc.as_ref(),
                            /* augmented = */ false,
                        );
                        Ok(arc)
                    }
                    None => Err(()),
                }
            })
            .ok()
    }

    /// Look up a route and return both the result and its recorded
    /// fact-dep signature. Returns `None` on a cold miss or if the
    /// candidate's signature fails validation against `view`.
    ///
    /// Callers that need to bubble the route's dependencies into an
    /// active outer tracer scope (R28 fact-bubble-up) use this variant
    /// instead of [`Self::get_route`] so the facts are visible without a
    /// second round-trip through the cache.
    pub fn get_route_with_facts<V: StoreView>(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        view: &V,
    ) -> Option<(Arc<RouteResult>, Arc<[FactVersionRef]>)> {
        let key = (provider_canonical.to_owned(), exported_name.to_owned());
        self.routes.get_if_valid_with_facts(&key, view)
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
    pub fn get_or_resolve_route_observing_facts<V, F>(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        view: &V,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView,
        F: FnOnce() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        // Warm-hit fast path: validated cache lookup with fact bubbling.
        if let Some((value, facts)) =
            self.get_route_with_facts(provider_canonical, exported_name, view)
        {
            crate::fact_signature_helpers::observe_fact_signature(&facts);
            #[cfg(any(test, debug_assertions))]
            self.route_warm_fact_bubble_emissions
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(value);
        }

        // Cold path: delegate to the shared singleflight helper, then
        // observe the leader / follower role and bump the matching
        // provenance counter on the post-admission re-read.
        let key = (provider_canonical.to_owned(), exported_name.to_owned());
        let run_result = self.resolve_route_singleflight_inner(key, view, resolve)?;

        // Post-admission re-read: fan the just-stored facts into the
        // current thread's tracer stack. Leader: the closure ran here
        // and admitted; the re-read finds the freshly-stored entry.
        // Follower: another thread won the singleflight and admitted;
        // this thread's re-read picks up the admitted entry and the
        // bubble fans the leader's facts into this thread's outer
        // tracer scope.
        if let Some((_value, facts)) =
            self.get_route_with_facts(provider_canonical, exported_name, view)
        {
            crate::fact_signature_helpers::observe_fact_signature(&facts);
            #[cfg(any(test, debug_assertions))]
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
        Some((*run_result.value).clone())
    }

    /// Insert a pre-resolved route. **Test-only**: the empty-facts variant
    /// admits entries that would warm under any [`StoreView`] — production
    /// paths must use [`Self::insert_route_with_facts`].
    #[cfg(test)]
    pub fn insert_route(
        &self,
        provider_canonical: String,
        exported_name: String,
        result: RouteResult,
    ) {
        let key = (provider_canonical, exported_name);
        self.routes.insert(key, result, Vec::new());
    }

    /// Insert a pre-resolved route with explicit fact validation.
    pub fn insert_route_with_facts(
        &self,
        provider_canonical: String,
        exported_name: String,
        result: RouteResult,
        facts: Vec<FactVersionRef>,
    ) {
        let key = (provider_canonical, exported_name);
        self.routes.insert(key, result, facts);
    }

    /// Evict all routes for a provider.
    pub fn evict_provider(&self, provider_canonical: &str) {
        let route_keys: Vec<_> = self
            .routes
            .snapshot_all()
            .into_iter()
            .map(|(key, _)| key)
            .filter(|(provider, _)| provider == provider_canonical)
            .collect();
        for key in route_keys {
            self.routes.remove(&key);
        }

        self.barrel_surfaces.remove(&provider_canonical.to_owned());

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
        barrel_canonical: &str,
        view: &V,
    ) -> Option<Arc<BarrelRouteSurface>> {
        self.barrel_surfaces
            .get_if_valid(&barrel_canonical.to_owned(), view)
    }

    /// Look up or build a barrel surface.
    pub fn get_or_build_barrel_surface<V, F>(
        &self,
        barrel_canonical: &str,
        view: &V,
        build: F,
    ) -> Option<Arc<BarrelRouteSurface>>
    where
        V: StoreView,
        F: FnOnce() -> Option<BarrelRouteSurface>,
    {
        let key = barrel_canonical.to_owned();

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
                        // Strict admission. Barrel surfaces with a
                        // non-empty fact-dep signature admit through
                        // the strict entry-point; an empty signature
                        // (no dependency facts to validate against)
                        // skips admission rather than caching a
                        // phantom-fact entry.
                        if !facts.is_empty() {
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

    /// Insert a pre-built barrel surface.
    pub fn insert_barrel_surface(&self, surface: BarrelRouteSurface) {
        let key = surface.barrel_canonical.clone();
        let facts = self.barrel_validation_facts(&surface);
        self.barrel_surfaces.insert(key, surface, facts);
    }

    // ────────────────────────────────────────────────────────────
    // Effective export set (post-augmentation stitching) — R29 + G1
    // ────────────────────────────────────────────────────────────

    /// Warm-hit lookup for an effective export surface, validated
    /// against the current view. Returns the cached entry if the
    /// recorded `fact_dep_signature` still holds; otherwise `None`
    /// (caller routes through [`Self::get_or_compute_effective_export_set`]
    /// for the cold path).
    pub fn get_effective_export_set<V: StoreView>(
        &self,
        key: &EffectiveExportSetKey,
        view: &V,
    ) -> Option<Arc<EffectiveExportSetEntry>> {
        self.effective_export_sets.get_if_valid(key, view)
    }

    /// Look up or compute the effective export surface for a provider
    /// under the given env, stitching module augmentations from the
    /// host's augmentation index.
    ///
    /// `target` classifies the queried specifier into one of the four
    /// `AugmentationTargetKind` archetypes (R29). The cold path:
    ///
    /// 1. Builds `AugmentationTargetKey` from `key` + `target`.
    /// 2. Calls
    ///    [`FileArtifactStore::ensure_augmentation_index_populated`]
    ///    to materialise the augmenter set (and emit a
    ///    `ModuleAugmentationIndexShape` event on first install).
    /// 3. Iterates each augmenter's parse-domain
    ///    [`ModuleAugmentationFact`] entries that match the target,
    ///    stitches `(augmented_name, space)` contributions into the
    ///    effective set sorted by
    ///    `(augmented_name, space, contributor_canonical)`.
    /// 4. Records a
    ///    [`FactVersionRef::RouteSurface`] entry for
    ///    [`FactKey::ModuleAugmentationIndexShape`] (with
    ///    `expected_hash = AugmenterSet.fingerprint`) so a future
    ///    augmenter-set change invalidates the consumer (per G1),
    ///    plus a per-contributor `FileWholeHash` so an edit to an
    ///    augmenting file's content also invalidates the consumer.
    /// 5. Emits a typed
    ///    [`StructuredAuditEvent::ModuleAugmentationStitched`] audit
    ///    event for the cold compute.
    ///
    /// `resolve_relative_canonical` is the caller-supplied resolver
    /// hook used for the `ResolvedRelativeCanonical` target archetype.
    pub fn get_or_compute_effective_export_set<V, FH, RR>(
        &self,
        key: EffectiveExportSetKey,
        target: AugmentationTargetKind,
        view: &V,
        artifact_store: &FileArtifactStore,
        contributor_whole_hash: FH,
        resolve_relative_canonical: RR,
    ) -> Arc<EffectiveExportSetEntry>
    where
        V: StoreView,
        FH: Fn(&str) -> Option<Hash16>,
        RR: Fn(&str, &str) -> Option<Arc<str>>,
    {
        // Fail-closed base-only guard. The augmentation index
        // (`ModuleAugmentationIndex` on `FileArtifactStore`) is keyed by
        // a base resolve-domain identity with no base/session
        // discriminator — its cold scan and refresh path filter to base
        // (`FileArtifactKey::is_legacy`) artifacts. A session that
        // overlays a `declare module` block would observe the BASE
        // augmenter set, not its own, so an `EffectiveExportSet`
        // computed under a session view is base content presented as
        // session-correct. Until the augmentation index gains
        // artifact-population identity (the future block that wires the
        // live session `EffectiveExportSet` consumer), this surface is
        // base-only: a session call must be observably an error rather
        // than silent wrong data.
        assert!(
            view.compat_token().session.is_none(),
            "EffectiveExportSet is base-only: get_or_compute_effective_export_set \
             rejects a session view (compat_token().session.is_some()). The \
             augmentation index has no base/session population identity, so a \
             session call would observe the base augmenter set, not the \
             session's overlay. Augmentation stitching gains session support in \
             the block that adds the overlay-aware augmentation-index schema."
        );

        if let Some(existing) = self.effective_export_sets.get_if_valid(&key, view) {
            return existing;
        }

        let flight =
            self.effective_export_singleflight
                .run(key.clone(), view.compat_token(), || {
                    if let Some(existing) = self.effective_export_sets.get_if_valid(&key, view) {
                        return Ok(existing);
                    }

                    let augmentation_target_key = AugmentationTargetKey {
                        project_identity: key.project_identity,
                        resolve_env_hash: key.resolve_env_hash,
                        lib_env_hash: key.lib_env_hash,
                        target: target.clone(),
                    };
                    let augmenter_set = artifact_store.ensure_augmentation_index_populated(
                        &augmentation_target_key,
                        &resolve_relative_canonical,
                    );

                    // Stitch each augmenter's contributions for the
                    // queried target. The augmenter's `.augmentations`
                    // are re-fetched by the EXACT `FileArtifactKey`
                    // captured at index-population time
                    // (`get_artifacts(&key)`), never a content-agnostic
                    // canonical-only scan: with lazy cache invalidation
                    // a stale pre-edit version of the augmenter can
                    // linger alongside the current one, and a
                    // canonical-only scan could surface a different
                    // version than the one the augmenter-set
                    // fingerprint was computed over.
                    //
                    // Stale-key self-heal. The captured exact key can
                    // still go stale: when an augmenter is reparsed
                    // under a NEW `FileArtifactKey` but its
                    // `parse_stable_hash` is unchanged (a member-body
                    // edit that leaves the decl skeleton intact), the
                    // augmenter-set fingerprint does not move, so the
                    // cached `AugmenterSet` is not invalidated and its
                    // `AugmenterEntry.artifact_key` keeps pointing at
                    // the PRE-edit content hash. A same-canonical edit
                    // routed through `FileArtifactStore::insert` drains
                    // that pre-edit key, so `get_artifacts` on the
                    // captured key misses. Silently dropping the
                    // augmenter there would shrink the stitched surface
                    // while keeping the (now wrong) fingerprint/count.
                    // On a miss we therefore re-derive the augmenter's
                    // CURRENT exact key from the scheduler-authoritative
                    // content hash (`contributor_whole_hash`, the same
                    // scheduler oracle the per-contributor `FileWholeHash`
                    // anchors below are built from — never a
                    // content-agnostic `get_artifacts_any` /
                    // `content_hash_for_canonical` scan) and read with
                    // that. Refreshed keys are written back into the
                    // cached `AugmenterSet` after the loop so subsequent
                    // reads hit the fast exact-key path.
                    let mut stitched: Vec<EffectiveExportEntry> = Vec::new();
                    let mut refreshed_keys: Vec<(usize, FileArtifactKey)> = Vec::new();
                    for (idx, augmenter) in augmenter_set.entries.iter().enumerate() {
                        let augmenter_canonical = augmenter.canonical();
                        let art = match artifact_store.get_artifacts(&augmenter.artifact_key) {
                            Some(art) => Some(art),
                            None => {
                                // Refresh: re-derive the current exact
                                // key from scheduler authority and read
                                // pinned to it.
                                contributor_whole_hash(augmenter_canonical.as_ref())
                                    .map(|current_hash| {
                                        FileArtifactKey::legacy(
                                            Arc::clone(augmenter_canonical),
                                            current_hash,
                                        )
                                    })
                                    .and_then(|current_key| {
                                        let refreshed = artifact_store.get_artifacts(&current_key);
                                        if refreshed.is_some() {
                                            refreshed_keys.push((idx, current_key));
                                        }
                                        refreshed
                                    })
                            }
                        };
                        // A genuine miss after the current-key re-fetch
                        // (the augmenter's `IndexedReady` is not
                        // materialised under its current content hash)
                        // is a principled skip — never a content-agnostic
                        // fallback scan.
                        let Some(art) = art else {
                            continue;
                        };
                        for fact in art.augmentations.iter() {
                            if !crate::file_artifact_store::augmenter_matches_target(
                                fact,
                                &augmentation_target_key,
                                augmenter_canonical.as_ref(),
                                &resolve_relative_canonical,
                            ) {
                                continue;
                            }
                            stitched.push(EffectiveExportEntry {
                                augmented_name: fact.augmented_name.clone(),
                                space: fact.space,
                                contributor_canonical: Arc::clone(augmenter_canonical),
                            });
                        }
                    }

                    // Write any refreshed exact keys back into the
                    // cached `AugmenterSet`. The augmenter-set
                    // fingerprint is folded over `parse_stable_hash`
                    // values (never `content_hash`), and a stale key is
                    // only refreshed when the augmenter's
                    // `parse_stable_hash` is unchanged — so the rebuilt
                    // set carries the SAME fingerprint and the same
                    // per-entry `parse_stable_hash` values; only the
                    // `artifact_key` content-hash dimension advances.
                    // Re-publishing under the identical fingerprint
                    // keeps every recorded `ModuleAugmentationIndexShape`
                    // signature valid while making the next stitch hit
                    // the fast exact-key path.
                    let augmenter_set = if refreshed_keys.is_empty() {
                        augmenter_set
                    } else {
                        let mut entries = augmenter_set.entries.clone();
                        for (idx, current_key) in refreshed_keys {
                            entries[idx] = AugmenterEntry {
                                artifact_key: current_key,
                                parse_stable_hash: entries[idx].parse_stable_hash,
                            };
                        }
                        let refreshed_set = Arc::new(AugmenterSet {
                            entries,
                            fingerprint: augmenter_set.fingerprint,
                        });
                        artifact_store.populate_augmenter_set(
                            augmentation_target_key.clone(),
                            Arc::clone(&refreshed_set),
                        );
                        refreshed_set
                    };
                    stitched.sort_by(|a, b| {
                        a.augmented_name
                            .as_ref()
                            .cmp(b.augmented_name.as_ref())
                            .then_with(|| compare_symbol_space(a.space, b.space))
                            .then_with(|| {
                                a.contributor_canonical
                                    .as_ref()
                                    .cmp(b.contributor_canonical.as_ref())
                            })
                    });

                    // Build the validation signature: the
                    // augmentation-index-shape fact + per-contributor
                    // file-whole-hash anchors.
                    let mut facts: Vec<FactVersionRef> = Vec::new();
                    facts.push(FactVersionRef::RouteSurface(RouteSurfaceFactRef {
                        canonical_id: key.provider_canonical.clone(),
                        key: build_module_augmentation_index_shape_fact_key(&target),
                        lane: verter_semantic::facts::FactLane::Semantic,
                        expected_hash: augmenter_set.fingerprint,
                    }));
                    for augmenter in augmenter_set.entries.iter() {
                        let augmenter_canonical = augmenter.canonical();
                        if let Some(hash) = contributor_whole_hash(augmenter_canonical.as_ref()) {
                            facts.push(FactVersionRef::FileWholeHash {
                                canonical_id: augmenter_canonical.as_ref().to_owned(),
                                hash,
                            });
                        }
                    }
                    let signature: Arc<[FactVersionRef]> =
                        Arc::from(facts.clone().into_boxed_slice());

                    let augmenter_count = augmenter_set.entries.len() as u32;
                    let entry = Arc::new(EffectiveExportSetEntry {
                        entries: Arc::from(stitched.into_boxed_slice()),
                        augmenter_count,
                        augmenter_set_fingerprint: augmenter_set.fingerprint,
                        fact_dep_signature: signature,
                    });
                    // Strict admission. The `EffectiveExportSet`
                    // cold-build always pushes at least the
                    // `ModuleAugmentationIndexShape` fact above, so
                    // `facts` is non-empty by construction here —
                    // strict admission is unconditionally safe.
                    self.effective_export_sets.insert_arc_with_kind(
                        key.clone(),
                        Arc::clone(&entry),
                        facts,
                        "route_db.effective_export_sets",
                    );

                    // Emit the cold-path audit event.
                    emit_module_augmentation_stitched_event(
                        &target,
                        augmenter_count,
                        augmenter_set.fingerprint,
                    );

                    Ok(entry)
                });

        match flight {
            Ok(run_result) => (*run_result.value).clone(),
            Err(()) => {
                // Singleflight returned Err only when the inner
                // closure does — our closure always Ok's. This arm
                // remains as a defensive fall-through so the type
                // signature stays infallible to callers.
                Arc::new(EffectiveExportSetEntry {
                    entries: Arc::from(Vec::<EffectiveExportEntry>::new().into_boxed_slice()),
                    augmenter_count: 0,
                    augmenter_set_fingerprint: [0u8; 16],
                    fact_dep_signature: Arc::from(Vec::<FactVersionRef>::new().into_boxed_slice()),
                })
            }
        }
    }

    /// Insert a pre-built `EffectiveExportSetEntry` directly. Test-only
    /// helper for asserting cache-state assumptions without driving a
    /// full cold compute.
    #[cfg(test)]
    pub fn insert_effective_export_set(
        &self,
        key: EffectiveExportSetKey,
        entry: EffectiveExportSetEntry,
        facts: Vec<FactVersionRef>,
    ) {
        self.effective_export_sets.insert(key, entry, facts);
    }

    /// Number of slots in the effective-export-set table.
    #[must_use]
    pub fn effective_export_set_len(&self) -> usize {
        self.effective_export_sets.len()
    }

    /// View-free permissive lookup of the cached
    /// [`EffectiveExportSetEntry::augmenter_set_fingerprint`] for
    /// the supplied key. Used by the route-surface-domain validator
    /// (R26) so a consumer that recorded
    /// `RouteSurfaceFactRef::EffectiveExportSet` with
    /// `expected_hash = augmenter_set_fingerprint` can revalidate on
    /// read without re-entering the view's `validates` dispatch
    /// (the validator itself runs inside the view, so calling the
    /// view-aware `get_effective_export_set` would recurse).
    ///
    /// Returns `Some(fingerprint)` when an entry exists for the
    /// composed `(provider, project, resolve_env, lib_env)` slot,
    /// `None` otherwise. Multi-candidate slots return the
    /// last-admitted candidate's fingerprint (consistent with
    /// `ValidatedFactCache::snapshot_all` last-writer-wins
    /// semantics for permissive reads).
    #[must_use]
    pub fn lookup_effective_export_set_fingerprint(
        &self,
        key: &EffectiveExportSetKey,
    ) -> Option<Hash16> {
        self.effective_export_sets
            .lookup_any_candidate(key)
            .map(|entry| entry.augmenter_set_fingerprint)
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
        self.effective_export_singleflight.clear();
        // Reset the test-only fact-bubble provenance counters so each
        // test sees a clean baseline after a host-wide clear.
        #[cfg(any(test, debug_assertions))]
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
    fn barrel_validation_facts(&self, surface: &BarrelRouteSurface) -> Vec<FactVersionRef> {
        surface.fact_dep_signature.as_ref().to_vec()
    }
}

impl Default for RouteDb {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the parse-domain `FactKey::ModuleAugmentationIndexShape`
/// payload that an `EffectiveExportSet` consumer observes for the
/// queried target. The parallel optional fields hold the concrete
/// target value; the `target_kind_tag` discriminates.
fn build_module_augmentation_index_shape_fact_key(
    target: &AugmentationTargetKind,
) -> verter_semantic::facts::FactKey {
    use verter_semantic::facts::registry::AugmentationTargetKindTag;
    match target {
        AugmentationTargetKind::ExternalSpecifier(spec) => {
            verter_semantic::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
                external_specifier: Some(spec.clone()),
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            }
        }
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => {
            verter_semantic::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ResolvedRelativeCanonical,
                external_specifier: None,
                resolved_relative_canonical: Some(Arc::clone(canon)),
                wildcard_pattern: None,
            }
        }
        AugmentationTargetKind::WildcardAmbient(pat) => {
            verter_semantic::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::WildcardAmbient,
                external_specifier: None,
                resolved_relative_canonical: None,
                wildcard_pattern: Some(pat.clone()),
            }
        }
        AugmentationTargetKind::GlobalAugmentation => {
            verter_semantic::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::GlobalAugmentation,
                external_specifier: None,
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            }
        }
    }
}

/// Emit a typed
/// [`StructuredAuditEvent::ModuleAugmentationStitched`] for the
/// cold-path compute. Silent no-op when no audit accumulator is
/// installed on the active thread.
fn emit_module_augmentation_stitched_event(
    target: &AugmentationTargetKind,
    augmenter_count: u32,
    fingerprint: Hash16,
) {
    use verter_audit::AugmentationTargetKindTag;
    let (tag, external_specifier, resolved_relative_canonical, wildcard_pattern) = match target {
        AugmentationTargetKind::ExternalSpecifier(spec) => (
            AugmentationTargetKindTag::ExternalSpecifier,
            Some(Arc::<str>::from(spec.as_ref())),
            None,
            None,
        ),
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => (
            AugmentationTargetKindTag::ResolvedRelativeCanonical,
            None,
            Some(Arc::clone(canon)),
            None,
        ),
        AugmentationTargetKind::WildcardAmbient(pat) => (
            AugmentationTargetKindTag::WildcardAmbient,
            None,
            None,
            Some(Arc::<str>::from(pat.as_ref())),
        ),
        AugmentationTargetKind::GlobalAugmentation => (
            AugmentationTargetKindTag::GlobalAugmentation,
            None,
            None,
            None,
        ),
    };
    crate::host_manage::push_structured_event(
        crate::component_meta_audit::StructuredAuditEvent::ModuleAugmentationStitched {
            target_kind_tag: tag,
            external_specifier,
            resolved_relative_canonical,
            wildcard_pattern,
            augmenter_count,
            fingerprint,
        },
    );
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

/// Total ordering over `SymbolSpace` variants for deterministic
/// stitching order. Type < Value < Namespace.
fn compare_symbol_space(a: SymbolSpace, b: SymbolSpace) -> std::cmp::Ordering {
    fn rank(s: SymbolSpace) -> u8 {
        match s {
            SymbolSpace::Type => 0,
            SymbolSpace::Value => 1,
            SymbolSpace::Namespace => 2,
        }
    }
    rank(a).cmp(&rank(b))
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

    #[test]
    fn insert_and_get_route() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        db.insert_route(
            "index.ts".to_owned(),
            "Foo".to_owned(),
            RouteResult::Resolved {
                defining_canonical: "foo.ts".to_owned(),
                defining_symbol: "Foo".to_owned(),
            },
        );

        let result = db.get_route("index.ts", "Foo", &view);
        assert!(result.is_some());
        let route = result.unwrap();
        assert!(
            matches!(&*route, RouteResult::Resolved { defining_canonical, .. } if defining_canonical == "foo.ts")
        );
    }

    #[test]
    fn miss_is_cached() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        db.insert_route(
            "index.ts".to_owned(),
            "Missing".to_owned(),
            RouteResult::Miss,
        );

        let result = db.get_route("index.ts", "Missing", &view);
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
        let result = db.get_or_resolve_route_with_facts("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some((
                RouteResult::Resolved {
                    defining_canonical: "bar.ts".to_owned(),
                    defining_symbol: "Bar".to_owned(),
                },
                vec![dummy_fact.clone()],
            ))
        });
        assert!(result.is_some());

        // Second call should hit cache because we admitted with a
        // non-empty fact signature on the first pass.
        let result2 = db.get_or_resolve_route_with_facts("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some((RouteResult::Miss, vec![dummy_fact.clone()]))
        });
        assert!(result2.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn get_or_resolve_route_with_empty_facts_does_not_cache() {
        // Strict-admission discrimination: the zero-facts variant
        // must NOT admit a cache entry. The second call re-invokes
        // the resolver because the first call skipped admission.
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let _result = db.get_or_resolve_route("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(RouteResult::Resolved {
                defining_canonical: "bar.ts".to_owned(),
                defining_symbol: "Bar".to_owned(),
            })
        });
        let _result2 = db.get_or_resolve_route("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(RouteResult::Resolved {
                defining_canonical: "bar.ts".to_owned(),
                defining_symbol: "Bar".to_owned(),
            })
        });
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "Zero-fact route resolves are not cached under strict \
             admission; the second call MUST re-invoke the resolver. \
             Migrate to `get_or_resolve_route_with_facts` with a \
             non-empty fact signature to opt back into caching."
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

        db.insert_barrel_surface(surface);

        let result = db.get_barrel_surface("barrel.ts", &view);
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

        let result = db.get_or_build_barrel_surface("barrel.ts", &view, || {
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

        let result2 = db.get_or_build_barrel_surface("barrel.ts", &view, || {
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
            "a.ts".to_owned(),
            "X".to_owned(),
            RouteResult::Resolved {
                defining_canonical: "x.ts".to_owned(),
                defining_symbol: "X".to_owned(),
            },
        );
        db.insert_barrel_surface(BarrelRouteSurface {
            barrel_canonical: "b.ts".to_owned(),
            wildcard_edges: FxHashMap::default(),
            fact_dep_signature: Arc::from(
                vec![FactVersionRef::FileWholeHash {
                    canonical_id: "b.ts".to_owned(),
                    hash: [1; 16],
                }]
                .into_boxed_slice(),
            ),
        });

        db.clear();

        assert!(db.get_route("a.ts", "X", &view).is_none());
        assert!(db.get_barrel_surface("b.ts", &view).is_none());
    }
}
