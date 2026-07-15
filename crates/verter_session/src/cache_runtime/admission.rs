//! Typed admission outcomes for the cache runtime.
//!
//! A cold compute produces one of three node-facing outcomes
//! ([`CacheAdmission`]): a cacheable value with its validity metadata, a
//! valid-but-non-cacheable value that returns to the winning flight
//! alone, or a failure. The runtime lowers a `Cacheable` arm into a
//! [`CacheEntry`] (artifact caches) or a [`Candidate`] (query-identity
//! caches), wrapping the bare value at admission so producers hand back
//! the unwrapped value rather than pre-wrapping an `Arc`.
//!
//! Signature finalisation ([`SignatureAdmission`]) classifies a tracer
//! result before the value is even built: an `Ok` signature is
//! cacheable; intrinsic non-cacheability and overflow are both
//! non-cacheable with typed reasons. The
//! refusal reason ([`NonAdmissionReason`]) lives in the audit leaf
//! crate so structured refusal events can depend on it without a
//! back-edge to `verter_session`.
//!

use std::sync::Arc;

use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};

/// Refusal classification for a non-cacheable admission. Re-exported
/// from the audit leaf crate ([`verter_audit::NonAdmissionReason`]) so
/// the runtime and structured refusal events name one type.
pub(crate) use verter_audit::NonAdmissionReason;

/// Classify whether a typed cache-refusal reason is confined to the cache
/// family making the decision or taints every enclosing derivation that
/// consumes the returned value.
///
/// Keep this match exhaustive: adding an audit reason without choosing its
/// propagation semantics is a compile error. Intrinsics and the deterministic
/// test refusal are locally valid values whose non-retention is family policy;
/// every correctness, provenance, completeness, or world-stability failure is
/// transitive.
#[inline]
pub(crate) fn non_admission_propagation(
    reason: NonAdmissionReason,
) -> crate::resolver_core::fact_read_set::NonCacheablePropagation {
    use crate::resolver_core::fact_read_set::NonCacheablePropagation;

    match reason {
        NonAdmissionReason::IntrinsicNonCacheable | NonAdmissionReason::ForcedTestRefusal => {
            NonCacheablePropagation::LocalOnly
        }
        NonAdmissionReason::SignatureOverflow
        | NonAdmissionReason::EmptySignature
        | NonAdmissionReason::SelfRootConflict
        | NonAdmissionReason::RouteGenerationDependency
        | NonAdmissionReason::GenerationSuperseded
        | NonAdmissionReason::PostComputeRevalidationFailed
        | NonAdmissionReason::BudgetExceeded
        | NonAdmissionReason::Cancelled
        | NonAdmissionReason::UnresolvedProvenance
        | NonAdmissionReason::ComputeFailed
        | NonAdmissionReason::PartialResult => NonCacheablePropagation::Transitive,
    }
}

/// Propagate a returned non-admitted value into the active enclosing tracer
/// scopes when its typed reason describes a transitive derivation hazard.
/// Local-only refusal has already been enforced by the cache owner and must
/// not poison a caller that can independently root its own result.
#[inline]
pub(crate) fn propagate_non_admission(reason: NonAdmissionReason) {
    let propagation = non_admission_propagation(reason);
    if propagation == crate::resolver_core::fact_read_set::NonCacheablePropagation::Transitive {
        crate::resolver_core::resolver_context::note_non_cacheable_propagation(propagation);
    }
}

