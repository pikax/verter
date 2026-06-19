//! Roundtrip contract for member [`MemberVisibility`] through the MANUAL
//! `TypeExpr` JSON path (`TypeExpr::to_json_value` / `type_expr_from_json`).
//!
//! `TypeExpr` cannot use a serde derive (recursive `Arc` tree), so its JSON is
//! hand-rolled. The struct-level `#[serde(default)]` on `ObjectProperty.visibility`
//! / `MethodSignature.visibility` does NOT cover this hand-written path — the
//! manual serializer must emit `visibility` (only when non-public) and the
//! manual deserializer must read it back (missing → `Public`).
//!
//! Discrimination: against the tree where the manual serializer OMITS
//! visibility and the manual deserializer rebuilds via `synthetic` (forcing
//! `Public`), every `assert_eq!` on a `Protected` / `Private` member below
//! FAILS (the decoded member is `Public`). With the fix they PASS.

use std::sync::Arc;

use verter_type_expr::{
    type_expr_from_json, FunctionExpr, MemberSpans, MemberVisibility, MethodSignature, ObjectExpr,
    ObjectMember, ObjectProperty, PrimitiveName, TypeExpr,
};

fn object(members: Vec<ObjectMember>) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }))
}

fn roundtrip(expr: &TypeExpr) -> TypeExpr {
    let json = expr.to_json_value();
    type_expr_from_json(&json).expect("manual JSON roundtrip must decode")
}

fn first_property(expr: &TypeExpr) -> &ObjectProperty {
    let TypeExpr::Object(obj) = expr else {
        panic!("expected object, got {expr:?}");
    };
    match &obj.properties[0] {
        ObjectMember::Property(p) => p,
        other => panic!("expected property, got {other:?}"),
    }
}

fn first_method(expr: &TypeExpr) -> &MethodSignature {
    let TypeExpr::Object(obj) = expr else {
        panic!("expected object, got {expr:?}");
    };
    match &obj.properties[0] {
        ObjectMember::Method(m) => m,
        other => panic!("expected method, got {other:?}"),
    }
}

#[test]
fn manual_json_roundtrip_preserves_property_visibility() {
    for vis in [
        MemberVisibility::Public,
        MemberVisibility::Protected,
        MemberVisibility::Private,
    ] {
        let expr = object(vec![ObjectMember::Property(
            ObjectProperty::with_visibility(
                "x".into(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
                vis,
                MemberSpans::default(),
            ),
        )]);
        let decoded = roundtrip(&expr);
        assert_eq!(
            first_property(&decoded).visibility,
            vis,
            "manual JSON roundtrip must preserve property visibility {vis:?}",
        );
    }
}

#[test]
fn manual_json_roundtrip_preserves_method_visibility() {
    for vis in [
        MemberVisibility::Public,
        MemberVisibility::Protected,
        MemberVisibility::Private,
    ] {
        let expr = object(vec![ObjectMember::Method(
            MethodSignature::with_visibility(
                "m".into(),
                FunctionExpr::synthetic(Vec::new(), None, Vec::new()),
                false,
                vis,
                MemberSpans::default(),
            ),
        )]);
        let decoded = roundtrip(&expr);
        assert_eq!(
            first_method(&decoded).visibility,
            vis,
            "manual JSON roundtrip must preserve method visibility {vis:?}",
        );
    }
}

/// A public member's JSON must contain NO `visibility` key (so the wire shape is
/// unchanged from before visibility existed); a non-public member's JSON MUST
/// contain it. This pins the marker-only-for-non-public wire scheme.
#[test]
fn public_member_json_omits_visibility_key_non_public_includes_it() {
    let public = object(vec![ObjectMember::Property(
        ObjectProperty::synthetic_public(
            "x".into(),
            TypeExpr::Primitive(PrimitiveName::Number),
            false,
            false,
        ),
    )]);
    let public_json = public.to_json_value();
    let public_member = &public_json["properties"][0];
    assert!(
        public_member.get("visibility").is_none(),
        "a public member's JSON must omit the visibility key, got {public_member}",
    );

    let private = object(vec![ObjectMember::Property(
        ObjectProperty::with_visibility(
            "x".into(),
            TypeExpr::Primitive(PrimitiveName::Number),
            false,
            false,
            MemberVisibility::Private,
            MemberSpans::default(),
        ),
    )]);
    let private_json = private.to_json_value();
    let private_member = &private_json["properties"][0];
    assert_eq!(
        private_member.get("visibility").and_then(|v| v.as_str()),
        Some("private"),
        "a private member's JSON must include visibility=private, got {private_member}",
    );
}

/// JSON missing the `visibility` field (the pre-existing wire shape, or any
/// external producer) deserializes as `Public`.
#[test]
fn manual_json_missing_visibility_parses_as_public() {
    let json = serde_json::json!({
        "kind": "object",
        "properties": [{
            "memberKind": "property",
            "name": "x",
            "ty": { "kind": "primitive", "name": "string" },
            "optional": false,
            "readonly": false,
        }],
    });
    let decoded = type_expr_from_json(&json).expect("decode");
    assert_eq!(
        first_property(&decoded).visibility,
        MemberVisibility::Public,
        "a member with no visibility key must parse as Public",
    );
}
