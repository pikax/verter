//! Fallthrough/inheritance resolution with persistent node caching.
//!
//! The fallthrough resolver handles component attribute inheritance through the
//! template's root element chain. It uses [`FallthroughNodeKey`] for caching,
//! where cache keys are based on component identity + override identity
//! (not symbol identity).
//!
//! # Cross-Subsystem Cycle Safety
//!
//! Fallthrough resolution may trigger symbol resolution (to expand a child
//! component's type surface) and symbol resolution may trigger fallthrough
//! resolution (to compute accepted surfaces). Both subsystems share a single
//! [`crate::resolver_core::symbol_resolver::ResolveContext`] per root request,
//! which carries tagged recursion stacks for both subsystems. This prevents
//! deadlock and false cycle cuts.

use std::sync::Arc;

use crate::resolver_core::{
    FactVersionRef, FallthroughNodeKey, FallthroughOverrideIdentity, ResolverCounters,
    ResolverDiagnostic, StoreView, ValidatedFactCache,
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
    pub members: Vec<crate::resolver_core::IntrinsicSurfaceMember>,
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
    counters: Arc<ResolverCounters>,
}

impl FallthroughResolverState {
    pub fn new(counters: Arc<ResolverCounters>) -> Self {
        Self {
            cache: ValidatedFactCache::default(),
            counters,
        }
    }

    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    pub fn remove_node_for_test(&self, key: &FallthroughNodeKey) {
        self.cache.remove(key);
    }

    /// Number of KEYS currently warm in the fallthrough node cache.
    ///
    /// The admission observable: a no-poison refusal is visible as a count that
    /// does NOT grow across a compute the caller was still served. Reading the
    /// count (rather than a warm `get_cached_node`) keeps the assertion
    /// independent of the read-side fact validation — an empty-signature
    /// candidate validates vacuously, so a warm-read probe could not
    /// distinguish "refused" from "admitted but stale".
    #[cfg(test)]
    pub fn cached_node_count(&self) -> usize {
        self.cache.len()
    }

    /// The candidate count currently warm under `key` — `0` when the key was
    /// never admitted (or was refused).
    #[cfg(test)]
    pub fn cached_candidate_count(&self, key: &FallthroughNodeKey) -> usize {
        self.cache.candidate_signatures_for_key(key).len()
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
        // An override-bearing key whose identity is `Uncacheable` is never
        // stored, so it can never hit — and reading through it must not
        // alias another override set's warm entry.
        if !key.is_cacheable() {
            self.counters.record_cache_miss();
            return None;
        }
        if let Some(cached) = self.cache.get_if_valid(key, view) {
            self.counters.record_cache_hit();
            return Some((*cached).clone());
        }
        self.counters.record_cache_miss();
        None
    }

