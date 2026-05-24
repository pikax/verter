//! @ai-generated - Built-in utility-type contracts over top / bottom
//! inputs.
//!
//! Codifies TS7's behaviour when `ReturnType`, `Parameters`,
//! `ConstructorParameters`, `InstanceType`, `Awaited`, `NonNullable`,
//! `Extract`, and `Exclude` are applied to `any` / `unknown` / `never` /
//! `null` / `undefined` / `void`. These are the edge inputs where regular
//! object behaviour can pass but degenerate inputs reveal dispatch gaps.

use super::support::*;

const UTILITY_TOP_BOTTOM: &str = include_str!("fixtures/utility_top_bottom.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/utility_top_bottom.ts", UTILITY_TOP_BOTTOM);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility_top_bottom.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// =====================================================================
// ReturnType matrix
// =====================================================================

// TS7: `ReturnType<any>` = `any`. The conditional distributes over `any`,
// both branches contribute, the merged result collapses to `any`.
#[test]
#[ignore = "typeinfo currently does not reduce `ReturnType<any>` to `any`; keep as the future Utb01 ReturnType-any contract"]
fn utility_top_bottom_utb01_return_type_of_any_is_any() {
    let expr = resolve_alias("Utb01ReturnTypeOfAny");
    assert_primitive(&expr, PrimitiveName::Any);
}

// TS7: `ReturnType<never>` = `never`. Distribution over `never` collapses
// to `never`.
#[test]
#[ignore = "typeinfo currently does not reduce `ReturnType<never>` to `never`; keep as the future Utb02 ReturnType-never contract"]
fn utility_top_bottom_utb02_return_type_of_never_is_never() {
    let expr = resolve_alias("Utb02ReturnTypeOfNever");
    assert_primitive(&expr, PrimitiveName::Never);
}

// TS7: `ReturnType<() => any>` = `any`.
#[test]
fn utility_top_bottom_utb03_return_type_any_arrow_is_any() {
    let expr = resolve_alias("Utb03ReturnTypeAnyArrow");
    assert_primitive(&expr, PrimitiveName::Any);
}

// TS7: `ReturnType<() => never>` = `never`.
#[test]
fn utility_top_bottom_utb04_return_type_never_arrow_is_never() {
    let expr = resolve_alias("Utb04ReturnTypeNeverArrow");
    assert_primitive(&expr, PrimitiveName::Never);
}

