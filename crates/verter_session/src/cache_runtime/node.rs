//! Node-facing cache-runtime entry points.
//!
//! A cache family implements [`ArtifactNode`] (content-addressed, one
//! entry per key) or [`QueryNode`] (query-identity, several concurrent
//! candidates per slot key) and routes every cold build through
//! [`lookup`] / [`query::lookup`]. Those entry points own ONE thing —
//! the singleflight protocol (one cold computer, cooperative joiner
//! waits, panic safety, post-compute revalidation) — and lower it onto
//! the shared [`singleflight`](super::singleflight) state machine.
//! Storage stays the node's: an artifact node owns a
//! `DashMap<Key, Arc<CacheEntry>>`, a query node owns its multi-candidate
//! slot store and admits through [`publish`].
//!
//! The flight-lane identity is [`QueryFlightKey`] — the cache key plus
//! the store-view compat token. Two requests carrying the same cache
//! key under different overlays do NOT coalesce onto one cold build:
//! their results are view-specific and not interchangeable. The map
//! stays keyed by the bare cache key (the compat token is a flight-lane
//! dimension, not a cache-key dimension — R21).

use std::hash::Hash;
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;

use super::admission::{
    CacheAdmission, CacheEntry, Candidate, DeferredVictims, PublishCoreOutcome, SignatureAdmission,
};
use super::lookup_publish::cooperative_admit_with_lookup_publish;
use super::singleflight::{
    cooperative_admit_with_post_publish_by_flight_key, ComputeAdmission, InflightTable,
};
use crate::resolver_core::{FactReadSetFinalise, ResolverContext, StoreViewCompatToken};

/// Per-compute context threaded into a node's `compute` / `validate`.
///
/// Carries the resolver (the view a warm hit validates against and the
/// fact-bubble target) plus the request-concurrency identity the
/// runtime needs: the store-view compat token that forms the flight
/// lane, and the world generation a fresh value is stamped with. Both
/// are read from the resolver at the lookup boundary via
/// [`ComputeCtx::from_resolver`], so a caller that already holds a
/// `ResolverContext` does not have to assemble a full request snapshot
/// to drive an artifact lookup.
pub(crate) struct ComputeCtx<'a> {
    /// The resolver / view the compute runs under.
    pub resolver: &'a dyn ResolverContext,
    /// Store-view compat token — the flight-lane dimension that keeps
    /// distinct overlays on the same cache key from coalescing.
    ///
    /// `allow(dead_code)`: the `lookup` / `query::lookup` entry points
    /// build the flight key from the resolver's compat token directly
    /// and thread the SAME token onto the context for query-node compute
    /// helpers that key sub-work on the flight lane. The artifact path
    /// reads only `generation`; the field is exercised by the
    /// `cache_runtime` tests.
    #[allow(dead_code)]
    pub compat_token: StoreViewCompatToken,
    /// World generation a freshly built value is stamped with and warm
    /// hits are gated on.
    pub generation: u64,
}

impl<'a> ComputeCtx<'a> {
    /// Build a context from a resolver, reading the compat token from
    /// its active store view and the generation from its project type
    /// store.
    ///
    /// `allow(dead_code)`: the `lookup` / `query::lookup` entry points
    /// build the context inline (they need the compat token for the
    /// flight key before constructing the context); this standalone
    /// constructor is for callers that already hold only a resolver. It
    /// is exercised by the `cache_runtime` tests.
    #[allow(dead_code)]
    pub(crate) fn from_resolver(resolver: &'a dyn ResolverContext) -> Self {
        Self {
            compat_token: resolver.store_view().compat_token(),
            generation: resolver.project_type_store().current_project_generation(),
            resolver,
        }
    }

    /// World generation this compute runs under.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Lift a finalised fact tracer into the admission vocabulary.
    ///
    /// `allow(dead_code)`: query-node compute helpers finalise their
    /// tracer through this; exercised by the `cache_runtime` tests.
    #[allow(dead_code)]
    pub(crate) fn signature_from(&self, finalise: FactReadSetFinalise) -> SignatureAdmission {
        SignatureAdmission::from_finalise(finalise)
    }
}

