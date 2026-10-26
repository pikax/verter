//! @ai-generated - Synthetic value-level typeinfo inference tests.

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

/// The work one chain length spent: its cold member visits, cold body
/// inferences and the member visits of its warm read.
struct SpreadChainWork {
    levels: usize,
    visits: u64,
    evaluations: u64,
    warm_visits: u64,
}

/// Spread-return chains across `levels` modules for each listed length:
/// every module's literal survives, each body is inferred once, a warm read
/// reuses the projection, and the 20-module chain answers its edit and its
/// method-bearing receivers. Returns each length's work.
fn assert_cross_module_spread_chains(lengths: &[usize]) -> Vec<SpreadChainWork> {
    use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
    use verter_type_engine::semantic_query::object_spread_projection::test_support::work_counts;

    let mut doubled_control = None;
    let mut measured = Vec::new();
    for &levels in lengths {
        let host = make_host_with_footprint();
        for level in 0..levels {
            let source = if level == 0 {
                "export function f0() { return { k0: 0 as const }; }".to_string()
            } else {
                let previous = level - 1;
                format!(
                    "import {{ f{previous} }} from './m-{previous}';\n\
                     export function f{level}() {{ const p = f{previous}(); return {{ ...p, k{level}: {level} as const }}; }}"
                )
            };
            upsert_ts(&host, &format!("/fixtures/returns/m-{level}.ts"), &source);
        }
        upsert_ts(
            &host,
            "/fixtures/returns/scenario.ts",
            &format!(
                "import {{ f{last} }} from './m-{last}'; type Result = ReturnType<typeof f{last}>;",
                last = levels - 1
            ),
        );
        let before = flow_evaluations_for_tests();
        let work_before = work_counts();
        let (result, _) = resolve_expr(
            &host,
            "/fixtures/returns/scenario.ts",
            "Result",
            &[],
            ProjectionMode::Expanded,
        );
        let props = object_props(&result);
        assert_eq!(props.len(), levels, "every module contributes one property");
        for level in 0..levels {
            let property = &props[&format!("k{level}")];
            assert_number_literal(&property.ty, level as f64);
            assert!(!property.optional && !property.readonly);
        }
        let cold_evaluations = flow_evaluations_for_tests() - before;
        let cold_work = work_counts();
        let visits = cold_work.0 - work_before.0;
        let comparisons = cold_work.1 - work_before.1;
        eprintln!("{levels} links: {visits} member visits, {comparisons} collision comparisons");
        assert!(visits >= levels, "the production member path was observed");
        assert!(visits <= levels * 4, "only the demanded surface is copied");
        if levels == 20 {
            doubled_control = Some((visits, comparisons));
        } else if levels == 40 {
            let (base_visits, base_comparisons) = doubled_control.unwrap();
            assert!(
                visits <= base_visits * 3,
                "doubling links must keep member work linear"
            );
            assert!(
                comparisons <= base_comparisons * 3 + levels,
                "doubling links must keep collision work linear"
            );
        }
        assert_eq!(
            cold_evaluations, levels,
            "each function's body is inferred once"
        );
        let (warm, _) = resolve_expr(
            &host,
            "/fixtures/returns/scenario.ts",
            "Result",
            &[],
            ProjectionMode::Expanded,
        );
        assert_eq!(warm, result);
        assert_eq!(flow_evaluations_for_tests() - before, cold_evaluations);
        let warm_work = work_counts();
        assert_eq!(
            warm_work.1, cold_work.1,
            "warm reads reuse the inferred projection"
        );
        assert!(
            warm_work.0 - cold_work.0 <= levels,
            "warm consumer only materialises its final surface"
        );
        measured.push(SpreadChainWork {
            levels,
            visits: visits as u64,
            evaluations: cold_evaluations as u64,
            warm_visits: (warm_work.0 - cold_work.0) as u64,
        });
        if levels == 20 {
            upsert_ts(
                &host,
                "/fixtures/returns/m-0.ts",
                "export function f0() { return { k0: -1 as const }; }",
            );
            let (edited, _) = resolve_expr(
                &host,
                "/fixtures/returns/scenario.ts",
                "Result",
                &[],
                ProjectionMode::Expanded,
            );
            let props = object_props(&edited);
            assert_eq!(props.len(), levels);
            for level in 0..levels {
                assert_number_literal(
                    &props[&format!("k{level}")].ty,
                    if level == 0 { -1.0 } else { level as f64 },
                );
            }
            // A method-bearing consumer still needs the copied members in
            // its receiver, including source-order overwrites and getters.
            upsert_ts(
                &host,
                "/fixtures/returns/receiver.ts",
                "import { f19 } from './m-19';\n\
                 export function withMethod() { return { ...f19(), k0: 999 as const, read() { return this.k0 }, get first() { return this.k1 } }; }\n\
                 export function withFunction() { return { ...f19(), read: function() { return this.k0 } }; }\n\
                 type MethodReturn = ReturnType<ReturnType<typeof withMethod>['read']>;\n\
                 type GetterReturn = ReturnType<typeof withMethod>['first'];\n\
                 type FunctionReturn = ReturnType<ReturnType<typeof withFunction>['read']>;",
            );
            for (name, expected) in [
                ("MethodReturn", 999.0),
                ("GetterReturn", 1.0),
                ("FunctionReturn", -1.0),
            ] {
                let (value, _) = resolve_expr(
                    &host,
                    "/fixtures/returns/receiver.ts",
                    name,
                    &[],
                    ProjectionMode::Expanded,
                );
                assert_number_literal(&value, expected);
            }
        }
    }
    measured
}

