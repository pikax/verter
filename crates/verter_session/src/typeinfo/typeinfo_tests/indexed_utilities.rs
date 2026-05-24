//! @ai-generated - Synthetic nested indexed utility typeinfo tests.

use super::support::*;

#[test]
fn direct_parameters_tuple_preserves_function_arguments() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/indexed-utilities.ts", INDEXED_UTILITIES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "DirectParametersTuple",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, .. } = expr else {
        panic!("expected Parameters<T> to resolve to tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 2);
    assert_ref(&elements[0].ty, "SubmitPayload");
    assert_primitive(&elements[1].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently leaves Parameters<T>[0] as an indexed access over the resolved tuple; keep as the future tuple-index projection contract"]
fn direct_parameters_payload_extracts_function_argument() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/indexed-utilities.ts", INDEXED_UTILITIES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "DirectParametersPayload",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["id", "meta", "valid"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["valid"].ty, PrimitiveName::Boolean);
    let meta = object_props(&props["meta"].ty);
    assert_literal_union(&meta["source"].ty, &["keyboard", "pointer"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently leaves Parameters<T>[1] as an indexed access over the resolved tuple; keep as the future numeric tuple-index projection contract"]
fn direct_parameters_second_extracts_number_argument() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/indexed-utilities.ts", INDEXED_UTILITIES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "DirectParametersSecond",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn direct_return_type_resolves_object_payload() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/indexed-utilities.ts", INDEXED_UTILITIES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "DirectReturnPayload",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["status", "submitted"]);
    assert_string_literal(&props["status"].ty, "ok");
    assert_ref(&props["submitted"].ty, "SubmitPayload");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently leaves Parameters<NonNullable<NonNullable<T['slots']>['submit']>>[0] behind a semanticMiss indexed access; keep as the future nested Parameters/NonNullable contract"]
fn nested_parameters_nonnullable_indexed_payload_resolves() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/indexed-utilities.ts", INDEXED_UTILITIES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "NestedSubmitPayload",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["id", "meta", "valid"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["valid"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently preserves nested indexed-access utility aliases inside object fields instead of reducing their terminal payloads; keep as the future deep utility-surface contract"]
fn nested_indexed_utility_surface_resolves_all_terminal_members() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/indexed-utilities.ts", INDEXED_UTILITIES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "NestedIndexedUtilitySurface",
        &[],
        ProjectionMode::Expanded,
    );

    let surface = object_props(&expr);
    let submit = object_props(&surface["submitPayload"].ty);
    assert_primitive(&submit["id"].ty, PrimitiveName::String);
    let item = object_props(&surface["firstItem"].ty);
    assert_primitive(&item["value"].ty, PrimitiveName::Number);
    let cancel = function_type(&surface["cancel"].ty);
    assert_eq!(cancel.parameters.len(), 0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce NonNullable<T['items']>[number] through a nested indexed-access chain; keep as the future array-element projection contract"]
fn nested_nonnullable_array_indexed_access_resolves_element() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/indexed-utilities.ts", INDEXED_UTILITIES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "NestedFirstItem",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["id", "value"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["value"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
