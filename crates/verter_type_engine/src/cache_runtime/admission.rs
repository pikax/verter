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
//! cacheable; intrinsic non-cacheability and mutation instability are
//! both non-cacheable with typed reasons; signature width never is. The
//! refusal reason ([`NonAdmissionReason`]) lives in the audit leaf
//! crate so structured refusal events can depend on it without a
//! back-edge to `verter_session`.
//!

use std::sync::Arc;

use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_session_query::facts::fact_cache::ReadSetSignature;

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
pub fn non_admission_propagation(
    reason: NonAdmissionReason,
) -> verter_session_query::facts::fact_read_set::NonCacheablePropagation {
    use verter_session_query::facts::fact_read_set::NonCacheablePropagation;

    match reason {
        // A retention refusal declines to STORE a value that is complete
        // and correct. The caller still returns it, and an enclosing
        // derivation that consumed it observed no defective input — so
        // the refusal stays confined to the family that made it. Marking
        // it transitive would make one cache's memory pressure suppress
        // every enclosing cache's admission.
        NonAdmissionReason::IntrinsicNonCacheable
        | NonAdmissionReason::ForcedTestRefusal
        | NonAdmissionReason::RetentionPressure => NonCacheablePropagation::LocalOnly,
        NonAdmissionReason::SignatureOverflow
        // A domain that moved mid-scope invalidates the DERIVATION BASIS,
        // not just this scope's own entry: an enclosing scope that
        // consumed this value observed the same moving domain.
        | NonAdmissionReason::MutationUnstable
        | NonAdmissionReason::EmptySignature
        | NonAdmissionReason::SelfRootConflict
        | NonAdmissionReason::RouteGenerationDependency
        | NonAdmissionReason::GenerationSuperseded
        | NonAdmissionReason::PostComputeRevalidationFailed
        | NonAdmissionReason::BudgetExceeded
        | NonAdmissionReason::Cancelled
        | NonAdmissionReason::UnresolvedProvenance
        | NonAdmissionReason::ComputeFailed
        | NonAdmissionReason::PartialResult
        | NonAdmissionReason::ResolutionInaccessiblePath
        | NonAdmissionReason::ResolutionUnknownPath
        | NonAdmissionReason::ResolutionWorldChanged
        | NonAdmissionReason::ResolutionViewSuperseded
        | NonAdmissionReason::ResolutionUntrackedBackend
        | NonAdmissionReason::ResolutionIncompleteProvenance
        | NonAdmissionReason::ResolutionRetryExhausted => NonCacheablePropagation::Transitive,
    }
}

/// Propagate a returned non-admitted value into the active enclosing tracer
/// scopes when its typed reason describes a transitive derivation hazard.
/// Local-only refusal has already been enforced by the cache owner and must
/// not poison a caller that can independently root its own result.
#[inline]
pub fn propagate_non_admission(reason: NonAdmissionReason) {
    let propagation = non_admission_propagation(reason);
    if propagation
        == verter_session_query::facts::fact_read_set::NonCacheablePropagation::Transitive
    {
        crate::fact_tracing::note_non_cacheable_propagation(propagation);
    }
}

