//! Fallthrough/inheritance resolution with persistent node caching.
//!
//! The fallthrough resolver handles component attribute inheritance through the
//! template's root element chain. It uses [`FallthroughNodeKey`] for caching,
//! where cache keys are based on component identity + override fingerprint
//! (not symbol identity).
//!
//! # Cross-Subsystem Cycle Safety
//!
//! Fallthrough resolution may trigger symbol resolution (to expand a child
//! component's type surface) and symbol resolution may trigger fallthrough
//! resolution (to compute accepted surfaces). Both subsystems share a single
//! [`ResolveContext`] per root request, which carries tagged recursion stacks
//! for both subsystems. This prevents deadlock and false cycle cuts.

use std::sync::Arc;

use crate::resolver_core::{
    symbol_resolver::ResolveContext, FactVersionRef, FallthroughNodeKey, FallthroughNodeKind,
    ResolverCounters, ResolverDiagnostic, SingleflightGroup, StableExecutionValue, StoreView,
    ValidatedFactCache,
};
use verter_semantic::analysis::component_meta::{
    AcceptedEventAnalysis, AcceptedPropAnalysis, AcceptedSurfaceCompleteness, FallthroughSurface,
};

#[derive(Debug, Clone)]
pub enum FallthroughNodeValue {
    RootFollow(RootFollowResult),
    IntrinsicSurface(IntrinsicSurfaceResult),
    ChildSurfaceFollow(ChildSurfaceResult),
    ConsumedBindings(ConsumedBindingsResult),
    BranchUnion(BranchUnionResult),
}

#[derive(Debug, Clone)]
pub struct RootFollowResult {
    pub accepted_props: Vec<AcceptedPropAnalysis>,
    pub accepted_events: Vec<AcceptedEventAnalysis>,
    pub accepted_surface_completeness: AcceptedSurfaceCompleteness,
    pub fallthrough_surface: FallthroughSurface,
    pub has_single_root: bool,
    pub branches: Vec<FallthroughBranchResult>,
}

