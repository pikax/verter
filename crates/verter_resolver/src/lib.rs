use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use std::hash::Hash;
use std::sync::Arc;

pub type ResolverHash16 = verter_analysis::Hash16;

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

#[derive(Debug, Default)]
pub struct ValidatedFactCache<K, V>
where
    K: Eq + Hash,
{
    entries: Mutex<FxHashMap<K, ValidatedEntry<V>>>,
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
        self.entries.lock().insert(
            key,
            ValidatedEntry {
                value: Arc::new(value),
                facts,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;

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
}
