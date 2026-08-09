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
#[ignore = "the substrate's branch join composes the per-arm return objects, but the text arm's `value` stays `string | number` — `typeof` narrowing is a separate mechanism that has not landed; keep as the future flow-sensitive value inference contract"]
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
#[ignore = "`computed<T>(() => ...)` infers `T` from the callback's return body and the published shape carries the asserted `id`/`count`/`nested.ready` slots; the row PASSES under --include-ignored; it stays ignored because it has no `ORACLE_QUERY_SPECS` seat: `ProofRequirement::Ts7Oracle` requires a registry entry, a vendored source, a checked-in tsgo snapshot, and retained lift-migration provenance from the audited lift command. Lift under U6.FLOW_RETURN_SUBSTRATE when the row is seated"]
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
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict --ignoreConfig`),
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
    // KNOWN DIVERGENCE, asserted so it cannot drift unnoticed: the checker
    // marks the SPREAD-contributed member `readonly` too, and this
    // substrate does not — the spread program merges the source's members
    // with the source's own modifiers. That is the existing
    // spread-path `readonly` gap reached by one more shape, not a new
    // class; the member set and every member TYPE are exact.
    assert!(
        !as_const["label"].readonly,
        "the spread-contributed member does NOT yet take the enclosing `as const`'s \
         `readonly` (checker: `readonly label: string`) — flip this assertion when the \
         spread path carries the modifier"
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
    // RECORDED DIVERGENCE: the checker widens here (`{ ...base(), n: 1 }
    // satisfies object` is `{ label: string; n: number }`, because
    // `object` contextually types nothing) and pins where the target does
    // (`{ mode: "dark" } satisfies { mode: "dark" | "light" }` is
    // `{ mode: "dark" }`, which `flow_return_catalog::
    // flow_return_ob05_satisfies_preserves_value_shape` pins). Closing the
    // split is the deferred contextual-widening contract, and it moves
    // BOTH rows together.
    let satisfies = object_props(&resolve("TSatisfies"));
    assert_eq!(satisfies.keys().collect::<Vec<_>>(), vec!["label", "n"]);
    assert_primitive(&satisfies["label"].ty, PrimitiveName::String);
    assert_eq!(
        satisfies["n"].ty,
        TypeExpr::Literal(verter_type_expr::LiteralValue::Number(1.0)),
        "`satisfies` preserves the member literal uniformly — the target-driven half of \
         tsc's rule is the deferred contextual-widening contract"
    );
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
