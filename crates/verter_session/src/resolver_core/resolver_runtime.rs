//! Unified resolver runtime holding both symbol and fallthrough subsystems.
//!
//! The runtime is created once per host and reused across requests. Both native
//! (scheduler) and WASM (non-scheduler) backends use the same runtime type —
//! the backend difference is in the [`StoreView`] implementation, not the
//! resolver state.

use std::hash::Hash;
use std::sync::Arc;

use crate::resolver_core::{
    fallthrough_resolver::FallthroughResolverState, imported_root_db::ImportedRootDb,
    prepared_decl::PreparedDeclBundle, route_db::RouteDb, symbol_resolver::SymbolResolverState,
    FactVersionRef, FallthroughNodeKey, ResolutionNodeKey, ResolverCounters, SingleflightGroup,
    StableExecutionValue, StoreView, ValidatedFactCache,
};

pub struct StableRequestState<K, V>
where
    K: Eq + Hash,
    V: Clone,
{
    cache: ValidatedFactCache<K, V>,
    singleflight: SingleflightGroup<K, StableExecutionValue<Option<V>>, ()>,
}

impl<K, V> StableRequestState<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new() -> Self {
        Self {
            cache: ValidatedFactCache::default(),
            singleflight: SingleflightGroup::default(),
        }
    }

    pub fn clear(&self) {
        self.cache.clear();
        self.singleflight.clear();
    }

    pub fn get_if_valid<TView>(&self, key: &K, view: &TView) -> Option<Arc<V>>
    where
        TView: StoreView + ?Sized,
    {
        self.cache.get_if_valid(key, view)
    }

    /// Strict-self-root warm read — forwards to
    /// [`ValidatedFactCache::get_if_valid_self_rooted`]. A `FileWholeHash`
    /// fact for a canonical in `self_root_canonicals` validates
    /// strictly; every other fact keeps the lazy cross-file
    /// permissiveness. A canonical-keyed stable cache passes its keyed
    /// canonical so a deleted keyed file rejects the stale entry.
    pub fn get_if_valid_self_rooted<TView>(
        &self,
        key: &K,
        view: &TView,
        self_root_canonicals: &[&str],
    ) -> Option<Arc<V>>
    where
        TView: StoreView + ?Sized,
    {
        self.cache
            .get_if_valid_self_rooted(key, view, self_root_canonicals)
    }

    /// Attributed warm-read sibling of [`Self::get_if_valid_self_rooted`].
    /// Forwards to [`ValidatedFactCache::get_if_valid_self_rooted_attributed`].
    /// `FactVersionRef` is large by design — see the cache-side helper's
    /// rationale for the `clippy::result_large_err` allowance.
    #[allow(clippy::result_large_err)]
    pub fn get_if_valid_self_rooted_attributed<TView>(
        &self,
        key: &K,
        view: &TView,
        self_root_canonicals: &[&str],
    ) -> Result<Arc<V>, (Option<crate::resolver_core::FactVersionRef>, usize)>
    where
        TView: crate::resolver_core::StoreView + ?Sized,
    {
        self.cache
            .get_if_valid_self_rooted_attributed(key, view, self_root_canonicals)
    }

    pub fn insert_arc(&self, key: K, value: Arc<V>, facts: Vec<FactVersionRef>) {
        self.cache.insert_arc(key, value, facts);
    }

    /// Strict-admission wrapper: forwards through to
    /// [`ValidatedFactCache::insert_arc_with_kind`] so producers
    /// admit through the fact-completeness guard. Empty signatures
    /// refuse + emit `FactSignatureAdmissionRefused`; over-cap
    /// signatures refuse + emit `FactSignatureOverflow`.
    pub fn insert_arc_with_kind(
        &self,
        key: K,
        value: Arc<V>,
        facts: Vec<FactVersionRef>,
        cache_kind: &'static str,
    ) {
        self.cache
            .insert_arc_with_kind(key, value, facts, cache_kind);
    }

    pub fn cached_values(&self) -> Vec<Arc<V>> {
        self.cache.values()
    }

    pub fn remove(&self, key: &K) {
        self.cache.remove(key);
    }

    /// Hard-remove: clear from both primary and archive maps.
    pub fn hard_remove(&self, key: &K) {
        self.cache.hard_remove(key);
    }

    /// Remove all entries whose key satisfies the predicate.
    pub fn retain<F>(&self, predicate: F)
    where
        F: FnMut(&K) -> bool,
    {
        self.cache.retain(predicate);
    }

    pub fn singleflight(&self) -> &SingleflightGroup<K, StableExecutionValue<Option<V>>, ()> {
        &self.singleflight
    }
}

