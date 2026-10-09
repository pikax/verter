//! Fallthrough/inheritance resolution with persistent node caching.
//!
//! The fallthrough resolver handles component attribute inheritance through the
//! template's root element chain. It uses [`FallthroughNodeKey`] for caching,
//! where cache keys are based on component identity + override identity
//! (not symbol identity).
//!
use std::sync::Arc;

use crate::resolver_core::{
    FallthroughNodeKey, FallthroughOverrideIdentity, ResolverCounters, ResolverDiagnostic,
    ValidatedFactCache,
};
use verter_session_query::analysis::component_meta::{
    AcceptedEventAnalysis, AcceptedPropAnalysis, AcceptedSurfaceCompleteness, FallthroughSurface,
};
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_session_query::facts::store_view::StoreView;
use verter_type_engine::resolver_core::ResolverContext;

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
                reason: verter_session_query::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot,
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
    /// The workspace content generation this surface was projected under.
    ///
    /// The version axis lives HERE, on the value, and not in
    /// [`crate::resolver_core::FallthroughNodeKey::IntrinsicSurfaceLoad`]:
    /// a superseded surface is REPLACED under its stable
    /// `(project_anchor, tag)` key instead of accumulating one dead map
    /// entry per edit. The reader compares it against the live generation
    /// and retires the entry when the warm surface is strictly OLDER, so a
    /// stale surface is never served — the node carries no validated fact
    /// signature of its own. The comparison is ordered rather than a bare
    /// mismatch because the reader's own live sample may itself have been
    /// overtaken; see
    /// [`FallthroughResolverState::retire_superseded_intrinsic_surface`].
    pub cache_generation: u64,
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
                reason: verter_session_query::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot,
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
    pub partial_reasons: Vec<verter_session_query::analysis::component_meta::PartialBranchReason>,
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
                reason: verter_session_query::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot,
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

impl FallthroughNodeResult {
    /// Estimated bytes one cached candidate keeps alive: the result, its fact
    /// signature (held twice, once as the candidate's signature) and the
    /// top-level rows of its value.
    fn retained_bytes(&self, key: &FallthroughNodeKey) -> usize {
        use std::mem::size_of;
        let value = match &self.value {
            FallthroughNodeValue::RootFollow(result) => {
                result.accepted_props.len() * size_of::<AcceptedPropAnalysis>()
                    + result.accepted_events.len() * size_of::<AcceptedEventAnalysis>()
                    + result.branches.len() * size_of::<FallthroughBranchResult>()
            }
            FallthroughNodeValue::BranchUnion(result) => {
                result.accepted_props.len() * size_of::<AcceptedPropAnalysis>()
                    + result.accepted_events.len() * size_of::<AcceptedEventAnalysis>()
                    + result.branches.len() * size_of::<FallthroughBranchResult>()
            }
            FallthroughNodeValue::IntrinsicSurface(surface) => {
                surface.members.len() * size_of::<crate::resolver_core::IntrinsicSurfaceMember>()
            }
            FallthroughNodeValue::ChildSurfaceFollow(_) => 0,
            FallthroughNodeValue::ConsumedBindings(result) => {
                (result.attrs.len() + result.listeners.len() + result.consumed_names.len())
                    * size_of::<String>()
            }
        };
        size_of::<Self>()
            + 2 * self.facts.len() * size_of::<FactVersionRef>()
            + self.diagnostics.len() * size_of::<ResolverDiagnostic>()
            + value
            + key.canonical().len()
    }
}

/// The most keys the fallthrough node cache keeps: the bound the semantic
/// memo keeps on its families.
pub(crate) const FALLTHROUGH_NODE_CAP: usize =
    verter_type_engine::bounded_query_retention::DEFAULT_BUDGET_CAP;

