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
//! cacheable; an overflow is non-cacheable with a typed reason. The
//! refusal reason ([`NonAdmissionReason`]) lives in the audit leaf
//! crate so structured refusal events can depend on it without a
//! back-edge to `verter_session`.
//!
//! # `let _ = SetReasonGuard::arm(...)` is a SILENT BYPASS
//!
//! The `#[must_use]` attribute on [`SetReasonGuard`] does NOT cover
//! `let _ = SetReasonGuard::arm(reason);` because the underscore
//! pattern itself binds the expression — and that binding drops
//! immediately on the very next semicolon. The slot is armed and
//! cleared on the same line, before the producer constructs
//! `ComputeAdmission::ReturnOnly(...)`, so the lowering observes an
//! empty slot and the debug-assert fires.
//!
//! Always use a NAMED binding:
//!
//! ```ignore
//! let _reason_guard = SetReasonGuard::arm(reason);   // CORRECT
//! return ComputeAdmission::ReturnOnly(value);
//! ```
//!
//! The leading underscore on `_reason_guard` keeps the dead-code
//! lint quiet without dropping early; the named local extends the
//! guard's lifetime to the end of the enclosing scope, which is what
//! the panic-safety contract relies on.
//!
//! This module is `deny`-linted on `let_underscore_must_use` so the
//! mistake is a compile error inside `admission.rs` itself; callers
//! in other modules are guided by the docstring above and by the
//! `let _reason_guard` convention every existing production callsite
//! follows.

#![deny(clippy::let_underscore_must_use)]

use std::sync::Arc;

use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};

/// Refusal classification for a non-cacheable admission. Re-exported
/// from the audit leaf crate ([`verter_audit::NonAdmissionReason`]) so
/// the runtime and structured refusal events name one type.
pub(crate) use verter_audit::NonAdmissionReason;

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
///   ([`crate::request_context::current_materialization_cache_suppress`]),
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
/// [`SignatureAdmission::Cacheable`]; an overflow is
/// [`SignatureAdmission::NonCacheable`] with
/// [`NonAdmissionReason::SignatureOverflow`].
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

thread_local! {
    /// Single-slot per-thread pass-through for the
    /// [`NonAdmissionReason`] a cooperative-admission producer chose
    /// when it returned [`super::singleflight::ComputeAdmission::ReturnOnly(value)`].
    ///
    /// The [`super::singleflight::ComputeAdmission::ReturnOnly(V)`]
    /// substrate variant intentionally does NOT carry the reason —
    /// widening it would force every cooperative producer to pre-commit
    /// to a reason at the variant call site. Cache-runtime lowering
    /// boundaries (`ComputeAdmission` → `CacheAdmission`) need the
    /// reason to attribute structured refusal telemetry. This TLS slot
    /// bridges the two without widening the substrate enum.
    ///
    /// Contract: the producer arms the slot with a [`SetReasonGuard`]
    /// IMMEDIATELY BEFORE constructing `ComputeAdmission::ReturnOnly(v)`;
    /// the lowering reads + clears the slot with
    /// [`take_return_only_reason`] in its `ReturnOnly` arm. The producer's
    /// `compute()` and the lowering's `match` run on the same thread
    /// (the singleflight winner's thread), in immediate sequence, so the
    /// TLS slot survives unmodified between set and take.
    ///
    /// Safety: [`SetReasonGuard`]'s `Drop` clears the slot on producer
    /// panic (via `std::thread::panicking()`) so a panic between `set`
    /// and `ReturnOnly` cannot leak a stale reason onto the next
    /// lowering on the same thread. In the normal (non-panic) flow the
    /// guard's `Drop` is a no-op; the lowering's
    /// [`take_return_only_reason`] consumes the slot.
    ///
    /// Debug builds enforce the pairing: a `take` against an empty slot
    /// is a [`debug_assert!`] failure, so a producer that forgets to
    /// arm the guard surfaces loudly under `cargo test` /
    /// `cargo build`. Release builds fall back to
    /// [`NonAdmissionReason::SignatureOverflow`] so structured refusal
    /// telemetry stays non-fatal in production.
    static LAST_RETURN_ONLY_REASON: std::cell::Cell<Option<NonAdmissionReason>> =
        const { std::cell::Cell::new(None) };
}

/// Producer-side: record the structured refusal reason for the
/// upcoming `ComputeAdmission::ReturnOnly(...)`. The matching
/// [`take_return_only_reason`] call at the lowering boundary consumes
/// the slot. Setting twice without an intervening take is harmless —
/// the lowering takes the latest value.
///
/// Prefer [`SetReasonGuard::arm`] over calling this directly: the RAII
/// guard adds panic-safety (the slot is cleared if the producer
/// unwinds between `set` and `ReturnOnly`).
#[inline]
pub(crate) fn set_return_only_reason(reason: NonAdmissionReason) {
    LAST_RETURN_ONLY_REASON.with(|c| c.set(Some(reason)));
}

/// Lowering-side primitive: consume the producer-recorded refusal
/// reason. Returns the reason the producer armed via
/// [`SetReasonGuard::arm`] / [`set_return_only_reason`], or `None` if
/// the slot is empty. Clears the slot.
///
/// This primitive is unconditional — it does NOT debug-assert on an
/// empty slot. The cache-runtime ComputeAdmission → CacheAdmission
/// lowering boundaries call [`consume_return_only_reason_for_lowering`]
/// instead, which wraps this primitive with the producer-pairing
/// debug-assert. Tests and the RAII guard's panic-path use this
/// primitive directly.
#[inline]
pub(crate) fn take_return_only_reason() -> Option<NonAdmissionReason> {
    LAST_RETURN_ONLY_REASON.with(|c| c.take())
}

