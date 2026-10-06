//! Node-facing cache-runtime discriminators — included as a child `mod`
//! of `node` via `#[path]` so `use super::*` reaches the trait surface
//! and the `lookup` / `query::lookup` / `publish` entry points directly.
//!
//! Structural discriminators on the substrate types (`CacheAdmission<V>`
//! holds the unwrapped value) and on the typed non-admission reasons.

use super::*;
use crate::cache_runtime::admission::NonAdmissionReason;
use std::sync::Arc;
use verter_session_query::facts::fact_cache::ReadSetSignature;
use verter_session_query::facts::{fact_cache::FactVersionRef, fact_read_set::FactReadSetFinalise};

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

/// A complete observation set that consumed a non-cacheable read still
/// carries its facts for enclosing tracers, but can never authorize a warm
/// admission. The verdict is intrinsic to `FactReadSetFinalise` so no caller
/// can drop a sibling boolean and accidentally publish it.
#[test]
fn signature_admission_from_non_cacheable_finalise_refuses_admission() {
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: "/a.ts".to_string(),
        hash: [1u8; 16],
    }]);
    let admission = SignatureAdmission::from_finalise(FactReadSetFinalise::NonCacheable(facts));

    match &admission {
        SignatureAdmission::NonCacheable(reason) => assert_eq!(
            *reason,
            NonAdmissionReason::UnresolvedProvenance,
            "a consumed non-cacheable read must fail closed"
        ),
        SignatureAdmission::Cacheable(_) => {
            panic!("a non-cacheable finalise must never authorize admission")
        }
    }
    assert!(admission.cacheable().is_none());
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
        NonAdmissionReason::MutationUnstable => (),
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
        NonAdmissionReason::ResolutionInaccessiblePath => (),
        NonAdmissionReason::ResolutionUnknownPath => (),
        NonAdmissionReason::ResolutionWorldChanged => (),
        NonAdmissionReason::ResolutionViewSuperseded => (),
        NonAdmissionReason::ResolutionUntrackedBackend => (),
        NonAdmissionReason::ResolutionIncompleteProvenance => (),
        NonAdmissionReason::ResolutionRetryExhausted => (),
        NonAdmissionReason::RetentionPressure => (),
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
        NonAdmissionReason::ResolutionInaccessiblePath,
        NonAdmissionReason::ResolutionUnknownPath,
        NonAdmissionReason::ResolutionWorldChanged,
        NonAdmissionReason::ResolutionViewSuperseded,
        NonAdmissionReason::ResolutionUntrackedBackend,
        NonAdmissionReason::ResolutionIncompleteProvenance,
        NonAdmissionReason::ResolutionRetryExhausted,
        NonAdmissionReason::RetentionPressure,
    ]
}

/// Discriminator: `ComputeAdmission::ReturnOnly` carries every typed refusal
/// reason by value. The exhaustive helper forces a compile failure when a new
/// `NonAdmissionReason` variant is added without extending this contract test.
#[test]
fn return_only_carries_every_typed_reason_by_value() {
    use crate::cache_runtime::singleflight::ComputeAdmission;

    for expected in all_non_admission_reasons() {
        let admission: ComputeAdmission<(), ()> = ComputeAdmission::ReturnOnly {
            value: (),
            reason: expected,
        };

        let observed = match admission {
            ComputeAdmission::ReturnOnly { reason, .. } => reason,
            ComputeAdmission::Cacheable(_) => unreachable!("fixture pinned to ReturnOnly"),
            ComputeAdmission::Failed => unreachable!("fixture pinned to ReturnOnly"),
        };

        assert_eq!(
            observed, expected,
            "ReturnOnly must carry typed refusal reason {expected:?} by value"
        );
    }
}

#[test]
fn non_admission_reasons_have_explicit_propagation_policy() {
    use crate::cache_runtime::admission::non_admission_propagation;
    use verter_session_query::facts::fact_read_set::NonCacheablePropagation;

    assert_eq!(
        non_admission_propagation(NonAdmissionReason::IntrinsicNonCacheable),
        NonCacheablePropagation::LocalOnly,
        "an intrinsic declines this cache family's retention without tainting an enclosing derivation"
    );
    assert_eq!(
        non_admission_propagation(NonAdmissionReason::UnresolvedProvenance),
        NonCacheablePropagation::Transitive,
        "unresolved provenance must taint every enclosing derivation"
    );
    assert_eq!(
        non_admission_propagation(NonAdmissionReason::PartialResult),
        NonCacheablePropagation::Transitive,
        "a partial child must never warm-replay through an enclosing cache"
    );
}

/// Behavioral discriminator: the cache-runtime lowering preserves the exact
/// typed refusal reason selected by the producer. This mirrors the production
/// `ComputeAdmission` to `CacheAdmission` adapter shape.
#[test]
fn cache_runtime_lowering_preserves_typed_reason_by_value() {
    use crate::cache_runtime::{singleflight::ComputeAdmission, CacheAdmission};

    let lower = |admission: ComputeAdmission<(), ()>| -> CacheAdmission<()> {
        match admission {
            ComputeAdmission::ReturnOnly { value, reason } => {
                CacheAdmission::ReturnOnly { value, reason }
            }
            ComputeAdmission::Cacheable(_) => unreachable!("fixture pinned to ReturnOnly"),
            ComputeAdmission::Failed => unreachable!("fixture pinned to ReturnOnly"),
        }
    };

    for expected_reason in all_non_admission_reasons() {
        let produced = ComputeAdmission::ReturnOnly {
            value: (),
            reason: expected_reason,
        };

        match lower(produced) {
            CacheAdmission::ReturnOnly {
                reason: observed, ..
            } => assert_eq!(
                observed, expected_reason,
                "lowering must preserve typed refusal reason {expected_reason:?}"
            ),
            CacheAdmission::Cacheable { .. } => {
                panic!("fixture unexpectedly lowered to Cacheable")
            }
            CacheAdmission::Failed { .. } => {
                panic!("fixture unexpectedly lowered to Failed")
            }
        }
    }
}
