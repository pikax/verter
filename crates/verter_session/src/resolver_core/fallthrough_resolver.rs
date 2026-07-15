//! Fallthrough/inheritance resolution with persistent node caching.
//!
//! The fallthrough resolver handles component attribute inheritance through the
//! template's root element chain. It uses [`FallthroughNodeKey`] for caching,
//! where cache keys are based on component identity + override identity
//! (not symbol identity).
//!
use std::sync::Arc;

use crate::resolver_core::{
    FactVersionRef, FallthroughNodeKey, FallthroughOverrideIdentity, ResolverContext,
    ResolverCounters, ResolverDiagnostic, StoreView, ValidatedFactCache,
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

    /// Run a fallthrough-node producer inside the cache owner's complete
    /// cacheability and supersession fence, then admit the optional candidate.
    ///
    /// The producer returns its caller-visible value separately from the cache
    /// candidate so a refused admission is still served.  The owner captures
    /// the external-supersession fingerprint before the compute and rechecks it
    /// after the compute; a result built across an epoch/project/env/identity
    /// transition is therefore return-only.  No caller-supplied probe or raw
    /// write surface is involved.
    pub(crate) fn compute_and_maybe_admit<R>(
        &self,
        ctx: &dyn ResolverContext,
        compute: impl FnOnce() -> (R, Option<(FallthroughNodeKey, FallthroughNodeResult)>),
    ) -> R {
        let host = ctx.host_for_fact_tracer_install();
        let supersession_before = host.current_external_supersession_fingerprint();
        let ((value, candidate), non_cacheable) =
            crate::fact_signature_helpers::with_cacheability_scope(host, |_probe| compute());

        if !non_cacheable && host.current_external_supersession_fingerprint() == supersession_before
        {
            if let Some((key, result)) = candidate {
                self.insert_admissible_node(key, result);
            }
        }

        value
    }

    fn insert_admissible_node(&self, key: FallthroughNodeKey, result: FallthroughNodeResult) {
        if !key.is_cacheable() {
            return;
        }
        if crate::cache_runtime::refuse_result_cache_admission_if_partial(
            crate::request_context::current_cold_compute_completeness().is_partial(),
        ) {
            return;
        }
        // An empty validated-fact signature is true for every future view.
        // The intrinsic surface is the sole exception: its key carries the
        // project cache generation that changes with its source registry.
        // Consumed bindings have no such version axis and must recompute until
        // their producer supplies a real fact root.
        if !result.facts.is_empty()
            || matches!(result.value, FallthroughNodeValue::IntrinsicSurface(_))
        {
            self.cache.insert(key, result.clone(), result.facts.clone());
        }
    }

    /// Admit a node produced by the stable request owner.
    ///
    /// `admission` is sealed evidence minted by the stable request owner after
    /// the owner-opened cacheability scope enclosed the compute.
    ///
    /// THREE independent no-poison rails, all fail-closed:
    ///
    /// 1. **uncacheable key** — an override-bearing key whose identity is
    ///    `Uncacheable` would alias two genuinely-different override sets.
    /// 2. **non-cacheable compute** (`admission.non_cacheable()`) — the compute
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
    pub(crate) fn admit_stable_node(
        &self,
        key: FallthroughNodeKey,
        result: FallthroughNodeResult,
        admission: &crate::resolver_core::FallthroughStableAdmission<'_>,
    ) {
        if admission.non_cacheable() {
            return;
        }
        self.insert_admissible_node(key, result);
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

    fn cacheable_root_node(canonical: &str, hash: u8) -> FallthroughNodeResult {
        FallthroughNodeResult {
            value: FallthroughNodeValue::RootFollow(RootFollowResult::default()),
            facts: vec![FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_string(),
                hash: [hash; 16],
            }],
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn owner_compute_scope_admits_control_and_refuses_transitive_hazard() {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        let counters = Arc::new(ResolverCounters::default());
        let state = FallthroughResolverState::new(counters);
        let canonical = "/src/Child.vue";
        let key = root_follow_key(canonical, FallthroughOverrideIdentity::NoOverrides, false);

        let control = state.compute_and_maybe_admit(&host, || {
            (
                7usize,
                Some((key.clone(), cacheable_root_node(canonical, 1))),
            )
        });
        assert_eq!(control, 7);
        assert_eq!(
            state.cached_candidate_count(&key),
            1,
            "control: a clean, fact-rooted owner compute must admit exactly one candidate"
        );

        state.clear_cache();
        let served = state.compute_and_maybe_admit(&host, || {
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
            );
            (
                11usize,
                Some((key.clone(), cacheable_root_node(canonical, 1))),
            )
        });
        assert_eq!(served, 11, "a non-cacheable compute is still served");
        assert_eq!(
            state.cached_candidate_count(&key),
            0,
            "a transitive hazard observed inside the owner-run compute must refuse admission"
        );
    }

    #[test]
    fn empty_unversioned_consumed_binding_is_served_but_never_admitted() {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        let state = FallthroughResolverState::new(Arc::new(ResolverCounters::default()));
        let key = consumed_bindings_key(
            "/src/Owner.vue",
            "root:0",
            FallthroughOverrideIdentity::NoOverrides,
        );
        let node = FallthroughNodeResult {
            value: FallthroughNodeValue::ConsumedBindings(ConsumedBindingsResult::default()),
            facts: Vec::new(),
            diagnostics: Vec::new(),
        };

        let served = state.compute_and_maybe_admit(&host, || ("served", Some((key.clone(), node))));

        assert_eq!(served, "served");
        assert_eq!(
            state.cached_candidate_count(&key),
            0,
            "an empty signature under a content-unversioned consumed-binding key validates vacuously forever and must not be retained"
        );
    }

    #[test]
    fn owner_compute_crossing_external_supersession_is_return_only() {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        let state = FallthroughResolverState::new(Arc::new(ResolverCounters::default()));
        let canonical = "/src/Child.vue";
        let key = root_follow_key(canonical, FallthroughOverrideIdentity::NoOverrides, false);

        let served = state.compute_and_maybe_admit(&host, || {
            let _ = host
                .upsert(crate::UpsertRequest {
                    canonical_id: None,
                    input_id: canonical.to_string(),
                    source: Arc::from("<template><div /></template>"),
                    file_language: crate::FileLanguage::vue(),
                    aliases: Vec::new(),
                })
                .expect("fixture upsert must advance the external supersession state");
            (
                13usize,
                Some((key.clone(), cacheable_root_node(canonical, 1))),
            )
        });

        assert_eq!(served, 13, "the unstable result remains return-only");
        assert_eq!(
            state.cached_candidate_count(&key),
            0,
            "a nested node built across an external state transition must not publish before the outer request fence"
        );
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