impl Default for RootFollowResult {
    fn default() -> Self {
        Self {
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness: AcceptedSurfaceCompleteness::LowerBound,
            fallthrough_surface: FallthroughSurface::None {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot,
            },
            has_single_root: false,
            branches: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FallthroughBranchResult {
    pub branch_key: String,
    pub inherited_prop_names: Vec<String>,
    pub inherited_event_names: Vec<String>,
    pub resolved: bool,
}

#[derive(Debug, Clone, Default)]
pub struct IntrinsicSurfaceResult {
    pub members: Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>,
    pub attr_names: Vec<String>,
    pub event_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ChildSurfaceResult {
    pub accepted_props: Vec<AcceptedPropAnalysis>,
    pub accepted_events: Vec<AcceptedEventAnalysis>,
    pub accepted_surface_completeness: AcceptedSurfaceCompleteness,
    pub fallthrough_surface: FallthroughSurface,
    pub inherited_prop_names: Vec<String>,
    pub inherited_event_names: Vec<String>,
    pub resolved: bool,
}

impl Default for ChildSurfaceResult {
    fn default() -> Self {
        Self {
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness: AcceptedSurfaceCompleteness::LowerBound,
            fallthrough_surface: FallthroughSurface::None {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot,
            },
            inherited_prop_names: Vec::new(),
            inherited_event_names: Vec::new(),
            resolved: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConsumedBindingsResult {
    pub attrs: Vec<String>,
    pub listeners: Vec<String>,
    pub has_dynamic_attr_name: bool,
    pub has_dynamic_listener_name: bool,
    pub partial_reasons: Vec<verter_semantic::analysis::component_meta::PartialBranchReason>,
    pub consumed_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BranchUnionResult {
    pub accepted_props: Vec<AcceptedPropAnalysis>,
    pub accepted_events: Vec<AcceptedEventAnalysis>,
    pub accepted_surface_completeness: AcceptedSurfaceCompleteness,
    pub fallthrough_surface: FallthroughSurface,
    pub branches: Vec<FallthroughBranchResult>,
    pub all_resolved: bool,
}

impl Default for BranchUnionResult {
    fn default() -> Self {
        Self {
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness: AcceptedSurfaceCompleteness::LowerBound,
            fallthrough_surface: FallthroughSurface::None {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot,
            },
            branches: Vec::new(),
            all_resolved: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FallthroughNodeResult {
    pub value: FallthroughNodeValue,
    pub facts: Vec<FactVersionRef>,
    pub diagnostics: Vec<ResolverDiagnostic>,
}

pub struct FallthroughResolverState {
    cache: ValidatedFactCache<FallthroughNodeKey, FallthroughNodeResult>,
    singleflight:
        SingleflightGroup<FallthroughNodeKey, StableExecutionValue<FallthroughNodeResult>, ()>,
    counters: Arc<ResolverCounters>,
}

impl FallthroughResolverState {
    pub fn new(counters: Arc<ResolverCounters>) -> Self {
        Self {
            cache: ValidatedFactCache::default(),
            singleflight: SingleflightGroup::default(),
            counters,
        }
    }

    pub fn clear_cache(&self) {
        self.cache.clear();
        self.singleflight.clear();
    }

    pub fn remove_node_for_test(&self, key: &FallthroughNodeKey) {
        self.cache.remove(key);
    }

    pub fn counters(&self) -> &ResolverCounters {
        &self.counters
    }

    pub fn get_cached_node<V>(
        &self,
        key: &FallthroughNodeKey,
        view: &V,
    ) -> Option<FallthroughNodeResult>
    where
        V: StoreView,
    {
        if let Some(cached) = self.cache.get_if_valid(key, view) {
            self.counters.record_cache_hit();
            return Some((*cached).clone());
        }
        self.counters.record_cache_miss();
        None
    }

    pub fn store_node(&self, key: FallthroughNodeKey, result: FallthroughNodeResult) {
        if !result.facts.is_empty()
            || matches!(
                result.value,
                FallthroughNodeValue::IntrinsicSurface(_)
                    | FallthroughNodeValue::ConsumedBindings(_)
            )
        {
            self.cache.insert(key, result.clone(), result.facts.clone());
        }
    }

    pub fn resolve_node<V, F>(
        &self,
        key: FallthroughNodeKey,
        view: &V,
        ctx: &mut ResolveContext,
        compute_fn: F,
    ) -> FallthroughNodeResult
    where
        V: StoreView,
        F: FnOnce(&mut ResolveContext) -> FallthroughNodeResult,
    {
        if let Some(cached) = self.cache.get_if_valid(&key, view) {
            self.counters.record_cache_hit();
            return (*cached).clone();
        }
        self.counters.record_cache_miss();

        if ctx.fallthrough_visiting.contains(&key) {
            self.counters.record_cycle_detection();
            return FallthroughNodeResult {
                value: FallthroughNodeValue::RootFollow(RootFollowResult::default()),
                facts: vec![],
                diagnostics: vec![ResolverDiagnostic {
                    code: "fallthrough-cycle".to_string(),
                    message: format!(
                        "Cycle detected in fallthrough resolution for {}::{:?}",
                        key.canonical_component_id, key.node_kind
                    ),
                    canonical_path: Some(key.canonical_component_id.clone()),
                    span_start: None,
                }],
            };
        }

        let token = view.compat_token();
        let result = self.singleflight.run(key.clone(), token, || {
            ctx.fallthrough_visiting.insert(key.clone());
            let result = compute_fn(ctx);
            ctx.fallthrough_visiting.remove(&key);

            let stable = !result.facts.is_empty()
                || matches!(
                    result.value,
                    FallthroughNodeValue::IntrinsicSurface(_)
                        | FallthroughNodeValue::ConsumedBindings(_)
                );
            if stable {
                self.cache
                    .insert(key.clone(), result.clone(), result.facts.clone());
            }

            Ok(StableExecutionValue {
                value: result,
                stable,
                // This singleflight closure always runs `compute_fn`
                // (no in-closure warm-hit short-circuit), so the winner
                // always performed a cold build.
                computed: true,
            })
        });

        match result {
            Ok(flight) => {
                if flight.role == crate::resolver_core::SingleflightRole::Follower {
                    self.counters.record_singleflight_coalesce();
                }
                if flight.forked_lane {
                    self.counters.record_cross_view_lane_fork();
                }
                flight.value.value.clone()
            }
            Err(()) => FallthroughNodeResult {
                value: FallthroughNodeValue::RootFollow(RootFollowResult::default()),
                facts: vec![],
                diagnostics: vec![],
            },
        }
    }
}

pub fn root_follow_key(
    canonical_component_id: &str,
    override_fingerprint: u64,
    generic_propagation: bool,
) -> FallthroughNodeKey {
    FallthroughNodeKey {
        canonical_component_id: canonical_component_id.to_string(),
        node_kind: FallthroughNodeKind::ComponentRootFollow,
        override_fingerprint,
        behavior_flags: if generic_propagation { 1 } else { 0 },
        branch_selector: None,
    }
}

pub fn intrinsic_surface_key(
    project_anchor: &str,
    cache_generation: u64,
    tag: &str,
) -> FallthroughNodeKey {
    FallthroughNodeKey {
        canonical_component_id: project_anchor.to_string(),
        node_kind: FallthroughNodeKind::IntrinsicSurfaceLoad,
        override_fingerprint: cache_generation,
        behavior_flags: 0,
        branch_selector: Some(tag.to_string()),
    }
}

pub fn child_surface_key(
    canonical_component_id: &str,
    override_fingerprint: u64,
) -> FallthroughNodeKey {
    FallthroughNodeKey {
        canonical_component_id: canonical_component_id.to_string(),
        node_kind: FallthroughNodeKind::ChildComponentSurfaceFollow,
        override_fingerprint,
        behavior_flags: 0,
        branch_selector: None,
    }
}

pub fn consumed_bindings_key(canonical_component_id: &str, branch_key: &str) -> FallthroughNodeKey {
    FallthroughNodeKey {
        canonical_component_id: canonical_component_id.to_string(),
        node_kind: FallthroughNodeKind::ConsumedBindingEvaluation,
        override_fingerprint: 0,
        behavior_flags: 0,
        branch_selector: Some(branch_key.to_string()),
    }
}

pub fn branch_union_key(
    canonical_component_id: &str,
    override_fingerprint: u64,
) -> FallthroughNodeKey {
    FallthroughNodeKey {
        canonical_component_id: canonical_component_id.to_string(),
        node_kind: FallthroughNodeKind::BranchUnionMerge,
        override_fingerprint,
        behavior_flags: 0,
        branch_selector: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::StoreViewCompatToken;
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

    fn make_fact(id: &str) -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: id.to_string(),
            hash: [1; 16],
        }
    }

    fn make_view(token: u64, facts: Vec<FactVersionRef>) -> TestView {
        TestView {
            token: StoreViewCompatToken {
                epoch: token,
                session: None,
                validity_fingerprint: 0,
            },
            valid_facts: facts.into_iter().collect(),
        }
    }

    #[test]
    fn resolve_node_caches_and_reuses_on_valid_facts() {
        let counters = Arc::new(ResolverCounters::new());
        let state = FallthroughResolverState::new(counters.clone());
        let fact = make_fact("/src/Child.vue");
        let view = make_view(1, vec![fact.clone()]);
        let key = root_follow_key("/src/App.vue", 0, false);

        let mut ctx = ResolveContext::new();
        state.resolve_node(key.clone(), &view, &mut ctx, |_| FallthroughNodeResult {
            value: FallthroughNodeValue::RootFollow(RootFollowResult {
                has_single_root: true,
                branches: vec![],
                ..RootFollowResult::default()
            }),
            facts: vec![fact.clone()],
            diagnostics: vec![],
        });
        assert_eq!(counters.snapshot().node_cache_misses, 1);

        let mut ctx2 = ResolveContext::new();
        let result = state.resolve_node(key.clone(), &view, &mut ctx2, |_| {
            panic!("should not recompute");
        });
        assert!(matches!(
            result.value,
            FallthroughNodeValue::RootFollow(ref r) if r.has_single_root
        ));
        assert_eq!(counters.snapshot().node_cache_hits, 1);
    }

    #[test]
    fn resolve_node_detects_fallthrough_cycle() {
        let counters = Arc::new(ResolverCounters::new());
        let state = FallthroughResolverState::new(counters.clone());
        let view = make_view(1, vec![]);
        let key = root_follow_key("/src/Recursive.vue", 0, false);

        let mut ctx = ResolveContext::new();
        ctx.fallthrough_visiting.insert(key.clone());

        let result = state.resolve_node(key.clone(), &view, &mut ctx, |_| {
            panic!("should not compute for cycle");
        });

        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "fallthrough-cycle");
        assert_eq!(counters.snapshot().cycle_detections, 1);
    }

    #[test]
    fn cross_subsystem_visiting_sets_are_independent() {
        let mut ctx = ResolveContext::new();

        let symbol_key = crate::resolver_core::ResolutionNodeKey {
            symbol_id: "/src/types.ts#Props".to_string(),
            node_kind: crate::resolver_core::ResolutionNodeKind::SymbolExpand,
            traversal_lens: crate::resolver_core::TraversalLens::StructuralObject,
            member_path_hash: 0,
            type_args_hash: 0,
            behavior_flags: 0,
            view_fingerprint: 0,
        };
        ctx.visiting.insert(symbol_key.clone());

        let ft_key = root_follow_key("/src/App.vue", 0, false);
        ctx.fallthrough_visiting.insert(ft_key.clone());

        assert!(ctx.visiting.contains(&symbol_key));
        assert!(ctx.fallthrough_visiting.contains(&ft_key));

        ctx.visiting.remove(&symbol_key);
        assert!(!ctx.visiting.contains(&symbol_key));
        assert!(ctx.fallthrough_visiting.contains(&ft_key));
    }

    #[test]
    fn root_follow_key_uses_override_fingerprint() {
        let key_a = root_follow_key("/src/App.vue", 0, false);
        let key_b = root_follow_key("/src/App.vue", 42, false);
        assert_ne!(
            key_a, key_b,
            "different override fingerprints should differ"
        );
    }

    #[test]
    fn root_follow_key_uses_generic_propagation() {
        let key_a = root_follow_key("/src/App.vue", 0, false);
        let key_b = root_follow_key("/src/App.vue", 0, true);
        assert_ne!(
            key_a, key_b,
            "different generic propagation flags should differ"
        );
    }

    #[test]
    fn intrinsic_surface_key_keyed_by_tag() {
        let key_div = intrinsic_surface_key("/workspace|/workspace/tsconfig.json", 7, "div");
        let key_span = intrinsic_surface_key("/workspace|/workspace/tsconfig.json", 7, "span");
        assert_ne!(key_div, key_span);

        let key_div2 = intrinsic_surface_key("/workspace|/workspace/tsconfig.json", 7, "div");
        assert_eq!(key_div, key_div2);

        let key_other_project = intrinsic_surface_key("/other|/other/tsconfig.json", 7, "div");
        assert_ne!(
            key_div, key_other_project,
            "project-owned intrinsic caches must not be shared across projects"
        );
    }

    #[test]
    fn consumed_bindings_key_keyed_by_branch() {
        let key_a = consumed_bindings_key("/src/App.vue", "0");
        let key_b = consumed_bindings_key("/src/App.vue", "0.1");
        assert_ne!(key_a, key_b);
    }
}