/// What the node cache keeps, and why. The cache itself answers reads; this
/// is its single write-side consistency domain: every admission and removal
/// updates the cache and this record under one lock, so a key is kept exactly
/// while it has a record here.
#[derive(Default)]
struct NodeResidency {
    /// Per kept key: its latest admission and one retention charge per
    /// candidate, oldest first, mirroring the cache's candidate order.
    kept: rustc_hash::FxHashMap<FallthroughNodeKey, KeptNode>,
    /// Kept keys per owning component (or project, for an intrinsic surface),
    /// so a close or a delete releases exactly its component's keys.
    by_owner: rustc_hash::FxHashMap<String, rustc_hash::FxHashSet<FallthroughNodeKey>>,
    /// Keys in admission order, oldest first. An entry whose sequence is not
    /// its key's latest admission is stale and skipped.
    admitted: std::collections::VecDeque<(u64, FallthroughNodeKey)>,
    next_seq: u64,
}

struct KeptNode {
    seq: u64,
    charges: smallvec::SmallVec<
        [verter_session_query::retention::RetentionCharge;
            verter_session_query::facts::fact_cache::CANDIDATE_CAP],
    >,
}

impl NodeResidency {
    /// Forget `key`'s record and its place in the owner index.
    fn forget(&mut self, key: &FallthroughNodeKey) -> Option<KeptNode> {
        let kept = self.kept.remove(key)?;
        if let Some(keys) = self.by_owner.get_mut(key.canonical()) {
            keys.remove(key);
            if keys.is_empty() {
                self.by_owner.remove(key.canonical());
            }
        }
        Some(kept)
    }

    /// Drop the stale entries from the admission order once they outnumber
    /// the live ones, so the queue stays proportional to the kept keys.
    fn compact(&mut self) {
        if self.admitted.len() <= 2 * self.kept.len() + 16 {
            return;
        }
        let kept = &self.kept;
        self.admitted
            .retain(|(seq, key)| kept.get(key).is_some_and(|node| node.seq == *seq));
    }
}

/// The fallthrough node cache, bounded and owned.
///
/// **Lifetime.** A node is keyed by the component whose surface it describes
/// (or, for an intrinsic element surface, by its project), and it lives until
/// the first of: that component closes or is deleted
/// ([`Self::release_owner`]); [`FALLTHROUGH_NODE_CAP`] newer keys are admitted
/// after it was last admitted (oldest first); the reader retires a superseded
/// intrinsic surface. An edit does not retire a key: the edited component's
/// next resolution admits a fresh candidate beside the stale ones, at most
/// [`verter_session_query::facts::fact_cache::CANDIDATE_CAP`] per key.
///
/// **Accounting.** Every candidate holds a `Retained` charge on the process's
/// retention account for its bytes; a candidate the account refuses is served
/// uncached. The key count is reported as the host retention snapshot's
/// `fallthrough_nodes`. Evicting a live node only forces a recompute.
pub struct FallthroughResolverState {
    cache: ValidatedFactCache<FallthroughNodeKey, FallthroughNodeResult>,
    residency: parking_lot::Mutex<NodeResidency>,
    retention_account: verter_session_query::retention::StoreAccount,
    counters: Arc<ResolverCounters>,
}

impl FallthroughResolverState {
    pub fn new(counters: Arc<ResolverCounters>) -> Self {
        Self {
            cache: ValidatedFactCache::default(),
            residency: parking_lot::Mutex::new(NodeResidency::default()),
            retention_account: verter_session_query::retention::StoreAccount::default(),
            counters,
        }
    }

    pub fn clear_cache(&self) {
        let mut residency = self.residency.lock();
        self.cache.clear();
        *residency = NodeResidency::default();
    }

    /// Keys the node cache holds (retention observability).
    pub fn retained_node_count(&self) -> usize {
        self.cache.len()
    }

    /// Release every node the closed or deleted `owner` keyed: they can
    /// never be read again under a live component. Returns how many keys
    /// went.
    pub fn release_owner(&self, owner: &str) -> usize {
        let mut residency = self.residency.lock();
        let Some(keys) = residency.by_owner.remove(owner) else {
            return 0;
        };
        for key in &keys {
            residency.kept.remove(key);
            self.cache.remove(key);
        }
        residency.compact();
        keys.len()
    }

    pub fn remove_node_for_test(&self, key: &FallthroughNodeKey) {
        let mut residency = self.residency.lock();
        residency.forget(key);
        self.cache.remove(key);
    }

