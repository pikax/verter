//! @ai-generated - Synthetic value-level typeinfo inference tests.

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

fn upsert_value_fixture(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/value-inference.ts", VALUE_INFERENCE);
}

#[test]
fn value_inference_regular_variables_resolve_typeof_aliases_and_scratch_expressions() {
    let host = make_host_with_footprint();
    upsert_value_fixture(&host);

    let (literal, literal_record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "LiteralConstType",
        &[],
        ProjectionMode::Expanded,
    );
    assert_string_literal(&literal, "ready");

    let (number, number_record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "NumberConstType",
        &[],
        ProjectionMode::Expanded,
    );
    assert_number_literal(&number, 42.0);

    let (label, label_record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "MutableLabelType",
        &[],
        ProjectionMode::Expanded,
    );
    assert_primitive(&label, PrimitiveName::String);

    let (count, count_record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "MutableCountType",
        &[],
        ProjectionMode::Expanded,
    );
    assert_primitive(&count, PrimitiveName::Number);

    let (scratch, scratch_record) = evaluate_expr(
        &host,
        "/fixtures/value-inference.ts",
        "typeof literalConst",
        ProjectionMode::Expanded,
    );
    assert_string_literal(&scratch, "ready");

    assert_query_mode(&literal_record, ProjectionModeTag::Expanded);
    assert_query_mode(&number_record, ProjectionModeTag::Expanded);
    assert_query_mode(&label_record, ProjectionModeTag::Expanded);
    assert_query_mode(&count_record, ProjectionModeTag::Expanded);
    assert_query_mode(&scratch_record, ProjectionModeTag::Expanded);
}

// RUNNING-UNRATIFIED: the U6.FLOW_RETURN_SUBSTRATE `as const` value lowering
// projects `typeof objectConst` with its readonly literal tuple `list` and
// nested literal members — the pre-substrate tree lowered the list to a
// mutable `Array<1 | 2 | 3>`, so the former `#[ignore]` reason no longer
// holds. NOT oracle-liftable today: this ORIGINAL two-query body carries
// control flow around its second query, which the closed migration extractor
// rejects (`ControlFlowAroundQuery`), and a rewritten-to-extract body would
// not be the frozen original the provenance rail anchors on.
#[test]
fn value_inference_const_object_literal_expands_nested_shape() {
    let host = make_host_with_footprint();
    upsert_value_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "ObjectConstType",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["id", "list", "nested"]);
    assert_string_literal(&props["id"].ty, "item");
    let nested = object_props(&props["nested"].ty);
    assert_boolean_literal(&nested["flag"].ty, true);
    assert_number_literal(&nested["value"].ty, 7.0);
    let TypeExpr::Tuple { elements, readonly } = &props["list"].ty else {
        panic!(
            "expected readonly tuple for const list, got {:?}",
            props["list"].ty
        );
    };
    assert!(*readonly);
    assert_eq!(elements.len(), 3);
    assert_number_literal(&elements[0].ty, 1.0);
    assert_number_literal(&elements[1].ty, 2.0);
    assert_number_literal(&elements[2].ty, 3.0);

    let (nested_expr, nested_record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "ObjectNestedType",
        &[],
        ProjectionMode::Expanded,
    );
    let nested_props = object_props(&nested_expr);
    assert_eq!(prop_names(&nested_props), vec!["flag", "value"]);
    assert_boolean_literal(&nested_props["flag"].ty, true);
    assert_number_literal(&nested_props["value"].ty, 7.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    assert_query_mode(&nested_record, ProjectionModeTag::Expanded);
}