/// Flight-lane identity: the cache key plus the store-view compat token.
///
/// The compat token keeps two requests under different overlays (a base
/// view and a session/overlay view, or two overlays) from coalescing
/// onto one cold build for the same cache key — their results are not
/// interchangeable. The published map remains keyed by the bare `K`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct QueryFlightKey<K> {
    /// The cache key.
    pub key: K,
    /// The store-view compat token forming the flight lane.
    pub compat_token: StoreViewCompatToken,
}

/// A content-addressed cache family: one entry per key, validated by a
/// path-precise fact signature plus a generation gate.
///
/// `Value` is the caller-visible value (often already an `Arc<T>`); the
/// runtime wraps the stored [`CacheEntry`] in `Arc` at admission, so the
/// node never sees a double-wrapped `Arc<Arc<T>>`. The associated
/// `inflight()` table is keyed by [`QueryFlightKey`], while `entries()`
/// is keyed by the bare `Key`.
pub(crate) trait ArtifactNode {
    /// The cache key (also the published map key).
    type Key: Eq + Hash + Clone + Send + Sync + 'static;
    /// The caller-visible cached value.
    type Value: Clone + Send + Sync + 'static;

    /// The node's published entry map, keyed by the bare cache key.
    fn entries(&self) -> &DashMap<Self::Key, Arc<CacheEntry<Self::Value>>>;
    /// The node's per-cache flight table, keyed by the flight identity.
    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>>;

    /// Cold build for `key`. Returns the three-way admission outcome.
    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value>;

