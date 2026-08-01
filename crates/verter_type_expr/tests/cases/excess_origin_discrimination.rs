//! Node-identity + wire discrimination tests for `ExcessPropertyOrigin` on
//! the shared type-IR surface (`ObjectProperty` / `MethodSignature`) and for
//! the ordered object-literal spread entry (`ObjectMember::Spread`).
//!
//! `excess_origin` intentionally EXTENDS member identity (mirroring
//! `MemberVisibility`): a `FreshOwn a` and a `NonLiteral a` are genuinely
//! distinct surfaces — excess-property checking reads the origin off the
//! type, so collapsing them would let one interned node carry two different
//! assignability outcomes.
//!
//! The discriminating properties:
//! - two members identical except their excess origin compare UNEQUAL and
//!   hash DISTINCTLY, both standalone (derive Hash) AND embedded in a
//!   `TypeExpr::Object` (the hand-written iterative `Hash for TypeExpr`);
//! - the JSON path round-trips a non-`NonLiteral` value and a spread entry;
//! - `ObjectExpr.properties` preserves direct members and spread entries in
//!   source order (the ordered pre-fold IR);
//! - the wire-compat default is `NonLiteral`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use verter_type_expr::{
    type_expr_from_json, ExcessPropertyOrigin, FunctionExpr, MethodSignature, ObjectExpr,
    ObjectMember, ObjectProperty, PrimitiveName, SpreadMember, TypeExpr,
};

fn hash_one<H: Hash>(value: &H) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn string_ty() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::String)
}

fn object_of(members: Vec<ObjectMember>) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }))
}

fn prop_with(origin: ExcessPropertyOrigin) -> ObjectProperty {
    ObjectProperty::synthetic_public_key("a".into(), string_ty(), false, false)
        .with_excess_origin(origin)
}

#[test]
fn excess_origin_default_is_non_literal() {
    assert_eq!(
        ExcessPropertyOrigin::default(),
        ExcessPropertyOrigin::NonLiteral
    );
    // Every constructor is NonLiteral by construction (annotation / synthetic
    // origins have no literal syntax).
    let p = ObjectProperty::synthetic_public_key("a".into(), string_ty(), false, false);
    assert_eq!(p.excess_origin, ExcessPropertyOrigin::NonLiteral);
    let m = MethodSignature::synthetic_public_key(
        "m".into(),
        FunctionExpr::synthetic(Vec::new(), None, Vec::new()),
        false,
    );
    assert_eq!(m.excess_origin, ExcessPropertyOrigin::NonLiteral);
}

#[test]
fn object_property_excess_origin_participates_in_identity() {
    let non_literal = prop_with(ExcessPropertyOrigin::NonLiteral);
    let fresh = prop_with(ExcessPropertyOrigin::FreshOwn);
    let tainted = prop_with(ExcessPropertyOrigin::SpreadTainted);

    // Identical except origin => UNEQUAL and DISTINCT hashes (derive).
    assert_ne!(non_literal, fresh);
    assert_ne!(non_literal, tainted);
    assert_ne!(fresh, tainted);
    assert_ne!(hash_one(&non_literal), hash_one(&fresh));
    assert_ne!(hash_one(&non_literal), hash_one(&tainted));
    assert_ne!(hash_one(&fresh), hash_one(&tainted));

    // Embedded in a TypeExpr::Object, the hand-written iterative Hash also
    // distinguishes (the marker-only-for-non-NonLiteral fold).
    let obj_nl = object_of(vec![ObjectMember::Property(prop_with(
        ExcessPropertyOrigin::NonLiteral,
    ))]);
    let obj_fresh = object_of(vec![ObjectMember::Property(prop_with(
        ExcessPropertyOrigin::FreshOwn,
    ))]);
    let obj_tainted = object_of(vec![ObjectMember::Property(prop_with(
        ExcessPropertyOrigin::SpreadTainted,
    ))]);
    assert_ne!(obj_nl, obj_fresh);
    assert_ne!(obj_fresh, obj_tainted);
    assert_ne!(hash_one(&obj_nl), hash_one(&obj_fresh));
    assert_ne!(hash_one(&obj_fresh), hash_one(&obj_tainted));

    // Sanity: equal values still agree.
    assert_eq!(
        prop_with(ExcessPropertyOrigin::FreshOwn),
        prop_with(ExcessPropertyOrigin::FreshOwn)
    );
    assert_eq!(hash_one(&obj_fresh), {
        let again = object_of(vec![ObjectMember::Property(prop_with(
            ExcessPropertyOrigin::FreshOwn,
        ))]);
        hash_one(&again)
    });
}

#[test]
fn method_signature_excess_origin_participates_in_identity() {
    let base = || {
        MethodSignature::synthetic_public_key(
            "m".into(),
            FunctionExpr::synthetic(Vec::new(), None, Vec::new()),
            false,
        )
    };
    let non_literal = base();
    let fresh = base().with_excess_origin(ExcessPropertyOrigin::FreshOwn);
    assert_ne!(non_literal, fresh);
    assert_ne!(hash_one(&non_literal), hash_one(&fresh));
}

