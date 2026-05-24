//! @ai-generated - Built-in utility-type edge contracts.
//!
//! TDD-red and active tests for utility-type edge inputs:
//!   * `Pick<T, never>` / `Omit<T, never>` / `Pick<T, keyof T>` / `Omit<T, keyof T>`.
//!   * `Required<{ a?: T }>`, `Readonly<Required<T>>` composition.
//!   * `NonNullable<string | null | undefined>` collapses to `string`.
//!   * `Extract` / `Exclude` over a primitive union.

use super::support::*;

const UTILITY_EDGE: &str = include_str!("fixtures/utility_edge.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/utility_edge.ts", UTILITY_EDGE);
}

#[test]
#[ignore = "typeinfo currently does not reduce `Pick<T, never>` to the empty object; keep as the future Pick-never edge contract"]
fn utility_edge_pick_never_yields_empty_object() {
    // TS7 contract: `Pick<Base, never>` = `{}`. The mapped form is
    // `{ [K in never]: Base[K] }` which has no members.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "PickNever",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert!(props.is_empty(), "expected empty object, got {props:?}");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn utility_edge_omit_never_yields_input_shape() {
    // TS7 contract: `Omit<Base, never>` = `Base`. Omitting nothing keeps every
    // member. Active baseline: Verter already handles this.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "OmitNever",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b", "c"]);
    assert_primitive(&props["a"].ty, PrimitiveName::Number);
    assert_primitive(&props["b"].ty, PrimitiveName::String);
    assert_primitive(&props["c"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `Omit<T, keyof T>` to the empty object; keep as the future Omit-all edge contract"]
fn utility_edge_omit_all_keys_yields_empty_object() {
    // TS7 contract: `Omit<Base, keyof Base>` removes every declared member,
    // leaving `{}`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "OmitAll",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert!(props.is_empty(), "expected empty object, got {props:?}");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `Pick<T, keyof T>` to the input shape; keep as the future Pick-all identity contract"]
fn utility_edge_pick_all_keys_yields_input_shape() {
    // TS7 contract: `Pick<Base, keyof Base>` = `Base`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "PickAll",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b", "c"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `Required<T>` to a shape where every member loses its `?` marker; keep as the future Required edge contract"]
fn utility_edge_required_strips_optional_markers() {
    // TS7 contract: `Required<{ a?: string; b?: number }>` = `{ a: string; b: number }`.
    // Every optional marker is removed and the slot type loses `undefined`
    // (since `Required` strips the optional-driven `undefined`).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "RequiredOptional",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    assert!(!props["a"].optional);
    assert!(!props["b"].optional);
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not compose `Readonly<Required<T>>` to mark every required member readonly; keep as the future Readonly+Required composition contract"]
fn utility_edge_readonly_required_composes_modifiers() {
    // TS7 contract: `Readonly<Required<{ a?: string; b?: number }>>` =
    // `{ readonly a: string; readonly b: number }`. Both modifier passes
    // compose: optionality is stripped, readonly is added.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "ReadonlyRequiredOptional",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    assert!(!props["a"].optional);
    assert!(!props["b"].optional);
    assert!(props["a"].readonly);
    assert!(props["b"].readonly);
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `NonNullable<string | null | undefined>` to the bare primitive; keep as the future NonNullable nullable-primitive contract"]
fn utility_edge_non_nullable_strips_null_and_undefined() {
    // TS7 contract: `NonNullable<string | null | undefined>` = `string`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "NonNullablePrim",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn utility_edge_extract_keeps_matching_primitive_only() {
    // TS7 contract: `Extract<string | number | boolean, string>` = `string`.
    // Active baseline: Verter already handles primitive-union Extract.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "ExtractStringOnly",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn utility_edge_exclude_drops_matching_primitive_only() {
    // TS7 contract: `Exclude<string | number | boolean, number>` =
    // `string | boolean` (a two-arm primitive union; number is filtered).
    // Active baseline: Verter already handles primitive-union Exclude.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "ExcludeNumberOnly",
        &[],
        ProjectionMode::Expanded,
    );

    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
    let TypeExpr::Union(types) = &expr else {
        panic!("expected union, got {expr:?}");
    };
    assert_eq!(
        types.len(),
        2,
        "expected exactly two arms (string, boolean), got {types:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
