//! @ai-generated - Tuple element label contracts.
//!
//! TDD-red tests for `Parameters<T>` label preservation, indexed access by
//! position (drops the label), `[number]` projection (drops label, unions
//! element types), and direct labelled tuple aliases.

use super::support::*;

const TUPLE_LABELS: &str = include_str!("fixtures/tuple_labels.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/tuple_labels.ts", TUPLE_LABELS);
}

#[test]
#[ignore = "typeinfo currently does not preserve named tuple element labels through Parameters<T>; keep as the future tuple-label parameters contract"]
fn tuple_labels_parameters_preserves_named_labels_and_optional_marker() {
    // TS7 contract: `Parameters<(name: string, count: number, active?: boolean)
    // => void>` = `[name: string, count: number, active?: boolean | undefined]`.
    // Each tuple element carries its declared label and the `active` element
    // is optional (TS adds `| undefined` to the slot type and marks
    // `optional`).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/tuple_labels.ts",
        "HandlerParams",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[0].label.as_deref(), Some("name"));
    assert!(!elements[0].optional);
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_eq!(elements[1].label.as_deref(), Some("count"));
    assert!(!elements[1].optional);
    assert_primitive(&elements[1].ty, PrimitiveName::Number);
    assert_eq!(elements[2].label.as_deref(), Some("active"));
    assert!(elements[2].optional);
    // The optional element carries `boolean | undefined`.
    assert_union_contains_primitive(&elements[2].ty, PrimitiveName::Boolean);
    assert_union_contains_primitive(&elements[2].ty, PrimitiveName::Undefined);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `Parameters<T>[0]` to the first element type (drop label, drop optional); keep as the future tuple-label numeric-index contract"]
fn tuple_labels_numeric_position_access_drops_label() {
    // TS7 contract: `HandlerParams[0]` = `string`. Numeric-position indexed
    // access drops the label and the optional marker (since the slot is
    // required at position 0).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/tuple_labels.ts",
        "HandlerFirstParam",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `Parameters<T>[2]` for an optional tuple slot to the union with `undefined`; keep as the future tuple-label optional-slot contract"]
fn tuple_labels_numeric_position_access_on_optional_slot_carries_undefined() {
    // TS7 contract: `HandlerParams[2]` = `boolean | undefined`. The optional
    // slot's slot-type includes `undefined`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/tuple_labels.ts",
        "HandlerThirdParam",
        &[],
        ProjectionMode::Expanded,
    );

    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `Parameters<T>[number]` to the union of all element types; keep as the future tuple-[number]-access contract"]
fn tuple_labels_number_index_projects_all_elements_union() {
    // TS7 contract: `HandlerParams[number]` = `string | number | boolean |
    // undefined` (every element type unioned together; the optional slot
    // contributes `boolean | undefined`).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/tuple_labels.ts",
        "HandlerNumberElement",
        &[],
        ProjectionMode::Expanded,
    );

    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn tuple_labels_direct_labelled_tuple_alias_publishes_labels() {
    // Active baseline: a directly-declared labelled tuple alias must publish
    // the labels and the optional marker without any utility-type traversal.
    // This characterises the labels-from-syntax path independently from the
    // Parameters<>-reduction tests above.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/tuple_labels.ts",
        "DirectLabelledTuple",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[0].label.as_deref(), Some("first"));
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_eq!(elements[1].label.as_deref(), Some("second"));
    assert_primitive(&elements[1].ty, PrimitiveName::Number);
    assert_eq!(elements[2].label.as_deref(), Some("third"));
    assert!(elements[2].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
