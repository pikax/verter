//! Discrimination + serde/hash/accessor witnesses for the three-state
//! [`SourcePosition`] carrier — the source-position sibling of
//! [`super::fact_witnesses`] (split out for the production file-size gate).
//! The compile-time marker witnesses for the carrier live in the parent
//! witness file's `assert_fact_carriers!` list; this file owns the runtime
//! discrimination fixtures: the three states never alias (the aliasing —
//! a failure encoded as the Unknown-leaf success — is the fail-open the
//! carrier eliminates), absence reasons discriminate, serde round-trips are
//! identity, and the accessors expose ONLY the present arm.

use crate::facts::*;
use crate::locators::*;
use crate::PrimitiveName;

fn anchor() -> AuthoredAnchor {
    AuthoredAnchor {
        canonical_id: std::sync::Arc::from("/ws/a.ts"),
        owner: crate::TopLevelOwnerId::ordinary_file(),
        symbol: std::sync::Arc::from("A"),
        space: LocatorSymbolSpace::Type,
    }
}

fn slot() -> TypeBodySlot {
    TypeBodySlot {
        anchor: anchor(),
        path: std::sync::Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
    }
}

#[test]
fn source_position_three_states_construct_and_discriminate() {
    // The three states are DISTINCT typed values: an authored `unknown`
    // success, a schema-absent position, and a source-construction failure
    // must never alias — the aliasing (failure encoded as the Unknown-leaf
    // success) is exactly the fail-open the carrier exists to prevent.
    let present_unknown = SourcePosition::Present(SemanticTypeSource::Closed(
        ClosedTypeFact::Leaf(LeafTypeFact::Primitive(PrimitiveName::Unknown)),
    ));
    let absent = SourcePosition::Absent(SchemaAbsence::Unannotated);
    let failed = SourcePosition::Failed(SemanticSourceFailure::UnrepresentableRequiredPayload);

    let all = [present_unknown.clone(), absent.clone(), failed.clone()];
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b, "the three source-position states must discriminate");
            }
        }
    }
    // CRUX pin: `Failed` is NOT the Present Unknown leaf — a required
    // position's failure must never compare equal to the authored/open
    // `unknown` success value.
    assert_ne!(failed, present_unknown);
    assert_ne!(
        failed, absent,
        "failure-state and schema-absence are distinct"
    );
    // Absence reasons discriminate (each is a PROVEN structural reason,
    // never a generic catch-all).
    assert_ne!(
        SourcePosition::Absent(SchemaAbsence::Unannotated),
        SourcePosition::Absent(SchemaAbsence::BranchDivergent),
    );

    // Hash identity: distinct states are distinct set entries.
    let set: std::collections::HashSet<SourcePosition> = all.iter().cloned().collect();
    assert_eq!(set.len(), 3);

    // Serde round-trip is identity for every arm (the carrier is persisted
    // wherever its inner source is).
    for position in [
        present_unknown,
        absent,
        failed,
        SourcePosition::Absent(SchemaAbsence::BranchDivergent),
        SourcePosition::Present(SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(
            slot(),
        ))),
    ] {
        let json = serde_json::to_string(&position).expect("serialize");
        let back: SourcePosition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            back, position,
            "source-position round-trip must be identity"
        );
    }
}

#[test]
fn source_position_accessors_expose_only_the_present_arm() {
    let source = SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
        PrimitiveName::String,
    )));
    let present = SourcePosition::Present(source.clone());
    let absent = SourcePosition::Absent(SchemaAbsence::Unannotated);
    let failed = SourcePosition::Failed(SemanticSourceFailure::UnrepresentableRequiredPayload);

    assert_eq!(present.present(), Some(&source));
    assert_eq!(present.clone().into_present(), Some(source.clone()));
    assert!(present.is_present());
    assert!(!present.is_failed());

    // An ABSENT and a FAILED position expose NO source: a consumer that
    // reads only `present()` can never mistake either for a value.
    assert_eq!(absent.present(), None);
    assert_eq!(absent.clone().into_present(), None);
    assert!(!absent.is_present());
    assert!(!absent.is_failed());

    assert_eq!(failed.present(), None);
    assert_eq!(failed.clone().into_present(), None);
    assert!(!failed.is_present());
    assert!(failed.is_failed());

    // `present_mut` mutates the present source in place and leaves the
    // other arms untouched.
    let mut mutable = SourcePosition::Present(source.clone());
    if let Some(inner) = mutable.present_mut() {
        *inner = SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
            PrimitiveName::Number,
        )));
    }
    assert_eq!(
        mutable.present(),
        Some(&SemanticTypeSource::Closed(ClosedTypeFact::Leaf(
            LeafTypeFact::Primitive(PrimitiveName::Number)
        ))),
    );
    let mut failed_mut =
        SourcePosition::Failed(SemanticSourceFailure::UnrepresentableRequiredPayload);
    assert!(failed_mut.present_mut().is_none());
}
