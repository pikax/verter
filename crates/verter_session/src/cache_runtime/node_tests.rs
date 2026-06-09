//! Node-facing cache-runtime discriminators — included as a child `mod`
//! of `node` via `#[path]` so `use super::*` reaches the trait surface
//! and the `lookup` / `query::lookup` / `publish` entry points directly.
//!
//! Two families:
//!   * Structural discriminators on the substrate types
//!     (`CacheAdmission<V>` holds the unwrapped value; `ArtifactNode`
//!     has no `Entry` associated type).
//!   * Behavioural discriminators driven through a bare `VerterHost`
//!     (the `ResolverContext` rail): `lookup` dedups cold compute, the
//!     query publish gate rejects a generation-superseded candidate.

use super::*;
use crate::cache_runtime::admission::{FactCandidateDiscriminant, NonAdmissionReason};
use crate::cache_runtime::candidate_store::ReverseIndexedCandidateStore;
use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// `CacheAdmission::Cacheable` holds the caller-visible value UNWRAPPED.
///
/// The runtime wraps the stored carrier (`CacheEntry` / `Candidate`) in
/// `Arc` at admission, NOT the value — forcing a universal `Arc<V>`
/// would double-wrap callers whose `V` is already `Arc<T>`. This test
/// destructures the `Cacheable` arm and binds `value` as a bare `String`
/// (not `Arc<String>`): a regression that wrapped the value would fail
/// to compile here.
#[test]
fn cacheable_arm_holds_unwrapped_value() {
    let admission: CacheAdmission<String> = CacheAdmission::Cacheable {
        value: "bare".to_string(),
        signature: ReadSetSignature::empty(),
        self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
        validated_at_generation: 0,
    };
    match admission {
        CacheAdmission::Cacheable { value, .. } => {
            // `value` is `String`, not `Arc<String>` — a bare move.
            let _bound: String = value;
            assert_eq!(_bound, "bare");
        }
        _ => panic!("expected Cacheable"),
    }
}

/// `CacheAdmission<V>` is a three-arm contract — `Cacheable`,
/// `ReturnOnly`, and `Failed` — and the non-cacheable arms carry a typed
/// [`NonAdmissionReason`]. This test constructs all three arms and reads
/// every field, so a regression dropping an arm or a reason field fails
/// to compile / fails its assertion here.
#[test]
fn cache_admission_has_three_arms_with_typed_reasons() {
    let cacheable: CacheAdmission<u8> = CacheAdmission::Cacheable {
        value: 1,
        signature: ReadSetSignature::empty(),
        self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
        validated_at_generation: 0,
    };
    let return_only: CacheAdmission<u8> = CacheAdmission::ReturnOnly {
        value: 2,
        reason: NonAdmissionReason::SignatureOverflow,
    };
    let failed: CacheAdmission<u8> = CacheAdmission::Failed {
        reason: NonAdmissionReason::ComputeFailed,
    };

    match cacheable {
        CacheAdmission::Cacheable { value, .. } => assert_eq!(value, 1),
        _ => panic!("expected Cacheable"),
    }
    match return_only {
        CacheAdmission::ReturnOnly { value, reason } => {
            assert_eq!(value, 2);
            assert_eq!(reason, NonAdmissionReason::SignatureOverflow);
        }
        _ => panic!("expected ReturnOnly"),
    }
    match failed {
        CacheAdmission::Failed { reason } => {
            assert_eq!(reason, NonAdmissionReason::ComputeFailed);
        }
        _ => panic!("expected Failed"),
    }
}

/// `SignatureAdmission::from_finalise` lifts an OK finalised tracer into
/// the `Cacheable` arm, carrying the observed facts as the warm-hit
/// validation signature.
///
/// Discriminating: `from_finalise` must construct `Cacheable` from
/// `FactReadSetFinalise::Ok(facts)` and surface the same facts back
/// through `cacheable()`; a regression that mapped `Ok` to
/// `NonCacheable` would yield `None` from `cacheable()` and fail here.
#[test]
fn signature_admission_from_ok_finalise_is_cacheable() {
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: "/a.ts".to_string(),
        hash: [1u8; 16],
    }]);
    let admission = SignatureAdmission::from_finalise(FactReadSetFinalise::Ok(Arc::clone(&facts)));

    match &admission {
        SignatureAdmission::Cacheable(sig) => {
            assert_eq!(
                sig.facts.len(),
                1,
                "the cacheable signature carries the finalised observation set"
            );
        }
        SignatureAdmission::NonCacheable(reason) => {
            panic!("an OK finalise must be cacheable, got NonCacheable({reason:?})")
        }
    }

    // `cacheable()` surfaces the signature for the cacheable arm.
    let sig = admission
        .cacheable()
        .expect("an OK finalise must expose its signature through cacheable()");
    assert_eq!(sig.facts.len(), 1);
}

