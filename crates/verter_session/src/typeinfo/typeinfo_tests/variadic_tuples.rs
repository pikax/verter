//! @ai-generated - Variadic tuple contracts.
//!
//! TDD-red tests for `[...A, ...B]` spreads, `Head`/`Tail`/`Last`/`Init`
//! tuple helpers, and variadic function-signature `infer`.

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

const VARIADIC_TUPLES: &str = include_str!("fixtures/variadic_tuples.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/variadic_tuples.ts", VARIADIC_TUPLES);
}

#[test]
#[ignore = "conditional-infer over a variadic tuple pattern is Relate-carrying (the relation engine binds `infer H` against `[infer H, ...unknown[]]`) and is owned by the relation-oracle block, not U2 utilities; NOT oracle-liftable — the conditional/infer source constructs are outside the oracle's positive allowlist. Lift pending the relation-oracle block's variadic infer support"]
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
#[ignore = "conditional-infer over a variadic tuple pattern is Relate-carrying (the relation engine binds `infer R` against `[unknown, ...infer R]`) and is owned by the relation-oracle block, not U2 utilities; NOT oracle-liftable — the conditional/infer source constructs are outside the oracle's positive allowlist. Lift pending the relation-oracle block's variadic infer support"]
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
#[ignore = "conditional-infer over a variadic tuple pattern is Relate-carrying (the relation engine binds `infer L` against `[...unknown[], infer L]`) and is owned by the relation-oracle block, not U2 utilities; NOT oracle-liftable — the conditional/infer source constructs are outside the oracle's positive allowlist. Lift pending the relation-oracle block's variadic infer support"]
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
#[ignore = "conditional-infer over a variadic tuple pattern is Relate-carrying (the relation engine binds `infer I` against `[...infer I, unknown]`) and is owned by the relation-oracle block, not U2 utilities; NOT oracle-liftable — the conditional/infer source constructs are outside the oracle's positive allowlist. Lift pending the relation-oracle block's variadic infer support"]
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

    let TypeExpr::Tuple { elements, readonly } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 2);
    assert_number_literal(&elements[0].ty, 1.0);
    assert_number_literal(&elements[1].ty, 2.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `Concat<[1, 2], [3, 4]>` = `[1, 2, 3, 4]` — the `[...A, ...B]`
// variadic spread splices the substituted concrete tuples in place (the
// normalize-on-intern spread rule, no utility name special-casing). The
// lifted body is the registry-keyed `oracle::run_row` shared-driver call
// comparing Verter's `Expanded` projection against the checked-in tsgo
// snapshot.
#[oracle_row]
#[test]
fn variadic_tuple_concat_alias_produces_joined_literal_tuple() {}

#[test]
#[ignore = "explicit type-argument instantiation of a generic function VALUE (`typeof variadic<[1, 2], [3, 4]>`) is a typeof-instantiation + Relate-carrying signature path owned by the relation-oracle block, not U2 utilities; NOT oracle-liftable — the typeof source construct is outside the oracle's positive allowlist (DeferredConstruct(typeof)). Lift pending the relation-oracle block"]
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

    let TypeExpr::Tuple { elements, readonly } = &expr else {
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
