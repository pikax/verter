//! @ai-generated - Variadic tuple contracts.
//!
//! TDD-red tests for `[...A, ...B]` spreads, `Head`/`Tail`/`Last`/`Init`
//! tuple helpers, and variadic function-signature `infer`.

use super::support::*;

const VARIADIC_TUPLES: &str = include_str!("fixtures/variadic_tuples.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/variadic_tuples.ts", VARIADIC_TUPLES);
}

#[test]
#[ignore = "typeinfo currently does not infer the head element through a conditional `[infer H, ...unknown[]]`; keep as the future tuple-head contract"]
fn variadic_tuple_head_of_sample_resolves_to_first_literal() {
    // TS7 contract: `Head<[1, 2, 3]>` = `1` (first element literal).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/variadic_tuples.ts",
        "HeadOfSample",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 1.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not infer the tail tuple through a conditional `[unknown, ...infer R]`; keep as the future tuple-tail contract"]
fn variadic_tuple_tail_of_sample_resolves_to_remaining_tuple() {
    // TS7 contract: `Tail<[1, 2, 3]>` = `[2, 3]`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/variadic_tuples.ts",
        "TailOfSample",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 2);
    assert_number_literal(&elements[0].ty, 2.0);
    assert_number_literal(&elements[1].ty, 3.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not infer the last element through a conditional `[...unknown[], infer L]`; keep as the future tuple-last contract"]
fn variadic_tuple_last_of_sample_resolves_to_terminal_literal() {
    // TS7 contract: `Last<[1, 2, 3]>` = `3` (terminal element literal).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/variadic_tuples.ts",
        "LastOfSample",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 3.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not infer the initial-prefix tuple through a conditional `[...infer I, unknown]`; keep as the future tuple-init contract"]
fn variadic_tuple_init_of_sample_resolves_to_prefix_tuple() {
    // TS7 contract: `Init<[1, 2, 3]>` = `[1, 2]` (drop terminal element).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/variadic_tuples.ts",
        "InitOfSample",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 2);
    assert_number_literal(&elements[0].ty, 1.0);
    assert_number_literal(&elements[1].ty, 2.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate `[...A, ...B]` spread alias concatenation; keep as the future tuple-concat contract"]
fn variadic_tuple_concat_alias_produces_joined_literal_tuple() {
    // TS7 contract: `Concat<[1, 2], [3, 4]>` = `[1, 2, 3, 4]`. The `[...A, ...B]`
    // form is the canonical variadic-spread literal tuple.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/variadic_tuples.ts",
        "ConcatPair",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 4);
    assert_number_literal(&elements[0].ty, 1.0);
    assert_number_literal(&elements[1].ty, 2.0);
    assert_number_literal(&elements[2].ty, 3.0);
    assert_number_literal(&elements[3].ty, 4.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not instantiate a generic function with explicit variadic type arguments; keep as the future generic-variadic-call contract"]
fn variadic_tuple_variadic_function_with_explicit_type_args_concatenates_tuples() {
    // TS7 contract: `ReturnType<typeof variadic<[1, 2], [3, 4]>>` = `[1, 2, 3, 4]`.
    // The function signature is `(a: [...A], b: [...B]) => [...A, ...B]`;
    // when instantiated with `A=[1,2]` and `B=[3,4]` the spread concatenates
    // the explicit type arguments into a single tuple of literals.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/variadic_tuples.ts",
        "VariadicCallResult",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 4);
    assert_number_literal(&elements[0].ty, 1.0);
    assert_number_literal(&elements[1].ty, 2.0);
    assert_number_literal(&elements[2].ty, 3.0);
    assert_number_literal(&elements[3].ty, 4.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