/// `SignatureAdmission::from_finalise` lifts an overflowed finalised
/// tracer into the `NonCacheable` arm with the
/// [`NonAdmissionReason::SignatureOverflow`] reason, and `cacheable()`
/// returns `None`.
///
/// Discriminating: a regression that admitted an overflow as cacheable
/// (or carried a different reason) would fail the match / reason
/// assertion, and `cacheable()` returning `Some` for an overflow would
/// fail the final assertion.
#[test]
fn signature_admission_from_overflow_finalise_is_non_cacheable() {
    let admission = SignatureAdmission::from_finalise(FactReadSetFinalise::Overflow);

    match &admission {
        SignatureAdmission::NonCacheable(reason) => {
            assert_eq!(
                *reason,
                NonAdmissionReason::SignatureOverflow,
                "an overflowed tracer is non-cacheable with the overflow reason"
            );
        }
        SignatureAdmission::Cacheable(_) => {
            panic!("an overflowed finalise must NOT be cacheable")
        }
    }

    // `cacheable()` returns None for the non-cacheable arm.
    assert!(
        admission.cacheable().is_none(),
        "a non-cacheable admission must not expose a signature through cacheable()"
    );
}

/// `ComputeCtx` carries the store-view compat token (the flight-lane
/// dimension) alongside the generation. `from_resolver` reads both from
/// the resolver. This test reads `compat_token` so the field's role is
/// exercised — a regression dropping it from the context fails here.
#[test]
fn compute_ctx_from_resolver_carries_compat_token_and_generation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext = &host;
    let cx = ComputeCtx::from_resolver(ctx);
    // The compat token matches the resolver's store-view token, and the
    // generation matches the project-type-store generation.
    assert_eq!(cx.compat_token, ctx.store_view().compat_token());
    assert_eq!(
        cx.generation(),
        ctx.project_type_store().current_project_generation()
    );
}

/// A minimal `ArtifactNode` impl over a bare value type. The point of
/// this test is structural: `ArtifactNode` exposes ONLY `Key` and
/// `Value` associated types — there is NO `Entry` associated type (the
/// runtime owns the `CacheEntry<Value>` carrier internally). If a future
/// refactor reintroduced an `Entry` associated type, this impl would
/// fail to satisfy the trait (missing associated type) and the test
/// would fail to compile.
struct CountingArtifactNode {
    entries: dashmap::DashMap<u32, Arc<CacheEntry<String>>>,
    inflight: InflightTable<QueryFlightKey<u32>>,
    compute_count: Arc<AtomicUsize>,
}

impl ArtifactNode for CountingArtifactNode {
    type Key = u32;
    type Value = String;

    fn entries(&self) -> &dashmap::DashMap<Self::Key, Arc<CacheEntry<Self::Value>>> {
        &self.entries
    }

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        self.compute_count.fetch_add(1, Ordering::SeqCst);
        CacheAdmission::Cacheable {
            value: format!("v{key}"),
            // Empty signature with no self-roots validates vacuously, so
            // the generation gate alone decides validity here.
            signature: ReadSetSignature::empty(),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            validated_at_generation: cx.generation(),
        }
    }

    fn validate(
        &self,
        _key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        if entry.validated_at_generation == cx.generation() {
            Some(entry.value.clone())
        } else {
            None
        }
    }
}

