//! [P2] discrimination fixtures for the projected REPLAY-ROUTE facts —
//! the content-free replay addresses ([`ProjectedTypeFact::MemberPath`],
//! [`ProjectedTypeFact::CallableParams`], [`ProjectedTypeFact::IndexPosition`])
//! a publication surface stamps for members/payloads/positions the closed
//! vocabulary cannot faithfully express. Each fixture proves every identity
//! axis independently discriminates (base anchor, base macro ordinal, the
//! route-specific ordinals/roles), the arms discriminate from their sibling
//! projected arms over the SAME base, hash identity follows equality, and the
//! serde round-trip is identity. Split from `fact_witnesses` along the
//! replay-route boundary.

use crate::facts::*;
use crate::locators::*;

fn empty_projected_surface() -> ProjectedSurfaceFact {
    ProjectedSurfaceFact {
        members: std::sync::Arc::from(Vec::<ProjectedMemberFact>::new().into_boxed_slice()),
        call_signatures: std::sync::Arc::from(
            Vec::<FunctionSignatureFact>::new().into_boxed_slice(),
        ),
        construct_signatures: std::sync::Arc::from(
            Vec::<FunctionSignatureFact>::new().into_boxed_slice(),
        ),
        index_signatures: std::sync::Arc::from(
            Vec::<ProjectedIndexSignatureFact>::new().into_boxed_slice(),
        ),
        has_index_signature: false,
    }
}

#[test]
fn projected_member_path_fact_discriminates_base_and_path() {
    let base = |canonical: &str, macro_index: u32| {
        AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
            anchor: AuthoredAnchor {
                canonical_id: std::sync::Arc::from(canonical),
                symbol: std::sync::Arc::from("default"),
                space: LocatorSymbolSpace::Value,
            },
            macro_index,
            payload: MacroPayloadPosition::TypeArgument,
        })
    };
    let mk = |canonical: &str, macro_index: u32, path: &[&str]| ProjectedTypeFact::MemberPath {
        base: base(canonical, macro_index),
        path: std::sync::Arc::from(
            path.iter()
                .map(|segment| segment.to_string())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    };
    let save = mk("/App.vue", 0, &["save"]);
    // Identity: an identical rebuild is equal.
    assert_eq!(save, mk("/App.vue", 0, &["save"]));
    // Each axis independently discriminates: the base anchor, the base macro
    // ordinal, the path member, and the path LENGTH (a deeper hop chain is a
    // different projection).
    assert_ne!(save, mk("/Other.vue", 0, &["save"]), "base anchor");
    assert_ne!(save, mk("/App.vue", 1, &["save"]), "base macro ordinal");
    assert_ne!(save, mk("/App.vue", 0, &["cancel"]), "path member");
    assert_ne!(save, mk("/App.vue", 0, &["save", "0"]), "path depth");
    // The arm discriminates from every sibling projected arm (a member-path
    // route is never a whole surface / member fact).
    assert_ne!(
        save,
        ProjectedTypeFact::Surface(empty_projected_surface()),
        "member-path vs surface"
    );
    // Hash identity follows equality: distinct values are distinct entries.
    let set: std::collections::HashSet<ProjectedTypeFact> = [
        save.clone(),
        mk("/Other.vue", 0, &["save"]),
        mk("/App.vue", 1, &["save"]),
        mk("/App.vue", 0, &["cancel"]),
        mk("/App.vue", 0, &["save", "0"]),
    ]
    .into_iter()
    .collect();
    assert_eq!(set.len(), 5);
    // Serde round-trip is identity (the fact substrate persists sources).
    let source = SemanticTypeSource::Projected(save.clone());
    let json = serde_json::to_string(&source).expect("serialize");
    let back: SemanticTypeSource = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, source, "member-path round-trip must be identity");
    assert_eq!(back, SemanticTypeSource::Projected(save));
}

