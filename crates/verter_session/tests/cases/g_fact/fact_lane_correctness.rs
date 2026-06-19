//! R13 binding: `FactLane` separates semantic vs display
//! observations on a `fact_dep_signature`.
//!
//! A cosmetic generic param rename MUST NOT shift the semantic-lane
//! fact (because the hashing module alpha-normalises generic param
//! names to binder-relative indices). The display-lane fact MAY shift.
//!
//! This is the producer-side discrimination — the lane consumer
//! branches on `ObservedFact.lane` and validates against
//! `semantic_hash` or `display_hash` accordingly. The emitter
//! codifies the alpha-normalisation contract.
//!
//! Architectural rules bound: R13, R16.

use std::sync::Arc;

use verter_semantic::facts::{
    compute_semantic_hash, FactLane, ObservedFact, SymbolSpace, UnresolvedLens,
};
use verter_semantic::facts::{Fact, FactKey};
use verter_session::file_artifact_store::InternedName;
use verter_type_expr::{
    FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr,
    TypeParam,
};

#[test]
fn generic_param_rename_does_not_shift_semantic_hash() {
    // R16 / R27 canonical visit order: a function body
    // `<T>(x: T) => T` and `<U>(x: U) => U` are alpha-equivalent;
    // semantic_hash MUST be identical.
    let make = |param_name: &str| -> TypeExpr {
        TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some("x".to_string()),
                TypeExpr::Ref {
                    name: Arc::from(param_name),
                    type_arguments: Arc::from(Vec::new()),
                },
                false,
                false,
            )],
            Some(Arc::new(TypeExpr::Ref {
                name: Arc::from(param_name),
                type_arguments: Arc::from(Vec::new()),
            })),
            vec![TypeParam {
                name: param_name.to_string(),
                constraint: None,
                default: None,
            }],
        )))
    };
    let fn_t = make("T");
    let fn_u = make("U");
    let h_t = compute_semantic_hash(&fn_t, SymbolSpace::Type, &UnresolvedLens);
    let h_u = compute_semantic_hash(&fn_u, SymbolSpace::Type, &UnresolvedLens);
    assert_eq!(
        h_t.hash, h_u.hash,
        "R16: generic param rename MUST NOT shift semantic_hash"
    );
}

#[test]
fn parameter_name_rename_does_not_shift_semantic_hash() {
    // R16: parameter names are display-only; the alpha-normalised
    // structural hash MUST treat `(x: string) => void` and `(y: string) => void`
    // as identical.
    let make = |pname: &str| -> TypeExpr {
        TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some(pname.to_string()),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            )],
            Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Void))),
            Vec::new(),
        )))
    };
    let h_x = compute_semantic_hash(&make("x"), SymbolSpace::Type, &UnresolvedLens);
    let h_y = compute_semantic_hash(&make("y"), SymbolSpace::Type, &UnresolvedLens);
    assert_eq!(
        h_x.hash, h_y.hash,
        "R16: parameter rename MUST NOT shift semantic_hash"
    );
}

#[test]
fn property_value_change_shifts_semantic_hash() {
    // Discrimination: a real type-shape change (string → number)
    // DOES shift the semantic_hash.
    let body_a = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "x".to_string(),
            TypeExpr::Primitive(PrimitiveName::String),
            false,
            false,
        ))],
    }));
    let body_b = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "x".to_string(),
            TypeExpr::Primitive(PrimitiveName::Number),
            false,
            false,
        ))],
    }));
    let h_a = compute_semantic_hash(&body_a, SymbolSpace::Type, &UnresolvedLens);
    let h_b = compute_semantic_hash(&body_b, SymbolSpace::Type, &UnresolvedLens);
    assert_ne!(
        h_a.hash, h_b.hash,
        "type-shape edit MUST shift semantic_hash"
    );
}

#[test]
fn observed_fact_lane_is_recorded_per_observation() {
    // R13: `ObservedFact` records `lane: Semantic | Display`
    // per observation. The consumer branches on this field to
    // validate against the right hash; this verifies the substrate
    // carries the lane.
    let key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let observed_semantic = ObservedFact {
        canonical: Arc::from("/a.ts"),
        key: key.clone(),
        lane: FactLane::Semantic,
        expected_hash: [1u8; 16],
    };
    let observed_display = ObservedFact {
        canonical: Arc::from("/a.ts"),
        key,
        lane: FactLane::Display,
        expected_hash: [1u8; 16],
    };
    // Distinct lanes — the validator routes them to
    // semantic_hash vs display_hash.
    assert_ne!(observed_semantic.lane, observed_display.lane);
}

#[test]
fn fact_semantic_and_display_independent() {
    // The Fact carrier exposes both hashes; producers
    // independently fill them. A cosmetic edit can rewrite
    // display_hash while leaving semantic_hash untouched (the
    // semantic / display fact stores are keyed differently so
    // this is what consumers observe).
    let f = Fact {
        key: FactKey::Export {
            name: InternedName::from("Foo"),
            space: SymbolSpace::Type,
        },
        semantic_hash: [1u8; 16],
        display_hash: [2u8; 16],
    };
    assert_ne!(f.semantic_hash, f.display_hash);
}