/// The chains at 20, 40, 60 and 101 modules: the gate version of the 20-,
/// 40-, 100- and 300-module test in the nightly scale tier, with every one of
/// its checks at each length (the 20-module edit and receivers included, and
/// the doubling check on collision comparisons, which count hash collisions
/// and are not affine). The cold member visits, body inferences and warm
/// visits grow by the same amount per 20 modules, so they are affine in the chain,
/// and at 100 and 300 modules they meet the full-size test's per-length
/// bounds: between one and four member visits per module, one inference per
/// body, and a warm read within one visit per module. The 101-module chain is
/// past the checker's instantiation depth of 100.
#[test]
fn cross_module_spread_work_is_affine_and_meets_the_full_size_bounds() {
    use crate::scale_gate::{affine, Counter};
    let measured = assert_cross_module_spread_chains(&[20, 40, 60, 101]);
    let affine_lengths = [20, 40, 60];
    let counter = |name, read: fn(&SpreadChainWork) -> u64| Counter {
        name,
        values: measured[..3].iter().map(read).collect(),
    };
    assert_eq!(
        measured[..3]
            .iter()
            .map(|work| work.levels)
            .collect::<Vec<_>>(),
        affine_lengths
    );
    let work = affine(
        "spread-return chains",
        affine_lengths,
        vec![
            counter("member visits", |work| work.visits),
            counter("body inferences", |work| work.evaluations),
            counter("warm visits", |work| work.warm_visits),
        ],
    );
    for levels in [100, 300] {
        let visits = work.at("member visits", levels);
        assert!(
            visits >= levels as u64 && visits <= 4 * levels as u64,
            "{levels} links would visit {visits} members"
        );
        assert_eq!(work.at("body inferences", levels), levels as u64);
        assert!(work.at("warm visits", levels) <= levels as u64);
    }
}

#[test]
fn conditional_spread_return_chains_preserve_each_objects_union_boundary() {
    let mut control = None;
    for general in [true, false] {
        let host = make_host_with_footprint();
        let leading = if general { "...{}," } else { "" };
        let mut source =
            "export function f0(c: boolean) { return { x: 1 as const }; }\n".to_string();
        for level in 1..=10 {
            source.push_str(&format!(
                "export function f{level}(c: boolean) {{ return {{ {leading}...(c ? f{}(c) : {{ z: 2 as const }}), k{level}: {level} as const }}; }}\n",
                level - 1
            ));
        }
        source.push_str("type Result = ReturnType<typeof f10>;");
        let canonical = "/fixtures/conditional-spread-returns.ts";
        upsert_ts(&host, canonical, &source);
        let (result, _) = resolve_expr(&host, canonical, "Result", &[], ProjectionMode::Expanded);
        let TypeExpr::Union(arms) = &result else {
            panic!("finite spread alternatives must resolve, general={general}: {result:?}");
        };
        assert_eq!(arms.len(), 11);
        let mut shapes = Vec::new();
        for arm in arms.iter() {
            let props = object_props(arm);
            assert_number_literal(&props["k10"].ty, 10.0);
            for (name, property) in &props {
                assert!(!property.optional && !property.readonly);
                let expected = match name.as_str() {
                    "x" => 1.0,
                    "z" => 2.0,
                    _ => name.strip_prefix('k').unwrap().parse::<f64>().unwrap(),
                };
                assert_number_literal(&property.ty, expected);
            }
            shapes.push(props.keys().cloned().collect::<Vec<_>>());
        }
        shapes.sort();
        let mut expected = vec![{
            let mut keys = (1..=10).map(|i| format!("k{i}")).collect::<Vec<_>>();
            keys.push("x".to_string());
            keys.sort();
            keys
        }];
        for first in 1..=10 {
            let mut keys = (first..=10).map(|i| format!("k{i}")).collect::<Vec<_>>();
            keys.push("z".to_string());
            keys.sort();
            expected.push(keys);
        }
        expected.sort();
        assert_eq!(shapes, expected);
        if let Some(control) = &control {
            assert_eq!(&shapes, control, "composition preserves the general answer");
        } else {
            control = Some(shapes);
        }
    }
}

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