    /// Read-side validation of a published entry against the caller's
    /// view. `Some(value)` is a warm hit (and performs the fact-bubble
    /// side effect); `None` rejects the entry as stale.
    fn validate(
        &self,
        key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value>;

    /// The cache's retention gate, when it carries a retention budget.
    /// The publish sequence runs under this fence's read guard. Default:
    /// no fence.
    fn publish_fence(&self) -> Option<&RwLock<()>> {
        None
    }
    /// Winner-only hook after a successful publish (reverse-index
    /// registration, live-counter bump). Default: no-op.
    fn post_publish(&self, _key: &Self::Key, _entry: &Arc<CacheEntry<Self::Value>>) {}
    /// Removal-side counterpart of [`Self::post_publish`], fired when the
    /// substrate removes a published entry. Default: no-op.
    fn removal_cleanup(&self, _key: &Self::Key, _entry: &Arc<CacheEntry<Self::Value>>) {}
}

/// Cooperative warm-or-cold lookup over an [`ArtifactNode`].
///
/// Derives the flight key from `cx.compat_token`, then drives the shared
/// singleflight protocol: one cold computer, cooperative joiner waits,
/// panic safety, and a generation + self-root revalidation gate atomic
/// with the publish. A `Cacheable` admission is lowered into a
/// [`CacheEntry`] and admitted (the runtime wraps it in `Arc`); a
/// `ReturnOnly` returns the value to the winning flight alone (joiners
/// fork and recompute); a `Failed` surfaces `None`.
pub(crate) fn lookup<N: ArtifactNode>(
    node: &N,
    key: N::Key,
    resolver: &dyn ResolverContext,
) -> Option<N::Value> {
    let compat_token = resolver.store_view().compat_token();
    // The compute-time generation snapshot, captured BEFORE the cold
    // build dispatches any work. The freshly built entry is STAMPED with
    // this snapshot (`compute` reads `cx.generation()`). The validity
    // GATES (`validate` on warm hits / joiners, and the post-compute
    // revalidation) re-read the LIVE generation each time and compare it
    // to the entry's stamp: a project-generation bump that lands during
    // the cold window leaves the stamp behind the live generation, so
    // the entry is rejected (no stale publish, no warm hit).
    let snapshot_generation = resolver.project_type_store().current_project_generation();
    let flight_key = QueryFlightKey {
        key: key.clone(),
        compat_token,
    };

    cooperative_admit_with_post_publish_by_flight_key(
        node.entries(),
        node.inflight(),
        key.clone(),
        flight_key,
        |entry: &CacheEntry<N::Value>| {
            // Warm hit / joiner: gate against the LIVE generation.
            let cx = ComputeCtx {
                resolver,
                compat_token,
                generation: resolver.project_type_store().current_project_generation(),
            };
            node.validate(&key, entry, &cx)
        },
        || {
            // Cold winner: stamp the entry with the pre-compute snapshot.
            let mut cx = ComputeCtx {
                resolver,
                compat_token,
                generation: snapshot_generation,
            };
            match node.compute(&key, &mut cx) {
                CacheAdmission::Cacheable {
                    value,
                    signature,
                    self_root_canonicals,
                    validated_at_generation,
                } => ComputeAdmission::Cacheable(CacheEntry {
                    value,
                    signature,
                    self_root_canonicals,
                    validated_at_generation,
                }),
                CacheAdmission::ReturnOnly { value, reason } => {
                    ComputeAdmission::ReturnOnly { value, reason }
                }
                CacheAdmission::Failed { .. } => ComputeAdmission::Failed,
            }
        },
        // Winner-side projection. The cold winner bubbles the admitted
        // signature into its outer fact tracer here — `validate` (which
        // also bubbles) runs only on warm hits and joiners, so without
        // this the cold winner's outer caches would miss the entry's
        // transitive facts even though warm paths are correct.
        |entry: &CacheEntry<N::Value>| {
            entry.signature.bubble(resolver);
            entry.value.clone()
        },
        |entry: &CacheEntry<N::Value>| {
            // Post-compute revalidation: gate the freshly built entry's
            // stamp against the LIVE generation. A generation bump that
            // landed during the cold window rejects the publish.
            entry.validated_at_generation
                == resolver.project_type_store().current_project_generation()
                && entry
                    .signature
                    .validate_with_self_roots(resolver, &entry.self_root_canonicals)
        },
        |removed_key: &N::Key, removed: &Arc<CacheEntry<N::Value>>| {
            node.removal_cleanup(removed_key, removed)
        },
        |entry: &Arc<CacheEntry<N::Value>>, published_key: &N::Key| {
            node.post_publish(published_key, entry)
        },
        node.publish_fence(),
    )
}

/// A query-identity cache family: several concurrent candidates coexist
/// under one slot key, each validated by its own fact signature, with a
/// per-candidate `Discriminant` selecting the candidate that matches the
/// caller's observed version.
///
/// Storage is the node's own multi-candidate slot store — the shared
/// [`ReverseIndexedCandidateStore`](super::candidate_store::ReverseIndexedCandidateStore):
/// [`Self::lookup_candidate`] reads it, [`Self::publish_core`] admits into
/// it, and [`Self::evict_deferred`] runs the deferred FIFO eviction. The
/// runtime owns only the singleflight protocol, so candidate-level stale
/// rejection never evicts a whole slot.
///
/// `allow(dead_code)`: this is the query-identity substrate. The
/// query-identity cache families (materialize-structure, ref-cycle,
/// imported-registry, the semantic-graph query nodes) implement it; it
/// is exercised by the `cache_runtime` tests independent of those
/// consumers.
#[allow(dead_code)]
pub(crate) trait QueryNode {
    /// The slot key (carries NO content/version hash — R6).
    type Key: Eq + Hash + Clone + Send + Sync + 'static;
    /// Identity selecting a candidate among the slot's candidates.
    type Discriminant: Eq + Hash + Clone + Send + Sync + 'static;
    /// The caller-visible cached value.
    type Value: Clone + Send + Sync + 'static;

    /// The node's per-cache flight table, keyed by the flight identity.
    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>>;