    /// Retire an intrinsic-surface node whose own value-carried generation is
    /// strictly OLDER than the live one the reader observed.
    ///
    /// Used by the intrinsic-surface reader: that node carries an EMPTY
    /// validated-fact signature, so the store view can never reject it and the
    /// superseded candidate would otherwise stay warm under its stable key
    /// forever, shadowing the fresh one on every later read.
    ///
    /// The retirement is ORDERED and decided inside the slot's own lock
    /// ([`ValidatedFactCache::remove_if_all_candidates`]) against
    /// `observed_generation`, not unconditional. The workspace content
    /// generation only ever advances, so a candidate at or beyond the
    /// generation this reader sampled is NEWER than anything this reader
    /// knows and must survive: an unconditional remove would let a reader
    /// that paused between observing staleness and retiring erase a value a
    /// concurrent writer admitted meanwhile.
    ///
    /// Returns `true` when the entry was actually retired.
    pub(crate) fn retire_superseded_intrinsic_surface(
        &self,
        key: &FallthroughNodeKey,
        observed_generation: u64,
    ) -> bool {
        let mut residency = self.residency.lock();
        let retired = self
            .cache
            .remove_if_all_candidates(key, |node| match &node.value {
                FallthroughNodeValue::IntrinsicSurface(surface) => {
                    surface.cache_generation < observed_generation
                }
                // A non-intrinsic value has no value-carried generation axis, so
                // this reader cannot judge it superseded; leave it alone.
                _ => false,
            });
        if retired {
            residency.forget(key);
            residency.compact();
        }
        retired
    }

    /// Admit a node through the same admission body the compute path uses,
    /// without opening a cacheability scope. Lets a test stage the exact warm
    /// slot contents a concurrent writer would have published.
    #[cfg(test)]
    pub(crate) fn admit_node_for_test(
        &self,
        key: FallthroughNodeKey,
        result: FallthroughNodeResult,
    ) {
        self.insert_admissible_node(key, result);
    }

    /// The members and value-carried generation of the NEWEST warm intrinsic
    /// surface under `key`, bypassing view validation — `None` when the slot
    /// is empty or its newest candidate is not an intrinsic surface.
    ///
    /// Test-only: the intrinsic node carries an empty fact signature, so a
    /// validated read cannot tell "retired" from "still warm but superseded",
    /// and it returns the OLDEST valid candidate. Pair with
    /// [`Self::cached_candidate_count`] to pin the exact slot contents.
    #[cfg(test)]
    pub(crate) fn warm_intrinsic_surface_for_test(
        &self,
        key: &FallthroughNodeKey,
    ) -> Option<(Vec<crate::resolver_core::IntrinsicSurfaceMember>, u64)> {
        self.cache
            .peek_any_candidate(key)
            .and_then(|node| match &node.value {
                FallthroughNodeValue::IntrinsicSurface(surface) => {
                    Some((surface.members.clone(), surface.cache_generation))
                }
                _ => None,
            })
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
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities>,
        compute: impl FnOnce() -> (R, Option<(FallthroughNodeKey, FallthroughNodeResult)>),
    ) -> R {
        let supersession_before = ctx.current_external_supersession_fingerprint();
        let ((value, candidate), non_cacheable) =
            verter_type_engine::fact_signature_helpers::with_cacheability_scope(
                &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
                |_probe| compute(),
            );

        if !non_cacheable && ctx.current_external_supersession_fingerprint() == supersession_before
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
        if verter_type_engine::cache_runtime::refuse_result_cache_admission_if_partial(
            verter_type_engine::request_context::current_cold_compute_completeness().is_partial(),
        ) {
            return;
        }
        // An empty validated-fact signature is true for every future view.
        // The intrinsic surface is the sole exception: its VALUE carries the
        // project cache generation that changes with its source registry, and
        // its reader retires the entry the moment that generation no longer
        // matches the live one (`intrinsic_members_for_tag`). The version axis
        // is deliberately NOT in the key: a generation-keyed entry is never
        // superseded, so an editing session accumulates one whole dead
        // intrinsic surface per tag per edit.
        // Consumed bindings have no such version axis and must recompute until
        // their producer supplies a real fact root.
        if !result.facts.is_empty()
            || matches!(result.value, FallthroughNodeValue::IntrinsicSurface(_))
        {
            self.keep(key, result);
        }
    }

