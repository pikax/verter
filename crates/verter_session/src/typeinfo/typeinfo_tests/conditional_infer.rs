//! @ai-generated - Synthetic conditional/infer chain typeinfo tests.

use super::support::*;

#[test]
fn conditional_infer_surface_preserves_resolved_alias_boundaries() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/conditional-infer.ts", CONDITIONAL_INFER);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/conditional-infer.ts",
        "ConditionalInferSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec![
            "callbackPayload",
            "current",
            "functionResult",
            "item",
            "status",
            "tuplePair"
        ]
    );
    assert_ref(&props["item"].ty, "ConcreteArrayItem");
    assert_ref(&props["current"].ty, "ConcreteDeepCurrent");
    assert_ref(&props["callbackPayload"].ty, "ConcreteFirstParameter");
    assert_ref(&props["status"].ty, "ConcreteElementStatus");
    assert_ref(&props["tuplePair"].ty, "ConcreteTuplePair");
    assert_ref(&props["functionResult"].ty, "ConcreteFunctionResult");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently preserves decidable conditional-infer aliases instead of binding infer variables and selecting the true branch; keep as the future InferBind reduction contract"]
fn conditional_infer_aliases_reduce_when_requested_directly() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/conditional-infer.ts", CONDITIONAL_INFER);

    let (array_item, array_record) = resolve_expr(
        &host,
        "/fixtures/conditional-infer.ts",
        "ConcreteArrayItem",
        &[],
        ProjectionMode::Expanded,
    );
    assert_ref(&array_item, "ActionPayload");

    let (current, current_record) = resolve_expr(
        &host,
        "/fixtures/conditional-infer.ts",
        "ConcreteDeepCurrent",
        &[],
        ProjectionMode::Expanded,
    );
    assert_ref(&current, "ActionPayload");

    let (first_parameter, parameter_record) = resolve_expr(
        &host,
        "/fixtures/conditional-infer.ts",
        "ConcreteFirstParameter",
        &[],
        ProjectionMode::Expanded,
    );
    assert_ref(&first_parameter, "ActionPayload");

    let (status_expr, status_record) = resolve_expr(
        &host,
        "/fixtures/conditional-infer.ts",
        "ConcreteElementStatus",
        &[],
        ProjectionMode::Expanded,
    );
    let status = object_props(&status_expr);
    assert_string_literal(&status["kind"].ty, "action");
    assert_ref(&status["item"].ty, "ActionPayload");
    assert_query_mode(&array_record, ProjectionModeTag::Expanded);
    assert_query_mode(&current_record, ProjectionModeTag::Expanded);
    assert_query_mode(&parameter_record, ProjectionModeTag::Expanded);
    assert_query_mode(&status_record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently preserves tuple-pattern conditional-infer aliases instead of binding each tuple slot; keep as the future tuple InferBind contract"]
fn conditional_infer_tuple_pattern_resolves_each_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/conditional-infer.ts", CONDITIONAL_INFER);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/conditional-infer.ts",
        "ConcreteTuplePair",
        &[],
        ProjectionMode::Expanded,
    );

    let pair = object_props(&expr);
    assert_ref(&pair["head"].ty, "ActionPayload");
    let tail = object_props(&pair["tail"].ty);
    assert_primitive(&tail["count"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn conditional_infer_function_return_resolves_object_result() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/conditional-infer.ts", CONDITIONAL_INFER);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/conditional-infer.ts",
        "ConcreteFunctionResult",
        &[],
        ProjectionMode::Expanded,
    );

    let result = object_props(&expr);
    assert_boolean_literal(&result["ok"].ty, true);
    assert_ref(&result["payload"].ty, "ActionPayload");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
