//! @ai-generated - truthiness-narrowing contracts.
//!
//! Each test pins ONE TS7 emission for an `if (x)` / `if (!x)` truthiness
//! discriminator across nullable unions, literal unions, boolean,
//! property guards, early-return guards, compound `&&` chains, and the
//! quirks around plain primitives, `unknown`, `object | null`, and
//! `number | undefined`. Each scenario is one function in the fixture;
//! one alias per function via `type TrNNResult = ReturnType<typeof trNN>`.
//!
//! All emissions verified against tsgo 7.0.0-dev.20260523.1 via IsExactly
//! probes BEFORE encoding the Rust assertions.
//!
//! Each scenario is one `*Result = ReturnType<typeof trXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.

use super::support::*;

const NARROW_TRUTHINESS: &str = include_str!("fixtures/narrow_truthiness.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/narrow_truthiness.ts", NARROW_TRUTHINESS);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/narrow_truthiness.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) if (x) on string | undefined ----------------------------------
// TS7: if-branch returns string, else returns undefined. Joined: string | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate truthiness narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Tr01 truthiness-narrowing string-or-undefined contract"]
fn narrow_truthiness_tr01_string_or_undefined() {
    let expr = resolve_alias("Tr01Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 2) if (x) on string | null ---------------------------------------
// TS7: if-branch returns string, else returns null. Joined: string | null.
#[test]
#[ignore = "typeinfo currently does not propagate truthiness narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Tr02 truthiness-narrowing string-or-null contract"]
fn narrow_truthiness_tr02_string_or_null() {
    let expr = resolve_alias("Tr02Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}

// ----- 3) if (x) on string | null | undefined ---------------------------
// TS7: if-branch returns string, else returns null | undefined.
// Joined: string | null | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate truthiness narrowing on a nullish-string union through `ReturnType<typeof fn>` to the joined return type; keep as the future Tr03 truthiness-narrowing string-or-nullish contract"]
fn narrow_truthiness_tr03_string_or_nullish() {
    let expr = resolve_alias("Tr03Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 4) if (x) on string (no nullable) --------------------------------
// TS7 quirk: a wider declared `string` is NOT narrowed in the else branch
// to `""`. Both branches see `string`. Joined: string.
#[test]
#[ignore = "typeinfo currently does not faithfully model the TS7 quirk that `if (x)` on a wider declared `string` does NOT narrow the else branch to `\"\"` through `ReturnType<typeof fn>`; keep as the future Tr04 truthiness-narrowing string-no-nullable contract"]
fn narrow_truthiness_tr04_string_no_nullable_does_not_narrow() {
    let expr = resolve_alias("Tr04Result");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 5) if (x) on 0 | 1 | 2 -------------------------------------------
// TS7: if-branch narrows to 1 | 2 (truthy arms), else narrows to 0 (falsy arm).
// Joined: 0 | 1 | 2.
#[test]
#[ignore = "typeinfo currently does not propagate truthiness narrowing on a number-literal union (splitting 0 from 1|2) through `ReturnType<typeof fn>` to the joined return type; keep as the future Tr05 truthiness-narrowing number-literal-union contract"]
fn narrow_truthiness_tr05_number_literal_union() {
    let expr = resolve_alias("Tr05Result");
    assert_number_literal_union(&expr, &[0.0, 1.0, 2.0]);
}

// ----- 6) if (x) on "" | "a" | "b" --------------------------------------
// TS7: if-branch narrows to "a" | "b" (truthy arms), else narrows to "" (falsy arm).
// Joined: "" | "a" | "b".
#[test]
#[ignore = "typeinfo currently does not propagate truthiness narrowing on a string-literal union (splitting \"\" from \"a\"|\"b\") through `ReturnType<typeof fn>` to the joined return type; keep as the future Tr06 truthiness-narrowing string-literal-union contract"]
fn narrow_truthiness_tr06_string_literal_union() {
    let expr = resolve_alias("Tr06Result");
    assert_literal_union(&expr, &["", "a", "b"]);
}

// ----- 7) if (x) on false | true ----------------------------------------
// TS7: if-branch narrows to true, else narrows to false.
// Joined: boolean (= true | false).
#[test]
#[ignore = "typeinfo currently does not propagate truthiness narrowing on `boolean` (treated as `true | false`) through `ReturnType<typeof fn>` to the joined return type; keep as the future Tr07 truthiness-narrowing boolean-union contract"]
fn narrow_truthiness_tr07_boolean_union() {
    let expr = resolve_alias("Tr07Result");
    assert_primitive(&expr, PrimitiveName::Boolean);
}

// ----- 8) if (!x) (negated) on string | undefined -----------------------
// TS7: if-branch (negated): undefined. else: string. Joined: string | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate negated truthiness narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Tr08 negated-truthiness-narrowing string-or-undefined contract"]
fn narrow_truthiness_tr08_negated_string_or_undefined() {
    let expr = resolve_alias("Tr08Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 9) Property truthiness guard -------------------------------------
// TS7: `if (obj.foo) return obj.foo;` where `foo: string | undefined` -
// if-branch returns string, else returns undefined. Joined: string | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate property-truthiness narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Tr09 truthiness-narrowing property-guard contract"]
fn narrow_truthiness_tr09_property_truthiness() {
    let expr = resolve_alias("Tr09Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 10) Early-return guard -------------------------------------------
// TS7: `if (!x) return; /* x narrowed */` on `string | undefined` -
// the bare `return;` contributes `undefined` to the join; the trailing
// `return x` sees x as `string`. Joined: string | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate post-guard truthiness narrowing across an early-return into the trailing return through `ReturnType<typeof fn>`; keep as the future Tr10 truthiness-narrowing early-return-guard contract"]
fn narrow_truthiness_tr10_early_return_guard() {
    let expr = resolve_alias("Tr10Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 11) Truthiness on unknown ----------------------------------------
// TS7 quirk: `if (x)` on `unknown` narrows the if-branch to `{}` (the
// non-nullish truthy upper bound) and leaves the else as `unknown`. The
// joined return collapses to `unknown` because `{}` is subsumed.
#[test]
#[ignore = "typeinfo currently does not faithfully model the TS7 quirk that `if (x)` on `unknown` narrows the if-branch to `{}` (subsumed by the else `unknown`) through `ReturnType<typeof fn>`; keep as the future Tr11 truthiness-narrowing unknown contract"]
fn narrow_truthiness_tr11_unknown_collapses_to_unknown() {
    let expr = resolve_alias("Tr11Result");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// ----- 12) Truthiness on object | null ----------------------------------
// TS7: if-branch narrows out null -> object. else: null. Joined: object | null.
#[test]
#[ignore = "typeinfo currently does not propagate truthiness narrowing on `object | null` through `ReturnType<typeof fn>` to the joined return type; keep as the future Tr12 truthiness-narrowing object-or-null contract"]
fn narrow_truthiness_tr12_object_or_null() {
    let expr = resolve_alias("Tr12Result");
    assert_union_contains_primitive(&expr, PrimitiveName::Object);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}

// ----- 13) Truthiness compound && chain ---------------------------------
// TS7: `if (x && x.length > 0)` on `string | undefined` -> if-branch x is
// `string` (length probe requires it). else: the FULL original union
// (TS cannot represent "string with length === 0" as a distinct arm).
// Both branches return x. Joined: string | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate compound `truthy && property-guard` narrowing through `ReturnType<typeof fn>`; keep as the future Tr13 compound-truthiness-and-property contract"]
fn narrow_truthiness_tr13_compound_and_chain() {
    let expr = resolve_alias("Tr13Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 14) Truthiness on number | undefined -----------------------------
// TS7 quirk: if-branch narrows to `number` (TS does NOT narrow out `0`
// because `0` is a number, not a separate arm). else: `number | undefined`
// (since `0` is falsy too). Joined: number | undefined.
#[test]
#[ignore = "typeinfo currently does not faithfully model the TS7 quirk that `if (x)` on `number | undefined` does NOT narrow out `0` from the if-branch (and keeps `number` in the else alongside `undefined`) through `ReturnType<typeof fn>`; keep as the future Tr14 truthiness-narrowing number-or-undefined contract"]
fn narrow_truthiness_tr14_number_or_undefined_does_not_split_zero() {
    let expr = resolve_alias("Tr14Result");
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 15) Optional chaining truthiness ---------------------------------
// TS7: `if (obj?.foo)` on `obj: { foo: string } | undefined` -> in the
// if-branch obj is `{ foo: string }` so `obj.foo` is `string`. The else
// path returns `undefined` explicitly. Joined: string | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate optional-chain truthiness narrowing through `ReturnType<typeof fn>`; keep as the future Tr15 truthiness-narrowing optional-chain contract"]
fn narrow_truthiness_tr15_optional_chain_truthiness() {
    let expr = resolve_alias("Tr15Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}