/// THE shared result-cache partial-admission gate — **PURE** over the
/// supplied completeness.
///
/// A GENUINE partial (a budget exhaustion / fatal `QueryError` /
/// same-path recursion / walker fatal) must NOT warm-replay as a complete
/// result. Every result-cache admission path (`MaterializeStructureDb`,
/// `ShapeCacheDb`, `ImportedRegistryDb`, `ResolvabilityDb`) routes its
/// `Cacheable` decision through this one predicate so a future
/// result-cache cannot silently forget the rule.
///
/// Returns `true` when admission MUST be refused (route the value through
/// `ComputeAdmission::ReturnOnly` under
/// [`NonAdmissionReason::PartialResult`] instead of admitting it).
///
/// **The gate is PURE**: it returns its `value_is_partial` argument and
/// does NOT OR-in any request-global / thread-local state. Each caller
/// supplies the completeness scoped to the value/entry it is admitting:
///
/// - The SHARED semantic-cache producers (`MaterializeStructureDb`) pass
///   their PER-COLD-COMPUTE completeness
///   ([`crate::request_context::current_cold_compute_completeness`]`.is_partial()`)
///   so each entry carries its OWN completeness — one consumer's partial
///   never poisons a sibling consumer's complete entry.
/// - Per-value producers pass the value's own
///   [`crate::semantic_query::CacheRead::result_is_partial`].
/// - The registry / resolvability rails (no per-value flag) pass the
///   request-result completeness
///   ([`crate::request_context::current_request_result_is_partial`]),
///   which for the component-meta entry point IS result-scoped (one
///   request resolves one component's meta).
///
/// CRITICAL — this gate keys on PARTIALITY, never on bare
/// `cache_suppress`. A benign non-cacheable result (a `ReturnOnly`
/// cross-owner-reuse admission, a tracer-signature overflow, an
/// unrootable self-root) is COMPLETE and MUST still be allowed to warm
/// its result cache; only a genuine partial is refused here. The
/// inner-memo `cache_suppress || result_is_partial` gate (the semantic
/// family memo) is a SEPARATE, stricter gate that stays as it is.
#[inline]
#[must_use]
pub(crate) fn refuse_result_cache_admission_if_partial(value_is_partial: bool) -> bool {
    value_is_partial
}

/// Outcome of finalising a fact-read tracer, before the cached value is
/// constructed.
///
/// A cold compute installs a tracer, walks its dependency graph, and
/// finalises the tracer into a [`FactReadSetFinalise`]. This type lifts
/// that raw result into the admission vocabulary: an `Ok` signature is
/// [`SignatureAdmission::Cacheable`]; intrinsic non-cacheability and
/// overflow are [`SignatureAdmission::NonCacheable`] with their
/// corresponding typed reason.
///
/// `allow(dead_code)`: the query-identity cache families that finalise
/// signatures through this surface land alongside it; the variants and
/// methods are constructed and read by the `cache_runtime` tests
/// (`signature_admission_from_ok_finalise_is_cacheable`,
/// `signature_admission_from_overflow_finalise_is_non_cacheable`), so the
/// allow only covers the not-yet-wired production constructor.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum SignatureAdmission {
    /// The tracer finalised within the size cap; the value is cacheable
    /// and validated against this signature on every warm hit.
    Cacheable(ReadSetSignature),
    /// The tracer could not be admitted; the value is returned to the
    /// winning flight alone with the carried reason.
    NonCacheable(NonAdmissionReason),
}

#[allow(dead_code)]
impl SignatureAdmission {
    /// Lift a finalised tracer result into the admission vocabulary.
    pub(crate) fn from_finalise(finalise: FactReadSetFinalise) -> Self {
        match finalise {
            FactReadSetFinalise::Ok(facts) => {
                SignatureAdmission::Cacheable(ReadSetSignature::new(facts))
            }
            FactReadSetFinalise::NonCacheable(_) => {
                SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance)
            }
            FactReadSetFinalise::Overflow => {
                SignatureAdmission::NonCacheable(NonAdmissionReason::SignatureOverflow)
            }
        }
    }

    /// The cacheable signature, if this admission is cacheable.
    pub(crate) fn cacheable(&self) -> Option<&ReadSetSignature> {
        match self {
            SignatureAdmission::Cacheable(sig) => Some(sig),
            SignatureAdmission::NonCacheable(_) => None,
        }
    }

    /// Consume the admission, returning the cacheable signature when
    /// admission was granted. Producers that need to project the
    /// signature into a `Option<ReadSetSignature>` (e.g. test fixtures
    /// asserting fact-rail content, or `get_or_compute` callsites that
    /// store the rail under a different field shape) take this owned
    /// projection rather than re-cloning through the borrowed
    /// [`Self::cacheable`].
    pub(crate) fn into_cacheable(self) -> Option<ReadSetSignature> {
        match self {
            SignatureAdmission::Cacheable(sig) => Some(sig),
            SignatureAdmission::NonCacheable(_) => None,
        }
    }
}