// RUNNING-UNRATIFIED: the flow-return substrate's `as const` value lowering
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

// TS7 contract: ComputedBlockValue = { state: true; count: number }.
// The callback body declares `const local = { ready: true as const, count: 3 }`.
//   - `local.ready` keeps the literal `true` because of `as const`.
//   - `local.count` widens to `number` (no `as const`, the `const` binding
//     does NOT make nested properties literal).
// The callback returns `{ state: local.ready, count: local.count }`, which
// therefore has type `{ state: true; count: number }`. T is inferred from
// that, so the final published shape preserves `state: true` and widens
// `count` to the primitive `number`.
#[oracle_row]
#[test]
#[ignore = "the flow lane does not yet carry block-bodied callback local bindings through the generic computed<T> helper (the block-callback family belongs to the value-inference lane); the carve-out row fails closed with an unknown node where tsgo publishes `{ state: true; count: number }`"]
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

/// PRIMARY, SEMANTIC — an object return's ENTRY FORM never costs the
/// literal its structural lowering, so a CALL-sourced spread inside it
/// still reduces.
///
/// The structural lowering handles each entry of an object return
/// separately: a spread whose source is a call rides the evaluator's one
/// call sink and reduces there. THREE entry forms used to abandon that
/// lowering for the WHOLE literal — a computed key (`[k]`), a numeric key
/// (`{ 1: x }`, whose authored text is not its property name), and a type
/// carrier (`as const` / `satisfies`) — and fold every sibling into one
/// shallow-pass leaf answer instead. That answer embeds the spread
/// callee's unreduced `ReturnType<…>` carrier, which the leaf's
/// fabricated-value gate refuses, so ONE unmodellable entry failed the
/// whole RETURN closed for values the checker types without difficulty.
///
/// The fix is at the lowering, not at the gate: a key whose property name
/// is not its authored text is its OWN lowered value position
/// (`SliceObjectKey::Computed`), and a type carrier over an object literal
/// contributes a MEMBER POLICY (`ObjectMemberPolicy`) rather than
/// swallowing the literal. The gate keeps refusing exactly what it
/// refused; nothing reaches it any more for these shapes.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`),
/// every row `Eq<…>`-probed with a negative control the checker REJECTS
/// (and the `Eq` probe is separately proven `readonly`-discriminating by
/// `Eq<{ readonly a: 1 }, { a: 1 }>` erroring).
///
/// Discrimination: restoring the whole-literal bail makes every
/// spread-bearing row resolve to the unmodelled-position marker instead of
/// an object; the OPEN-key row is the control that the computed-key
/// lowering did not simply start trusting keys it cannot name.
#[test]
fn object_return_entry_forms_lower_structurally_over_a_call_spread() {
    const SRC: &str = r#"
export function base() { return { label: "x" } }
export const k = "z";
export function mComputed() { return { ...base(), [k]: 1 } }
export function mNumeric() { return { ...base(), 1: 2 } }
export function mAsConst() { return { ...base(), n: 1 } as const }
export function mAsConstOnly() { return { ...base() } as const }
export function mSatisfies() { return { ...base(), n: 1 } satisfies object }
export function mBareComputed() { return { [k]: 1, a: 2 } }
export function mOpenComputed(key: string) { return { ...base(), [key]: 1 } }
export type TComputed = ReturnType<typeof mComputed>;
export type TNumeric = ReturnType<typeof mNumeric>;
export type TAsConst = ReturnType<typeof mAsConst>;
export type TAsConstOnly = ReturnType<typeof mAsConstOnly>;
export type TSatisfies = ReturnType<typeof mSatisfies>;
export type TBareComputed = ReturnType<typeof mBareComputed>;
export type TOpenComputed = ReturnType<typeof mOpenComputed>;
"#;
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/entry-forms.ts", SRC);

    let resolve = |name: &str| {
        resolve_expr(
            &host,
            "/fixtures/entry-forms.ts",
            name,
            &[],
            ProjectionMode::Expanded,
        )
        .0
    };

    // A COMPUTED key resolves through its own value: `k` is `"z"`, so the
    // member is `z`. The spread reduced, so `label` is present and exact.
    // Checker: `{ label: string; z: number }`.
    let computed = object_props(&resolve("TComputed"));
    assert_eq!(
        computed.keys().collect::<Vec<_>>(),
        vec!["label", "z"],
        "the spread's member and the computed-key member both survive"
    );
    assert_primitive(&computed["label"].ty, PrimitiveName::String);
    assert_primitive(&computed["z"].ty, PrimitiveName::Number);

    // A NUMERIC key takes the SHARED canonical numeric spelling, so it is
    // the number `1` rather than the string `"1"`. Checker:
    // `{ 1: number; label: string }`.
    let numeric = resolve("TNumeric");
    let TypeExpr::Object(numeric_object) = &numeric else {
        panic!("a numeric key does not stop the literal lowering structurally: {numeric:?}");
    };
    let numeric_keys: Vec<_> = numeric_object
        .properties
        .iter()
        .filter_map(|member| match member {
            verter_type_expr::ObjectMember::Property(property) => Some(property.key.clone()),
            _ => None,
        })
        .collect();
    assert!(
        numeric_keys.iter().any(|key| matches!(
            key,
            verter_type_expr::AuthoredPropertyKey::Number(index) if index.get() == 1
        )),
        "`{{ 1: 2 }}` names the NUMBER 1, not the string \"1\": {numeric_keys:?}"
    );
    assert!(
        numeric_keys.iter().any(|key| matches!(
            key,
            verter_type_expr::AuthoredPropertyKey::String(name) if name.as_ref() == "label"
        )),
        "and the spread's member survives beside it: {numeric_keys:?}"
    );

    // `as const` pins the literal AND marks the literal's own members
    // `readonly`. Checker: `{ readonly label: string; readonly n: 1 }`.
    let as_const = object_props(&resolve("TAsConst"));
    assert_eq!(as_const.keys().collect::<Vec<_>>(), vec!["label", "n"]);
    assert_primitive(&as_const["label"].ty, PrimitiveName::String);
    assert_eq!(
        as_const["n"].ty,
        TypeExpr::Literal(verter_type_expr::LiteralValue::Number(1.0)),
        "`as const` pins the member literal instead of widening it to `number`"
    );
    assert!(
        as_const["n"].readonly,
        "and marks the literal's own member `readonly`"
    );
    // The const context copies the SPREAD-contributed member `readonly`
    // too, as the checker does.
    assert!(
        as_const["label"].readonly,
        "the spread-contributed member takes the enclosing `as const`'s `readonly` \
         (checker: `readonly label: string`)"
    );

    let as_const_only = object_props(&resolve("TAsConstOnly"));
    assert_eq!(as_const_only.keys().collect::<Vec<_>>(), vec!["label"]);
    assert_primitive(&as_const_only["label"].ty, PrimitiveName::String);

    // `satisfies T` keeps the operand's SOURCE type. Whether a fresh
    // member literal survives in that source type depends on whether the
    // TARGET contextually types it, and this substrate performs no
    // contextual typing — so it takes the PRESERVING side uniformly,
    // which is the shared shallow pass's own long-standing choice (a
    // carrier that lowers structurally and one that reaches the leaf
    // lowering must not disagree about the same literal). The member SET
    // and the spread's reduction — the things this test is about — are
    // exact either way.
    //
    // The satisfies target contextually types the members: `{ ...base(),
    // n: 1 } satisfies object` is `{ label: string; n: number }` because
    // `object` contextually types nothing, while a literal target pins
    // (`{ mode: "dark" } satisfies { mode: "dark" | "light" }` is
    // `{ mode: "dark" }`, which `flow_return_catalog::
    // flow_return_ob05_satisfies_preserves_value_shape` pins).
    let satisfies = object_props(&resolve("TSatisfies"));
    assert_eq!(satisfies.keys().collect::<Vec<_>>(), vec!["label", "n"]);
    assert_primitive(&satisfies["label"].ty, PrimitiveName::String);
    assert_primitive(&satisfies["n"].ty, PrimitiveName::Number);
    assert!(
        !satisfies["n"].readonly,
        "`satisfies` is not a const assertion and mints no `readonly`"
    );

    // A computed key with NO spread is the same rule without the spread —
    // proving the key lowering is not a spread-specific patch.
    let bare = object_props(&resolve("TBareComputed"));
    assert_eq!(bare.keys().collect::<Vec<_>>(), vec!["a", "z"]);

    // CONTROL — an OPEN key domain still fails the literal CLOSED. A key
    // whose value is not a literal provisions a property the surface
    // cannot name, and an object surface has no way to say "these keys,
    // plus one more I cannot name"; publishing the modelled siblings alone
    // would declare a member set missing a key the authored value has.
    let open = resolve("TOpenComputed");
    assert!(
        matches!(&open, TypeExpr::Unknown(value) if value.raw() == "unmodeledPosition"),
        "a key whose value is not a literal leaves the surface's key SET unknown and must \
         fail closed, not publish `{{ label }}` as if the literal had one member: {open:?}"
    );
}

/// Full-size workloads, run by the nightly scale tier (`.config/nextest.toml`).
/// Each has a gate version outside this module.
mod nightly_scale {
    use super::*;

    #[test]
    fn cross_module_spread_returns_preserve_every_literal_and_reuse_each_body() {
        assert_cross_module_spread_chains(&[20, 40, 100, 300]);
    }
}