impl<K, V> Default for StableRequestState<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Unified resolver runtime holding both subsystems and shared counters.
///
/// Created once per host. Both native (scheduler-backed) and WASM
/// (files-map-backed) hosts use this same type — the backend difference
/// is in the `StoreView` provided to `resolve_node()`, not in the runtime
/// itself.
pub struct UnifiedResolverRuntime<MetaV = (), FallthroughV = ()>
where
    MetaV: Clone,
    FallthroughV: Clone,
{
    /// Symbol/type resolution subsystem.
    pub symbol: SymbolResolverState,
    /// Fallthrough/inheritance resolution subsystem.
    pub fallthrough: FallthroughResolverState,
    /// Top-level materialized component-meta request state.
    pub component_meta: StableRequestState<ResolutionNodeKey, MetaV>,
    /// Host-owned prepared declaration bundles, keyed by canonical file ID.
    /// Validated by file whole hash and import-route facts.
    pub prepared_decl_bundles: StableRequestState<String, PreparedDeclBundle>,
    /// Top-level fallthrough singleflight, with runtime fallthrough nodes remaining the cache authority.
    pub top_level_fallthrough_singleflight:
        SingleflightGroup<FallthroughNodeKey, StableExecutionValue<Option<FallthroughV>>, ()>,
    /// Shared observability counters.
    pub counters: Arc<ResolverCounters>,

    // -- Semantic DB layers --
    /// Singleflight used by `ensure_indexed_ready` to collapse concurrent cold
    /// loads for the same canonical file onto one materialization path. The
    /// flight value is the materialized `Arc<IndexedReady>`, which is then
    /// unwrapped from the outer singleflight `Arc` at the call site.
    pub indexed_singleflight:
        SingleflightGroup<String, Arc<crate::project_type_store::IndexedReady>, ()>,
    /// Host-owned cross-file route subsystem: barrel surfaces, route results,
    /// and stable negative answers.
    ///
    /// Authority owned by
    /// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore).
    /// The runtime holds an `Arc` clone of the store's instance so resolver
    /// hot-path mutations land on the project-shared `RouteDb`. See
    /// [`Self::routes_handle`].
    pub routes: Arc<RouteDb>,
    /// Host-owned imported-root proof cache (positive and negative).
    ///
    /// same `Arc`-shared discipline as `routes`. See
    /// [`Self::imported_roots_handle`].
    pub imported_roots: Arc<ImportedRootDb>,
    /// Singleflight for the route-only shallow materialiser. Collapses
    /// concurrent cold callers for the same canonical onto one
    /// materialisation path. Mirrors the
    /// [`Self::indexed_singleflight`] pattern: key is `Arc<str>`
    /// (canonical alone), error type is `()` (matches `indexed_singleflight`;
    /// the host materialiser maps internal errors to `()` and returns
    /// `Option<...>` to callers).
    pub route_owned_shallow_singleflight:
        SingleflightGroup<Arc<str>, Arc<crate::project_type_store::RouteOwnedShallowEntry>, ()>,
}

