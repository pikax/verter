//! @ai-generated - Synthetic TypeScript enum typeinfo contracts.
//!
//! These tests describe the TS7 expected projection for declarative numeric
//! enums, declarative string enums, and `const enum`. They are TDD-red: every
//! ignored test asserts the TS7 emission, not Verter's current behaviour.

use super::support::*;

fn upsert_enum_fixture(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/enums.ts", ENUMS);
}

const ENUMS: &str = include_str!("fixtures/enums.ts");

#[test]
#[ignore = "typeinfo currently does not lower declarative numeric enum members into TypeScript's branded literal-number type; keep as the future numeric-enum-member contract"]
fn enum_numeric_member_resolves_to_branded_literal_zero() {
    // TS7 contract: `Color.Red` is a branded numeric-enum member type whose
    // value-side numeric literal is `0`. The published surface for the type
    // alias `ColorRed = Color.Red` is the literal `0` (TS treats numeric enum
    // members as assignable to/from the corresponding number literal at the
    // type level).
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "ColorRed",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 0.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not preserve TypeScript's branded string-enum member identity; keep as the future string-enum-member contract"]
fn enum_string_member_resolves_to_branded_string_literal() {
    // TS7 contract: `Status.Idle` is a branded string-enum member. At the
    // structural type level it surfaces as the string literal `"idle"`
    // (Verter publishes the literal value; the brand identity is a TS-only
    // nominal-typing trick that has no runtime structure).
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "StatusIdle",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "idle");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate template-literal types over a string-enum reference; keep as the future enum template-literal contract"]
fn enum_template_literal_over_string_enum_produces_value_union() {
    // TS7 contract: `${Status}` is a template-literal type that expands the
    // string-enum value union, producing `"idle" | "active" | "done"`.
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "StatusValueUnion",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["active", "done", "idle"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not project keyof typeof EnumDecl as the literal member-name union; keep as the future enum keyof-typeof contract"]
fn enum_keyof_typeof_numeric_yields_member_name_union() {
    // TS7 contract: `keyof typeof Color` is the union of the enum's declared
    // member names, NOT the reverse-mapped numeric keys. So
    // `ColorKeyUnion = "Red" | "Green" | "Blue"`. (For numeric enums TS also
    // exposes a numeric index signature on the typeof Enum value, but that
    // doesn't appear in the keyof.)
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "ColorKeyUnion",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["Blue", "Green", "Red"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not project keyof typeof EnumDecl as the literal member-name union; keep as the future enum keyof-typeof contract"]
fn enum_keyof_typeof_string_yields_member_name_union() {
    // TS7 contract: `keyof typeof Status` = `"Idle" | "Active" | "Done"`.
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "StatusKeyUnion",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["Active", "Done", "Idle"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not preserve const-enum member literals; keep as the future const-enum-member contract"]
fn enum_const_enum_member_resolves_to_inlined_string_literal() {
    // TS7 contract: `Direction.Up` from a `const enum` produces the string
    // literal `"UP"` (const enums inline at use sites; the type-level
    // projection equals the assigned literal).
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "DirectionUp",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "UP");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not narrow discriminated-union arms when the discriminant is an enum member; keep as the future enum discriminant-projection contract"]
fn enum_discriminant_extract_projects_matching_arm_payload() {
    // TS7 contract: `Extract<StatefulNode, { status: Status.Idle }>["payload"]`
    // selects the `Status.Idle` arm and projects its payload object:
    //   { hint: string }
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "IdleNodePayload",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["hint"]);
    assert!(!props["hint"].optional);
    assert_primitive(&props["hint"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