    /// Warm the fallthrough node cache with `result`.
    ///
    /// `probe` is the [`CacheabilityProbe`](crate::fact_signature_helpers::CacheabilityProbe)
    /// of the cacheability tracer scope that ENCLOSES the compute which
    /// produced `result`. It cannot be forged — `with_cacheability_scope` is
    /// its only constructor — so an untraced producer cannot reach this funnel
    /// at all.
    ///
    /// THREE independent no-poison rails, all fail-closed:
    ///
    /// 1. **uncacheable key** — an override-bearing key whose identity is
    ///    `Uncacheable` would alias two genuinely-different override sets.
    /// 2. **non-cacheable compute** (`probe.non_cacheable()`) — the compute
    ///    consumed a FENCED (ReturnOnly, `store_published == false`) serve, a
    ///    broken decl-body lease, an unrootable import route, or an
    ///    unobservable contributor source env; or its observation set
    ///    overflowed the fact-signature cap. Those reasons are CONTENT-NEUTRAL:
    ///    the artifacts stay published and content-current, so an admitted
    ///    entry would root on the LIVE hashes and revalidate on every warm read
    ///    FOREVER — nothing downstream can reject it. The value is still SERVED
    ///    to the caller verbatim; only the shared-cache admission is refused.
    ///    A non-cacheable read is NEVER a `ResultCompleteness::Partial`.
    /// 3. **partial result** — a budget/fuse trip folded into the active
    ///    cold-compute completeness scope. The typed completeness signal is the
    ///    no-poison rail shared with the component-meta materialiser, not a
    ///    fallthrough-private predicate.
    pub fn store_node(
        &self,
        key: FallthroughNodeKey,
        result: FallthroughNodeResult,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
    ) {
        if !key.is_cacheable() {
            return;
        }
        if probe.non_cacheable() {
            return;
        }
        if crate::cache_runtime::refuse_result_cache_admission_if_partial(
            crate::request_context::current_cold_compute_completeness().is_partial(),
        ) {
            return;
        }
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
}

pub fn root_follow_key(
    canonical_component_id: &str,
    overrides: FallthroughOverrideIdentity,
    generic_propagation: bool,
) -> FallthroughNodeKey {
    FallthroughNodeKey::ComponentRootFollow {
        canonical: canonical_component_id.to_string(),
        overrides,
        generic_root_propagation: generic_propagation,
    }
}

pub fn intrinsic_surface_key(
    project_anchor: &str,
    cache_generation: u64,
    tag: &str,
) -> FallthroughNodeKey {
    FallthroughNodeKey::IntrinsicSurfaceLoad {
        project_anchor: project_anchor.to_string(),
        cache_generation,
        tag: tag.to_string(),
    }
}

pub fn child_surface_key(
    canonical_component_id: &str,
    overrides: FallthroughOverrideIdentity,
) -> FallthroughNodeKey {
    FallthroughNodeKey::ChildComponentSurfaceFollow {
        canonical: canonical_component_id.to_string(),
        overrides,
    }
}

pub fn consumed_bindings_key(
    canonical_component_id: &str,
    branch_key: &str,
    overrides: FallthroughOverrideIdentity,
) -> FallthroughNodeKey {
    FallthroughNodeKey::ConsumedBindingEvaluation {
        canonical: canonical_component_id.to_string(),
        branch_key: branch_key.to_string(),
        overrides,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::symbol_resolver::ResolveContext;

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

        let ft_key = root_follow_key(
            "/src/App.vue",
            FallthroughOverrideIdentity::NoOverrides,
            false,
        );
        ctx.fallthrough_visiting.insert(ft_key.clone());

        assert!(ctx.visiting.contains(&symbol_key));
        assert!(ctx.fallthrough_visiting.contains(&ft_key));

        ctx.visiting.remove(&symbol_key);
        assert!(!ctx.visiting.contains(&symbol_key));
        assert!(ctx.fallthrough_visiting.contains(&ft_key));
    }

    #[test]
    fn root_follow_key_uses_override_identity() {
        // Wholesale-uncacheable: the only non-`NoOverrides` identity is
        // `Uncacheable`; an override-bearing key differs from the no-override
        // key and is not cacheable, so it can never be reused as the
        // no-override surface.
        let key_a = root_follow_key(
            "/src/App.vue",
            FallthroughOverrideIdentity::NoOverrides,
            false,
        );
        let key_b = root_follow_key(
            "/src/App.vue",
            FallthroughOverrideIdentity::Uncacheable,
            false,
        );
        assert_ne!(key_a, key_b, "different override identities should differ");
        assert!(key_a.is_cacheable(), "the no-override key is cacheable");
        assert!(
            !key_b.is_cacheable(),
            "the override-bearing (Uncacheable) key is not cacheable"
        );
    }

    #[test]
    fn root_follow_key_uses_generic_propagation() {
        let key_a = root_follow_key(
            "/src/App.vue",
            FallthroughOverrideIdentity::NoOverrides,
            false,
        );
        let key_b = root_follow_key(
            "/src/App.vue",
            FallthroughOverrideIdentity::NoOverrides,
            true,
        );
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
        let key_a = consumed_bindings_key(
            "/src/App.vue",
            "0",
            FallthroughOverrideIdentity::NoOverrides,
        );
        let key_b = consumed_bindings_key(
            "/src/App.vue",
            "0.1",
            FallthroughOverrideIdentity::NoOverrides,
        );
        assert_ne!(key_a, key_b);
    }
}
