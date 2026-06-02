//! Node-identity discrimination tests for `MemberVisibility` on the shared
//! type-IR surface (`ObjectProperty` / `MethodSignature`).
//!
//! `MemberVisibility` intentionally EXTENDS member identity: a `private foo`
//! and a `public foo` are genuinely distinct surfaces (mirroring how `spans`
//! already participates). Each test must FAIL against the pre-change tree
//! (where the field did not exist / was not in Eq+Hash) and PASS afterwards.
//!
//! The discriminating properties:
//! - two members identical except their visibility compare UNEQUAL and hash
//!   DISTINCTLY, both as standalone structs (derive Hash) AND when embedded in
//!   a `TypeExpr::Object` (the hand-written iterative `Hash for TypeExpr`),
//! - the derive serde path round-trips a non-public value (proving
//!   `#[serde(default)]`, not `#[serde(skip)]`),
//! - the default is `Public`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use verter_type_expr::{
    FunctionExpr, MemberVisibility, MethodSignature, ObjectExpr, ObjectMember, ObjectProperty,
    PrimitiveName, TypeExpr,
};

fn hash_one<H: Hash>(value: &H) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn string_ty() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::String)
}

fn empty_fn() -> FunctionExpr {
    FunctionExpr::synthetic(Vec::new(), None, Vec::new())
}

fn object_of(member: ObjectMember) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![member],
    }))
}

#[test]
fn member_visibility_default_is_public() {
    assert_eq!(MemberVisibility::default(), MemberVisibility::Public);
    // `synthetic` / `with_spans` constructors default to Public.
    let p = ObjectProperty::synthetic("a".into(), string_ty(), false, false);
    assert_eq!(p.visibility, MemberVisibility::Public);
    let m = MethodSignature::synthetic("m".into(), empty_fn(), false);
    assert_eq!(m.visibility, MemberVisibility::Public);
}

#[test]
fn object_property_visibility_participates_in_identity() {
    let public = ObjectProperty::synthetic("a".into(), string_ty(), false, false);
    let protected = ObjectProperty::with_visibility(
        "a".into(),
        string_ty(),
        false,
        false,
        MemberVisibility::Protected,
        Default::default(),
    );
    let private = ObjectProperty::with_visibility(
        "a".into(),
        string_ty(),
        false,
        false,
        MemberVisibility::Private,
        Default::default(),
    );

    // Identical except visibility => UNEQUAL and DISTINCT hashes (derive).
    assert_ne!(public, protected);
    assert_ne!(public, private);
    assert_ne!(protected, private);
    assert_ne!(hash_one(&public), hash_one(&protected));
    assert_ne!(hash_one(&public), hash_one(&private));
    assert_ne!(hash_one(&protected), hash_one(&private));

    // Same visibility => equal + equal hash.
    let public2 = ObjectProperty::synthetic("a".into(), string_ty(), false, false);
    assert_eq!(public, public2);
    assert_eq!(hash_one(&public), hash_one(&public2));
}

#[test]
fn method_signature_visibility_participates_in_identity() {
    let public = MethodSignature::synthetic("m".into(), empty_fn(), false);
    let private = MethodSignature::with_visibility(
        "m".into(),
        empty_fn(),
        false,
        MemberVisibility::Private,
        Default::default(),
    );

    assert_ne!(public, private);
    assert_ne!(hash_one(&public), hash_one(&private));
}

/// The EMBEDDED path: two `TypeExpr::Object` values differing only in a
/// member's visibility must be distinct under both `Eq` and the hand-written
/// iterative `Hash for TypeExpr`. This discriminates a manual hash that forgot
/// to fold visibility into the `TypeExpr` byte stream.
#[test]
fn object_member_visibility_distinguishes_embedded_type_expr() {
    let public_prop = object_of(ObjectMember::Property(ObjectProperty::synthetic(
        "a".into(),
        string_ty(),
        false,
        false,
    )));
    let private_prop = object_of(ObjectMember::Property(ObjectProperty::with_visibility(
        "a".into(),
        string_ty(),
        false,
        false,
        MemberVisibility::Private,
        Default::default(),
    )));
    assert_ne!(public_prop, private_prop);
    assert_ne!(hash_one(&public_prop), hash_one(&private_prop));

    let public_method = object_of(ObjectMember::Method(MethodSignature::synthetic(
        "m".into(),
        empty_fn(),
        false,
    )));
    let protected_method = object_of(ObjectMember::Method(MethodSignature::with_visibility(
        "m".into(),
        empty_fn(),
        false,
        MemberVisibility::Protected,
        Default::default(),
    )));
    assert_ne!(public_method, protected_method);
    assert_ne!(hash_one(&public_method), hash_one(&protected_method));
}

/// The derive serde path uses `#[serde(default)]` (NOT `#[serde(skip)]`), so a
/// non-public value survives a round-trip and absent JSON deserializes as
/// `Public`.
#[test]
fn object_property_serde_roundtrip_preserves_non_public_visibility() {
    let private = ObjectProperty::with_visibility(
        "a".into(),
        string_ty(),
        false,
        false,
        MemberVisibility::Private,
        Default::default(),
    );
    let value = serde_json::to_value(&private).expect("serialize");
    let decoded: ObjectProperty = serde_json::from_value(value).expect("deserialize");
    assert_eq!(decoded.visibility, MemberVisibility::Private);
    assert_eq!(private, decoded);

    // JSON missing the `visibility` field deserializes as Public (back-compat).
    let json_without = serde_json::json!({
        "name": "a",
        "ty": { "kind": "primitive", "name": "string" },
        "optional": false,
        "readonly": false,
    });
    let decoded_default: ObjectProperty =
        serde_json::from_value(json_without).expect("deserialize default");
    assert_eq!(decoded_default.visibility, MemberVisibility::Public);
}

#[test]
fn method_signature_serde_roundtrip_preserves_non_public_visibility() {
    let protected = MethodSignature::with_visibility(
        "m".into(),
        empty_fn(),
        false,
        MemberVisibility::Protected,
        Default::default(),
    );
    let value = serde_json::to_value(&protected).expect("serialize");
    let decoded: MethodSignature = serde_json::from_value(value).expect("deserialize");
    assert_eq!(decoded.visibility, MemberVisibility::Protected);
    assert_eq!(protected, decoded);
}