/// Lowering-side: consume the producer-recorded refusal reason on a
/// `ComputeAdmission::ReturnOnly(...)` arm. Returns the reason the
/// producer armed, or `None` if the slot is empty.
///
/// Debug builds: `debug_assert!`s on a `None` slot — a producer
/// reached `ComputeAdmission::ReturnOnly(...)` without arming the
/// reason, which would silently mis-attribute structured refusal
/// telemetry to the conservative fallback reason. Surfacing under
/// `cargo test` forces the producer-side omission to be fixed at the
/// callsite rather than masked.
///
/// Release builds: returns `None` on an empty slot; the caller folds
/// to [`NonAdmissionReason::SignatureOverflow`] as a conservative
/// non-fatal default.
#[inline]
pub(crate) fn consume_return_only_reason_for_lowering() -> Option<NonAdmissionReason> {
    let taken = take_return_only_reason();
    debug_assert!(
        taken.is_some(),
        "cache-runtime lowering: `consume_return_only_reason_for_lowering` \
         called with an empty slot. Every \
         `ComputeAdmission::ReturnOnly(value)` arm MUST be preceded by \
         a `SetReasonGuard::arm(reason)` on the producer side so \
         structured refusal telemetry attributes the actual cause. \
         The release-build fallback masks this omission with \
         `SignatureOverflow`; debug builds surface it."
    );
    taken
}

/// RAII guard that arms the TLS refusal-reason slot for the upcoming
/// `ComputeAdmission::ReturnOnly(...)`. The construct-and-drop sequence
/// is:
///
///   * `SetReasonGuard::arm(reason)` records the reason in the TLS
///     slot and returns the guard.
///   * The producer returns `ComputeAdmission::ReturnOnly(value)`.
///   * The guard drops at the end of `compute()`; in the normal flow
///     `Drop` is a no-op (the lowering will consume the slot).
///   * The lowering matches `ComputeAdmission::ReturnOnly(value)` and
///     calls [`take_return_only_reason`], consuming the slot.
///
/// On panic between `arm` and `ReturnOnly`, `Drop` runs with
/// [`std::thread::panicking()`] returning `true`. The guard clears the
/// slot so the NEXT unrelated lowering on the same thread does not
/// inherit a stale reason. The window between `arm` and the inner
/// `ComputeAdmission::ReturnOnly(...)` construction is small (an
/// `Arc::clone` and a struct literal), but the panic-safety is cheap
/// and the alternative (a leaked reason silently mis-attributing the
/// next refusal) is silent.
///
/// # Binding convention — `#[must_use]` does NOT cover `let _`
///
/// The `#[must_use]` attribute below WARNS only when the returned
/// guard is fully ignored (e.g. `SetReasonGuard::arm(reason);` with
/// no binding). It does NOT trigger on
/// `let _ = SetReasonGuard::arm(reason);` because the underscore
/// pattern IS a binding — and that binding drops the guard
/// IMMEDIATELY on the very next semicolon. The slot is armed and
/// then cleared on the same line, before the producer's
/// `ComputeAdmission::ReturnOnly(...)` constructor runs, so the
/// lowering sees an empty slot and the debug-assert fires.
///
/// **Use `let _reason_guard = SetReasonGuard::arm(reason);`** — a
/// named binding extends the guard's lifetime to the end of the
/// enclosing scope, which is what the panic-safety contract relies
/// on. A leading underscore (`_reason_guard`) suppresses the
/// dead-code lint without dropping early.
///
/// `let _ = ...` for this guard is the same defect as forgetting
/// `arm` entirely. The lint
/// `#[deny(clippy::let_underscore_must_use)]` at the module level
/// makes this mistake a compile error.
#[must_use = "the guard must be bound to a named local (e.g. \
              `let _reason_guard = SetReasonGuard::arm(reason);`) so \
              it stays alive until the producer constructs \
              ComputeAdmission::ReturnOnly. `let _ = ...` drops the \
              guard immediately on the same statement and breaks \
              panic-safety."]
pub(crate) struct SetReasonGuard {
    // Marker to prevent direct construction (force callers through
    // `arm` so the slot is always armed).
    _private: (),
}

impl SetReasonGuard {
    /// Arm the TLS slot with `reason` and return the guard. Pair with
    /// `ComputeAdmission::ReturnOnly(value)` on the next statement;
    /// the lowering's [`take_return_only_reason`] consumes the slot.
    #[inline]
    pub(crate) fn arm(reason: NonAdmissionReason) -> Self {
        set_return_only_reason(reason);
        SetReasonGuard { _private: () }
    }
}

impl Drop for SetReasonGuard {
    #[inline]
    fn drop(&mut self) {
        if std::thread::panicking() {
            // Producer panicked between `arm` and `ReturnOnly`. Clear
            // the TLS slot so the next unrelated lowering on this
            // thread does not inherit a stale reason. Cleared via the
            // bare slot write rather than `take_return_only_reason`
            // so the debug-assert in `take` does not fire on the
            // already-cleared next call.
            LAST_RETURN_ONLY_REASON.with(|c| c.set(None));
        }
        // Normal flow: the lowering's `take_return_only_reason` will
        // (or already did) consume the slot. The guard's `Drop` is a
        // no-op so the take sees the armed value.
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
