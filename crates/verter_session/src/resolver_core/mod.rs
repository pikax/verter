use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;
use std::hash::Hash;
use std::sync::Arc;

pub(crate) mod ambient_resolve;
pub(crate) mod bare_name_resolve;
pub(crate) mod cache_keys;
pub(crate) mod component_meta;
pub mod component_meta_query_engine;
pub mod component_meta_registry;
mod component_meta_request;
mod declaration_metadata;
mod export_graph;
mod external_macro_types;
mod external_type_body;
pub mod external_type_frontier;
mod fallthrough;
mod fallthrough_request;
pub mod fallthrough_resolver;
pub mod prepared_decl;
pub mod resolver_runtime;
pub mod route_demand;
mod runtime_values;
pub mod shallow_file_state;
pub(crate) mod surface_projector;
pub mod symbol_resolver;
pub mod type_expansion;
pub mod type_expansion_host;
pub mod type_expansion_verter;
pub mod type_text_parser;

pub mod fuses;
pub mod imported_root_db;
pub mod route_db;

pub use fuses::{FuseBudgets, FuseState, FuseTrip};
pub use imported_root_db::{ImportedRootDb, ImportedRootResult};
pub use route_db::{BarrelRouteSurface, RouteDb, RouteResult};

pub type ResolverHash16 = verter_semantic::analysis::Hash16;
pub use component_meta::{
    collect_requested_binding_names, component_meta_resolved_macros, component_meta_type_registry,
    resolve_component_meta_parts, resolved_elements_to_type_expr_via_type_text,
    ComponentMetaEvalOutputs, ComponentMetaResolutionPurpose, ComponentMetaResolverHost,
    ResolvedComponentMetaParts, ResolvedImportedMacroSurface, ResolvedJsdocBlock, ResolvedJsdocTag,
    ResolvedMacroMeta, ResolvedTypeRegistryMeta,
};
pub use component_meta_query_engine::ComponentMetaQueryEngine;
pub use component_meta_request::{run_component_meta_request, ComponentMetaRequestHost};
pub use declaration_metadata::{
    resolve_direct_local_type_declaration, resolve_local_type_declaration,
    resolve_type_declaration, DeclarationMetadataResolver, ResolvedDeclarationKind,
    ResolvedExportTarget, ResolvedLocalTypeSymbolMetadata, ResolvedTypeDeclaration,
};
pub use export_graph::{
    get_export_span_follow_reexports_from_graph, resolve_exports_from_graph,
    resolve_exports_from_graph_best_effort, resolve_named_export_from_graph, ExportGraphFileKind,
    ExportGraphResolver, ExportSurface, ResolvedGraphExport,
};
pub use external_macro_types::{
    collect_external_macro_types, ExternalMacroTypeCollection, ExternalMacroTypeCollectorHost,
    ExternalMacroTypeDiagnostic,
};
pub use external_type_body::{
    resolve_external_type_from_source_body, ExternalTypeBodyCache, ExternalTypeBodyResolver,
};
pub use external_type_frontier::{
    ExternalTypeFrontier, FrontierHost, PendingExternalSymbol, ResolvedRouteProvenance,
    ResolvedSymbol, ResolvedSymbolStatus, RouteKind,
};
pub use fallthrough::{
    append_component_candidate_branches, append_native_candidate_branch,
    collect_dynamic_root_candidates_from_type, evaluate_value_expression_via_env_or_dispatch,
    extend_unique_fact_versions, fallthrough_cache_key, hash_prop_type_overrides,
    inject_prop_type_overrides, known_spread_keys_from_type_expr, merge_fallthrough_branches,
    push_partial_reason, resolve_fallthrough_surface, resolve_usage_prop_type,
    structural_substitute_typeof_refs, DynamicRootCandidate, FallthroughComputeHost,
    FallthroughResolutionView, FallthroughResolverHost, KnownSpreadKeys, ResolvedConsumedBindings,
    ResolvedFallthroughSurface,
};
pub use fallthrough_request::{run_fallthrough_request, FallthroughRequestHost};
pub use prepared_decl::{
    build_prepared_type_decl_cache, build_prepared_value_decl_cache, prepare_exported_type_decl,
    prepare_exported_value_decl, prepare_local_type_decl, prepare_local_value_decl,
};
pub use route_demand::{
    merge_route_demands, RouteDemand, RouteProvenance, RouteProvenanceKind, RoutedExternalDep,
    RoutedSymbolResult, RoutedSymbolStatus, SymbolSpace,
};
pub use runtime_values::{
    materialize_imported_runtime_values_into_env, ImportedRuntimeValueResolver,
};
pub use shallow_file_state::{
    BudgetDomain, BudgetExceededFailure, ExportTarget, ExternalSymbolRef, ImportTarget,
    LocalClosureResult, LocalClosureStatus, ResolutionBudgets, ResolutionCounters,
    ShallowFileState, ShallowImportResolver, ShallowTypeSymbol, ShallowTypeView,
    ShallowValueSymbol, WildcardReexport,
};
pub use surface_projector::{
    extract_slot_info_from_type_text, project_macro_surfaces, ProjectedMacroSurfaces,
    ResolvedNativeProp,
};