    /// Admit one candidate under `key`, charged and bounded (see the type
    /// docs). A refused charge serves the result uncached.
    fn keep(&self, key: FallthroughNodeKey, result: FallthroughNodeResult) {
        let Some(charge) = verter_session_query::facts::receipt::reserve_retained_with_evidence(
            self.retention_account.get(),
            result.retained_bytes(&key),
            &[&result.facts],
        )
        .admitted() else {
            return;
        };
        let mut evicted = Vec::new();
        {
            let mut residency = self.residency.lock();
            let facts = result.facts.clone();
            self.cache.insert(key.clone(), result, facts);
            let seq = residency.next_seq;
            residency.next_seq += 1;
            match residency.kept.get_mut(&key) {
                Some(kept) => {
                    kept.seq = seq;
                    kept.charges.push(charge);
                    // The cache keeps the newest candidates; so do the charges.
                    let over = kept
                        .charges
                        .len()
                        .saturating_sub(verter_session_query::facts::fact_cache::CANDIDATE_CAP);
                    evicted.extend(kept.charges.drain(..over));
                }
                None => {
                    residency.kept.insert(
                        key.clone(),
                        KeptNode {
                            seq,
                            charges: smallvec::smallvec![charge],
                        },
                    );
                    residency
                        .by_owner
                        .entry(key.canonical().to_string())
                        .or_default()
                        .insert(key.clone());
                }
            }
            residency.admitted.push_back((seq, key));
            while residency.kept.len() > FALLTHROUGH_NODE_CAP {
                let Some((seq, oldest)) = residency.admitted.pop_front() else {
                    break;
                };
                if residency
                    .kept
                    .get(&oldest)
                    .is_some_and(|kept| kept.seq == seq)
                {
                    if let Some(kept) = residency.forget(&oldest) {
                        evicted.extend(kept.charges);
                    }
                    self.cache.remove(&oldest);
                }
            }
            residency.compact();
        }
        // The charges of what left the cache release here, outside its lock.
        drop(evicted);
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
    ///    unobservable contributor source env. Those reasons are CONTENT-NEUTRAL:
    ///    the artifacts stay published and content-current, so an admitted
    ///    entry would root on the LIVE hashes and revalidate on every warm read
    ///    FOREVER — nothing downstream can reject it. The value is still SERVED
    ///    to the caller verbatim; only the shared-cache admission is refused.
    ///    A non-cacheable read is NEVER a `ResultCompleteness::Partial`.
    /// 3. **partial result** — a budget/fuse trip folded into the active
    ///    cold-compute completeness scope. The typed completeness signal is the
    ///    no-poison rail shared with the component-meta materialiser, not a
    ///    fallthrough-private predicate.
    pub(crate) fn admit_stable_node<
        W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
    >(
        &self,
        key: FallthroughNodeKey,
        result: FallthroughNodeResult,
        admission: &crate::resolver_core::FallthroughStableAdmission<'_, W>,
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

/// The STABLE identity of one project's intrinsic surface for one tag.
///
/// Deliberately carries no generation: see
/// [`FallthroughNodeKey::IntrinsicSurfaceLoad`].
pub fn intrinsic_surface_key(project_anchor: &str, tag: &str) -> FallthroughNodeKey {
    FallthroughNodeKey::IntrinsicSurfaceLoad {
        project_anchor: project_anchor.to_string(),
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

    /// The node cache keeps at most [`FALLTHROUGH_NODE_CAP`] keys, the
    /// least recently admitted leaving first, and a component release drops
    /// exactly its own keys. Discriminating: the cache kept every key ever
    /// admitted.
    #[test]
    fn the_node_cache_is_bounded_and_released_per_component() {
        let state = FallthroughResolverState::new(Arc::new(ResolverCounters::default()));
        let key = |n: usize| {
            root_follow_key(
                &format!("/src/C{n}.vue"),
                FallthroughOverrideIdentity::NoOverrides,
                false,
            )
        };
        let over = 64;
        for n in 0..FALLTHROUGH_NODE_CAP + over {
            state.admit_node_for_test(key(n), cacheable_root_node(&format!("/src/C{n}.vue"), 1));
        }
        assert_eq!(state.retained_node_count(), FALLTHROUGH_NODE_CAP);
        assert_eq!(
            state.cached_candidate_count(&key(0)),
            0,
            "the oldest key left first"
        );
        assert_eq!(
            state.cached_candidate_count(&key(FALLTHROUGH_NODE_CAP + over - 1)),
            1
        );
        assert!(
            state.residency.lock().admitted.len() <= 2 * FALLTHROUGH_NODE_CAP + 16,
            "the admission order stays proportional to the kept keys"
        );

        let last = format!("/src/C{}.vue", FALLTHROUGH_NODE_CAP + over - 1);
        assert_eq!(state.release_owner(&last), 1);
        assert_eq!(state.retained_node_count(), FALLTHROUGH_NODE_CAP - 1);
        assert_eq!(state.release_owner(&last), 0, "a release is idempotent");
    }

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
            verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::FencedServe,
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

    fn intrinsic_surface_node(generation: u64) -> FallthroughNodeResult {
        FallthroughNodeResult {
            value: FallthroughNodeValue::IntrinsicSurface(IntrinsicSurfaceResult {
                cache_generation: generation,
                ..IntrinsicSurfaceResult::default()
            }),
            facts: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn warm_intrinsic_generation(
        state: &FallthroughResolverState,
        key: &FallthroughNodeKey,
    ) -> Option<u64> {
        state
            .cache
            .peek_any_candidate(key)
            .and_then(|node| match &node.value {
                FallthroughNodeValue::IntrinsicSurface(surface) => Some(surface.cache_generation),
                _ => None,
            })
    }

    /// A reader that observed a stale intrinsic surface may retire ONLY what
    /// it outranks. A newer surface admitted between that observation and the
    /// retirement — the interleaving a paused reader produces — survives, so
    /// the cache never loses the freshest completion; a genuinely older
    /// surface is still retired rather than left shadowing it.
    #[test]
    fn stale_intrinsic_retirement_never_erases_a_newer_generation() {
        let key = intrinsic_surface_key("/workspace|/workspace/tsconfig.json", "div");

        // Reader A samples generation 7, observes the warm generation-6
        // surface as stale, and is descheduled. Writer B advances the
        // workspace and admits generation 8 under the same stable key.
        let state = FallthroughResolverState::new(Arc::new(ResolverCounters::default()));
        state.admit_node_for_test(key.clone(), intrinsic_surface_node(8));

        // Reader A resumes and retires. It must not erase B's newer value.
        let retired = state.retire_superseded_intrinsic_surface(&key, 7);
        assert!(
            !retired,
            "a reader holding a generation-7 sample must not retire a generation-8 surface"
        );
        assert_eq!(
            warm_intrinsic_generation(&state, &key),
            Some(8),
            "the newer surface admitted by the concurrent writer must stay warm"
        );

        // The uncontended case still retires: a surface strictly older than
        // the reader's sample would otherwise shadow the fresh one forever.
        let state = FallthroughResolverState::new(Arc::new(ResolverCounters::default()));
        state.admit_node_for_test(key.clone(), intrinsic_surface_node(6));
        assert!(
            state.retire_superseded_intrinsic_surface(&key, 7),
            "a generation-6 surface is superseded by a generation-7 sample and must retire"
        );
        assert_eq!(
            warm_intrinsic_generation(&state, &key),
            None,
            "the superseded surface must not stay warm under the stable key"
        );
    }

    #[test]
    fn intrinsic_surface_key_keyed_by_tag() {
        let key_div = intrinsic_surface_key("/workspace|/workspace/tsconfig.json", "div");
        let key_span = intrinsic_surface_key("/workspace|/workspace/tsconfig.json", "span");
        assert_ne!(key_div, key_span);

        let key_div2 = intrinsic_surface_key("/workspace|/workspace/tsconfig.json", "div");
        assert_eq!(key_div, key_div2);

        let key_other_project = intrinsic_surface_key("/other|/other/tsconfig.json", "div");
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
