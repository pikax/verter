//! @ai-generated - Empty-path Shallow projection COMPLETE surface fact set
//! + the P2-1 heritage-vs-authored merge rule.
//!
//! These are the typeinfo-primary discriminators for the unification-stage
//! batch U1. Each test FAILS against the pre-change tree (the Shallow
//! projection dropped call/construct/index signatures; the intersection
//! merge did not distinguish interface heritage from authored intersection)
//! and PASSES against the post-change tree.

use super::support::*;

const SHALLOW_SURFACE_FACTS: &str = include_str!("fixtures/shallow_surface_facts.ts");
const FILE: &str = "/fixtures/shallow_surface_facts.ts";

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, FILE, SHALLOW_SURFACE_FACTS);
}

// ---------------------------------------------------------------------------
// (1) The empty-path Shallow projection preserves CALL signatures.
//
// Discriminating: before U1, `surface_view_from_shallow` hardcoded empty
// `call_signatures`, so the projected `HybridSurface` object carried zero
// `ObjectMember::CallSignature`. After U1 the call signature survives.
// ---------------------------------------------------------------------------

#[test]
fn shallow_surface_preserves_call_signature() {
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, FILE, "HybridSurface");

    let call_sigs = object_call_signatures(&expr);
    assert_eq!(
        call_sigs.len(),
        1,
        "HybridSurface must carry its call signature through the empty-path \
         Shallow projection; got {expr:?}"
    );
    // The call signature is `(token: string): number`.
    assert_eq!(call_sigs[0].parameters.len(), 1);
    let ret = call_sigs[0]
        .return_type
        .as_ref()
        .expect("call signature must carry a return type");
    assert_primitive(ret, PrimitiveName::Number);
}

// ---------------------------------------------------------------------------
// (2) The empty-path Shallow projection preserves CONSTRUCT signatures.
// ---------------------------------------------------------------------------

#[test]
fn shallow_surface_preserves_construct_signature() {
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, FILE, "HybridSurface");

    let construct_sigs = object_construct_signatures(&expr);
    assert_eq!(
        construct_sigs.len(),
        1,
        "HybridSurface must carry its construct signature through the \
         empty-path Shallow projection; got {expr:?}"
    );
    assert_eq!(construct_sigs[0].parameters.len(), 1);
}

// ---------------------------------------------------------------------------
// (3) The empty-path Shallow projection preserves INDEX signatures.
// ---------------------------------------------------------------------------

#[test]
fn shallow_surface_preserves_index_signature() {
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, FILE, "HybridSurface");

    let index_sigs = object_index_signatures(&expr);
    assert_eq!(
        index_sigs.len(),
        1,
        "HybridSurface must carry its index signature through the empty-path \
         Shallow projection; got {expr:?}"
    );
}

// ---------------------------------------------------------------------------
// (4) A pure call-signature carrier publishes the call signature (does NOT
//     collapse to an empty / unknown object surface).
// ---------------------------------------------------------------------------

#[test]
fn shallow_surface_call_only_publishes_call_signature() {
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, FILE, "CallOnly");

    // `interface CallOnly { (value: string): boolean }` projects to a bare
    // Function (a single-call-signature object collapses to the function).
    let function = function_type(&expr);
    assert_eq!(function.parameters.len(), 1);
    let ret = function
        .return_type
        .as_ref()
        .expect("call-only signature must carry a return type");
    assert_primitive(ret, PrimitiveName::Boolean);
}

// ---------------------------------------------------------------------------
// (5) Named members + flags survive (sanity + negative: a non-member name is
//     absent, and the optional flag is carried).
// ---------------------------------------------------------------------------

#[test]
fn shallow_surface_named_members_and_flags_present() {
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, FILE, "HybridSurface");

    let props = object_props(&expr);
    assert!(props.contains_key("named"), "named member must be present");
    assert!(props.contains_key("flag"), "flag member must be present");
    assert!(
        !props.contains_key("absent"),
        "a name never declared must be absent: {:?}",
        prop_names(&props)
    );
    assert!(props["named"].ty == TypeExpr::Primitive(PrimitiveName::String));
    assert!(!props["named"].optional, "`named` is required");
    assert!(props["flag"].optional, "`flag` is optional");
}