    /// Read the slot for a candidate valid under the caller's view.
    /// `Some(value)` is a warm hit (and performs the fact-bubble side
    /// effect); `None` falls through to the cold path.
    fn lookup_candidate(&self, key: &Self::Key, cx: &ComputeCtx<'_>) -> Option<Self::Value>;

    /// Cold build for `key`. Returns the three-way admission outcome.
    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value>;

    /// Compute the discriminant for a freshly built value (e.g. the
    /// observed `VersionedDeclIdentity`).
    ///
    /// `validated_at_generation` is the EXACT generation the admitted
    /// candidate carries — the value the `Cacheable` arm stamped, threaded
    /// straight from the publish path. The discriminant MUST derive its
    /// own generation from this argument, never from `cx.generation()`: the
    /// candidate's stamp is the producer's snapshot taken inside its cold
    /// compute, while `cx.generation()` is the runtime's lookup-entry
    /// snapshot, and a project-generation bump in the gap between them
    /// would otherwise skew the two. A skewed discriminant would make a
    /// re-publish under the same view COEXIST as a distinct candidate
    /// instead of REPLACING in place (wrong cap-budget consumption,
    /// eviction of unrelated valid candidates).
    fn discriminant(
        &self,
        key: &Self::Key,
        value: &Self::Value,
        signature: &crate::fact_signature_helpers::ReadSetSignature,
        validated_at_generation: u64,
    ) -> Self::Discriminant;

    /// The cache's retention gate, when it carries a retention budget.
    /// The whole publish lifecycle (post-compute revalidation,
    /// [`Self::publish_core`], and [`Self::evict_deferred`]) runs under
    /// this fence's read guard, so a project-generation `clear` cannot
    /// interleave between the core publish and the deferred eviction.
    /// Default: no fence.
    fn publish_fence(&self) -> Option<&RwLock<()>> {
        None
    }

    /// Non-reentrant publish step — runs under the store's slot/shard
    /// guard.
    ///
    /// Install/replace the candidate, bump the live counter, register the
    /// reverse index, and record the retention admission. Return any FIFO
    /// retention victims for deferred eviction via the
    /// [`PublishCoreOutcome`] — those MUST NOT be evicted here (eviction
    /// re-enters the slot map / reverse index and would self-deadlock on
    /// the slot/shard guard this step holds). Winner-only.
    fn publish_core(
        &self,
        key: Self::Key,
        candidate: Candidate<Self::Discriminant, Self::Value>,
    ) -> PublishCoreOutcome<Self::Key>;

    /// Evict the FIFO retention victims [`Self::publish_core`] captured.
    ///
    /// Runs AFTER the slot/shard guard has dropped (still under the
    /// [`Self::publish_fence`] read guard). Each victim is removed
    /// identity-scoped by `(key, admission_seq)`. Default: no-op (a node
    /// with no retention budget returns no victims). Winner-only.
    fn evict_deferred(&self, _victims: DeferredVictims<Self::Key>) {}

    /// Winner-side lowering for a freshly-COMPUTED value whose admission
    /// was REFUSED by post-compute revalidation (a mutation landed in the
    /// cold window / the winner's view snapshot went stale).
    ///
    /// `Some(lowered)` opts the node in: the winner returns the computed
    /// value as a non-cacheable `ReturnOnly`-style outcome (nothing
    /// published; joiners fork and cold-recompute for their own view).
    /// The node marks the lowered value non-cacheable in its own domain
    /// (e.g. `cache_suppress = true` on a `CacheRead`) so the enclosing
    /// build refuses memo admission. `None` (the default) keeps failure
    /// semantics: the winner returns `None` and the caller's fallback
    /// owns the substitute — only correct for nodes whose callers can
    /// tolerate a discarded complete value.
    fn lower_unadmitted(&self, _value: &Self::Value) -> Option<Self::Value> {
        None
    }
}

/// Query-identity cooperative lookup + publish entry points.
///
/// `allow(dead_code)`: query-identity substrate exercised by the
/// `cache_runtime` tests; the query-identity cache families route their
/// cold builds through `query::lookup` and admit through
/// `query::publish`.
#[allow(dead_code)]
pub(crate) mod query {
    use super::*;

