//! @ai-generated - Numeric / symbol / dual index-signature contracts.
//!
//! TDD-red tests for `[key: number]`, `[key: symbol]`, and mixed
//! `string | number` index signatures. Existing tests only exercise `[key:
//! string]`; this file fills the remaining key-type matrix.

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

const INDEX_SIGNATURES: &str = include_str!("fixtures/index_signatures.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/index_signatures.ts", INDEX_SIGNATURES);
}

// LIFTED: `NumericIndexed = { [key: number]: string }` publishes its index
// signature; the lifted body is the registry-keyed `oracle::run_row`
// shared-driver call the `#[oracle_row]` macro synthesizes. It resolves Verter's
// `Expanded` projection and compares it against the checked-in tsgo snapshot
// (captured in `Expanded`); the audit query-mode identity is proven live by
// `lifted_row_audit_query_mode_matches_spec` (oracle_query_specs.rs registry).
#[oracle_row]
#[test]
fn index_signatures_numeric_index_publishes_signature() {}

// LIFTED: `SymbolIndexed = { [key: symbol]: number }` publishes its symbol-key
// index signature; the lifted body is the registry-keyed oracle comparison,
// verified in the same `Expanded` projection mode.
#[oracle_row]
#[test]
fn index_signatures_symbol_index_publishes_signature() {}

#[test]
#[ignore = "the U2 IndexedAccess-reduction bridge has landed (operator-bodied alias reduction in resolve_named_symbol); the remaining blocker is index-signature lookup — reducing `NumericIndexed[42]` against the `[key: number]` signature to its value type"]
fn index_signatures_numeric_lookup_returns_signature_value() {
    // TS7 contract: `NumericIndexed[42]` = `string` (numeric literal `42`
    // matches the `[key: number]` signature, returning the declared value
    // type).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/index_signatures.ts",
        "NumericLookup",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "the U2 IndexedAccess-reduction bridge has landed (operator-bodied alias reduction in resolve_named_symbol); the remaining blocker is index-signature lookup — reducing `SymbolIndexed[symbol]` against the `[key: symbol]` signature to its value type"]
fn index_signatures_symbol_lookup_returns_signature_value() {
    // TS7 contract: `SymbolIndexed[symbol]` = `number`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/index_signatures.ts",
        "SymbolLookup",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "the U2 IndexedAccess-reduction bridge has landed (operator-bodied alias reduction in resolve_named_symbol); the remaining blocker is index-signature precedence — selecting the string-key signature's value union for a string-literal key against a dual string/numeric index signature"]
fn index_signatures_dual_string_key_returns_string_signature_value() {
    // TS7 contract: `DualIndexed["any-string-here"]` returns the string-key
    // signature's value type = `number | boolean`. The numeric signature
    // does NOT apply for arbitrary string keys.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/index_signatures.ts",
        "DualStringLookup",
        &[],
        ProjectionMode::Expanded,
    );

    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
    let TypeExpr::Union(types) = &expr else {
        panic!("expected union, got {expr:?}");
    };
    assert_eq!(types.len(), 2);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "the U2 IndexedAccess-reduction bridge has landed (operator-bodied alias reduction in resolve_named_symbol); the remaining blocker is index-signature precedence — selecting the numeric-key signature's value for a numeric-literal key when both string and numeric index signatures match"]
fn index_signatures_dual_numeric_key_returns_numeric_signature_value() {
    // TS7 contract: `DualIndexed[0]` returns the numeric-key signature's
    // value type = `number`. When both signatures could match, the more
    // specific numeric signature wins for numeric-literal keys.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/index_signatures.ts",
        "DualNumberLookup",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
