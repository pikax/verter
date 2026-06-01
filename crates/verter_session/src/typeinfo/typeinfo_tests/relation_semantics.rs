//! @ai-generated - Relation-engine contracts via `A extends B ? T : F`.
//!
//! Each test instantiates one conditional probe whose published value
//! (`"yes"` / `"no"` / `never` / a structural shape) tells us exactly
//! which branch the relation engine picked. Synthetic generic names —
//! no library references. The TS7 emissions encoded here were verified
//! out-of-band against `tsgo` with `IsExactly<TestType, ExpectedShape>`
//! probes.

use super::support::*;

const RELATION_SEMANTICS: &str = include_str!("fixtures/relation_semantics.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/relation_semantics.ts", RELATION_SEMANTICS);
}

// =====================================================================
// Row 1: Top / bottom / unknown rows.
// =====================================================================

#[test]
#[ignore = "typeinfo currently selects only the True branch (`\"yes\"`) for `any extends string ? \"yes\" : \"no\"` instead of distributing across both branches; keep as the future `any`-distribution relation contract"]
fn relation_any_extends_string_distributes_both_branches() {
    // TS7 contract: `any extends string ? "yes" : "no"` distributes the
    // `any` check across both branches, emitting the union `"yes" | "no"`.
    // This is TS7's special-case for `any` — the check type is treated as
    // "any could be anything" and both branches survive simultaneously,
    // even though the check site is NOT a bare type parameter.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "AnyExtendsString",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["yes", "no"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_unknown_extends_string_selects_false_branch() {
    // TS7 contract: `unknown extends string ? "yes" : "no"` reduces to
    // `"no"` — `unknown` is the top type and is not assignable to `string`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "UnknownExtendsString",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "no");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_never_extends_string_directly_selects_true_branch() {
    // TS7 contract: `never extends string ? "yes" : "no"` (DIRECT —
    // not via a generic helper) reduces to `"yes"`. The check site is a
    // concrete `never`, NOT a bare type parameter, so distribution does
    // not apply; `never` is the bottom type and is assignable to every
    // type, so the True branch wins.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "NeverExtendsStringDirect",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently emits `\"no\"` for `IsStringDistributive<never>` instead of collapsing the distributive conditional to `never` when the bare type parameter has no constituents; keep as the future never-distribution relation contract"]
fn relation_never_via_generic_helper_collapses_to_never() {
    // TS7 contract: `IsStringDistributive<never>` where the helper is
    // `T extends string ? "yes" : "no"` distributes over the bare type
    // parameter `T`. `never` has zero constituents to distribute over,
    // so the distributive conditional collapses to `never` itself.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "NeverExtendsStringViaGeneric",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Never);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_string_extends_any_selects_true_branch() {
    // TS7 contract: `string extends any ? "yes" : "no"` reduces to `"yes"`.
    // Every type is assignable to `any`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "StringExtendsAny",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_string_extends_unknown_selects_true_branch() {
    // TS7 contract: `string extends unknown ? "yes" : "no"` reduces to
    // `"yes"`. Every type is assignable to `unknown` (the top type).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "StringExtendsUnknown",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_string_extends_never_selects_false_branch() {
    // TS7 contract: `string extends never ? "yes" : "no"` reduces to
    // `"no"`. `never` is the bottom type — nothing extends it except
    // `never` itself.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "StringExtendsNever",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "no");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// =====================================================================
// Row 2: Optional property assignability.
// =====================================================================

#[test]
fn relation_required_property_assignable_to_optional() {
    // TS7 contract: `{ a: string } extends { a?: string } ? "yes" : "no"`
    // reduces to `"yes"`. A required property satisfies an optional slot
    // — the value is always present, so the "missing" case never arises.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "RequiredToOptional",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently treats `{ a?: string } extends { a: string }` as assignable (returning `\"yes\"`); the relation engine must reject optional-to-required because the producer slot may be absent"]
fn relation_optional_property_not_assignable_to_required() {
    // TS7 contract: `{ a?: string } extends { a: string } ? "yes" : "no"`
    // reduces to `"no"`. An optional property may be absent, so the
    // required slot's invariant fails.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "OptionalToRequired",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "no");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_empty_object_assignable_to_all_optional() {
    // TS7 contract: `{} extends { a?: string } ? "yes" : "no"` reduces to
    // `"yes"`. The empty object satisfies a fully-optional shape because
    // every slot may be absent.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "EmptyToAllOptional",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// =====================================================================
// Row 3: Readonly property assignability.
// =====================================================================

#[test]
fn relation_mutable_property_assignable_to_readonly() {
    // TS7 contract: `{ a: string } extends { readonly a: string } ? "yes"
    // : "no"` reduces to `"yes"`. A mutable slot satisfies a readonly
    // requirement (readonly is a contract on the consumer, not the
    // producer).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "MutableToReadonly",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently rejects `{ readonly a: string } extends { a: string }` (returning `\"no\"`); TypeScript's structural relation accepts readonly-to-mutable, so the relation engine must drop readonly enforcement on the producer side"]
fn relation_readonly_property_assignable_to_mutable() {
    // TS7 contract: `{ readonly a: string } extends { a: string } ? "yes"
    // : "no"` reduces to `"yes"`. Structural subtyping in TypeScript does
    // NOT enforce readonly bidirectionally — a readonly producer satisfies
    // a mutable consumer (this is a well-known soundness hole). Both
    // directions yield "yes".
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "ReadonlyToMutable",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// =====================================================================
// Row 4: Function parameter contravariance.
// =====================================================================

#[test]
fn relation_function_with_wider_param_assignable_to_narrower_target() {
    // TS7 contract: `((x: "a" | "b") => void) extends ((x: "a") => void)
    // ? "yes" : "no"` reduces to `"yes"`. Function parameters are
    // contravariant: a function accepting a wider parameter set is
    // assignable to one expecting a narrower set (because the wider
    // function still accepts every input the narrower target promises).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "WiderParamToNarrower",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_function_with_narrower_param_not_assignable_to_wider_target() {
    // TS7 contract: `((x: "a") => void) extends ((x: "a" | "b") => void)
    // ? "yes" : "no"` reduces to `"no"`. The narrower-parameter function
    // would crash when called with `"b"`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "NarrowerParamToWider",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "no");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// =====================================================================
// Row 5: Tuple length / rest assignability.
// =====================================================================

#[test]
#[ignore = "typeinfo currently rejects `[string, number] extends [string, ...unknown[]]` (returning `\"no\"`); the relation engine must accept a fixed tuple whose tail satisfies a `...unknown[]` rest slot"]
fn relation_fixed_tuple_assignable_to_first_plus_rest() {
    // TS7 contract: `[string, number] extends [string, ...unknown[]] ?
    // "yes" : "no"` reduces to `"yes"`. The fixed tuple satisfies the
    // first-element check and contributes its tail into the `...unknown[]`
    // rest slot.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "FixedToFirstRest",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_rest_tuple_not_assignable_to_fixed_tuple() {
    // TS7 contract: `[string, ...number[]] extends [string, number] ?
    // "yes" : "no"` reduces to `"no"`. The rest tuple may have 1 to N
    // elements; the fixed-length 2-tuple target requires exactly 2
    // elements — the producer cannot guarantee that.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "RestToFixed",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "no");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_one_tuple_assignable_to_one_with_optional_second_slot() {
    // TS7 contract: `[string] extends [string, number?] ? "yes" : "no"`
    // reduces to `"yes"`. The 1-tuple satisfies the first slot and the
    // optional second slot may be absent.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "OneToOneOptional",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_empty_tuple_assignable_to_readonly_array() {
    // TS7 contract: `[] extends readonly string[] ? "yes" : "no"` reduces
    // to `"yes"`. The empty tuple is assignable to any array type because
    // it has zero elements and therefore no element-type mismatch.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "EmptyToReadonlyArray",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// =====================================================================
// Row 6: Union distribution vs non-distribution.
// =====================================================================

#[test]
#[ignore = "typeinfo currently selects only the False branch (`\"no\"`) for `IsStringDistributive<string | number>` instead of distributing the union across the conditional and emitting `\"yes\" | \"no\"`; keep as the future bare-type-parameter distribution contract"]
fn relation_distributive_conditional_over_union_emits_branch_union() {
    // TS7 contract: `IsStringDistributive<string | number>` where the
    // helper is `T extends string ? "yes" : "no"` distributes the union
    // across the conditional. `string` selects True; `number` selects
    // False. Result: `"yes" | "no"`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "DistributiveOverUnion",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["yes", "no"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_tuple_wrapped_conditional_over_union_does_not_distribute() {
    // TS7 contract: `IsStringNonDistributive<string | number>` where the
    // helper is `[T] extends [string] ? "yes" : "no"` wraps the check
    // type-parameter in a 1-tuple, suppressing union distribution. The
    // relation engine then asks `[string | number] extends [string]`,
    // which fails because `number` is not assignable to `string`.
    // Result: `"no"` (not a union).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "NonDistributiveOverUnion",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "no");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// =====================================================================
// Row 7: Intersection assignability.
// =====================================================================

#[test]
fn relation_intersection_assignable_to_one_base_arm() {
    // TS7 contract: `{ a: string } & { b: number } extends { a: string }
    // ? "yes" : "no"` reduces to `"yes"`. An intersection carries every
    // member of every arm; it is structurally at-least-base.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "IntersectionExtendsBase",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_one_arm_not_assignable_to_intersection() {
    // TS7 contract: `{ a: string } extends { a: string } & { b: number }
    // ? "yes" : "no"` reduces to `"no"`. The intersection target requires
    // both `a` and `b`; the producer has only `a`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "BaseExtendsIntersection",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "no");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// =====================================================================
// Row 8: `infer` bindings.
// =====================================================================

#[test]
#[ignore = "typeinfo currently preserves the conditional `{ value: number } extends { value: infer V } ? V : never` unevaluated instead of binding `V = number` and selecting the True branch; keep as the future InferBind contract for object-property patterns"]
fn relation_infer_value_of_object_property() {
    // TS7 contract: `{ value: number } extends { value: infer V } ? V :
    // never` binds `V = number` via the relation engine's `InferBind`
    // hook. The selected True branch returns the bound V.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "InferValueOfObject",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently emits `never` for `[1, 2, 3] extends [infer H, ...unknown[]] ? H : never` instead of binding `H = 1`; keep as the future InferBind contract for tuple-head patterns"]
fn relation_infer_head_of_tuple_pattern() {
    // TS7 contract: `[1, 2, 3] extends [infer H, ...unknown[]] ? H :
    // never` binds `H = 1` (the first element literal).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "InferHeadOfTuple",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 1.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently emits `never` for `[1, 2, 3] extends [unknown, ...infer R] ? R : never` instead of binding `R = [2, 3]`; keep as the future InferBind contract for tuple-tail rest patterns"]
fn relation_infer_tail_of_tuple_pattern() {
    // TS7 contract: `[1, 2, 3] extends [unknown, ...infer R] ? R : never`
    // binds `R = [2, 3]` (the remaining tuple after consuming the head).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "InferTailOfTuple",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 2);
    assert_number_literal(&elements[0].ty, 2.0);
    assert_number_literal(&elements[1].ty, 3.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_infer_return_of_function() {
    // TS7 contract: `(() => "hello") extends (...args: any[]) => infer R
    // ? R : never` binds `R = "hello"` from the function's declared
    // return type.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "InferReturnOfFunction",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "hello");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently reduces `((x: string, y?: number) => void) extends (...args: infer A) => any ? A : never` to `string` instead of binding `A` to the full parameter tuple; keep as the future InferBind contract for variadic parameter-tuple patterns"]
fn relation_infer_params_of_function_preserves_optional_undefined() {
    // TS7 contract: `((x: string, y?: number) => void) extends (...args:
    // infer A) => any ? A : never` binds `A = [x: string, y?: number |
    // undefined]`. The optional parameter contributes its `| undefined`
    // through the tuple inference path.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "InferParamsOfFunction",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0].label.as_deref(), Some("x"));
    assert!(!elements[0].optional);
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_eq!(elements[1].label.as_deref(), Some("y"));
    assert!(elements[1].optional);
    assert_union_contains_primitive(&elements[1].ty, PrimitiveName::Number);
    assert_union_contains_primitive(&elements[1].ty, PrimitiveName::Undefined);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn relation_infer_single_param_of_function() {
    // TS7 contract: `((s: string) => void) extends (x: infer X) => any
    // ? X : never` binds `X = string` (the single declared parameter
    // type).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/relation_semantics.ts",
        "InferSingleParam",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
