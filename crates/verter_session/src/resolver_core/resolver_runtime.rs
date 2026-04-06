//! Unified resolver runtime holding both symbol and fallthrough subsystems.
//!
//! The runtime is created once per host and reused across requests. Both native
//! (scheduler) and WASM (non-scheduler) backends use the same runtime type —
//! the backend difference is in the [`StoreView`] implementation, not the
//! resolver state.

use std::hash::Hash;
use std::sync::Arc;

use crate::resolver_core::{
    fallthrough_resolver::FallthroughResolverState, prepared_decl::PreparedDeclBundle,
    symbol_resolver::SymbolResolverState, FactVersionRef, FallthroughNodeKey, ResolutionNodeKey,
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
    /// Fact-validated prepared declaration bundles, keyed by canonical file ID.
    /// Replaces the entry-owned `prepared_type_decls` / `prepared_value_decls`
    /// on `ImportedDependencyCacheEntry` with an atomic, fact-validated cache.
    pub prepared_decl_bundles: StableRequestState<String, PreparedDeclBundle>,
    /// Top-level fallthrough singleflight, with runtime fallthrough nodes remaining the cache authority.
    pub top_level_fallthrough_singleflight:
        SingleflightGroup<FallthroughNodeKey, StableExecutionValue<Option<FallthroughV>>, ()>,
    /// Shared observability counters.
    pub counters: Arc<ResolverCounters>,
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
        }
    }

    /// Clear all cached results in both subsystems.
    pub fn clear_caches(&self) {
        self.symbol.clear_cache();
        self.fallthrough.clear_cache();
        self.component_meta.clear();
        self.prepared_decl_bundles.clear();
        self.top_level_fallthrough_singleflight.clear();
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