/// Lane-identity token for singleflight deduplication.
///
/// Widened in Path C C14 to include session identity so that two sessions
/// with different overlays but the same epoch never coalesce into the same
/// singleflight lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreViewCompatToken {
    pub epoch: u64,
    pub session: Option<u64>,
}

pub trait StoreView {
    fn compat_token(&self) -> StoreViewCompatToken;
    fn validates(&self, fact: &FactVersionRef) -> bool;
    /// Whether this view should check the archive for soft-invalidated
    /// entries. Only strict (non-permissive) views return true.
    fn checks_archive(&self) -> bool {
        false
    }
    /// Validate a fact from an ARCHIVED entry. Archived entries may be stale
    /// (they were soft-invalidated from a prior generation). The default
    /// delegates to `validates`, but views that accept untracked files in
    /// the primary path should be STRICT for archived entries to prevent
    /// stale data from being returned after workspace-level content changes.
    fn validates_archived(&self, fact: &FactVersionRef) -> bool {
        self.validates(fact)
    }
    /// Whether the view tracks a specific file (has its hash in the snapshot).
    ///
    /// Used by route-derived cache materialization paths to decide whether to
    /// include `DerivedFactHash::ImportRoute` in validation facts. Untracked
    /// dependency files never have `set_import_dependencies` called on them,
    /// so their route facts are safe to omit — eliminating false cache misses.
    fn tracks_file(&self, _canonical_id: &str) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PermissiveStoreView;

impl StoreView for PermissiveStoreView {
    fn compat_token(&self) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch: 0,
            session: None,
        }
    }

    fn validates(&self, _fact: &FactVersionRef) -> bool {
        true
    }
}

pub trait ResolverStore {
    type View: StoreView;

    fn snapshot_view(&self) -> Self::View;
}