/// THE shared result-cache partial-admission gate — **PURE** over the
/// supplied completeness.
///
/// A GENUINE partial (a budget exhaustion / fatal `QueryError` /
/// same-path recursion / walker fatal) must NOT warm-replay as a complete
/// result. Every result-cache admission path (`ShapeCacheDb`,
/// `ImportedRegistryDb`, `ResolvabilityDb`) routes its
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
/// - A SHARED semantic-cache producer passes its PER-COLD-COMPUTE
///   completeness
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
/// cross-owner-reuse admission, a fenced serve, an
/// unrootable self-root) is COMPLETE and MUST still be allowed to warm
/// its result cache; only a genuine partial is refused here. The
/// inner-memo `cache_suppress || result_is_partial` gate (the semantic
/// family memo) is a SEPARATE, stricter gate that stays as it is.
#[inline]
#[must_use]
pub fn refuse_result_cache_admission_if_partial(value_is_partial: bool) -> bool {
    value_is_partial
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
/// families (non-cacheable read / forced-refusal / budget-exceeded → `ReturnOnly`).
#[allow(dead_code)]
pub enum CacheAdmission<V> {
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
///
/// An entry exists only charged ([`Self::admit`]). Its own allocations —
/// its map slot and key, its block, its fact slice, its self-root list,
/// whatever heap its value keeps outside owned cells, and the evidence
/// pages its signature reaches — are one refusable `Retained` reservation
/// released with the entry. The value's owned cells (the allocations a warm
/// hit hands out) carry their own charges, which the reservation holds:
/// when the entry goes, each passes to the handles callers still keep and
/// is released by the last of them.
pub struct CacheEntry<V> {
    /// The cached value.
    pub value: V,
    /// The path-precise fact signature, validated against the live
    /// store view on every warm hit.
    pub signature: ReadSetSignature,
    /// Canonicals validated strictly as self-roots.
    pub self_root_canonicals: Arc<[Arc<str>]>,
    /// World generation the entry was computed under.
    pub validated_at_generation: u64,
    _retention: verter_session_query::retention::RetentionCharge,
}

/// An entry the account refused to retain: its complete value, uncached,
/// with every owned cell it reaches charged to the callers that keep it.
pub struct RefusedEntry<V> {
    /// The complete value.
    pub value: V,
    /// Its signature.
    pub signature: ReadSetSignature,
    /// Its self-roots.
    pub self_root_canonicals: Arc<[Arc<str>]>,
    /// Its generation stamp.
    pub validated_at_generation: u64,
}

impl<V> CacheEntry<V> {
    /// Charge an entry for keeping `value` in a map slot keyed by `K`.
    /// `footprint` is what the key and the value keep beyond their inline
    /// bytes: the heap the entry owns alone, and the value's owned cells.
    ///
    /// # Errors
    ///
    /// The account's refusal: nothing is retained, the complete value comes
    /// back, and its owned cells are charged to its callers.
    pub fn admit<K>(
        value: V,
        signature: ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        validated_at_generation: u64,
        footprint: verter_session_query::retention::Footprint,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Result<Self, RefusedEntry<V>> {
        use verter_session_query::retention::allocation::{
            arc_block_bytes, arc_str_list_bytes, fact_slice_bytes, AliasSet,
        };
        use verter_session_query::retention::RetentionAdmission;
        let ctx = verter_session_query::retention::owned::ChargeCtx::new(account);
        let mut aliases = AliasSet::new();
        let own = verter_session_query::retention::ENTRY_OVERHEAD_BYTES
            + std::mem::size_of::<(K, Arc<Self>)>()
            + arc_block_bytes(std::mem::size_of::<Self>())
            + fact_slice_bytes(&signature.facts, &mut aliases)
            + arc_str_list_bytes(&self_root_canonicals, &mut aliases)
            + footprint.bytes;
        let handles = aliases.take_handles().merge(footprint.handles);
        let Ok(holds) = handles.hold_or_pin(ctx) else {
            return Err(RefusedEntry {
                value,
                signature,
                self_root_canonicals,
                validated_at_generation,
            });
        };
        match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            account,
            own,
            &[&signature.facts],
        ) {
            RetentionAdmission::Admitted(charge) => Ok(Self {
                value,
                signature,
                self_root_canonicals,
                validated_at_generation,
                _retention: charge.holding(holds),
            }),
            // Dropping the holds hands each cell back to its callers.
            RetentionAdmission::Refused(_) => {
                drop(holds);
                Err(RefusedEntry {
                    value,
                    signature,
                    self_root_canonicals,
                    validated_at_generation,
                })
            }
        }
    }

    /// Bytes the entry's own reservation holds, its owned cells' charges
    /// aside.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn charge_bytes_for_tests(&self) -> usize {
        self._retention.bytes()
    }
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
pub struct Candidate<D, V> {
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
    /// The candidate's own charge — its slot share, block, fact slice,
    /// self-roots, reverse-index registrations and the heap its value keeps
    /// outside owned cells — holding the value's owned cells; released with
    /// the candidate and its last clone ([`Candidate::admit`]).
    pub retention: Arc<verter_session_query::retention::RetentionCharge>,
}

impl<D, V> Candidate<D, V> {
    /// Charge a candidate keeping `value` in a multi-candidate slot keyed by
    /// `K` and registered under `registrations` canonicals of its store's
    /// reverse index. `footprint` is what the key and the value keep beyond
    /// their inline bytes: the heap the candidate owns alone, and the value's
    /// owned cells. Each candidate pays for its key's slot (a declared
    /// convention: a slot is shared by at most its per-slot cap of
    /// candidates) and one index record per registration.
    ///
    /// # Errors
    ///
    /// The account's refusal: nothing is retained, the complete value comes
    /// back, and its owned cells are charged to its callers.
    #[allow(clippy::too_many_arguments)]
    pub fn admit<K>(
        discriminant: D,
        value: V,
        signature: ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        validated_at_generation: u64,
        registrations: usize,
        footprint: verter_session_query::retention::Footprint,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Result<Self, RefusedEntry<V>> {
        use verter_session_query::retention::allocation::{
            arc_block_bytes, arc_str_list_bytes, fact_slice_bytes, AliasSet,
        };
        use verter_session_query::retention::{RetentionAdmission, ENTRY_OVERHEAD_BYTES};
        let ctx = verter_session_query::retention::owned::ChargeCtx::new(account);
        let mut aliases = AliasSet::new();
        let own = ENTRY_OVERHEAD_BYTES
            + std::mem::size_of::<(K, usize)>()
            + std::mem::size_of::<Arc<Self>>()
            + arc_block_bytes(std::mem::size_of::<Self>())
            + fact_slice_bytes(&signature.facts, &mut aliases)
            + arc_str_list_bytes(&self_root_canonicals, &mut aliases)
            + registrations * (ENTRY_OVERHEAD_BYTES + std::mem::size_of::<(K, u64)>())
            + footprint.bytes;
        let handles = aliases.take_handles().merge(footprint.handles);
        let refused = |value, signature, self_root_canonicals| RefusedEntry {
            value,
            signature,
            self_root_canonicals,
            validated_at_generation,
        };
        let Ok(holds) = handles.hold_or_pin(ctx) else {
            return Err(refused(value, signature, self_root_canonicals));
        };
        match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            account,
            own,
            &[&signature.facts],
        ) {
            RetentionAdmission::Admitted(charge) => Ok(Self {
                discriminant,
                value,
                signature,
                self_root_canonicals,
                admission_seq: 0,
                validated_at_generation,
                retention: Arc::new(charge.holding(holds)),
            }),
            // Dropping the holds hands each cell back to its callers.
            RetentionAdmission::Refused(_) => {
                drop(holds);
                Err(refused(value, signature, self_root_canonicals))
            }
        }
    }
}

/// Outcome of admitting a candidate into a query-identity slot.
///
/// `allow(dead_code)`: carried by [`PublishCoreOutcome`] from
/// `QueryNode::publish_core` / `ReverseIndexedCandidateStore::publish_core`
/// — query-node substrate exercised by the `cache_runtime` tests.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
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
pub struct FactCandidateDiscriminant {
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
pub type DeferredVictims<K> = Vec<(K, u64)>;

/// Outcome of [`crate::cache_runtime::node::QueryNode::publish_core`] —
/// the non-reentrant publish step that runs under the slot/shard guard.
///
/// Carries the slot-level admission outcome plus any FIFO retention
/// victims the admission displaced. The victims are evicted by
/// [`crate::cache_runtime::node::QueryNode::evict_deferred`] after the
/// internal guard drops, so a budgeted node's eviction (which re-enters
/// the slot map / reverse index) never self-deadlocks on the publish-core
/// guard.
pub struct PublishCoreOutcome<K> {
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