/// `ArtifactNode` exposes ONLY `Key` and `Value` associated types — no
/// `Entry` associated type. The runtime owns the `CacheEntry<Value>`
/// carrier internally; the node never names it as an associated type.
///
/// This is enforced structurally: the function below names every
/// associated type of `ArtifactNode` in a where-clause that binds
/// `Key = u32` and `Value = String`. If a future refactor added an
/// `Entry` associated type, `CountingArtifactNode`'s impl (which
/// supplies only `Key` and `Value`) would no longer satisfy the trait
/// and this would fail to compile.
#[test]
fn artifact_node_has_no_entry_associated_type() {
    fn assert_only_key_and_value<N>()
    where
        N: ArtifactNode<Key = u32, Value = String>,
    {
        // Naming `N::Key` and `N::Value` exhausts the public associated
        // types. `CacheEntry<N::Value>` is referenced as a CONCRETE
        // runtime type parameterised by `Value`, NOT as `N::Entry`.
        fn _carrier_is_runtime_owned<V: Clone + Send + Sync + 'static>(
        ) -> std::marker::PhantomData<CacheEntry<V>> {
            std::marker::PhantomData
        }
        let _: std::marker::PhantomData<(N::Key, N::Value)> = std::marker::PhantomData;
        let _ = _carrier_is_runtime_owned::<N::Value>();
    }
    assert_only_key_and_value::<CountingArtifactNode>();
}

/// `lookup` dedups the cold compute for repeated calls on the same key
/// under the same view: the first call computes and publishes, the
/// second is a warm map hit (no recompute).
#[test]
fn lookup_dedups_cold_compute_under_same_view() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext = &host;
    let node = CountingArtifactNode {
        entries: dashmap::DashMap::new(),
        inflight: InflightTable::new(),
        compute_count: Arc::new(AtomicUsize::new(0)),
    };

    let first = lookup(&node, 7u32, ctx);
    assert_eq!(first.as_deref(), Some("v7"));
    let second = lookup(&node, 7u32, ctx);
    assert_eq!(second.as_deref(), Some("v7"));

    assert_eq!(
        node.compute_count.load(Ordering::SeqCst),
        1,
        "lookup must compute once and serve the second call from the warm map"
    );
    assert_eq!(
        node.entries.len(),
        1,
        "exactly one entry published under the key"
    );
}

/// A test `QueryNode` whose `compute` stamps a SUPERSEDED generation on
/// the candidate, modelling a self-root edit / generation bump that
/// landed during the cold compute window. The publish gate in
/// `query::lookup` (`candidate.validated_at_generation == generation`)
/// must then reject the candidate: `publish_core` is never invoked, and
/// the caller receives `None`. Storage is the shared
/// [`ReverseIndexedCandidateStore`].
struct StaleGenerationQueryNode {
    inflight: InflightTable<QueryFlightKey<u32>>,
    publish_count: Arc<AtomicUsize>,
    store: ReverseIndexedCandidateStore<u32, String>,
}

impl QueryNode for StaleGenerationQueryNode {
    type Key = u32;
    type Discriminant = FactCandidateDiscriminant;
    type Value = String;

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    fn lookup_candidate(&self, key: &Self::Key, cx: &ComputeCtx<'_>) -> Option<Self::Value> {
        // The store validates by generation; the stale candidate never
        // matches the live generation, so the lookup is always a miss.
        let generation = cx.generation();
        self.store.lookup(key, |candidate| {
            if candidate.validated_at_generation == generation {
                Some(candidate.value.clone())
            } else {
                None
            }
        })
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        CacheAdmission::Cacheable {
            value: format!("stale{key}"),
            signature: ReadSetSignature::empty(),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            // SUPERSEDED: one generation behind the live generation. The
            // publish gate `validated_at_generation == generation` fails.
            validated_at_generation: cx.generation().wrapping_sub(1),
        }
    }

    fn discriminant(
        &self,
        _key: &Self::Key,
        _value: &Self::Value,
        signature: &ReadSetSignature,
        validated_at_generation: u64,
    ) -> Self::Discriminant {
        // The discriminant carries the candidate's OWN stamped generation
        // (passed straight from the `Cacheable` arm), so it matches the
        // superseded `validated_at_generation` this node stamps in
        // `compute` — the runtime never substitutes its lookup-entry
        // snapshot here.
        FactCandidateDiscriminant {
            validated_at_generation,
            facts: Arc::clone(&signature.facts),
        }
    }

    fn publish_core(
        &self,
        key: Self::Key,
        candidate: Candidate<Self::Discriminant, Self::Value>,
    ) -> PublishCoreOutcome<Self::Key> {
        self.publish_count.fetch_add(1, Ordering::SeqCst);
        self.store.publish_core(key, candidate)
    }

    fn evict_deferred(&self, victims: DeferredVictims<Self::Key>) {
        self.store.evict_deferred(victims);
    }
}

