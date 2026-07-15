//! @ai-generated - Branded / nominal-typing contracts.
//!
//! TDD-red tests describing TS7 behaviour for string/numeric brands via
//! intersection, `unique symbol` brands, brand-tag key projection, phantom
//! types, and branded-guard narrowing.

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

const BRANDED_TYPES: &str = include_str!("fixtures/branded_types.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/branded_types.ts", BRANDED_TYPES);
}

#[test]
fn branded_string_intersection_carries_brand_tag_property() {
    // TS7 contract: `UserId` = `string & { readonly __brand: "UserId" }`. The
    // type alias body is preserved as an intersection of the primitive and
    // the brand-tag object literal. Active baseline: Verter publishes the
    // raw intersection shape; subsequent tests probe individual arms.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/branded_types.ts",
        "UserId",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Intersection(parts) = &expr else {
        panic!("expected intersection, got {expr:?}");
    };
    assert_eq!(parts.len(), 2);
    let has_string = parts
        .iter()
        .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String)));
    assert!(
        has_string,
        "intersection must include the string primitive arm; got {parts:?}"
    );
    let object_arm = parts
        .iter()
        .find(|ty| matches!(ty, TypeExpr::Object(_)))
        .expect("intersection must include the brand-tag object arm");
    let object_props = object_props(object_arm);
    assert_eq!(prop_names(&object_props), vec!["__brand"]);
    assert!(object_props["__brand"].readonly);
    assert_string_literal(&object_props["__brand"].ty, "UserId");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn branded_number_intersection_carries_brand_tag_property() {
    // TS7 contract: `Cents` = `number & { readonly __cents: true }`. The
    // numeric brand pattern lifts a boolean-literal tag onto the intersection.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/branded_types.ts",
        "Cents",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Intersection(parts) = &expr else {
        panic!("expected intersection, got {expr:?}");
    };
    assert_eq!(parts.len(), 2);
    let has_number = parts
        .iter()
        .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::Number)));
    assert!(
        has_number,
        "intersection must include the number primitive arm; got {parts:?}"
    );
    let object_arm = parts
        .iter()
        .find(|ty| matches!(ty, TypeExpr::Object(_)))
        .expect("intersection must include the brand-tag object arm");
    let object_props = object_props(object_arm);
    assert_eq!(prop_names(&object_props), vec!["__cents"]);
    assert!(object_props["__cents"].readonly);
    assert_boolean_literal(&object_props["__cents"].ty, true);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not publish `unique symbol` computed-key members on a branded intersection generic; keep as the future unique-symbol brand contract"]
fn branded_unique_symbol_wrapper_publishes_branded_surface() {
    // TS7 contract: `AccountId` = `IdBranded<string>` = `string & { readonly
    // [idBrand]: string }`. The unique-symbol-keyed member becomes a computed
    // key in the brand-tag object; the wrapped type `T = string` flows into
    // both the carrier and the brand-key value.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/branded_types.ts",
        "AccountId",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Intersection(parts) = &expr else {
        panic!("expected intersection, got {expr:?}");
    };
    assert_eq!(parts.len(), 2);
    assert!(
        parts
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))),
        "intersection must carry the wrapped string primitive; got {parts:?}"
    );
    // The brand-tag object arm must publish exactly ONE member — the
    // unique-symbol-keyed brand slot — with value type `string` (because
    // `AccountId = IdBranded<string>` substitutes T=string into the wrapper's
    // computed-key value), and the slot must be `readonly`.
    let object_arm = parts
        .iter()
        .find(|ty| matches!(ty, TypeExpr::Object(_)))
        .expect("intersection must include the brand-tag object arm");
    let TypeExpr::Object(object) = object_arm else {
        panic!("brand-tag arm must be TypeExpr::Object, got {object_arm:?}")
    };
    assert_eq!(
        object.properties.len(),
        1,
        "brand-tag object must publish exactly one member (the unique-symbol \
         computed-key brand slot); got {:?}",
        object.properties
    );
    // The single member is either an IndexSignature (symbol-key) or a regular
    // Property with the brand value type. Both shapes are TS-equivalent
    // depending on how Verter lowers the `[uniqueSym]: T` form; assert the
    // semantic invariants that apply in both cases.
    use verter_type_expr::ObjectMember;
    match &object.properties[0] {
        ObjectMember::Property(prop) => {
            assert!(
                prop.readonly,
                "brand-slot property must publish readonly=true; got {prop:?}"
            );
            assert_eq!(
                prop.ty,
                TypeExpr::Primitive(PrimitiveName::String),
                "brand-slot value must be `string` (T=string substitution); got {:?}",
                prop.ty
            );
        }
        ObjectMember::IndexSignature(sig) => {
            assert_eq!(
                sig.key_type,
                TypeExpr::Primitive(PrimitiveName::Symbol),
                "brand-slot index signature must key on `symbol`; got {:?}",
                sig.key_type
            );
            assert_eq!(
                sig.value_type,
                TypeExpr::Primitive(PrimitiveName::String),
                "brand-slot index signature must value `string`; got {:?}",
                sig.value_type
            );
        }
        other => panic!("brand slot must be Property or IndexSignature, got {other:?}"),
    }
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `UserId["__brand"]` projects the string-literal brand tag `"UserId"` —
// the string-literal index chain walks the brand intersection arm path-precisely. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn branded_key_access_projects_literal_brand_tag() {}