impl<MetaV, FallthroughV> UnifiedResolverRuntime<MetaV, FallthroughV>
where
    MetaV: Clone,
    FallthroughV: Clone,
{
    /// Create a new runtime with fresh caches and counters, sharing the
    /// project-store-owned `RouteDb` / `ImportedRootDb` instances.
    ///
    /// The host's
    /// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore)
    /// is the project-global authority for both DBs. The host pulls
    /// [`ProjectTypeStore::routes_handle`](crate::project_type_store::ProjectTypeStore::routes_handle)
    /// and
    /// [`ProjectTypeStore::imported_roots_handle`](crate::project_type_store::ProjectTypeStore::imported_roots_handle)
    /// at construction time and threads them in here so resolver hot-path
    /// mutations land on the shared project authority.
    pub fn new(routes: Arc<RouteDb>, imported_roots: Arc<ImportedRootDb>) -> Self {
        let counters = Arc::new(ResolverCounters::new());
        Self {
            symbol: SymbolResolverState::new(counters.clone()),
            fallthrough: FallthroughResolverState::new(counters.clone()),
            component_meta: StableRequestState::new(),
            prepared_decl_bundles: StableRequestState::new(),
            top_level_fallthrough_singleflight: SingleflightGroup::default(),
            counters,
            indexed_singleflight: SingleflightGroup::default(),
            routes,
            imported_roots,
            route_owned_shallow_singleflight: SingleflightGroup::default(),
        }
    }

    /// Create a runtime with shared counters and project-store-owned
    /// `RouteDb` / `ImportedRootDb` instances. See [`Self::new`] for the
    /// authority chain.
    pub fn with_counters(
        counters: Arc<ResolverCounters>,
        routes: Arc<RouteDb>,
        imported_roots: Arc<ImportedRootDb>,
    ) -> Self {
        Self {
            symbol: SymbolResolverState::new(counters.clone()),
            fallthrough: FallthroughResolverState::new(counters.clone()),
            component_meta: StableRequestState::new(),
            prepared_decl_bundles: StableRequestState::new(),
            top_level_fallthrough_singleflight: SingleflightGroup::default(),
            counters,
            indexed_singleflight: SingleflightGroup::default(),
            routes,
            imported_roots,
            route_owned_shallow_singleflight: SingleflightGroup::default(),
        }
    }

    /// return a cloned `Arc<RouteDb>` handle for use as a
    /// stable shared reference. Mirrors
    /// [`ProjectTypeStore::routes_handle`](crate::project_type_store::ProjectTypeStore::routes_handle)
    /// — successive calls return Arcs that `Arc::ptr_eq` the inner
    /// instance, which is itself shared with the project-store handle.
    #[must_use]
    pub fn routes_handle(&self) -> Arc<RouteDb> {
        Arc::clone(&self.routes)
    }

    /// return a cloned `Arc<ImportedRootDb>` handle. See
    /// [`Self::routes_handle`] for the full rationale.
    #[must_use]
    pub fn imported_roots_handle(&self) -> Arc<ImportedRootDb> {
        Arc::clone(&self.imported_roots)
    }

    /// Clear all cached results in both subsystems.
    pub fn clear_caches(&self) {
        self.symbol.clear_cache();
        self.fallthrough.clear_cache();
        self.component_meta.clear();
        self.prepared_decl_bundles.clear();
        self.top_level_fallthrough_singleflight.clear();
        self.indexed_singleflight.clear();
        self.routes.clear();
        self.imported_roots.clear();
        // clear the route-only shallow singleflight too.
        // The `RouteOwnedShallowDb` itself is project-store-owned and
        // cleared via `ProjectTypeStore::route_owned_shallow().clear_all()`
        // from the host's cascade. We only clear the singleflight here
        // so any in-flight closures see a fresh start.
        self.route_owned_shallow_singleflight.clear();
    }

    /// Evict artifacts owned by one canonical file without clearing unrelated
    /// DB state. Cross-file node caches stay fact-validated and are rebuilt
    /// lazily on next access when their owner facts change.
    /// Hard-evict all artifacts for a canonical file (e.g. after content change).
    pub fn evict_canonical(&self, canonical_id: &str) {
        self.prepared_decl_bundles.remove(&canonical_id.to_string());
        self.routes.evict_provider(canonical_id);
        self.imported_roots.evict_provider(canonical_id);
    }

    /// Hard-evict all artifacts for a deleted file, including archived entries.
    /// Archived entries must be removed because untracked-file acceptance in
    /// the store view's `validates` method would otherwise return stale facts.
    pub fn hard_evict_canonical(&self, canonical_id: &str) {
        self.prepared_decl_bundles
            .hard_remove(&canonical_id.to_string());
        self.routes.evict_provider(canonical_id);
        self.imported_roots.evict_provider(canonical_id);
    }

    /// Soft-invalidate artifacts for a canonical file (e.g. after import
    /// route change where file content is unchanged). The `FileArtifactStore`
    /// is keyed by `(canonical_id, whole_hash)` and is invalidated at the
    /// project-store level by `evict_canonical`; this runtime-level helper
    /// only clears subsystem caches that live outside the project store.
    pub fn invalidate_canonical(&self, canonical_id: &str) {
        self.prepared_decl_bundles.remove(&canonical_id.to_string());
        self.routes.evict_provider(canonical_id);
        self.imported_roots.evict_provider(canonical_id);
    }

    /// Take a snapshot of the current counter values.
    pub fn counter_snapshot(&self) -> crate::resolver_core::ResolverCountersSnapshot {
        self.counters.snapshot()
    }

    /// Reset all counters to zero.
    pub fn reset_counters(&self) {
        self.counters.reset();
    }

    /// Test-only constructor minting a fresh runtime with newly-allocated
    /// `Arc<RouteDb>` / `Arc<ImportedRootDb>` instances.
    ///
    /// now that `UnifiedResolverRuntime::new` requires
    /// `Arc<RouteDb>` / `Arc<ImportedRootDb>` parameters supplied by the
    /// host's [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore),
    /// tests that mint their own runtime in isolation use this helper to
    /// create both `Arc`s locally without taking a project-store
    /// dependency. Each call produces a fresh, isolated authority — so
    /// these test runtimes are NOT project-shared (each test owns its own
    /// instance). For Arc-identity assertions (T1) the host is constructed
    /// via `VerterHost::new_standalone(...)`, which routes through the
    /// real project-store path.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self::new(Arc::new(RouteDb::new()), Arc::new(ImportedRootDb::new()))
    }

    /// Test-only constructor variant that shares counters with another
    /// runtime. Same lifecycle as `for_tests()`.
    #[cfg(test)]
    pub fn for_tests_with_counters(counters: Arc<ResolverCounters>) -> Self {
        Self::with_counters(
            counters,
            Arc::new(RouteDb::new()),
            Arc::new(ImportedRootDb::new()),
        )
    }
}

