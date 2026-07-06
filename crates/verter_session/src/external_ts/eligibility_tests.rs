//! Fail-open discrimination tests for eligibility composition.
//!
//! Every test pins the fail-CLOSED contract: incomplete positive evidence must
//! yield OWNED with the precise matching reason, and only an ALL-positive fact
//! set yields `Eligible`. A composition that treated a missing fact as "assume
//! satisfied" (the fail-OPEN bug) would let an under-evidenced project go
//! SHARED — these tests would catch it.

use std::sync::Arc;

use super::*;
use crate::external_ts::mode::{EligibilityFailure, ProjectEligibility, SharedSessionFacts};
use crate::external_ts::mode::{EngineSessionFacts, OwnedReason};
use crate::file_artifact_store::ProjectIdentity;

fn pid(b: u8) -> ProjectIdentity {
    ProjectIdentity([b; 16])
}

fn shared_facts() -> SharedSessionFacts {
    SharedSessionFacts::new(EngineSessionFacts {
        observed_version: Arc::<str>::from("7.0.1"),
        wire_pin: 7,
        editor_session_generation: 3,
    })
}

/// An ALL-positive fact set for project `bound_to`, with every precondition
/// satisfied. Individual tests knock ONE fact negative to prove that fact's
/// precise fail-closed reason.
fn all_positive(bound_to: ProjectIdentity) -> EligibilityFacts {
    EligibilityFacts {
        version_gate: VersionGateFact::Cleared {
            observed_version: Arc::<str>::from("7.0.1"),
        },
        attach: AttachFact::Live(shared_facts()),
        binding: BindingFact::Bound(bound_to),
        proxy: ProxyFact::Available,
        editor_binding: EditorBindingFact::Matched(bound_to),
    }
}

/// ALL-positive evidence composes to `Eligible` — the ONLY input that does.
#[test]
fn all_positive_facts_compose_to_eligible() {
    let facts = all_positive(pid(1));
    assert_eq!(compose_eligibility(&facts), ProjectEligibility::Eligible);
}

/// Each of the five facts, taken negative IN TURN over an otherwise all-positive
/// set, yields OWNED with that fact's precise `EligibilityFailure` — never
/// `Eligible`, never a different reason. This is the fail-open guard: a missing
/// fact is never assumed satisfied.
#[test]
fn each_missing_fact_fails_closed_to_its_precise_reason() {
    let p = pid(1);

    let mut f = all_positive(p);
    f.version_gate = VersionGateFact::NotGreen;
    assert_eq!(
        compose_eligibility(&f),
        ProjectEligibility::Owned(EligibilityFailure::VersionGateNotGreen)
    );

    let mut f = all_positive(p);
    f.attach = AttachFact::NotLive;
    assert_eq!(
        compose_eligibility(&f),
        ProjectEligibility::Owned(EligibilityFailure::AttachNotLive)
    );

    let mut f = all_positive(p);
    f.binding = BindingFact::NotBound;
    assert_eq!(
        compose_eligibility(&f),
        ProjectEligibility::Owned(EligibilityFailure::ProjectNotBound)
    );

    let mut f = all_positive(p);
    f.proxy = ProxyFact::Unavailable;
    assert_eq!(
        compose_eligibility(&f),
        ProjectEligibility::Owned(EligibilityFailure::ProxyUnavailable)
    );

    let mut f = all_positive(p);
    f.editor_binding = EditorBindingFact::Mismatch;
    assert_eq!(
        compose_eligibility(&f),
        ProjectEligibility::Owned(EligibilityFailure::EditorBindingMismatch)
    );
}