/// `query::lookup` rejects a candidate whose self-root was superseded
/// mid-compute (modelled by a stale `validated_at_generation`): the
/// publish closure is NOT invoked and the caller receives `None`.
#[test]
fn publish_rejects_candidate_when_self_root_edited_mid_compute() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext = &host;
    let node = StaleGenerationQueryNode {
        inflight: InflightTable::new(),
        publish_count: Arc::new(AtomicUsize::new(0)),
        store: ReverseIndexedCandidateStore::with_counter(Arc::new(AtomicU64::new(0))),
    };

    let result = query::lookup(&node, 1u32, ctx);
    assert_eq!(
        result, None,
        "a generation-superseded candidate must be rejected by the publish gate"
    );
    assert_eq!(
        node.publish_count.load(Ordering::SeqCst),
        0,
        "publish_core must NOT be invoked when post-compute revalidation rejects the entry"
    );
    assert_eq!(
        node.store.live_count(),
        0,
        "no candidate must enter the store"
    );
}

/// A `QueryNode` modelling the discriminant-generation-skew scenario.
///
/// `compute` bumps the host project generation on its FIRST cold build
/// (modelling a tsconfig / path-alias change that lands DURING the
/// runtime's cold window, AFTER `query::lookup` captured its lookup-entry
/// snapshot) and then stamps the candidate at the now-LIVE generation —
/// exactly as a real producer snapshots its generation inside its own
/// `install_fact_tracer` compute, distinct from the runtime's lookup-entry
/// snapshot. Subsequent cold builds re-read the (already-bumped) live
/// generation without bumping again, so every candidate carries the SAME
/// stamped generation with the SAME facts.
///
/// `lookup_candidate` always misses so each `query::lookup` call drives a
/// fresh cold publish (the slot keeps prior candidates; this node never
/// view-validates them).
struct SkewedDiscriminantQueryNode {
    inflight: InflightTable<QueryFlightKey<u32>>,
    store: ReverseIndexedCandidateStore<u32, String>,
    /// Cold-compute call counter — the first call bumps the generation.
    compute_calls: Arc<AtomicUsize>,
}

impl SkewedDiscriminantQueryNode {
    /// One fixed fact set shared by every candidate, so the discriminant's
    /// fact dimension is constant and only its GENERATION dimension can
    /// differ between candidates.
    fn fixed_signature() -> ReadSetSignature {
        let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
            canonical_id: "/skew.ts".to_string(),
            hash: [7u8; 16],
        }]);
        ReadSetSignature::new(facts)
    }
}

impl QueryNode for SkewedDiscriminantQueryNode {
    type Key = u32;
    type Discriminant = FactCandidateDiscriminant;
    type Value = String;

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    fn lookup_candidate(&self, _key: &Self::Key, _cx: &ComputeCtx<'_>) -> Option<Self::Value> {
        // Always miss: force the cold publish path on every call so the
        // second call re-publishes rather than serving a warm hit.
        None
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        // On the FIRST cold build, bump the project generation so the live
        // generation now LEADS the runtime's lookup-entry snapshot
        // (`cx.generation()` here is that snapshot). The candidate is then
        // stamped at the LIVE generation, so its stamp != the lookup-entry
        // snapshot — the exact skew the discriminant must NOT inherit.
        if self.compute_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            cx.resolver.project_type_store().bump_project_generation();
        }
        let live = cx
            .resolver
            .project_type_store()
            .current_project_generation();
        CacheAdmission::Cacheable {
            value: format!("v{key}"),
            signature: Self::fixed_signature(),
            // Empty self-roots so `validate_with_self_roots` is vacuous and
            // the generation gate alone decides revalidation.
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            // Stamp at the LIVE generation (the producer's in-compute
            // snapshot), NOT `cx.generation()` (the runtime's lookup-entry
            // snapshot). On the first call these differ; the post-compute
            // revalidation (`stamp == live`) still passes because the bump
            // already landed.
            validated_at_generation: live,
        }
    }

    fn discriminant(
        &self,
        _key: &Self::Key,
        _value: &Self::Value,
        signature: &ReadSetSignature,
        validated_at_generation: u64,
    ) -> Self::Discriminant {
        // Build from the candidate's OWN stamp (threaded straight from the
        // `Cacheable` arm). This is the fix under test: a regression that
        // read the runtime's lookup-entry snapshot instead would skew the
        // first publish's discriminant generation away from its candidate
        // generation, so the second same-view publish would COEXIST as a
        // duplicate.
        FactCandidateDiscriminant {
            validated_at_generation,
            facts: Arc::clone(&signature.facts),
        }
    }

    fn publish_core(
        &self,
        key: Self::Key,
        candidate: Candidate<Self::Discriminant, Self::Value>,
    ) -> PublishCoreOutcome<Self::Key> {
        self.store.publish_core(key, candidate)
    }

    fn evict_deferred(&self, victims: DeferredVictims<Self::Key>) {
        self.store.evict_deferred(victims);
    }
}