// ---------------------------------------------------------------------------
// (6) P2-1 POSITIVE — interface heritage duplicate SHADOWS.
//
// `interface HeritageDerived extends HeritageBase { dup: string }` with
// `HeritageBase.dup: number` => `HeritageDerived['dup']` is `string`
// (derived-member precedence), NOT `string & number`.
// ---------------------------------------------------------------------------

#[test]
fn interface_heritage_duplicate_shadows() {
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, FILE, "HeritageDerived");
    let props = object_props(&expr);

    // Own `dup: string` shadows the inherited `HeritageBase.dup: number`.
    assert_eq!(
        props["dup"].ty,
        TypeExpr::Primitive(PrimitiveName::String),
        "interface heritage must shadow (own `string` wins over inherited \
         `number`), not intersect; got {:?}",
        props["dup"]
    );
    // NEGATIVE: the merged `dup` must NOT be an intersection of number & string.
    assert!(
        !matches!(&props["dup"].ty, TypeExpr::Intersection(_)),
        "interface heritage `dup` must not be an intersection; got {:?}",
        props["dup"]
    );
    // Inherited-only and own-only members both survive.
    assert!(props.contains_key("baseOnly"), "inherited member survives");
    assert!(props.contains_key("derivedOnly"), "own member survives");
}

// ---------------------------------------------------------------------------
// (7) P2-1 NEGATIVE — authored intersection duplicate does NOT shadow.
//
// `type AuthoredIntersection = HeritageBase & { dup: string }` with
// `HeritageBase.dup: number` => `AuthoredIntersection['dup']` is the
// intersection `number & string`, NOT plain `string`.
// ---------------------------------------------------------------------------

#[test]
fn authored_intersection_duplicate_does_not_shadow() {
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, FILE, "AuthoredIntersection");
    let props = object_props(&expr);

    // The duplicate `dup` must be the intersection of both arms (`number &
    // string`), NOT a shadow.
    let dup_ty = &props["dup"].ty;
    assert_expr_contains_primitive(dup_ty, PrimitiveName::Number);
    assert_expr_contains_primitive(dup_ty, PrimitiveName::String);
    // NEGATIVE: must NOT have collapsed to plain `string` (the shadow bug).
    assert!(
        !matches!(dup_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "authored intersection must NOT shadow — `dup` must stay `number & \
         string`, not collapse to plain `string`; got {dup_ty:?}"
    );
    assert!(props.contains_key("baseOnly"), "referenced member survives");
    assert!(props.contains_key("authoredOnly"), "own member survives");
}

// ---------------------------------------------------------------------------
// (8) Union common-member merge — only names present in EVERY arm survive;
//     a non-common member is ABSENT (negative assertion).
// ---------------------------------------------------------------------------

#[test]
fn union_common_members_only() {
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, FILE, "CommonMembers");

    let props = object_props(&expr);
    let names = prop_names(&props);
    assert!(
        props.contains_key("shared"),
        "`shared` is in both arms — it must survive; got {names:?}"
    );
    assert!(
        props.contains_key("ro"),
        "`ro` is in both arms — it must survive; got {names:?}"
    );
    // NEGATIVE: arm-specific members must NOT appear on the common surface.
    assert!(
        !props.contains_key("onlyA"),
        "`onlyA` is only in arm A — it must be ABSENT from the common-member \
         surface; got {names:?}"
    );
    assert!(
        !props.contains_key("onlyB"),
        "`onlyB` is only in arm B — it must be ABSENT from the common-member \
         surface; got {names:?}"
    );
}

// ---------------------------------------------------------------------------
// (9) Union common-member value is the UNION of per-arm values; readonly is
//     true only when readonly in ALL arms.
// ---------------------------------------------------------------------------

#[test]
fn union_common_member_value_unions_and_readonly_intersects() {
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, FILE, "CommonMembers");
    let props = object_props(&expr);

    // `shared` is `string` in arm A and `number` in arm B => union value.
    let shared = &props["shared"].ty;
    assert_union_contains_primitive(shared, PrimitiveName::String);
    assert_union_contains_primitive(shared, PrimitiveName::Number);

    // `ro` is readonly in arm A but writable in arm B => NOT readonly on the
    // common surface (readonly only when readonly in ALL arms).
    assert!(
        !props["ro"].readonly,
        "`ro` is readonly only in arm A — the common member must NOT be \
         readonly (readonly-in-ALL rule); got {:?}",
        props["ro"]
    );
}