#[test]
fn value_inference_static_member_expression_typeof_path_resolves_terminal() {
    let host = make_host_with_footprint();
    upsert_value_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "DerivedValueType",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 7.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `ReturnType<typeof bodyReturn>` solves the two-return-site body
// through the demand-sliced FlowReturn dispatch to the exact per-arm union —
// `as const` `state` discriminants preserved, `value` widened to `number` at
// return position. Registry-keyed `oracle::run_row` body against the
// checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn value_inference_function_body_return_union_from_return_statements() {}

// LIFTED: TS7 contract — directArrow is inferred as
// `(input: string, count?: number) => { input: string; count: number |
// undefined; ok: boolean }`. The `ok: true` literal widens to `boolean` at
// return position (no contextual type, no `as const`); the pre-substrate tree
// returned a semantic miss for the arrow body. Registry-keyed
// `oracle::run_row` body against the checked-in tsgo snapshot. The companion
// row `value_inference_arrow_expression_body_substitutes_parameter_references`
// pins the `input`/`count` parameter-substitution side of the same contract.
#[oracle_row]
#[test]
fn value_inference_arrow_expression_body_publishes_return_shape() {}

// LIFTED: the parameter-substitution side of the arrow-body contract —
// `input` substitutes its `string` annotation; optional `count` injects
// `number | undefined`. Registry-keyed `oracle::run_row` body against the
// checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn value_inference_arrow_expression_body_substitutes_parameter_references() {}

#[test]
#[ignore = "the substrate's branch join composes the per-arm return objects, but the text arm's `value` stays `string | number` — narrowing `typeof current === \"string\"` is the U6.NARROW_TYPEOF mechanism, not yet landed; keep as the future flow-sensitive value inference contract"]
fn value_inference_flow_variables_narrow_return_value_by_branch() {
    let host = make_host_with_footprint();
    upsert_value_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "FlowReturnType",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Union(types) = &expr else {
        panic!("expected flow return union, got {expr:?}");
    };
    let text_arm = types
        .iter()
        .find(|ty| {
            let TypeExpr::Object(_) = ty else {
                return false;
            };
            let props = object_props(ty);
            matches!(props.get("kind"), Some(prop) if matches!(&prop.ty, TypeExpr::Literal(verter_type_expr::LiteralValue::String(value)) if value == "text"))
        })
        .expect("text branch arm");
    let text_props = object_props(text_arm);
    assert_primitive(&text_props["value"].ty, PrimitiveName::String);

    let number_arm = types
        .iter()
        .find(|ty| {
            let TypeExpr::Object(_) = ty else {
                return false;
            };
            let props = object_props(ty);
            matches!(props.get("kind"), Some(prop) if matches!(&prop.ty, TypeExpr::Literal(verter_type_expr::LiteralValue::String(value)) if value == "number"))
        })
        .expect("number branch arm");
    let number_props = object_props(number_arm);
    assert_primitive(&number_props["value"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not infer generic call-site type arguments from callback return bodies for computed<T>(() => ...); keep as the future callback-driven generic call inference contract"]
fn value_inference_computed_callback_object_value_resolves_from_callback_body() {
    // TS7 contract: ComputedObjectValue =
    //   { id: "computed"; count: number; nested: { ready: boolean } }
    // The callback returns an object literal:
    //   ({ id: "computed" as const, count: 2, nested: { ready: true } })
    // TS infers T from the callback return. `id` keeps its literal because of
    // `as const`. `count: 2` and `nested.ready: true` have no `as const` and
    // no contextual constraint, so they widen to `number` and `boolean` per
    // TS's standard inferred-property widening at generic inference sites.
    let host = make_host_with_footprint();
    upsert_value_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "ComputedObjectValue",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count", "id", "nested"]);
    assert_string_literal(&props["id"].ty, "computed");
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    let nested = object_props(&props["nested"].ty);
    assert_primitive(&nested["ready"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not carry block-bodied callback local variables and return object inference through computed<T>; keep as the future block-callback generic inference contract"]
fn value_inference_computed_block_callback_value_resolves_local_return_shape() {
    // TS7 contract: ComputedBlockValue = { state: true; count: number }.
    // The callback body declares `const local = { ready: true as const, count: 3 }`.
    //   - `local.ready` keeps the literal `true` because of `as const`.
    //   - `local.count` widens to `number` (no `as const`, the `const` binding
    //     does NOT make nested properties literal).
    // The callback returns `{ state: local.ready, count: local.count }`, which
    // therefore has type `{ state: true; count: number }`. T is inferred from
    // that, so the final published shape preserves `state: true` and widens
    // `count` to the primitive `number`.
    let host = make_host_with_footprint();
    upsert_value_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/value-inference.ts",
        "ComputedBlockValue",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count", "state"]);
    assert_boolean_literal(&props["state"].ty, true);
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
