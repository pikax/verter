//! @ai-generated - Numeric / symbol / dual index-signature contracts.
//!
//! TDD-red tests for `[key: number]`, `[key: symbol]`, and mixed
//! `string | number` index signatures. Existing tests only exercise `[key:
//! string]`; this file fills the remaining key-type matrix.

use super::support::*;

const INDEX_SIGNATURES: &str = include_str!("fixtures/index_signatures.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/index_signatures.ts", INDEX_SIGNATURES);
}

#[test]
#[ignore = "typeinfo currently does not publish a numeric-key index signature on declared object types; keep as the future numeric-index-signature contract"]
fn index_signatures_numeric_index_publishes_signature() {
    // TS7 contract: `NumericIndexed = { [key: number]: string }` must surface
    // a single index signature with numeric key type and string value type.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/index_signatures.ts",
        "NumericIndexed",
        &[],
        ProjectionMode::Expanded,
    );

    let sigs = object_index_signatures(&expr);
    assert_eq!(sigs.len(), 1);
    assert_primitive(&sigs[0].key_type, PrimitiveName::Number);
    assert_primitive(&sigs[0].value_type, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not publish a symbol-key index signature on declared object types; keep as the future symbol-index-signature contract"]
fn index_signatures_symbol_index_publishes_signature() {
    // TS7 contract: `SymbolIndexed = { [key: symbol]: number }` must surface
    // a single index signature with symbol key type and number value type.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/index_signatures.ts",
        "SymbolIndexed",
        &[],
        ProjectionMode::Expanded,
    );

    let sigs = object_index_signatures(&expr);
    assert_eq!(sigs.len(), 1);
    assert_primitive(&sigs[0].key_type, PrimitiveName::Symbol);
    assert_primitive(&sigs[0].value_type, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce indexed access by a numeric literal against a numeric index signature; keep as the future numeric-indexed-access contract"]
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
#[ignore = "typeinfo currently does not reduce indexed access by `symbol` against a symbol index signature; keep as the future symbol-indexed-access contract"]
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
#[ignore = "typeinfo currently does not honour the precedence of overlapping string vs numeric index signatures; keep as the future dual-index precedence contract"]
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
#[ignore = "typeinfo currently does not honour the precedence of the numeric index signature over the string signature for numeric-literal keys; keep as the future dual-index numeric-key contract"]
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