// TS7: `ReturnType<() => unknown>` = `unknown`.
#[test]
fn utility_top_bottom_utb05_return_type_unknown_arrow_is_unknown() {
    let expr = resolve_alias("Utb05ReturnTypeUnknownArrow");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// TS7: `ReturnType<() => void>` = `void`.
#[test]
fn utility_top_bottom_utb06_return_type_void_arrow_is_void() {
    let expr = resolve_alias("Utb06ReturnTypeVoidArrow");
    assert_primitive(&expr, PrimitiveName::Void);
}

// =====================================================================
// Parameters matrix
// =====================================================================

// TS7: `Parameters<any>` = `unknown[]`. When `T` is `any`, the conditional
// constraint check yields the inferred tuple type from
// `(...args: any) => any`'s rest-args slot = `unknown[]`. This is one of
// the well-known trap cases: it is NOT `any` and NOT `never`.
#[test]
#[ignore = "typeinfo currently does not reduce `Parameters<any>` to `unknown[]`; keep as the future Utb07 Parameters-any contract"]
fn utility_top_bottom_utb07_parameters_of_any_is_unknown_array() {
    let expr = resolve_alias("Utb07ParametersOfAny");
    assert_array_of_primitive(&expr, PrimitiveName::Unknown);
}

// TS7: `Parameters<never>` = `never`.
#[test]
#[ignore = "typeinfo currently does not reduce `Parameters<never>` to `never`; keep as the future Utb08 Parameters-never contract"]
fn utility_top_bottom_utb08_parameters_of_never_is_never() {
    let expr = resolve_alias("Utb08ParametersOfNever");
    assert_primitive(&expr, PrimitiveName::Never);
}

// TS7: `Parameters<(x: any) => void>` = `[x: any]` (a one-element tuple
// whose sole element is `any`).
#[test]
fn utility_top_bottom_utb09_parameters_any_arg_is_singleton_tuple_of_any() {
    let expr = resolve_alias("Utb09ParametersAnyArg");
    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected one-element tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_primitive(&elements[0].ty, PrimitiveName::Any);
}

// TS7: `Parameters<(x: never) => void>` = `[x: never]`.
#[test]
fn utility_top_bottom_utb10_parameters_never_arg_is_singleton_tuple_of_never() {
    let expr = resolve_alias("Utb10ParametersNeverArg");
    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected one-element tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_primitive(&elements[0].ty, PrimitiveName::Never);
}

// =====================================================================
// ConstructorParameters / InstanceType
// =====================================================================

// TS7: `ConstructorParameters<any>` = `unknown[]` (the inferred
// rest-tuple slot of `new (...args: any) => any`).
#[test]
#[ignore = "typeinfo currently does not reduce `ConstructorParameters<any>` to `unknown[]`; keep as the future Utb11 ConstructorParameters-any contract"]
fn utility_top_bottom_utb11_constructor_parameters_any_is_unknown_array() {
    let expr = resolve_alias("Utb11ConstructorParametersAny");
    assert_array_of_primitive(&expr, PrimitiveName::Unknown);
}

// TS7: `InstanceType<any>` = `any`.
#[test]
#[ignore = "typeinfo currently does not reduce `InstanceType<any>` to `any`; keep as the future Utb12 InstanceType-any contract"]
fn utility_top_bottom_utb12_instance_type_any_is_any() {
    let expr = resolve_alias("Utb12InstanceTypeAny");
    assert_primitive(&expr, PrimitiveName::Any);
}

// TS7: `ConstructorParameters<new (...args: any[]) => any>` = `any[]`.
#[test]
#[ignore = "typeinfo currently does not project the rest-args slot of a constructor signature to `any[]`; keep as the future Utb13 ConstructorParameters-any-ctor contract"]
fn utility_top_bottom_utb13_constructor_parameters_any_ctor_is_any_array() {
    let expr = resolve_alias("Utb13ConstructorParametersAnyCtor");
    assert_array_of_primitive(&expr, PrimitiveName::Any);
}

// =====================================================================
// Awaited matrix
// =====================================================================

// TS7: `Awaited<any>` = `any` (Awaited's conditional distributes over `any`).
#[test]
#[ignore = "typeinfo currently does not reduce `Awaited<any>` to `any`; keep as the future Utb14 Awaited-any contract"]
fn utility_top_bottom_utb14_awaited_any_is_any() {
    let expr = resolve_alias("Utb14AwaitedAny");
    assert_primitive(&expr, PrimitiveName::Any);
}

// TS7: `Awaited<unknown>` = `unknown` (no Promise / thenable branch
// matches; the final fallthrough returns `T`).
#[test]
#[ignore = "typeinfo currently does not reduce `Awaited<unknown>` to `unknown`; keep as the future Utb15 Awaited-unknown contract"]
fn utility_top_bottom_utb15_awaited_unknown_is_unknown() {
    let expr = resolve_alias("Utb15AwaitedUnknown");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// TS7: `Awaited<never>` = `never` (distribution over `never` collapses).
#[test]
#[ignore = "typeinfo currently does not reduce `Awaited<never>` to `never`; keep as the future Utb16 Awaited-never contract"]
fn utility_top_bottom_utb16_awaited_never_is_never() {
    let expr = resolve_alias("Utb16AwaitedNever");
    assert_primitive(&expr, PrimitiveName::Never);
}

// TS7: `Awaited<null>` = `null`. The first conditional clause
// `T extends null | undefined ? T : ...` short-circuits and returns `T`.
// This is the "Awaited preserves nullish inputs" trap.
#[test]
#[ignore = "typeinfo currently does not preserve `null` through `Awaited<null>` via the first conditional clause; keep as the future Utb17 Awaited-null contract"]
fn utility_top_bottom_utb17_awaited_null_is_null() {
    let expr = resolve_alias("Utb17AwaitedNull");
    assert_primitive(&expr, PrimitiveName::Null);
}

// TS7: `Awaited<undefined>` = `undefined` (same short-circuit clause as null).
#[test]
#[ignore = "typeinfo currently does not preserve `undefined` through `Awaited<undefined>`; keep as the future Utb18 Awaited-undefined contract"]
fn utility_top_bottom_utb18_awaited_undefined_is_undefined() {
    let expr = resolve_alias("Utb18AwaitedUndefined");
    assert_primitive(&expr, PrimitiveName::Undefined);
}

// TS7: `Awaited<Promise<Promise<string>>>` = `string`. The recursive
// `Awaited<V>` branch unwraps nested Promises until a non-thenable type
// remains.
#[test]
#[ignore = "typeinfo currently does not recursively unwrap `Awaited<Promise<Promise<T>>>` to `T` (mirrors typescript_rules AwaitedRules); keep as the future Utb19 Awaited-nested-promise contract"]
fn utility_top_bottom_utb19_awaited_nested_promise_is_inner_primitive() {
    let expr = resolve_alias("Utb19AwaitedNestedPromise");
    assert_primitive(&expr, PrimitiveName::String);
}

// =====================================================================
// NonNullable matrix
// =====================================================================

// TS7: `NonNullable<any>` = `any`. Defined as `T & {}`;
// `any & {}` = `any`.
#[test]
#[ignore = "typeinfo currently does not reduce `NonNullable<any>` to `any` via the `T & {}` definition; keep as the future Utb20 NonNullable-any contract"]
fn utility_top_bottom_utb20_non_nullable_any_is_any() {
    let expr = resolve_alias("Utb20NonNullableAny");
    assert_primitive(&expr, PrimitiveName::Any);
}

// TS7: `NonNullable<unknown>` = `{}` (`NonNullable<T> = T & {}`;
// `unknown & {}` collapses to the empty-object base `{}`). This is a
// trap: NOT `unknown` and NOT `never`.
#[test]
#[ignore = "typeinfo currently does not reduce `NonNullable<unknown>` to the empty object `{}`; keep as the future Utb21 NonNullable-unknown contract"]
fn utility_top_bottom_utb21_non_nullable_unknown_is_empty_object() {
    let expr = resolve_alias("Utb21NonNullableUnknown");
    let TypeExpr::Object(object) = &expr else {
        panic!("expected empty object, got {expr:?}");
    };
    assert!(
        object.properties.is_empty(),
        "expected empty `{{}}`, got {:?}",
        object.properties,
    );
}

// TS7: `NonNullable<never>` = `never`.
#[test]
#[ignore = "typeinfo currently does not reduce `NonNullable<never>` to `never`; keep as the future Utb22 NonNullable-never contract"]
fn utility_top_bottom_utb22_non_nullable_never_is_never() {
    let expr = resolve_alias("Utb22NonNullableNever");
    assert_primitive(&expr, PrimitiveName::Never);
}

// TS7: `NonNullable<null | undefined>` = `never`. Both arms intersect
// with `{}` to produce `never`, the union collapses.
#[test]
#[ignore = "typeinfo currently does not reduce `NonNullable<null | undefined>` to `never`; keep as the future Utb23 NonNullable-nullish-only contract"]
fn utility_top_bottom_utb23_non_nullable_null_undefined_is_never() {
    let expr = resolve_alias("Utb23NonNullableNullableOnly");
    assert_primitive(&expr, PrimitiveName::Never);
}

// =====================================================================
// Extract / Exclude matrix
// =====================================================================

// TS7: `Extract<any, string>` = `any`. The conditional distributes over
// `any`, both branches contribute, the merged result is `any`.
#[test]
fn utility_top_bottom_utb24_extract_any_against_string_is_any() {
    let expr = resolve_alias("Utb24ExtractAnyAgainstString");
    assert_primitive(&expr, PrimitiveName::Any);
}

// TS7: `Exclude<any, string>` = `any`. Same `any` distribution semantics.
#[test]
#[ignore = "typeinfo currently does not reduce `Exclude<any, string>` to `any`; keep as the future Utb25 Exclude-any contract"]
fn utility_top_bottom_utb25_exclude_any_against_string_is_any() {
    let expr = resolve_alias("Utb25ExcludeAnyAgainstString");
    assert_primitive(&expr, PrimitiveName::Any);
}

// TS7: `Extract<never, string>` = `never`. Distribution over `never`
// produces an empty union, collapsing to `never`.
#[test]
fn utility_top_bottom_utb26_extract_never_against_string_is_never() {
    let expr = resolve_alias("Utb26ExtractNeverAgainstString");
    assert_primitive(&expr, PrimitiveName::Never);
}

// TS7: `Exclude<never, string>` = `never`. Same as Utb26.
#[test]
fn utility_top_bottom_utb27_exclude_never_against_string_is_never() {
    let expr = resolve_alias("Utb27ExcludeNeverAgainstString");
    assert_primitive(&expr, PrimitiveName::Never);
}

// TS7: `Extract<unknown, string>` = `never`. `unknown` does not extend
// `string`, so the conditional resolves to the false branch (`never`).
// Unknown is NOT distributed (not a union).
#[test]
fn utility_top_bottom_utb28_extract_unknown_against_string_is_never() {
    let expr = resolve_alias("Utb28ExtractUnknownAgainstString");
    assert_primitive(&expr, PrimitiveName::Never);
}

// TS7: `Exclude<unknown, string>` = `unknown`. `unknown` does not extend
// `string`, so the false branch fires, returning `T` = `unknown`.
#[test]
fn utility_top_bottom_utb29_exclude_unknown_against_string_is_unknown() {
    let expr = resolve_alias("Utb29ExcludeUnknownAgainstString");
    assert_primitive(&expr, PrimitiveName::Unknown);
}