pub trait ResolverRuntime {
    fn store_view_token(&self) -> StoreViewCompatToken;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestTraceId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolveRequestTarget {
    Symbol(ResolutionNodeKey),
    Fallthrough(FallthroughNodeKey),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolveRequest {
    pub trace_id: RequestTraceId,
    pub target: ResolveRequestTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DerivedFactKind {
    /// Provider-owned export route surface hash.
    Route,
    /// Importer-owned effective import-target surface hash.
    ImportRoute,
    DirectSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FactVersionRef {
    FileWholeHash {
        canonical_id: String,
        hash: ResolverHash16,
    },
    DerivedFactHash {
        canonical_id: String,
        kind: DerivedFactKind,
        hash: ResolverHash16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TraversalLens {
    StructuralObject,
    KeySpace,
    CallableParams,
    CallableReturn,
    ValueTypeOf,
    MemberProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolutionNodeKind {
    /// Importer-side import-edge node: keyed by owner + import source +
    /// requested symbol + binding context.
    ImporterEdge,
    /// Provider-side export-route node: keyed by provider canonical +
    /// requested symbol + route demand + symbol space. Reusable across importers.
    ProviderExportRoute,
    BarrelLookup,
    DeclarationMetadata,
    SymbolExpand,
    MemberProjection,
    KeySpace,
    MappedExpand,
    TypeOfValue,
    Assemble,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FallthroughNodeKind {
    ComponentRootFollow,
    IntrinsicSurfaceLoad,
    ChildComponentSurfaceFollow,
    ConsumedBindingEvaluation,
    BranchUnionMerge,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolutionNodeKey {
    pub symbol_id: String,
    pub node_kind: ResolutionNodeKind,
    pub traversal_lens: TraversalLens,
    pub member_path_hash: u64,
    pub type_args_hash: u64,
    pub behavior_flags: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughNodeKey {
    pub canonical_component_id: String,
    pub node_kind: FallthroughNodeKind,
    pub override_fingerprint: u64,
    pub behavior_flags: u32,
    pub branch_selector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolverDiagnostic {
    pub code: String,
    pub message: String,
    pub canonical_path: Option<String>,
    pub span_start: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedEntry<V> {
    pub value: Arc<V>,
    pub facts: Vec<FactVersionRef>,
}

#[derive(Debug, Clone)]
pub struct StableExecutionValue<V> {
    pub value: V,
    pub stable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestSource {
    Cache,
    Flight {
        role: SingleflightRole,
        forked_lane: bool,
    },
    Fallback,
}

#[derive(Debug, Clone)]
pub struct RequestRunResult<V> {
    pub value: V,
    pub source: RequestSource,
    pub attempts: usize,
}

pub trait StableRequestExecutor<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    type View: StoreView;
    type Error: Clone;

    fn cache_key(&self) -> K;
    fn snapshot_view(&mut self) -> Self::View;
    fn try_get_cached(&mut self, view: &Self::View) -> Option<V>;
    fn compute(&mut self, view: &Self::View) -> Result<V, Self::Error>;
    fn is_stable(&mut self, view: &Self::View) -> bool;
    fn store_stable(&mut self, value: &V);

    fn max_attempts(&self) -> usize {
        3
    }
}

pub fn run_stable_request<K, V, X>(
    singleflight: &SingleflightGroup<K, StableExecutionValue<V>, X::Error>,
    executor: &mut X,
) -> Result<RequestRunResult<V>, X::Error>
where
    K: Clone + Eq + Hash,
    V: Clone,
    X: StableRequestExecutor<K, V>,
{
    let cache_key = executor.cache_key();
    let max_attempts = executor.max_attempts();

    for attempt in 0..max_attempts {
        let store_view = executor.snapshot_view();
        if let Some(cached) = executor.try_get_cached(&store_view) {
            return Ok(RequestRunResult {
                value: cached,
                source: RequestSource::Cache,
                attempts: attempt + 1,
            });
        }

        let flight = singleflight.run(cache_key.clone(), store_view.compat_token(), || {
            if let Some(cached) = executor.try_get_cached(&store_view) {
                return Ok(StableExecutionValue {
                    value: cached,
                    stable: true,
                });
            }

            let value = executor.compute(&store_view)?;
            let stable = executor.is_stable(&store_view);
            if stable {
                executor.store_stable(&value);
            }

            Ok(StableExecutionValue { value, stable })
        })?;

        if flight.value.stable {
            return Ok(RequestRunResult {
                value: flight.value.value.clone(),
                source: RequestSource::Flight {
                    role: flight.role,
                    forked_lane: flight.forked_lane,
                },
                attempts: attempt + 1,
            });
        }
    }

    let store_view = executor.snapshot_view();
    Ok(RequestRunResult {
        value: executor.compute(&store_view)?,
        source: RequestSource::Fallback,
        attempts: max_attempts + 1,
    })
}

#[derive(Debug)]
pub struct ValidatedFactCache<K, V>
where
    K: Eq + Hash,
{
    entries: Mutex<FxHashMap<K, ValidatedEntry<V>>>,
    /// Soft-invalidated entries that are no longer reachable via `get_if_valid`
    /// with a permissive view, but can still be found by store-view-validated
    /// lookups via `get_if_valid_archived`.
    archived: Mutex<FxHashMap<K, ValidatedEntry<V>>>,
}

impl<K, V> Default for ValidatedFactCache<K, V>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self {
            entries: Mutex::new(FxHashMap::default()),
            archived: Mutex::new(FxHashMap::default()),
        }
    }
}

impl<K, V> ValidatedFactCache<K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn get_if_valid<TView>(&self, key: &K, view: &TView) -> Option<Arc<V>>
    where
        TView: StoreView,
    {
        let entries = self.entries.lock();
        if let Some(entry) = entries.get(key) {
            if entry.facts.iter().all(|fact| view.validates(fact)) {
                return Some(entry.value.clone());
            }
        }
        drop(entries);

        // Check the archive — stale store views may still validate against
        // prior generations of facts that were soft-invalidated.
        // Uses validates_archived which is STRICT for untracked files to
        // prevent stale data from surviving workspace content changes.
        if view.checks_archive() {
            let archived = self.archived.lock();
            if let Some(entry) = archived.get(key) {
                if entry.facts.iter().all(|fact| view.validates_archived(fact)) {
                    return Some(entry.value.clone());
                }
            }
        }
        None
    }

    pub fn insert(&self, key: K, value: V, facts: Vec<FactVersionRef>) {
        self.insert_arc(key, Arc::new(value), facts);
    }

    pub fn insert_arc(&self, key: K, value: Arc<V>, facts: Vec<FactVersionRef>) {
        self.entries
            .lock()
            .insert(key, ValidatedEntry { value, facts });
    }

    pub fn values(&self) -> Vec<Arc<V>> {
        self.entries
            .lock()
            .values()
            .map(|entry| entry.value.clone())
            .collect()
    }

    pub fn clear(&self) {
        self.entries.lock().clear();
        self.archived.lock().clear();
    }

    pub fn remove(&self, key: &K) {
        self.entries.lock().remove(key);
        // Archive entries are NOT removed here — stale store views may
        // still need them. The validation mechanism (whole_hash mismatch)
        // prevents stale views from seeing facts for changed files.
    }

    /// Hard-remove: clear from both primary and archive maps.
    /// Used when a file is deleted — archived entries must not survive
    /// because untracked-file acceptance in `validates` would accept them.
    pub fn hard_remove(&self, key: &K) {
        self.entries.lock().remove(key);
        self.archived.lock().remove(key);
    }

    /// Soft-invalidate: remove the entry from the primary map and move
    /// it to the archive. Stale store views can still find the archived
    /// entry through `get_if_valid` (which checks both maps), while
    /// production (permissive) lookups will see no entry since the
    /// primary map no longer holds it.
    pub fn invalidate(&self, key: &K) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.remove(key) {
            drop(entries);
            self.archived.lock().insert(key.clone(), entry);
        }
    }

    /// Remove all entries whose key satisfies the predicate.
    pub fn retain<F>(&self, mut predicate: F)
    where
        F: FnMut(&K) -> bool,
    {
        self.entries.lock().retain(|k, _| predicate(k));
    }

    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    pub fn snapshot_all(&self) -> Vec<(K, Arc<V>)> {
        self.entries
            .lock()
            .iter()
            .map(|(k, entry)| (k.clone(), entry.value.clone()))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleflightRole {
    Leader,
    Follower,
}

#[derive(Debug, Clone)]
pub struct SingleflightRunResult<V> {
    pub value: Arc<V>,
    pub role: SingleflightRole,
    pub forked_lane: bool,
}

#[derive(Debug)]
pub struct SingleflightGroup<K, V, E>
where
    K: Eq + Hash,
{
    #[allow(clippy::type_complexity)]
    flights: Mutex<FxHashMap<(K, StoreViewCompatToken), Arc<FlightState<V, E>>>>,
}

#[derive(Debug)]
struct FlightState<V, E> {
    inner: Mutex<FlightInner<V, E>>,
    ready: Condvar,
}

#[derive(Debug, Clone)]
enum FlightInner<V, E> {
    Running { owner: std::thread::ThreadId },
    Done(Result<Arc<V>, E>),
}

impl<K, V, E> Default for SingleflightGroup<K, V, E>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self {
            flights: Mutex::new(FxHashMap::default()),
        }
    }
}

impl<K, V, E> SingleflightGroup<K, V, E>
where
    K: Eq + Hash + Clone,
    E: Clone,
{
    pub fn run<F>(
        &self,
        key: K,
        token: StoreViewCompatToken,
        compute: F,
    ) -> Result<SingleflightRunResult<V>, E>
    where
        F: FnOnce() -> Result<V, E>,
    {
        let lane_key = (key.clone(), token);
        let current_thread = std::thread::current().id();

        let (state, leader, forked_lane) = {
            let mut flights = self.flights.lock();
            let forked_lane = flights.keys().any(|(existing_key, existing_token)| {
                existing_key == &key && *existing_token != token
            });
            if let Some(existing) = flights.get(&lane_key).cloned() {
                (existing, false, forked_lane)
            } else {
                let state = Arc::new(FlightState {
                    inner: Mutex::new(FlightInner::Running {
                        owner: current_thread,
                    }),
                    ready: Condvar::new(),
                });
                flights.insert(lane_key.clone(), state.clone());
                (state, true, forked_lane)
            }
        };

        if leader {
            let result = compute().map(Arc::new);
            {
                let mut inner = state.inner.lock();
                *inner = FlightInner::Done(result.clone());
                state.ready.notify_all();
            }
            self.flights.lock().remove(&lane_key);
            return result.map(|value| SingleflightRunResult {
                value,
                role: SingleflightRole::Leader,
                forked_lane,
            });
        }

        let mut inner = state.inner.lock();
        loop {
            match &*inner {
                FlightInner::Running { owner } if *owner == current_thread => {
                    drop(inner);
                    return compute().map(|value| SingleflightRunResult {
                        value: Arc::new(value),
                        role: SingleflightRole::Leader,
                        forked_lane,
                    });
                }
                FlightInner::Running { .. } => state.ready.wait(&mut inner),
                FlightInner::Done(result) => {
                    return result.clone().map(|value| SingleflightRunResult {
                        value,
                        role: SingleflightRole::Follower,
                        forked_lane,
                    });
                }
            }
        }
    }

    pub fn clear(&self) {
        self.flights.lock().clear();
    }
}

// ---------------------------------------------------------------------------
// Observability counters
// ---------------------------------------------------------------------------

/// Atomic counters for resolver observability.
///
/// Thread-safe via `AtomicU64`. The resolver increments these during resolution;
/// consumers read snapshots via `snapshot()` for diagnostics, benchmarks, and tests.
#[derive(Debug, Default)]
pub struct ResolverCounters {
    /// Number of times a cached node result was reused (fact-validated hit).
    pub node_cache_hits: std::sync::atomic::AtomicU64,
    /// Number of times a node had to be recomputed (cache miss or stale).
    pub node_cache_misses: std::sync::atomic::AtomicU64,
    /// Number of times singleflight coalesced a follower onto an in-flight leader.
    pub singleflight_coalesces: std::sync::atomic::AtomicU64,
    /// Number of cycle detections during resolution.
    pub cycle_detections: std::sync::atomic::AtomicU64,
    /// Number of times incompatible StoreViews forked separate singleflight lanes.
    pub cross_view_lane_forks: std::sync::atomic::AtomicU64,
    /// Number of route/barrel fact reuses (cached route entries validated and reused).
    pub route_fact_reuses: std::sync::atomic::AtomicU64,
}

/// A non-atomic snapshot of `ResolverCounters` for reading/comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResolverCountersSnapshot {
    pub node_cache_hits: u64,
    pub node_cache_misses: u64,
    pub singleflight_coalesces: u64,
    pub cycle_detections: u64,
    pub cross_view_lane_forks: u64,
    pub route_fact_reuses: u64,
}

impl ResolverCounters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> ResolverCountersSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        ResolverCountersSnapshot {
            node_cache_hits: self.node_cache_hits.load(Relaxed),
            node_cache_misses: self.node_cache_misses.load(Relaxed),
            singleflight_coalesces: self.singleflight_coalesces.load(Relaxed),
            cycle_detections: self.cycle_detections.load(Relaxed),
            cross_view_lane_forks: self.cross_view_lane_forks.load(Relaxed),
            route_fact_reuses: self.route_fact_reuses.load(Relaxed),
        }
    }

    pub fn reset(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        self.node_cache_hits.store(0, Relaxed);
        self.node_cache_misses.store(0, Relaxed);
        self.singleflight_coalesces.store(0, Relaxed);
        self.cycle_detections.store(0, Relaxed);
        self.cross_view_lane_forks.store(0, Relaxed);
        self.route_fact_reuses.store(0, Relaxed);
    }

    #[inline]
    pub fn record_cache_hit(&self) {
        self.node_cache_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_cache_miss(&self) {
        self.node_cache_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_singleflight_coalesce(&self) {
        self.singleflight_coalesces
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_cycle_detection(&self) {
        self.cycle_detections
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_cross_view_lane_fork(&self) {
        self.cross_view_lane_forks
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_route_fact_reuse(&self) {
        self.route_fact_reuses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;
    use std::time::Duration;

    #[derive(Debug)]
    struct TestView {
        token: StoreViewCompatToken,
        valid_facts: FxHashSet<FactVersionRef>,
    }

    impl StoreView for TestView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }

        fn validates(&self, fact: &FactVersionRef) -> bool {
            self.valid_facts.contains(fact)
        }
    }

    struct TestRequestExecutor {
        key: String,
        cache: ValidatedFactCache<String, usize>,
        valid_fact: FactVersionRef,
        token: StoreViewCompatToken,
        compute_values: VecDeque<usize>,
        stability: VecDeque<bool>,
        published: Vec<usize>,
        computes: usize,
        max_attempts: usize,
        last_stable: bool,
    }

    impl TestRequestExecutor {
        fn new(key: &str, token: StoreViewCompatToken, max_attempts: usize) -> Self {
            Self {
                key: key.to_string(),
                cache: ValidatedFactCache::default(),
                valid_fact: FactVersionRef::FileWholeHash {
                    canonical_id: "/src/App.vue".to_string(),
                    hash: [1; 16],
                },
                token,
                compute_values: VecDeque::new(),
                stability: VecDeque::new(),
                published: Vec::new(),
                computes: 0,
                max_attempts,
                last_stable: true,
            }
        }

        fn view(&self) -> TestView {
            TestView {
                token: self.token,
                valid_facts: [self.valid_fact.clone()].into_iter().collect(),
            }
        }
    }

    impl StableRequestExecutor<String, usize> for TestRequestExecutor {
        type View = TestView;
        type Error = &'static str;

        fn cache_key(&self) -> String {
            self.key.clone()
        }

        fn snapshot_view(&mut self) -> Self::View {
            self.view()
        }

        fn try_get_cached(&mut self, view: &Self::View) -> Option<usize> {
            self.cache
                .get_if_valid(&self.key, view)
                .map(|cached| *cached)
        }

        fn compute(&mut self, _view: &Self::View) -> Result<usize, Self::Error> {
            self.computes += 1;
            self.last_stable = self.stability.pop_front().unwrap_or(true);
            self.compute_values
                .pop_front()
                .ok_or("missing compute value")
        }

        fn is_stable(&mut self, _view: &Self::View) -> bool {
            self.last_stable
        }

        fn store_stable(&mut self, value: &usize) {
            self.published.push(*value);
            self.cache
                .insert(self.key.clone(), *value, vec![self.valid_fact.clone()]);
        }

        fn max_attempts(&self) -> usize {
            self.max_attempts
        }
    }

    #[test]
    fn validated_cache_reuses_entry_when_all_facts_match() {
        let cache = ValidatedFactCache::<String, usize>::default();
        let fact = FactVersionRef::FileWholeHash {
            canonical_id: "/src/App.vue".to_string(),
            hash: [7; 16],
        };
        cache.insert("node".to_string(), 42, vec![fact.clone()]);

        let view = TestView {
            token: StoreViewCompatToken {
                epoch: 3,
                session: None,
            },
            valid_facts: [fact].into_iter().collect(),
        };

        assert_eq!(
            cache.get_if_valid(&"node".to_string(), &view),
            Some(Arc::new(42))
        );
    }

    #[test]
    fn validated_cache_rejects_entry_when_any_fact_mismatches() {
        let cache = ValidatedFactCache::<String, usize>::default();
        cache.insert(
            "node".to_string(),
            42,
            vec![FactVersionRef::FileWholeHash {
                canonical_id: "/src/index.ts".to_string(),
                hash: [99u8; 16],
            }],
        );

        let view = TestView {
            token: StoreViewCompatToken {
                epoch: 4,
                session: None,
            },
            valid_facts: FxHashSet::default(),
        };

        assert!(cache.get_if_valid(&"node".to_string(), &view).is_none());
    }

    #[test]
    fn compat_token_is_exact_snapshot_epoch_in_v1() {
        let first = StoreViewCompatToken {
            epoch: 10,
            session: None,
        };
        let second = StoreViewCompatToken {
            epoch: 10,
            session: None,
        };
        let third = StoreViewCompatToken {
            epoch: 11,
            session: None,
        };

        assert_eq!(first, second);
        assert_ne!(first, third);
    }

    #[test]
    fn stable_request_returns_cached_value_before_compute() {
        let singleflight =
            SingleflightGroup::<String, StableExecutionValue<usize>, &'static str>::default();
        let mut executor = TestRequestExecutor::new(
            "node",
            StoreViewCompatToken {
                epoch: 5,
                session: None,
            },
            3,
        );
        executor
            .cache
            .insert("node".to_string(), 41, vec![executor.valid_fact.clone()]);

        let result = run_stable_request(&singleflight, &mut executor).unwrap();

        assert_eq!(result.value, 41);
        assert_eq!(result.source, RequestSource::Cache);
        assert_eq!(result.attempts, 1);
        assert_eq!(executor.computes, 0);
        assert!(executor.published.is_empty());
    }

    #[test]
    fn stable_request_retries_until_compute_is_stable() {
        let singleflight =
            SingleflightGroup::<String, StableExecutionValue<usize>, &'static str>::default();
        let mut executor = TestRequestExecutor::new(
            "node",
            StoreViewCompatToken {
                epoch: 5,
                session: None,
            },
            3,
        );
        executor.compute_values.extend([11, 12]);
        executor.stability.extend([false, true]);

        let result = run_stable_request(&singleflight, &mut executor).unwrap();

        assert_eq!(result.value, 12);
        assert_eq!(
            result.source,
            RequestSource::Flight {
                role: SingleflightRole::Leader,
                forked_lane: false,
            }
        );
        assert_eq!(result.attempts, 2);
        assert_eq!(executor.computes, 2);
        assert_eq!(executor.published, vec![12]);
        assert_eq!(
            executor
                .cache
                .get_if_valid(&"node".to_string(), &executor.view())
                .map(|cached| *cached),
            Some(12)
        );
    }

    #[test]
    fn stable_request_uses_fallback_after_retries_exhausted() {
        let singleflight =
            SingleflightGroup::<String, StableExecutionValue<usize>, &'static str>::default();
        let mut executor = TestRequestExecutor::new(
            "node",
            StoreViewCompatToken {
                epoch: 5,
                session: None,
            },
            2,
        );
        executor.compute_values.extend([1, 2, 3]);
        executor.stability.extend([false, false, false]);

        let result = run_stable_request(&singleflight, &mut executor).unwrap();

        assert_eq!(result.value, 3);
        assert_eq!(result.source, RequestSource::Fallback);
        assert_eq!(result.attempts, 3);
        assert_eq!(executor.computes, 3);
        assert!(executor.published.is_empty());
    }

    #[test]
    fn singleflight_coalesces_same_key_and_token() {
        let group = Arc::new(SingleflightGroup::<String, usize, &'static str>::default());
        let start = Arc::new(Barrier::new(3));
        let computes = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..2)
            .map(|_| {
                let group = Arc::clone(&group);
                let start = Arc::clone(&start);
                let computes = Arc::clone(&computes);
                std::thread::spawn(move || {
                    start.wait();
                    group
                        .run(
                            "node".to_string(),
                            StoreViewCompatToken {
                                epoch: 7,
                                session: None,
                            },
                            || {
                                computes.fetch_add(1, Ordering::SeqCst);
                                std::thread::sleep(Duration::from_millis(50));
                                Ok(42)
                            },
                        )
                        .unwrap()
                })
            })
            .collect();

        start.wait();
        let mut handles = handles.into_iter();
        let first = handles.next().unwrap().join().unwrap();
        let second = handles.next().unwrap().join().unwrap();

        assert_eq!(computes.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(&first.value, &second.value));
        assert_ne!(first.role, second.role);
        assert_eq!(
            [first.role, second.role]
                .into_iter()
                .filter(|role| *role == SingleflightRole::Leader)
                .count(),
            1
        );
    }

    #[test]
    fn singleflight_forks_incompatible_tokens() {
        let group = Arc::new(SingleflightGroup::<String, usize, &'static str>::default());
        let start = Arc::new(Barrier::new(3));
        let computes = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = [
            StoreViewCompatToken {
                epoch: 1,
                session: None,
            },
            StoreViewCompatToken {
                epoch: 2,
                session: None,
            },
        ]
        .into_iter()
        .map(|token| {
            let group = Arc::clone(&group);
            let start = Arc::clone(&start);
            let computes = Arc::clone(&computes);
            std::thread::spawn(move || {
                start.wait();
                group
                    .run("node".to_string(), token, || {
                        computes.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(50));
                        Ok(token.epoch as usize)
                    })
                    .unwrap()
            })
        })
        .collect();

        start.wait();
        let mut handles = handles.into_iter();
        let first = handles.next().unwrap().join().unwrap();
        let second = handles.next().unwrap().join().unwrap();

        assert_eq!(computes.load(Ordering::SeqCst), 2);
        assert_eq!(first.role, SingleflightRole::Leader);
        assert_eq!(second.role, SingleflightRole::Leader);
        assert!(first.forked_lane || second.forked_lane);
        assert!(!Arc::ptr_eq(&first.value, &second.value));
    }

    // -----------------------------------------------------------------------
    // ResolverCounters tests
    // -----------------------------------------------------------------------

    #[test]
    fn resolver_counters_default_is_zero() {
        let counters = ResolverCounters::new();
        let snap = counters.snapshot();
        assert_eq!(snap.node_cache_hits, 0);
        assert_eq!(snap.node_cache_misses, 0);
        assert_eq!(snap.singleflight_coalesces, 0);
        assert_eq!(snap.cycle_detections, 0);
        assert_eq!(snap.cross_view_lane_forks, 0);
        assert_eq!(snap.route_fact_reuses, 0);
    }

    #[test]
    fn resolver_counters_increment_and_snapshot() {
        let counters = ResolverCounters::new();
        counters.record_cache_hit();
        counters.record_cache_hit();
        counters.record_cache_miss();
        counters.record_singleflight_coalesce();
        counters.record_cycle_detection();
        counters.record_cross_view_lane_fork();
        counters.record_route_fact_reuse();
        counters.record_route_fact_reuse();
        counters.record_route_fact_reuse();

        let snap = counters.snapshot();
        assert_eq!(snap.node_cache_hits, 2);
        assert_eq!(snap.node_cache_misses, 1);
        assert_eq!(snap.singleflight_coalesces, 1);
        assert_eq!(snap.cycle_detections, 1);
        assert_eq!(snap.cross_view_lane_forks, 1);
        assert_eq!(snap.route_fact_reuses, 3);
    }

    #[test]
    fn resolver_counters_reset_clears_all() {
        let counters = ResolverCounters::new();
        counters.record_cache_hit();
        counters.record_cache_miss();
        counters.record_singleflight_coalesce();

        counters.reset();
        let snap = counters.snapshot();
        assert_eq!(snap, ResolverCountersSnapshot::default());
    }

    #[test]
    fn resolver_counters_thread_safe() {
        let counters = Arc::new(ResolverCounters::new());
        let barrier = Arc::new(Barrier::new(4));

        let handles: Vec<_> = (0..3)
            .map(|_| {
                let counters = Arc::clone(&counters);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..100 {
                        counters.record_cache_hit();
                        counters.record_cache_miss();
                    }
                })
            })
            .collect();

        barrier.wait();
        for h in handles {
            h.join().unwrap();
        }

        let snap = counters.snapshot();
        assert_eq!(snap.node_cache_hits, 300);
        assert_eq!(snap.node_cache_misses, 300);
    }

    #[test]
    fn resolver_counters_snapshot_is_not_default_after_recording() {
        let counters = ResolverCounters::new();
        counters.record_cache_hit();
        let snap = counters.snapshot();
        assert_ne!(
            snap,
            ResolverCountersSnapshot::default(),
            "snapshot should differ from default after recording"
        );
    }
}