    /// Cooperative warm-or-cold lookup over a [`QueryNode`].
    ///
    /// Reuses the singleflight winner/joiner state machine but delegates
    /// storage to the node's `lookup_candidate` / `publish_core` /
    /// `evict_deferred`, so a stale candidate is skipped by the slot's own
    /// validation rather than evicting the whole slot. A `Cacheable`
    /// admission is lowered into a [`Candidate`] (with the node's
    /// discriminant) and published through the split lifecycle;
    /// `ReturnOnly` / `Failed` behave as for an artifact node.
    pub(crate) fn lookup<N: QueryNode>(
        node: &N,
        key: N::Key,
        resolver: &dyn ResolverContext,
    ) -> Option<N::Value> {
        let compat_token = resolver.store_view().compat_token();
        // Compute-time snapshot stamps the candidate; the validity gates
        // (`lookup_candidate` on warm hits / joiners, and the post-compute
        // revalidation) re-read the LIVE generation each time so a
        // mid-compute generation bump rejects the candidate. See the
        // artifact `lookup` for the snapshot-vs-live rationale.
        let snapshot_generation = resolver.project_type_store().current_project_generation();
        let flight_key = QueryFlightKey {
            key: key.clone(),
            compat_token,
        };
        // `lookup_candidate` / `compute` borrow `&key`; `publish_core`
        // takes the key by value. Hand the publish closure its own clone
        // so the borrowing closures keep `key`.
        let key_for_publish = key.clone();

        cooperative_admit_with_lookup_publish(
            node.inflight(),
            flight_key,
            || {
                let cx = ComputeCtx {
                    resolver,
                    compat_token,
                    generation: resolver.project_type_store().current_project_generation(),
                };
                node.lookup_candidate(&key, &cx)
            },
            || {
                let mut cx = ComputeCtx {
                    resolver,
                    compat_token,
                    generation: snapshot_generation,
                };
                match node.compute(&key, &mut cx) {
                    CacheAdmission::Cacheable {
                        value,
                        signature,
                        self_root_canonicals,
                        validated_at_generation,
                    } => {
                        // Build the discriminant from the EXACT generation
                        // the candidate is stamped with, NOT from
                        // `cx.generation()` (the lookup-entry snapshot). A
                        // project-generation bump that lands between the
                        // runtime's lookup-entry snapshot and the producer's
                        // in-compute snapshot would otherwise stamp the
                        // candidate with one generation while the
                        // discriminant carried another — breaking the
                        // replace-vs-coexist identity contract.
                        let discriminant =
                            node.discriminant(&key, &value, &signature, validated_at_generation);
                        ComputeAdmission::Cacheable(Candidate {
                            discriminant,
                            value,
                            signature,
                            self_root_canonicals,
                            admission_seq: 0,
                            validated_at_generation,
                        })
                    }
                    CacheAdmission::ReturnOnly { value, reason } => {
                        ComputeAdmission::ReturnOnly { value, reason }
                    }
                    CacheAdmission::Failed { .. } => ComputeAdmission::Failed,
                }
            },
            // Winner-side projections bubble the candidate's signature
            // into the outer tracer (the warm/joiner `lookup_candidate`
            // path bubbles separately); the cold winner must deliver its
            // facts too. ONE bubble site shared by the admitted and the
            // admission-REFUSED projections — the refused child's
            // observations must keep rooting the enclosing entries'
            // signatures so cross-file invalidation of the consuming
            // result keeps working.
            |candidate: &Candidate<N::Discriminant, N::Value>| {
                candidate.signature.bubble(resolver);
                candidate.value.clone()
            },
            // Winner-side projection for an admission-REFUSED computed
            // candidate (post-compute revalidation rejected it). When the
            // node opts in, the computed value still flows to the winner
            // (non-cacheable; joiners fork) with its facts bubbled.
            |candidate: &Candidate<N::Discriminant, N::Value>| {
                node.lower_unadmitted(&candidate.value).inspect(|_| {
                    candidate.signature.bubble(resolver);
                })
            },
            |candidate: &Candidate<N::Discriminant, N::Value>| {
                candidate.validated_at_generation
                    == resolver.project_type_store().current_project_generation()
                    && candidate
                        .signature
                        .validate_with_self_roots(resolver, &candidate.self_root_canonicals)
            },
            // publish_core — the non-reentrant publish step under the
            // store's slot/shard guard. Returns the FIFO victims for
            // deferred eviction.
            |candidate: Candidate<N::Discriminant, N::Value>| {
                node.publish_core(key_for_publish, candidate)
                    .deferred_victims
            },
            // evict_deferred — fired after the slot/shard guard drops,
            // still under the publish fence.
            |victims: DeferredVictims<N::Key>| node.evict_deferred(victims),
            node.publish_fence(),
        )
    }
}

/// Per-candidate cap before FIFO eviction. The `(CAP + 1)`-th admission
/// of a fresh discriminant evicts the oldest candidate. The shared
/// [`ReverseIndexedCandidateStore`](super::candidate_store::ReverseIndexedCandidateStore)
/// enforces this per-slot cap; concurrent overlay variants of one
/// content-free slot key coexist up to the cap (R20).
pub(crate) const QUERY_SLOT_CANDIDATE_CAP: usize = 4;

#[cfg(test)]
#[path = "node_tests.rs"]
mod tests;