/// Node-facing cold-compute outcome.
///
/// A producer's `compute` returns one of three arms. The runtime lowers
/// them onto the singleflight [`ComputeAdmission`](super::singleflight::ComputeAdmission)
/// engine: `Cacheable` becomes a stored entry/candidate (the bare value
/// is wrapped at admission), `ReturnOnly` returns the value to the
/// winning flight without admitting (joiners fork and recompute), and
/// `Failed` surfaces `None` to joiners.
///
/// `CacheAdmission` carries the value UNWRAPPED. The caller-visible
/// value `V` is whatever the node uses (often already an `Arc<T>`);
/// forcing a universal `Arc<V>` would double-wrap real callers, so the
/// runtime wraps the stored carrier (`CacheEntry` / `Candidate`) in
/// `Arc` at admission, not the value.
///
/// `allow(dead_code)`: the migrated single-entry artifact caches return
/// `Cacheable` / `Failed` (they never overflow to `ReturnOnly`); the
/// `ReturnOnly` arm and the carried `reason` fields are exercised by the
/// `cache_runtime` tests and constructed by the query-identity cache
/// families (overflow / forced-refusal / budget-exceeded → `ReturnOnly`).
#[allow(dead_code)]
pub(crate) enum CacheAdmission<V> {
    /// The result is valid AND cacheable. The runtime builds a
    /// [`CacheEntry`] / [`Candidate`] from these fields and admits it.
    Cacheable {
        /// The caller-visible value (unwrapped).
        value: V,
        /// The path-precise fact signature warm hits validate against.
        signature: ReadSetSignature,
        /// Canonicals validated strictly as self-roots on warm read.
        self_root_canonicals: Arc<[Arc<str>]>,
        /// World generation the value was computed under.
        validated_at_generation: u64,
    },
    /// The result is valid but NOT cacheable. The winning flight
    /// receives `value`; joiners fork and cold-recompute for their own
    /// view. `reason` classifies why admission was refused.
    ReturnOnly {
        /// The caller-visible value (unwrapped).
        value: V,
        /// Why the value was not admitted to the warm cache.
        reason: NonAdmissionReason,
    },
    /// The cold compute failed. Joiners surface `None`; the next caller
    /// retries the cold path.
    Failed {
        /// Why the compute failed.
        reason: NonAdmissionReason,
    },
}

/// Stored validity metadata for an artifact-cache entry.
///
/// Distinct from [`crate::resolver_core::CacheEntry`] (the
/// multi-candidate `ArcSwap` slot): this is a single artifact entry
/// carrying the value plus the validity rails a warm hit checks. Refer
/// to it by its qualified `cache_runtime` path to avoid the name
/// collision.
#[derive(Clone)]
pub(crate) struct CacheEntry<V> {
    /// The cached value.
    pub value: V,
    /// The path-precise fact signature, validated against the live
    /// store view on every warm hit.
    pub signature: ReadSetSignature,
    /// Canonicals validated strictly as self-roots.
    pub self_root_canonicals: Arc<[Arc<str>]>,
    /// World generation the entry was computed under.
    pub validated_at_generation: u64,
}

/// One candidate inside a query-identity multi-candidate slot.
///
/// Distinct from [`crate::resolver_core::Candidate`] (single type
/// parameter): this carries a `discriminant` so concurrent variants of
/// the same slot key (e.g. two file-content versions, two overlays)
/// coexist and a later query selects the candidate matching its own
/// observed version. Refer to it by its qualified `cache_runtime` path.
///
/// `allow(dead_code)`: query-node substrate exercised by the
/// `cache_runtime` tests; the multi-candidate query-identity families
/// construct candidates through `query::lookup`.
#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct Candidate<D, V> {
    /// Identity that selects this candidate among the slot's candidates
    /// (e.g. the observed `VersionedDeclIdentity`).
    pub discriminant: D,
    /// The cached value.
    pub value: V,
    /// The path-precise fact signature, validated on every warm hit.
    pub signature: ReadSetSignature,
    /// Canonicals validated strictly as self-roots.
    pub self_root_canonicals: Arc<[Arc<str>]>,
    /// Monotonic admission order, used for FIFO eviction when the slot
    /// reaches its candidate cap.
    pub admission_seq: u64,
    /// World generation the candidate was computed under.
    pub validated_at_generation: u64,
}