/// Fail-closed precedence is the exact order the `EligibilityFailure` enum
/// encodes: with EVERY fact negative, the FIRST (version gate) wins; peeling the
/// earlier negatives away reveals the next in order. This pins that the
/// composition is a precedence fold, not a last-write or arbitrary pick.
#[test]
fn precedence_follows_the_failure_enum_order() {
    let p = pid(1);
    // Everything negative → the first precondition (version gate) is reported.
    let all_negative = EligibilityFacts {
        version_gate: VersionGateFact::NotGreen,
        attach: AttachFact::NotLive,
        binding: BindingFact::NotBound,
        proxy: ProxyFact::Unavailable,
        editor_binding: EditorBindingFact::Mismatch,
    };
    assert_eq!(
        compose_eligibility(&all_negative),
        ProjectEligibility::Owned(EligibilityFailure::VersionGateNotGreen)
    );

    // Clear the gate → attach is next.
    let mut f = all_negative.clone();
    f.version_gate = VersionGateFact::Cleared {
        observed_version: Arc::<str>::from("7.0.1"),
    };
    assert_eq!(
        compose_eligibility(&f),
        ProjectEligibility::Owned(EligibilityFailure::AttachNotLive)
    );

    // Clear the gate + attach → binding is next.
    f.attach = AttachFact::Live(shared_facts());
    assert_eq!(
        compose_eligibility(&f),
        ProjectEligibility::Owned(EligibilityFailure::ProjectNotBound)
    );

    // + binding → proxy is next.
    f.binding = BindingFact::Bound(p);
    assert_eq!(
        compose_eligibility(&f),
        ProjectEligibility::Owned(EligibilityFailure::ProxyUnavailable)
    );

    // + proxy → editor binding is last.
    f.proxy = ProxyFact::Available;
    assert_eq!(
        compose_eligibility(&f),
        ProjectEligibility::Owned(EligibilityFailure::EditorBindingMismatch)
    );
}

/// The OWNED output can only carry an `EligibilityFailure` (the restricted
/// eligibility-INPUT set) — a derived decision reason is unrepresentable as a
/// composition result. This asserts the composition never emits any reason
/// OUTSIDE the five inputs (the smuggle-prevention `EligibilityFailure ⊂
/// OwnedReason` gives us at the type level), by round-tripping every OWNED
/// output through `OwnedReason::from` and checking it lands in the input set.
#[test]
fn owned_output_is_always_an_eligibility_input_reason_never_derived() {
    let input_reasons = [
        OwnedReason::VersionGateNotGreen,
        OwnedReason::AttachNotLive,
        OwnedReason::ProjectNotBound,
        OwnedReason::ProxyUnavailable,
        OwnedReason::EditorBindingMismatch,
    ];
    let derived_reasons = [
        OwnedReason::RedirectClosed,
        OwnedReason::ComponentMemberOwned,
        OwnedReason::IncompleteComponent,
        OwnedReason::UnresolvedRedirectInSnapshot,
        OwnedReason::SharedSessionUnavailable,
    ];

    let p = pid(1);
    for negate in 0..5u8 {
        let mut f = all_positive(p);
        match negate {
            0 => f.version_gate = VersionGateFact::NotGreen,
            1 => f.attach = AttachFact::NotLive,
            2 => f.binding = BindingFact::NotBound,
            3 => f.proxy = ProxyFact::Unavailable,
            _ => f.editor_binding = EditorBindingFact::Mismatch,
        }
        let ProjectEligibility::Owned(failure) = compose_eligibility(&f) else {
            panic!("a negated fact must fail closed to OWNED");
        };
        let reason = OwnedReason::from(failure);
        assert!(
            input_reasons.contains(&reason),
            "the composed OWNED reason must be one of the five eligibility inputs"
        );
        assert!(
            !derived_reasons.contains(&reason),
            "a derived/decision reason can never be a composition output"
        );
    }
}

/// `BindingFact::from_resolution` is `Bound` ONLY for a real `ProjectBinding` —
/// `NoProject`, `Ambiguous`, and `SyntheticScratch` are all `NotBound`. The
/// no-binding states must not masquerade as "bound" and let a project go SHARED.
#[test]
fn binding_fact_is_bound_only_for_a_real_project_binding() {
    use crate::external_ts::resolver::{AmbiguityCause, ProjectResolution};

    assert_eq!(
        BindingFact::from_resolution(&ProjectResolution::NoProject),
        BindingFact::NotBound
    );
    assert_eq!(
        BindingFact::from_resolution(&ProjectResolution::Ambiguous(
            AmbiguityCause::MultipleOwners
        )),
        BindingFact::NotBound
    );
    assert_eq!(
        BindingFact::from_resolution(&ProjectResolution::synthetic_scratch("untitled:1")),
        BindingFact::NotBound
    );
}

/// `EditorBindingFact::evaluate` routes through the shared `editor_binding_matches`
/// identity primitive: equal identities → `Matched` (carrying the agreed
/// identity), a mismatch → `Mismatch`. Composing a `Matched(p)` binding then
/// participates as a satisfied precondition.
#[test]
fn editor_binding_fact_uses_the_shared_identity_witness() {
    assert_eq!(
        EditorBindingFact::evaluate(&pid(1), &pid(1)),
        EditorBindingFact::Matched(pid(1))
    );
    assert_eq!(
        EditorBindingFact::evaluate(&pid(1), &pid(2)),
        EditorBindingFact::Mismatch
    );
}
