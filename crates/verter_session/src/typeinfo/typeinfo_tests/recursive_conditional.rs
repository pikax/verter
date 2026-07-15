//! @ai-generated - Recursive conditional-type contracts.
//!
//! TDD-red tests describing TS7 expected projection for recursive conditional
//! types: `Flatten`, `DeepReadonly`, `DeepPartial`, and recursive `Awaited`.

use super::support::*;

const RECURSIVE_CONDITIONAL: &str = include_str!("fixtures/recursive_conditional.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(
        host,
        "/fixtures/recursive_conditional.ts",
        RECURSIVE_CONDITIONAL,
    );
}

#[test]
#[ignore = "typeinfo currently does not iterate a conditional type through self-recursion until a non-array base case is reached; keep as the future recursive-conditional Flatten contract"]
fn recursive_conditional_flatten_unwraps_three_deep_array_to_primitive() {
    // TS7 contract: `Flatten<string[][][]>` recursively unwraps each array
    // layer until a non-array type remains. Result: `string`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/recursive_conditional.ts",
        "FlattenedThreeDeepArray",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn recursive_conditional_flatten_returns_non_array_input_unchanged() {
    // TS7 contract: `Flatten<number>` short-circuits at the base case
    // (number is not assignable to `readonly (infer U)[]`), so the result is
    // `number` itself. Active baseline: Verter already handles this single-
    // iteration conditional that fails the check on the first try.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/recursive_conditional.ts",
        "FlattenedAlreadyFlat",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not iterate a recursive mapped+conditional type that adds `readonly` at every nesting level; keep as the future DeepReadonly contract"]
fn recursive_conditional_deep_readonly_marks_every_nested_property() {
    // TS7 contract: `DeepReadonly<DeepConfig>` recursively walks every
    // structural property and marks it `readonly`. Every member at every
    // nesting depth carries the readonly modifier. Function/scalar members
    // pass through unchanged (function check + non-object base case).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/recursive_conditional.ts",
        "DeepReadonlyConfig",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["outer", "scalar"]);
    assert!(props["scalar"].readonly);
    assert_primitive(&props["scalar"].ty, PrimitiveName::Number);
    assert!(props["outer"].readonly);

    let outer = object_props(&props["outer"].ty);
    assert_eq!(prop_names(&outer), vec!["inner", "list"]);
    assert!(outer["inner"].readonly);
    assert!(outer["list"].readonly);

    let inner = object_props(&outer["inner"].ty);
    assert_eq!(prop_names(&inner), vec!["flag", "label"]);
    assert!(inner["flag"].readonly);
    assert_primitive(&inner["flag"].ty, PrimitiveName::Boolean);
    assert!(inner["label"].readonly);
    assert_primitive(&inner["label"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not iterate a recursive mapped+conditional type that makes every nested property optional; keep as the future DeepPartial contract"]
fn recursive_conditional_deep_partial_marks_every_nested_property_optional() {
    // TS7 contract: `DeepPartial<{ scalar: number; nested: { name: string; count: number } }>`
    // marks every property optional at every nesting depth. The terminal scalar
    // (number / string) is preserved.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/recursive_conditional.ts",
        "DeepPartialConfig",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["nested", "scalar"]);
    assert!(props["scalar"].optional);
    assert!(props["nested"].optional);

    let nested = object_props(&props["nested"].ty);
    assert_eq!(prop_names(&nested), vec!["count", "name"]);
    assert!(nested["count"].optional);
    assert!(nested["name"].optional);
    assert_primitive(&nested["count"].ty, PrimitiveName::Number);
    assert_primitive(&nested["name"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not iterate a recursive conditional that unwraps nested Promises until a non-Promise base case is reached; keep as the future recursive-Awaited contract"]
fn recursive_conditional_awaited_recursive_unwraps_nested_promises() {
    // TS7 contract: `AwaitedRecursive<Promise<Promise<{ id: string }>>>` peels
    // two layers of Promise and reduces to `{ id: string }` (the inner
    // payload). This mirrors the built-in `Awaited<T>` behaviour for
    // PromiseLike chains.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/recursive_conditional.ts",
        "DoubleAwaited",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["id"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