/// Outcome of admitting a candidate into a query-identity slot.
///
/// `allow(dead_code)`: carried by [`PublishCoreOutcome`] from
/// `QueryNode::publish_core` / `ReverseIndexedCandidateStore::publish_core`
/// — query-node substrate exercised by the `cache_runtime` tests.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublishOutcome {
    /// Candidate admitted into a fresh or non-full slot.
    Published,
    /// Candidate replaced an existing candidate with the same
    /// discriminant.
    Replaced,
    /// Candidate admitted; `count` oldest candidates were FIFO-evicted
    /// to stay within the slot's candidate cap.
    Evicted {
        /// Number of candidates evicted to make room.
        count: usize,
    },
    /// Admission refused (e.g. the signature failed a final gate). The
    /// caller still receives its own computed value.
    Rejected(NonAdmissionReason),
}

/// Replacement / admission identity for a candidate in a multi-candidate
/// query-identity slot whose slot key is content-free.
///
/// Two candidates in one content-free slot belong to different views (a
/// base view and an overlay view, or two overlays). Whether a freshly
/// admitted candidate REPLACES an existing one or coexists as a DISTINCT
/// candidate (subject to the per-slot FIFO cap) is decided by this
/// discriminant: same `validated_at_generation` AND same `facts` replaces;
/// a different generation OR a different fact set admits a new candidate.
///
/// This is admission/replacement identity ONLY — it is NOT the read-side
/// validity oracle. A warm read still validates the matched candidate's
/// [`ReadSetSignature`] against the live store view via
/// `validate_with_self_roots`; the discriminant merely selects which
/// candidate a re-publish overwrites so base and overlay candidates do
/// not clobber each other (R20 overlay isolation).
#[derive(Clone)]
pub(crate) struct FactCandidateDiscriminant {
    /// World generation the candidate was computed under.
    pub validated_at_generation: u64,
    /// The path-precise fact set the candidate was admitted with.
    pub facts: Arc<[FactVersionRef]>,
}

impl PartialEq for FactCandidateDiscriminant {
    /// Equal when both the generation and the full fact set match. The
    /// fact comparison is structural (`FactVersionRef: PartialEq`), so a
    /// re-publish under the SAME view (same generation, same observed
    /// facts) replaces in place, while any view difference admits a
    /// distinct candidate.
    fn eq(&self, other: &Self) -> bool {
        self.validated_at_generation == other.validated_at_generation && self.facts == other.facts
    }
}

impl Eq for FactCandidateDiscriminant {}

impl std::hash::Hash for FactCandidateDiscriminant {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.validated_at_generation.hash(state);
        self.facts.hash(state);
    }
}

/// Victims a [`crate::cache_runtime::node::QueryNode::publish_core`]
/// captured under its slot/shard guard for deferred eviction.
///
/// The split publish lifecycle records the FIFO retention victims while
/// holding the non-reentrant slot guard, then evicts them AFTER that
/// guard drops (but before the retention read guard / flight completion).
/// Each victim names a `(key, admission_seq)` so its removal is
/// identity-scoped: a concurrent same-key re-publish carrying a distinct
/// seq survives. A node with no retention budget returns an empty vector.
pub(crate) type DeferredVictims<K> = Vec<(K, u64)>;

/// Outcome of [`crate::cache_runtime::node::QueryNode::publish_core`] —
/// the non-reentrant publish step that runs under the slot/shard guard.
///
/// Carries the slot-level admission outcome plus any FIFO retention
/// victims the admission displaced. The victims are evicted by
/// [`crate::cache_runtime::node::QueryNode::evict_deferred`] after the
/// internal guard drops, so a budgeted node's eviction (which re-enters
/// the slot map / reverse index) never self-deadlocks on the publish-core
/// guard.
pub(crate) struct PublishCoreOutcome<K> {
    /// The slot-level admission outcome (published / replaced / evicted /
    /// rejected by the per-slot cap).
    ///
    /// `allow(dead_code)`: the query-identity consumers route only the
    /// `deferred_victims` into `evict_deferred` and do not branch on the
    /// slot outcome; the outcome is the substrate contract for callers
    /// that need it and is asserted by the `cache_runtime` tests.
    #[allow(dead_code)]
    pub outcome: PublishOutcome,
    /// FIFO retention-budget victims to evict after the guard drops.
    pub deferred_victims: DeferredVictims<K>,
}
