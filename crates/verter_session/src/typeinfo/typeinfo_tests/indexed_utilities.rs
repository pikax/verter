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

    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected Parameters<T> to resolve to tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 2);
    assert_ref(&elements[0].ty, "SubmitPayload");
    assert_primitive(&elements[1].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "reducer resolves this correctly (covered by the non-ignored `indexed_utilities_parameters_and_element_reducer_regression`); NOT oracle-liftable — the declared source body is an indexed-access construct the oracle's source-side positive allowlist rejects (measured Reject(DeferredConstruct(indexed-access)) on the tuple_labels generation probe) — `Parameters<...>[0]` shares the probe row's source shape. Lift pending an oracle source-walk carve-out for utility-rooted indexed-access chains"]
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
#[ignore = "reducer resolves this correctly (covered by the non-ignored `indexed_utilities_parameters_and_element_reducer_regression`); NOT oracle-liftable — the declared source body is an indexed-access construct the oracle's source-side positive allowlist rejects (measured Reject(DeferredConstruct(indexed-access)) on the tuple_labels generation probe) — `Parameters<...>[1]` shares the probe row's source shape. Lift pending an oracle source-walk carve-out for utility-rooted indexed-access chains"]
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

/// Non-ignored reducer regression for the four indexed-access utility rows
/// (`DirectParametersPayload` / `DirectParametersSecond` / `NestedFirstItem` /
/// `NestedSubmitPayload`): tuple positions over `Parameters<F>` project the
/// per-slot value, and the nested `NonNullable` / `[number]` chains reduce to
/// their terminal payloads. The sibling `#[ignore]`d rows stay manifest rows —
/// their indexed-access source bodies are outside the oracle's positive
/// allowlist — so this active regression carries the reducer proof.
#[test]
fn indexed_utilities_parameters_and_element_reducer_regression() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/indexed-utilities.ts", INDEXED_UTILITIES);

    let (payload, _) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "DirectParametersPayload",
        &[],
        ProjectionMode::Expanded,
    );
    let props = object_props(&payload);
    assert_eq!(prop_names(&props), vec!["id", "meta", "valid"]);

    let (second, _) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "DirectParametersSecond",
        &[],
        ProjectionMode::Expanded,
    );
    assert_primitive(&second, PrimitiveName::Number);

    let (item, _) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "NestedFirstItem",
        &[],
        ProjectionMode::Expanded,
    );
    let props = object_props(&item);
    assert_eq!(prop_names(&props), vec!["id", "value"]);

    let (nested, _) = resolve_expr(
        &host,
        "/fixtures/indexed-utilities.ts",
        "NestedSubmitPayload",
        &[],
        ProjectionMode::Expanded,
    );
    let props = object_props(&nested);
    assert_eq!(prop_names(&props), vec!["id", "meta", "valid"]);
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
#[ignore = "reducer resolves this correctly (covered by the non-ignored `indexed_utilities_parameters_and_element_reducer_regression`); NOT oracle-liftable — the declared source body is an indexed-access construct the oracle's source-side positive allowlist rejects (measured Reject(DeferredConstruct(indexed-access)) on the tuple_labels generation probe) — the nested NonNullable/indexed chain shares the probe row's source shape. Lift pending an oracle source-walk carve-out for utility-rooted indexed-access chains"]
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
#[ignore = "reducer publishes the surface with member-level alias refs kept SHALLOW (`Ref { NestedSubmitPayload }` etc.) per the Component-Meta Shallow-By-Default publication rule, so the row's eager member-inlining expectation stays open; NOT oracle-liftable regardless — the surface carries the callable `cancel` member (Reject(Callable)) and indexed-access source bodies. Lift pending a member-demand walk in the row plus an oracle admission extension"]
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
#[ignore = "reducer resolves this correctly (covered by the non-ignored `indexed_utilities_parameters_and_element_reducer_regression`); NOT oracle-liftable — the declared source body is an indexed-access construct the oracle's source-side positive allowlist rejects (measured Reject(DeferredConstruct(indexed-access)) on the tuple_labels generation probe) — the NonNullable/[number] chain shares the probe row's source shape. Lift pending an oracle source-walk carve-out for utility-rooted indexed-access chains"]
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