/// The candidate's discriminant generation MUST equal the candidate's
/// stamped `validated_at_generation`, even when a project-generation bump
/// lands between the runtime's lookup-entry snapshot and the producer's
/// in-compute snapshot.
///
/// Scenario: the first `query::lookup` enters at generation `G`; its cold
/// compute bumps the generation to `G+1` and stamps the candidate at
/// `G+1`. The second `query::lookup` enters at `G+1` and stamps a second
/// candidate, also at `G+1`, with the SAME facts. Both candidates share
/// one view (`G+1`, same facts), so the second publish must REPLACE the
/// first — exactly ONE candidate in the slot.
///
/// DISCRIMINATES: pre-fix the discriminant was built from
/// `cx.generation()` (the lookup-entry snapshot), so the first publish's
/// discriminant carried `G` while its candidate carried `G+1`; the second
/// publish's discriminant carried `G+1`. The two discriminants differed,
/// so the candidates COEXISTED — `slot_len == 2`, and the cap-4 budget /
/// FIFO order would be consumed by a phantom duplicate. Post-fix both
/// discriminants are `G+1` (the candidate stamp), the second publish
/// replaces in place, and `slot_len == 1`.
#[test]
fn discriminant_generation_tracks_candidate_stamp_not_lookup_snapshot() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext = &host;
    let gen_before = ctx.project_type_store().current_project_generation();
    let node = SkewedDiscriminantQueryNode {
        inflight: InflightTable::new(),
        store: ReverseIndexedCandidateStore::with_counter(Arc::new(AtomicU64::new(0))),
        compute_calls: Arc::new(AtomicUsize::new(0)),
    };

    // First publish: enters at `G`, bumps to `G+1` mid-compute, stamps the
    // candidate at `G+1`.
    let first = query::lookup(&node, 1u32, ctx);
    assert_eq!(first.as_deref(), Some("v1"), "first cold build publishes");
    let gen_after_first = ctx.project_type_store().current_project_generation();
    assert_eq!(
        gen_after_first,
        gen_before + 1,
        "the first cold compute bumped the project generation once"
    );
    assert_eq!(
        node.store.slot_len_for_test(&1),
        1,
        "the first publish admits exactly one candidate"
    );

    // Second publish: enters at `G+1` (no further bump), stamps a second
    // candidate at `G+1` with the same facts. Same view → must REPLACE.
    let second = query::lookup(&node, 1u32, ctx);
    assert_eq!(second.as_deref(), Some("v1"), "second cold build publishes");
    assert_eq!(
        ctx.project_type_store().current_project_generation(),
        gen_before + 1,
        "the second cold compute does NOT bump again"
    );

    // THE DISCRIMINATOR: both candidates carry the SAME stamped generation
    // (`G+1`) and the SAME facts, so they are one view and the second
    // publish replaced the first. A skewed discriminant (pre-fix) would
    // have produced two coexisting candidates here.
    assert_eq!(
        node.store.slot_len_for_test(&1),
        1,
        "the second same-view publish MUST replace the first candidate, not coexist as a \
         duplicate — the discriminant generation must equal the candidate's stamped generation, \
         not the runtime's lookup-entry snapshot"
    );
    assert_eq!(
        node.store.live_count(),
        1,
        "exactly one live candidate after the replace"
    );
    assert_eq!(
        node.compute_calls.load(Ordering::SeqCst),
        2,
        "both calls took the cold path (lookup_candidate always misses)"
    );
}