#[test]
fn ordered_ir_preserves_direct_members_and_spreads_in_source_order() {
    // `{ a, ...S, b }` — the pre-fold IR carries the exact source order.
    let entries = vec![
        ObjectMember::Property(prop_with(ExcessPropertyOrigin::FreshOwn)),
        ObjectMember::Spread(SpreadMember::new(TypeExpr::Ref {
            name: Arc::from("S"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        })),
        ObjectMember::Property(
            ObjectProperty::synthetic_public_key("b".into(), string_ty(), false, false)
                .with_excess_origin(ExcessPropertyOrigin::FreshOwn),
        ),
    ];
    let object = object_of(entries);
    let TypeExpr::Object(ref obj) = object else {
        unreachable!()
    };
    assert_eq!(obj.properties.len(), 3);
    assert!(
        matches!(&obj.properties[0], ObjectMember::Property(p) if p.key.as_known().and_then(|key| key.as_string().map(str::to_owned)).as_deref() == Some("a")),
        "entry 0 must be the direct member `a`"
    );
    assert!(
        matches!(
            &obj.properties[1],
            ObjectMember::Spread(s) if matches!(&s.ty, TypeExpr::Ref { name, .. } if name.as_ref() == "S")
        ),
        "entry 1 must be the spread of `S` BETWEEN the direct members"
    );
    assert!(
        matches!(&obj.properties[2], ObjectMember::Property(p) if p.key.as_known().and_then(|key| key.as_string().map(str::to_owned)).as_deref() == Some("b")),
        "entry 2 must be the direct member `b` AFTER the spread"
    );

    // A spread-bearing object keys distinctly from the spread-less one and
    // from a different spread position.
    let no_spread = object_of(vec![
        ObjectMember::Property(prop_with(ExcessPropertyOrigin::FreshOwn)),
        ObjectMember::Property(
            ObjectProperty::synthetic_public_key("b".into(), string_ty(), false, false)
                .with_excess_origin(ExcessPropertyOrigin::FreshOwn),
        ),
    ]);
    let spread_first = object_of(vec![
        ObjectMember::Spread(SpreadMember::new(TypeExpr::Ref {
            name: Arc::from("S"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        })),
        ObjectMember::Property(prop_with(ExcessPropertyOrigin::FreshOwn)),
        ObjectMember::Property(
            ObjectProperty::synthetic_public_key("b".into(), string_ty(), false, false)
                .with_excess_origin(ExcessPropertyOrigin::FreshOwn),
        ),
    ]);
    let with_spread = object_of(vec![
        ObjectMember::Property(prop_with(ExcessPropertyOrigin::FreshOwn)),
        ObjectMember::Spread(SpreadMember::new(TypeExpr::Ref {
            name: Arc::from("S"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        })),
        ObjectMember::Property(
            ObjectProperty::synthetic_public_key("b".into(), string_ty(), false, false)
                .with_excess_origin(ExcessPropertyOrigin::FreshOwn),
        ),
    ]);
    assert_ne!(with_spread, no_spread);
    assert_ne!(with_spread, spread_first, "spread POSITION is identity");
    assert_ne!(hash_one(&with_spread), hash_one(&no_spread));
    assert_ne!(hash_one(&with_spread), hash_one(&spread_first));
}

#[test]
fn json_round_trips_excess_origin_and_spread_entries() {
    let original = object_of(vec![
        ObjectMember::Property(prop_with(ExcessPropertyOrigin::FreshOwn)),
        ObjectMember::Spread(SpreadMember::new(string_ty())),
        ObjectMember::Property(
            ObjectProperty::synthetic_public_key("b".into(), string_ty(), false, false)
                .with_excess_origin(ExcessPropertyOrigin::SpreadTainted),
        ),
        // A NonLiteral member serializes WITHOUT the field (wire-stable).
        ObjectMember::Property(ObjectProperty::synthetic_public_key(
            "c".into(),
            string_ty(),
            false,
            false,
        )),
    ]);
    let json = serde_json::to_value(&original).expect("serialize");
    // Wire stability: the NonLiteral member's JSON has no `excessOrigin` key.
    let members = json
        .get("properties")
        .and_then(|p| p.as_array())
        .expect("properties array");
    assert_eq!(
        members[0].get("excessOrigin").and_then(|v| v.as_str()),
        Some("freshOwn")
    );
    assert_eq!(
        members[1].get("memberKind").and_then(|v| v.as_str()),
        Some("spread")
    );
    assert_eq!(
        members[2].get("excessOrigin").and_then(|v| v.as_str()),
        Some("spreadTainted")
    );
    assert!(
        members[3].get("excessOrigin").is_none(),
        "a NonLiteral member must serialize WITHOUT the excessOrigin key"
    );

    let round = type_expr_from_json(&json).expect("deserialize");
    assert_eq!(
        round, original,
        "origin + spread entries must survive the JSON round trip losslessly"
    );
}
