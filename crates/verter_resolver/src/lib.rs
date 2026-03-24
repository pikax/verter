use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;
use std::hash::Hash;
use std::sync::Arc;

mod component_meta;
mod declaration_metadata;
mod eval_env_build;
mod external_type_body;
mod fallthrough;
mod imported_decl_eval;
mod imported_eval_collect;
mod imported_eval_lookup;
mod imported_eval_types;
mod imported_type_alias;
mod runtime_values;
mod surface_projector;

pub type ResolverHash16 = verter_analysis::Hash16;
pub use component_meta::{
    resolve_component_meta_parts, resolved_elements_to_type_expr_via_type_text,
    ComponentMetaEvalOutputs, ComponentMetaResolverHost, ResolvedComponentMetaParts,
    ResolvedJsdocBlock, ResolvedJsdocTag, ResolvedMacroMeta, ResolvedTypeRegistryMeta,
};
pub use declaration_metadata::{
    resolve_local_type_declaration, resolve_type_declaration, DeclarationMetadataResolver,
    ResolvedDeclarationKind, ResolvedExportTarget, ResolvedTypeDeclaration,
};
pub use eval_env_build::{collect_requested_binding_names, inject_imported_type_aliases};
pub use external_type_body::{
    resolve_external_type_from_source_body, ExternalTypeBodyCache, ExternalTypeBodyResolver,
};
pub use fallthrough::{
    append_component_candidate_branches, append_native_candidate_branch,
    collect_dynamic_root_candidates_from_type, extend_unique_fact_versions,
    fallthrough_cache_key, hash_prop_type_overrides, known_spread_keys_from_type_expr,
    inject_prop_type_overrides, merge_fallthrough_branches, push_partial_reason,
    resolve_fallthrough_surface, resolve_usage_prop_type, DynamicRootCandidate,
    FallthroughComputeHost, FallthroughResolutionView, FallthroughResolverHost,
    KnownSpreadKeys, ResolvedConsumedBindings, ResolvedFallthroughSurface,
};
pub use imported_decl_eval::{
    evaluate_imported_decl_with_owner_env, ImportedDeclEvalResolver, PreparedImportedDeclContext,
};
pub use imported_eval_collect::{
    build_imported_eval_inputs, collect_imported_eval_inputs, imported_member_name_for_type_alias,
    record_required_source_merge_inputs_recursive, required_type_alias_names_for_import_binding,
    ImportedEvalBinding, ImportedEvalCollectorResolver, ImportedEvalOwnerResolver,
    ImportedEvalOwnerSnapshot, ImportedEvalSourceMergeResolver, ImportedEvalTraversalBudget,
};
pub use imported_eval_lookup::{
    ImportedEvalLookup, ImportedEvalLookupResolver, ImportedTypeAliasResolveRequest,
};
pub use imported_eval_types::{
    ComputedEvaluatedTypes, ImportedEvalInputs, ImportedEvalOverflow, ImportedEvalSource,
    ImportedTypeAlias,
};
pub use imported_type_alias::{
    choose_preferred_imported_type_body, imported_type_body_specificity_score,
    prepare_imported_type_alias, should_attempt_owner_env_resolution,
    ImportedTypeAliasPrepareError, ImportedTypeAliasResolver,
};
pub use runtime_values::{
    materialize_imported_runtime_values_into_env, ImportedRuntimeValueResolver,
};
pub use surface_projector::{
    extract_slot_info_from_type_text, project_macro_surfaces, ProjectedMacroSurfaces,
    ResolvedNativeProp,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreViewCompatToken(pub u64);

pub trait StoreView {
    fn compat_token(&self) -> StoreViewCompatToken;
    fn validates(&self, fact: &FactVersionRef) -> bool;
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
    ExportRegistry,
    Route,
    BarrelSurface,
    ExactResolution,
    DirectSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FactVersionRef {
    FileWholeHash {
        canonical_id: String,
        hash: ResolverHash16,
    },
    BarrelGeneration {
        canonical_id: String,
        generation: u64,
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
    Route,
    BarrelLookup,
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
}

impl<K, V> Default for ValidatedFactCache<K, V>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self {
            entries: Mutex::new(FxHashMap::default()),
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
        let entry = entries.get(key)?;
        if entry.facts.iter().all(|fact| view.validates(fact)) {
            Some(entry.value.clone())
        } else {
            None
        }
    }

    pub fn insert(&self, key: K, value: V, facts: Vec<FactVersionRef>) {
        self.insert_arc(key, Arc::new(value), facts);
    }

    pub fn insert_arc(&self, key: K, value: Arc<V>, facts: Vec<FactVersionRef>) {
        self.entries
            .lock()
            .insert(key, ValidatedEntry { value, facts });
    }

    pub fn clear(&self) {
        self.entries.lock().clear();
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
            token: StoreViewCompatToken(3),
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
            vec![FactVersionRef::BarrelGeneration {
                canonical_id: "/src/index.ts".to_string(),
                generation: 9,
            }],
        );

        let view = TestView {
            token: StoreViewCompatToken(4),
            valid_facts: FxHashSet::default(),
        };

        assert!(cache.get_if_valid(&"node".to_string(), &view).is_none());
    }

    #[test]
    fn compat_token_is_exact_snapshot_epoch_in_v1() {
        let first = StoreViewCompatToken(10);
        let second = StoreViewCompatToken(10);
        let third = StoreViewCompatToken(11);

        assert_eq!(first, second);
        assert_ne!(first, third);
    }

    #[test]
    fn stable_request_returns_cached_value_before_compute() {
        let singleflight =
            SingleflightGroup::<String, StableExecutionValue<usize>, &'static str>::default();
        let mut executor = TestRequestExecutor::new("node", StoreViewCompatToken(5), 3);
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
        let mut executor = TestRequestExecutor::new("node", StoreViewCompatToken(5), 3);
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
        let mut executor = TestRequestExecutor::new("node", StoreViewCompatToken(5), 2);
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
                        .run("node".to_string(), StoreViewCompatToken(7), || {
                            computes.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(50));
                            Ok(42)
                        })
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

        let handles: Vec<_> = [StoreViewCompatToken(1), StoreViewCompatToken(2)]
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
                            Ok(token.0 as usize)
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
}
