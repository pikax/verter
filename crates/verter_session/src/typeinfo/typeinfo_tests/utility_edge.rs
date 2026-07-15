//! @ai-generated - Built-in utility-type edge contracts.
//!
//! TDD-red and active tests for utility-type edge inputs:
//!   * `Pick<T, never>` / `Omit<T, never>` / `Pick<T, keyof T>` / `Omit<T, keyof T>`.
//!   * `Required<{ a?: T }>`, `Readonly<Required<T>>` composition.
//!   * `NonNullable<string | null | undefined>` collapses to `string`.
//!   * `Extract` / `Exclude` over a primitive union.

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

const UTILITY_EDGE: &str = include_str!("fixtures/utility_edge.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/utility_edge.ts", UTILITY_EDGE);
}

#[test]
#[ignore = "reducer resolves this correctly (covered by the non-ignored `utility_edge_object_filter_keyspace_reducer_regression`); NOT oracle-liftable — the `never` key argument in the declared source is outside the oracle's source-side positive allowlist (NeverKeyword). Lift pending an oracle admission extension for degenerate keyword arguments"]
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
#[ignore = "reducer resolves this correctly (covered by the non-ignored `utility_edge_object_filter_keyspace_reducer_regression`); NOT oracle-liftable — generation was attempted and measured Reject(DeferredConstruct(keyof)) on the `keyof Base` key argument. Lift pending an oracle source-walk carve-out for keyof key arguments"]
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
#[ignore = "reducer resolves this correctly (covered by the non-ignored `utility_edge_object_filter_keyspace_reducer_regression`); NOT oracle-liftable — generation was attempted and measured Reject(DeferredConstruct(keyof)) on the `keyof Base` key argument. Lift pending an oracle source-walk carve-out for keyof key arguments"]
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

// LIFTED: `Required<{ a?: string; b?: number }>` = `{ a: string; b: number }`.
// `Required<T>` is the library mapped type `{ [K in keyof T]-?: T[K] }`; the
// `-?` optional-stripping remap is the terminal MappedTemplateRemap producer.
// The lifted body is the registry-keyed `oracle::run_row` shared-driver call
// that resolves Verter's `Expanded` projection and compares it against the
// checked-in tsgo snapshot (captured in `Expanded`); the audit query-mode
// identity is proven live by `lifted_row_audit_query_mode_matches_spec`.
#[oracle_row]
#[test]
fn utility_edge_required_strips_optional_markers() {}

// LIFTED: `Readonly<Required<{ a?: string; b?: number }>>` =
// `{ readonly a: string; readonly b: number }`. Both library mapped-type
// modifier passes compose (`-?` optional stripping, then `+readonly`); the
// lifted body is the registry-keyed oracle comparison, verified in the same
// `Expanded` projection mode.
#[oracle_row]
#[test]
fn utility_edge_readonly_required_composes_modifiers() {}

// LIFTED: `NonNullable<string | null | undefined>` = `string` — the settled
// union filters its nullish arms and the lone survivor collapses to the bare
// primitive. The lifted body is the registry-keyed `oracle::run_row`
// shared-driver call comparing Verter's `Expanded` projection against the
// checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn utility_edge_non_nullable_strips_null_and_undefined() {}

/// Non-ignored reducer regression for the three keyspace-domain object-filter
/// rows (`PickNever` / `OmitAll` / `PickAll`): `Pick<Base, never>` and
/// `Omit<Base, keyof Base>` reduce to the representable empty object and
/// `Pick<Base, keyof Base>` reproduces the input shape. The sibling
/// `#[ignore]`d rows stay manifest rows — their `keyof` / `never` source
/// constructs are outside the oracle's positive allowlist — so this active
/// regression carries the reducer proof.
#[test]
fn utility_edge_object_filter_keyspace_reducer_regression() {
    let host = make_host_with_footprint();
    upsert(&host);

    let (pick_never, _) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "PickNever",
        &[],
        ProjectionMode::Expanded,
    );
    let props = object_props(&pick_never);
    assert!(
        props.is_empty(),
        "Pick<Base, never> must be empty, got {props:?}"
    );

    let (omit_all, _) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "OmitAll",
        &[],
        ProjectionMode::Expanded,
    );
    let props = object_props(&omit_all);
    assert!(
        props.is_empty(),
        "Omit<Base, keyof Base> must be empty, got {props:?}"
    );

    let (pick_all, _) = resolve_expr(
        &host,
        "/fixtures/utility_edge.ts",
        "PickAll",
        &[],
        ProjectionMode::Expanded,
    );
    let props = object_props(&pick_all);
    assert_eq!(prop_names(&props), vec!["a", "b", "c"]);
    assert_primitive(&props["a"].ty, PrimitiveName::Number);
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