impl<MetaV, FallthroughV> Default for UnifiedResolverRuntime<MetaV, FallthroughV>
where
    MetaV: Clone,
    FallthroughV: Clone,
{
    fn default() -> Self {
        Self::new(Arc::new(RouteDb::new()), Arc::new(ImportedRootDb::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_creates_with_shared_counters() {
        let runtime = UnifiedResolverRuntime::<(), ()>::for_tests();
        runtime.symbol.counters().record_cache_hit();
        runtime.fallthrough.counters().record_cache_miss();

        let snap = runtime.counter_snapshot();
        assert_eq!(snap.node_cache_hits, 1);
        assert_eq!(snap.node_cache_misses, 1);
    }

    #[test]
    fn runtime_clear_caches_resets_both_subsystems() {
        let runtime = UnifiedResolverRuntime::<(), ()>::for_tests();
        runtime.clear_caches();
    }

    #[test]
    fn runtime_with_shared_counters_shares_correctly() {
        let counters = Arc::new(ResolverCounters::new());
        counters.record_cache_hit();

        let runtime = UnifiedResolverRuntime::<(), ()>::for_tests_with_counters(counters.clone());
        runtime.symbol.counters().record_cache_hit();

        assert_eq!(runtime.counter_snapshot().node_cache_hits, 2);
        assert_eq!(counters.snapshot().node_cache_hits, 2);
    }

    #[test]
    fn runtime_reset_counters_clears_all() {
        let runtime = UnifiedResolverRuntime::<(), ()>::for_tests();
        runtime.symbol.counters().record_cache_hit();
        runtime.fallthrough.counters().record_cache_miss();

        runtime.reset_counters();
        let snap = runtime.counter_snapshot();
        assert_eq!(snap.node_cache_hits, 0);
        assert_eq!(snap.node_cache_misses, 0);
    }
}
