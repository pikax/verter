//! @ai-generated - `unique symbol` contracts.
//!
//! TDD-red tests describing TS7 behaviour for `unique symbol` value identity,
//! computed-key projection, and `keyof` over an object with a unique-symbol
//! member.

use super::support::*;

const UNIQUE_SYMBOL: &str = include_str!("fixtures/unique_symbol.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/unique_symbol.ts", UNIQUE_SYMBOL);
}

#[test]
#[ignore = "typeinfo currently does not bridge a `unique symbol` value identity into computed-key projection; keep as the future unique-symbol indexed-access contract"]
fn unique_symbol_indexed_access_via_typeof_returns_literal_value() {
    // TS7 contract: `Branded[typeof brandTag]` reduces to the literal string
    // `"branded"`. The unique-symbol value identity participates in object
    // key lookup; the projection returns the declared member type.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/unique_symbol.ts",
        "BrandValue",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "branded");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not project the non-symbol sibling member of an object that also has a unique-symbol-keyed member; keep as the future unique-symbol sibling-member contract"]
fn unique_symbol_string_key_access_returns_sibling_value() {
    // TS7 contract: `Branded["payload"]` = `string`. The presence of a
    // unique-symbol-keyed member next to it does NOT shadow ordinary
    // string-key access.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/unique_symbol.ts",
        "BrandPayload",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
