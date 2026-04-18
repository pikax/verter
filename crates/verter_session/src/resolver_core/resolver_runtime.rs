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
    type_surface_db::TypeSurfaceDb, FactVersionRef, FallthroughNodeKey, ResolutionNodeKey,
    ResolverCounters, SingleflightGroup, StableExecutionValue, StoreView, ValidatedFactCache,
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
        TView: StoreView,
    {
        self.cache.get_if_valid(key, view)
    }

    pub fn insert_arc(&self, key: K, value: Arc<V>, facts: Vec<FactVersionRef>) {
        self.cache.insert_arc(key, value, facts);
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
    pub routes: RouteDb,
    /// Host-owned imported-root proof cache (positive and negative).
    pub imported_roots: ImportedRootDb,
    /// Shared projected type surfaces for cross-request reuse.
    pub type_surfaces: TypeSurfaceDb,
}

impl<MetaV, FallthroughV> UnifiedResolverRuntime<MetaV, FallthroughV>
where
    MetaV: Clone,
    FallthroughV: Clone,
{
    /// Create a new runtime with fresh caches and counters.
    pub fn new() -> Self {
        let counters = Arc::new(ResolverCounters::new());
        Self {
            symbol: SymbolResolverState::new(counters.clone()),
            fallthrough: FallthroughResolverState::new(counters.clone()),
            component_meta: StableRequestState::new(),
            prepared_decl_bundles: StableRequestState::new(),
            top_level_fallthrough_singleflight: SingleflightGroup::default(),
            counters,
            indexed_singleflight: SingleflightGroup::default(),
            routes: RouteDb::new(),
            imported_roots: ImportedRootDb::new(),
            type_surfaces: TypeSurfaceDb::new(),
        }
    }

    /// Create a runtime with shared counters (e.g., from a parent runtime).
    pub fn with_counters(counters: Arc<ResolverCounters>) -> Self {
        Self {
            symbol: SymbolResolverState::new(counters.clone()),
            fallthrough: FallthroughResolverState::new(counters.clone()),
            component_meta: StableRequestState::new(),
            prepared_decl_bundles: StableRequestState::new(),
            top_level_fallthrough_singleflight: SingleflightGroup::default(),
            counters,
            indexed_singleflight: SingleflightGroup::default(),
            routes: RouteDb::new(),
            imported_roots: ImportedRootDb::new(),
            type_surfaces: TypeSurfaceDb::new(),
        }
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
        self.type_surfaces.clear();
    }

    /// Evict artifacts owned by one canonical file without clearing unrelated
    /// DB state. Cross-file node caches stay fact-validated and are rebuilt
    /// lazily on next access when their owner facts change.
    /// Hard-evict all artifacts for a canonical file (e.g. after content change).
    pub fn evict_canonical(&self, canonical_id: &str) {
        self.prepared_decl_bundles.remove(&canonical_id.to_string());
        self.routes.evict_provider(canonical_id);
        self.imported_roots.evict_provider(canonical_id);
        self.type_surfaces.evict_owner(canonical_id);
    }

    /// Hard-evict all artifacts for a deleted file, including archived entries.
    /// Archived entries must be removed because untracked-file acceptance in
    /// the store view's `validates` method would otherwise return stale facts.
    pub fn hard_evict_canonical(&self, canonical_id: &str) {
        self.prepared_decl_bundles
            .hard_remove(&canonical_id.to_string());
        self.routes.evict_provider(canonical_id);
        self.imported_roots.evict_provider(canonical_id);
        self.type_surfaces.evict_owner(canonical_id);
    }

    /// Soft-invalidate artifacts for a canonical file (e.g. after import
    /// route change where file content is unchanged). The `IndexedReadyDb`
    /// is keyed by `(canonical_id, whole_hash)` and is invalidated at the
    /// project-store level by `evict_canonical`; this runtime-level helper
    /// only clears subsystem caches that live outside the project store.
    pub fn invalidate_canonical(&self, canonical_id: &str) {
        self.prepared_decl_bundles.remove(&canonical_id.to_string());
        self.routes.evict_provider(canonical_id);
        self.imported_roots.evict_provider(canonical_id);
        self.type_surfaces.evict_owner(canonical_id);
    }

    /// Take a snapshot of the current counter values.
    pub fn counter_snapshot(&self) -> crate::resolver_core::ResolverCountersSnapshot {
        self.counters.snapshot()
    }

    /// Reset all counters to zero.
    pub fn reset_counters(&self) {
        self.counters.reset();
    }
}

impl<MetaV, FallthroughV> Default for UnifiedResolverRuntime<MetaV, FallthroughV>
where
    MetaV: Clone,
    FallthroughV: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_creates_with_shared_counters() {
        let runtime = UnifiedResolverRuntime::<(), ()>::new();
        runtime.symbol.counters().record_cache_hit();
        runtime.fallthrough.counters().record_cache_miss();

        let snap = runtime.counter_snapshot();
        assert_eq!(snap.node_cache_hits, 1);
        assert_eq!(snap.node_cache_misses, 1);
    }

    #[test]
    fn runtime_clear_caches_resets_both_subsystems() {
        let runtime = UnifiedResolverRuntime::<(), ()>::new();
        runtime.clear_caches();
    }

    #[test]
    fn runtime_with_shared_counters_shares_correctly() {
        let counters = Arc::new(ResolverCounters::new());
        counters.record_cache_hit();

        let runtime = UnifiedResolverRuntime::<(), ()>::with_counters(counters.clone());
        runtime.symbol.counters().record_cache_hit();

        assert_eq!(runtime.counter_snapshot().node_cache_hits, 2);
        assert_eq!(counters.snapshot().node_cache_hits, 2);
    }

    #[test]
    fn runtime_reset_counters_clears_all() {
        let runtime = UnifiedResolverRuntime::<(), ()>::new();
        runtime.symbol.counters().record_cache_hit();
        runtime.fallthrough.counters().record_cache_miss();

        runtime.reset_counters();
        let snap = runtime.counter_snapshot();
        assert_eq!(snap.node_cache_hits, 0);
        assert_eq!(snap.node_cache_misses, 0);
    }
}