/// Exhaustive enumeration of every [`NonAdmissionReason`] variant.
/// Defined as an exhaustive `match` over a representative value so a
/// new variant added to the enum forces a compile-fail here, prompting
/// the maintainer to include it in the bridge round-trip + behavioral
/// telemetry tests below.
///
/// The map-and-collect is intentional: returning the array literal
/// directly would NOT force exhaustiveness, but the inner `match`
/// does.
fn all_non_admission_reasons() -> Vec<NonAdmissionReason> {
    // The match is exhaustive: a new variant added to the enum makes
    // this fail to compile until the variant is added to the list
    // below. The dummy value here is `SignatureOverflow`; the match
    // arm bodies just yield the discriminant back, but the exhaustive
    // shape is the discriminator.
    let _exhaustive_compile_check = |r: NonAdmissionReason| match r {
        NonAdmissionReason::IntrinsicNonCacheable => (),
        NonAdmissionReason::SignatureOverflow => (),
        NonAdmissionReason::EmptySignature => (),
        NonAdmissionReason::SelfRootConflict => (),
        NonAdmissionReason::RouteGenerationDependency => (),
        NonAdmissionReason::ForcedTestRefusal => (),
        NonAdmissionReason::GenerationSuperseded => (),
        NonAdmissionReason::PostComputeRevalidationFailed => (),
        NonAdmissionReason::BudgetExceeded => (),
        NonAdmissionReason::Cancelled => (),
        NonAdmissionReason::UnresolvedProvenance => (),
        NonAdmissionReason::ComputeFailed => (),
        NonAdmissionReason::PartialResult => (),
    };
    vec![
        NonAdmissionReason::IntrinsicNonCacheable,
        NonAdmissionReason::SignatureOverflow,
        NonAdmissionReason::EmptySignature,
        NonAdmissionReason::SelfRootConflict,
        NonAdmissionReason::RouteGenerationDependency,
        NonAdmissionReason::ForcedTestRefusal,
        NonAdmissionReason::GenerationSuperseded,
        NonAdmissionReason::PostComputeRevalidationFailed,
        NonAdmissionReason::BudgetExceeded,
        NonAdmissionReason::Cancelled,
        NonAdmissionReason::UnresolvedProvenance,
        NonAdmissionReason::ComputeFailed,
        NonAdmissionReason::PartialResult,
    ]
}

/// Discriminator: the producer/lowering TLS pass-through (`set_return_only_reason`
/// → `take_return_only_reason`) preserves the typed
/// [`NonAdmissionReason`] across the
/// [`crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly(V)`]
/// boundary so cache-runtime lowerings can attribute the correct
/// structured refusal reason instead of hard-coding
/// `NonAdmissionReason::SignatureOverflow`.
///
/// The cache-runtime lowering sites in `component_meta_caches.rs`
/// (imported registry, materialize-structure, ref-cycle) read the
/// reason from this bridge in their `ReturnOnly(value)` arms;
/// producers `set_return_only_reason(reason)` immediately before
/// constructing `ComputeAdmission::ReturnOnly(...)`. This test pins
/// the bridge contract: every reason the cache-runtime producers
/// select round-trips intact through the TLS slot.
///
/// The cases iterate over [`all_non_admission_reasons`], which uses
/// an exhaustive `match` so a new `NonAdmissionReason` variant fails
/// to compile until it is added to the list.
#[test]
fn return_only_reason_bridge_round_trips_every_typed_reason() {
    use crate::cache_runtime::{set_return_only_reason, take_return_only_reason};

    // Slot starts empty on this thread (every test in this crate runs
    // on its own thread).
    assert!(
        take_return_only_reason().is_none(),
        "fixture invariant: the TLS slot starts empty on a fresh thread"
    );

    // Every typed reason the cache-runtime producers may select. The
    // helper's exhaustive match catches a new enum variant at compile
    // time — coverage extends to every NonAdmissionReason variant.
    for expected in all_non_admission_reasons() {
        set_return_only_reason(expected);
        let observed = take_return_only_reason();
        assert_eq!(
            observed,
            Some(expected),
            "the TLS pass-through MUST round-trip every typed refusal \
             reason intact: `set_return_only_reason({expected:?})` → \
             `take_return_only_reason()` should yield `Some({expected:?})`. \
             A regression that lost the reason at the bridge would \
             leave downstream telemetry attributing the wrong cause \
             (the conservative `SignatureOverflow` fallback)."
        );
        // The take must have cleared the slot — a subsequent take
        // yields `None`.
        assert!(
            take_return_only_reason().is_none(),
            "`take_return_only_reason` MUST clear the slot — a leaked \
             reason would attribute the WRONG cause to the NEXT \
             unrelated lowering on this thread."
        );
    }
}