#[test]
fn projected_callable_params_fact_discriminates_base_ordinal_and_first_param() {
    let base = |canonical: &str, macro_index: u32| {
        AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
            anchor: AuthoredAnchor {
                canonical_id: std::sync::Arc::from(canonical),
                symbol: std::sync::Arc::from("default"),
                space: LocatorSymbolSpace::Value,
            },
            macro_index,
            payload: MacroPayloadPosition::TypeArgument,
        })
    };
    let mk = |canonical: &str, macro_index: u32, signature_ordinal: u32, first_param: u32| {
        ProjectedTypeFact::CallableParams {
            base: base(canonical, macro_index),
            signature_ordinal,
            first_param,
        }
    };
    let save = mk("/App.vue", 0, 0, 1);
    // Identity: an identical rebuild is equal.
    assert_eq!(save, mk("/App.vue", 0, 0, 1));
    // Each axis independently discriminates: the base anchor, the base macro
    // ordinal, the SIGNATURE ordinal (a different call signature is a
    // different payload), and the first payload parameter index.
    assert_ne!(save, mk("/Other.vue", 0, 0, 1), "base anchor");
    assert_ne!(save, mk("/App.vue", 1, 0, 1), "base macro ordinal");
    assert_ne!(save, mk("/App.vue", 0, 1, 1), "signature ordinal");
    assert_ne!(save, mk("/App.vue", 0, 0, 0), "first payload param");
    // The arm discriminates from its sibling projected arms (a callable-params
    // route is never a member-path route or a whole surface, even over the
    // SAME base).
    assert_ne!(
        save,
        ProjectedTypeFact::MemberPath {
            base: base("/App.vue", 0),
            path: std::sync::Arc::from(Vec::<String>::new().into_boxed_slice()),
        },
        "callable-params vs member-path over the same base"
    );
    assert_ne!(
        save,
        ProjectedTypeFact::Surface(empty_projected_surface()),
        "callable-params vs surface"
    );
    // Hash identity follows equality: distinct values are distinct entries.
    let set: std::collections::HashSet<ProjectedTypeFact> = [
        save.clone(),
        mk("/Other.vue", 0, 0, 1),
        mk("/App.vue", 1, 0, 1),
        mk("/App.vue", 0, 1, 1),
        mk("/App.vue", 0, 0, 0),
    ]
    .into_iter()
    .collect();
    assert_eq!(set.len(), 5);
    // Serde round-trip is identity (the fact substrate persists sources).
    let source = SemanticTypeSource::Projected(save.clone());
    let json = serde_json::to_string(&source).expect("serialize");
    let back: SemanticTypeSource = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, source, "callable-params round-trip must be identity");
    assert_eq!(back, SemanticTypeSource::Projected(save));
}

#[test]
fn projected_index_position_fact_discriminates_base_ordinal_and_position() {
    let base = |canonical: &str, macro_index: u32| {
        AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
            anchor: AuthoredAnchor {
                canonical_id: std::sync::Arc::from(canonical),
                symbol: std::sync::Arc::from("default"),
                space: LocatorSymbolSpace::Value,
            },
            macro_index,
            payload: MacroPayloadPosition::TypeArgument,
        })
    };
    let mk = |canonical: &str,
              macro_index: u32,
              signature_ordinal: u32,
              position: IndexSignaturePosition| {
        ProjectedTypeFact::IndexPosition {
            base: base(canonical, macro_index),
            signature_ordinal,
            position,
        }
    };
    let value = mk("/App.vue", 0, 0, IndexSignaturePosition::Value);
    // Identity: an identical rebuild is equal.
    assert_eq!(value, mk("/App.vue", 0, 0, IndexSignaturePosition::Value));
    // Each axis independently discriminates: the base anchor, the base macro
    // ordinal, the SIGNATURE ordinal (a different index signature is a
    // different position), and the key-vs-value position.
    assert_ne!(
        value,
        mk("/Other.vue", 0, 0, IndexSignaturePosition::Value),
        "base anchor"
    );
    assert_ne!(
        value,
        mk("/App.vue", 1, 0, IndexSignaturePosition::Value),
        "base macro ordinal"
    );
    assert_ne!(
        value,
        mk("/App.vue", 0, 1, IndexSignaturePosition::Value),
        "signature ordinal"
    );
    assert_ne!(
        value,
        mk("/App.vue", 0, 0, IndexSignaturePosition::Key),
        "key vs value position"
    );
    // The arm discriminates from its sibling projected arms over the SAME
    // base (an index-position route is never a callable-params route or a
    // member-path route).
    assert_ne!(
        value,
        ProjectedTypeFact::CallableParams {
            base: base("/App.vue", 0),
            signature_ordinal: 0,
            first_param: 1,
        },
        "index-position vs callable-params over the same base"
    );
    assert_ne!(
        value,
        ProjectedTypeFact::MemberPath {
            base: base("/App.vue", 0),
            path: std::sync::Arc::from(Vec::<String>::new().into_boxed_slice()),
        },
        "index-position vs member-path over the same base"
    );
    // Hash identity follows equality: distinct values are distinct entries.
    let set: std::collections::HashSet<ProjectedTypeFact> = [
        value.clone(),
        mk("/Other.vue", 0, 0, IndexSignaturePosition::Value),
        mk("/App.vue", 1, 0, IndexSignaturePosition::Value),
        mk("/App.vue", 0, 1, IndexSignaturePosition::Value),
        mk("/App.vue", 0, 0, IndexSignaturePosition::Key),
    ]
    .into_iter()
    .collect();
    assert_eq!(set.len(), 5);
    // Serde round-trip is identity (the fact substrate persists sources).
    let source = SemanticTypeSource::Projected(value.clone());
    let json = serde_json::to_string(&source).expect("serialize");
    let back: SemanticTypeSource = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, source, "index-position round-trip must be identity");
    assert_eq!(back, SemanticTypeSource::Projected(value));
}