#[test]
fn branded_phantom_carrier_projects_underlying_with_phantom_tag() {
    // TS7 contract: `EmailString` = `Phantom<"email", string>` = `string &
    // { readonly __phantom: "email" }`. The two-parameter brand carrier
    // applies the literal `"email"` to the phantom tag while preserving the
    // wrapped `string` primitive.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/branded_types.ts",
        "EmailString",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Intersection(parts) = &expr else {
        panic!("expected intersection, got {expr:?}");
    };
    assert_eq!(parts.len(), 2);
    assert!(
        parts
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))),
        "intersection must include the wrapped string primitive; got {parts:?}"
    );
    let object_arm = parts
        .iter()
        .find(|ty| matches!(ty, TypeExpr::Object(_)))
        .expect("intersection must include the brand-tag object arm");
    let object_props = object_props(object_arm);
    assert_eq!(prop_names(&object_props), vec!["__phantom"]);
    assert!(object_props["__phantom"].readonly);
    assert_string_literal(&object_props["__phantom"].ty, "email");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn branded_guard_narrowing_publishes_union_of_branded_and_undefined() {
    // TS7 contract: `NarrowedUserId` = `ReturnType<typeof narrowUserId>`
    // where `narrowUserId(value: string): UserId | undefined`. The
    // declared return annotation is the authority — `ReturnType<>` extracts
    // exactly `UserId | undefined` (a union containing the `UserId` ref and
    // the `undefined` primitive arm). The type-guard `isUserId` is what
    // makes the narrowing legal in the body; the published return shape
    // does not erase the brand back to `string`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/branded_types.ts",
        "NarrowedUserId",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Union(types) = &expr else {
        panic!("expected union, got {expr:?}");
    };
    assert_eq!(types.len(), 2);
    assert!(
        types
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::Undefined))),
        "union must include the `undefined` arm; got {types:?}"
    );
    let user_id_arm = types
        .iter()
        .find(|ty| !matches!(ty, TypeExpr::Primitive(PrimitiveName::Undefined)))
        .expect("union must include the UserId arm");
    // TS7 publishes the alias by-ref OR expands to the underlying
    // intersection depending on the consumer. Either shape is acceptable
    // semantically — BUT in the expanded form the brand-object arm MUST
    // survive (the test's documented intent is that the brand is NOT erased
    // back to plain `string`).
    match user_id_arm {
        TypeExpr::Ref { name, .. } => {
            assert_eq!(name.as_ref(), "UserId");
        }
        TypeExpr::Intersection(parts) => {
            assert!(
                parts
                    .iter()
                    .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))),
                "expanded UserId arm must contain the string primitive; got {parts:?}"
            );
            // The brand-tag object arm `{ readonly __brand: "UserId" }` must be
            // present in the expanded intersection — otherwise the brand has
            // been erased back to bare `string`, defeating the nominal contract.
            let brand_object = parts
                .iter()
                .find(|ty| matches!(ty, TypeExpr::Object(_)))
                .expect(
                    "expanded UserId arm must preserve the brand-tag object \
                     `{ readonly __brand: \"UserId\" }`; brand was erased",
                );
            let props = object_props(brand_object);
            assert_eq!(
                prop_names(&props),
                vec!["__brand"],
                "brand object must publish exactly the `__brand` slot"
            );
            assert!(
                props["__brand"].readonly,
                "brand slot must be readonly; got {:?}",
                props["__brand"]
            );
            assert_string_literal(&props["__brand"].ty, "UserId");
        }
        other => panic!("expected UserId ref or its expanded intersection, got {other:?}"),
    }
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `Cents["__cents"]` projects the boolean-literal brand tag `true` —
// the same string-literal index chain over the numeric-brand intersection. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn branded_key_access_projects_boolean_literal_brand_tag() {}

#[test]
#[ignore = "typeinfo currently does not project `AccountId[typeof idBrand]` to the wrapped value type; keep as the future symbol-key value-projection contract"]
fn branded_symbol_key_access_projects_wrapped_value_type() {
    // TS7 contract: `AccountId` = `IdBranded<string>` = `string &
    // { readonly [idBrand]: string }`. Indexing the intersection at the
    // unique-symbol key projects the value type at that slot. Because
    // `IdBranded<T>` substitutes `T = string` into the symbol-key value
    // position, the projection is exactly `string`.
    //
    // Verified via tsgo `IsExactly<AccountIdBrandValue, string>`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/branded_types.ts",
        "AccountIdBrandValue",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not collapse a double-brand intersection (string-brand & number-brand) to `never`; keep as the future incompatible-intersection collapse contract"]
fn branded_double_intersection_collapses_to_never() {
    // TS7 contract: `UserIdCentsBoth = UserId & Cents` = `(string &
    // { __brand: "UserId" }) & (number & { __cents: true })`. The primitive
    // arms `string` and `number` are mutually incompatible, so the whole
    // intersection collapses to `never`. Even though the brand-tag objects
    // are independent, the incompatible primitive carriers force the
    // structural reduction.
    //
    // Verified via tsgo `IsExactly<UserIdCentsBoth, never>`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/branded_types.ts",
        "UserIdCentsBoth",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Never);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