/// Discriminator (behavioral): a producer that arms a typed reason
/// with [`SetReasonGuard::arm`] and returns
/// `ComputeAdmission::ReturnOnly(value)` MUST surface that exact
/// typed reason at the lowering boundary via
/// [`consume_return_only_reason_for_lowering`]. This exercises the
/// SAME pattern the 3 cache-runtime lowering sites in
/// `component_meta_caches.rs` use; a producer that forgets to arm
/// the guard (or a lowering that hard-codes `SignatureOverflow`)
/// would be caught here through the synthesised
/// `CacheAdmission::ReturnOnly { value, reason }`.
///
/// The round-trip primitive test above checks the raw set/take pair
/// in isolation; this test wires a synthetic producer/lowering pair
/// that mimics the production cache-runtime adapter shape (the
/// `compute → CacheAdmission` adapter inside `get_or_compute_admit`)
/// and asserts the typed reason flows end-to-end.
#[test]
fn cache_runtime_lowering_carries_armed_typed_reason() {
    use crate::cache_runtime::{
        consume_return_only_reason_for_lowering, singleflight::ComputeAdmission, CacheAdmission,
        NonAdmissionReason, SetReasonGuard,
    };

    // For every typed reason variant, run a synthetic producer that
    // arms the reason via the RAII guard and returns
    // `ComputeAdmission::ReturnOnly(())`. Lower it through the same
    // pattern the production lowering sites use and assert the
    // typed reason carried into `CacheAdmission::ReturnOnly` matches.
    for expected_reason in all_non_admission_reasons() {
        // Producer: arm the guard, then return `ReturnOnly(value)`.
        // The guard drops at producer scope exit; the lowering's
        // `consume_return_only_reason_for_lowering` reads the armed
        // value before the next iteration.
        let producer = |reason: NonAdmissionReason| -> ComputeAdmission<(), ()> {
            let _reason_guard = SetReasonGuard::arm(reason);
            ComputeAdmission::ReturnOnly(())
        };

        // Lowering: the production pattern used by the 3 cache-runtime
        // adapters in `component_meta_caches.rs`. The `ReturnOnly`
        // arm reads the typed reason from the TLS bridge; the
        // `Cacheable` / `Failed` arms route through their typed
        // counterparts (omitted here — this test focuses on the
        // `ReturnOnly` arm).
        let lower = |admission: ComputeAdmission<(), ()>| -> CacheAdmission<()> {
            match admission {
                ComputeAdmission::ReturnOnly(value) => {
                    let reason = consume_return_only_reason_for_lowering()
                        .unwrap_or(NonAdmissionReason::SignatureOverflow);
                    CacheAdmission::ReturnOnly { value, reason }
                }
                ComputeAdmission::Cacheable(_) => unreachable!("fixture pinned to ReturnOnly"),
                ComputeAdmission::Failed => unreachable!("fixture pinned to ReturnOnly"),
            }
        };

        let lowered = lower(producer(expected_reason));
        match lowered {
            CacheAdmission::ReturnOnly {
                reason: observed, ..
            } => {
                assert_eq!(
                    observed, expected_reason,
                    "behavioral telemetry contract: a producer that arms \
                     `SetReasonGuard::arm({expected_reason:?})` and returns \
                     `ComputeAdmission::ReturnOnly(value)` MUST yield \
                     `CacheAdmission::ReturnOnly {{ reason: {expected_reason:?}, .. }}` \
                     at the lowering boundary. A regression that hard-codes \
                     `SignatureOverflow` (the conservative release fallback) \
                     or drops the reason at the bridge would fail this \
                     assertion."
                );
            }
            CacheAdmission::Cacheable { .. } => panic!(
                "expected `CacheAdmission::ReturnOnly` for arm reason \
                 `{expected_reason:?}`, got `CacheAdmission::Cacheable` — \
                 the synthetic producer pins the `ReturnOnly` arm."
            ),
            CacheAdmission::Failed { .. } => panic!(
                "expected `CacheAdmission::ReturnOnly` for arm reason \
                 `{expected_reason:?}`, got `CacheAdmission::Failed` — \
                 the synthetic producer pins the `ReturnOnly` arm."
            ),
        }
    }
}
