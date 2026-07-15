//! @ai-generated - equality-narrowing contracts.
//!
//! Each test pins ONE TS7 emission for an equality-narrowing scenario
//! (`===`, `!==`, `==`) across union arms, primitive widths, literal
//! discriminants, `as const` RHS, double-equals nullish, impossible
//! compound guards, and the special quirks around `string`/`number` widths,
//! cross-union equality, and `NaN`. Each scenario is one function in the
//! fixture; one alias per function via `type EqNNResult = ReturnType<typeof eqNN>`.
//!
//! All emissions verified against tsgo 7.0.0-dev.20260523.1 via IsExactly
//! probes BEFORE encoding the Rust assertions.
//!
//! Each scenario is one `*Result = ReturnType<typeof eqXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.

use super::support::*;

const NARROW_EQUALITY: &str = include_str!("fixtures/narrow_equality.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/narrow_equality.ts", NARROW_EQUALITY);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/narrow_equality.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) x === "a" on "a" | "b" ----------------------------------------
// TS7: if-branch returns "a", else returns "b". Joined: "a" | "b".
#[test]
#[ignore = "typeinfo currently does not propagate `===` literal-narrowing on a literal-arm union through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq01 equality-narrowing string-literal-on-literal-union contract"]
fn narrow_equality_eq01_string_literal_on_literal_union() {
    let expr = resolve_alias("Eq01Result");
    assert_literal_union(&expr, &["a", "b"]);
}

// ----- 2) x !== "a" (negated) on "a" | "b" ------------------------------
// TS7: if-branch (negated) returns "b", else returns "a". Joined: "a" | "b".
#[test]
#[ignore = "typeinfo currently does not propagate negated `!==` literal-narrowing on a literal-arm union through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq02 negated-equality-narrowing string-literal-on-literal-union contract"]
fn narrow_equality_eq02_negated_string_literal_on_literal_union() {
    let expr = resolve_alias("Eq02Result");
    assert_literal_union(&expr, &["a", "b"]);
}

// ----- 3) x === 1 on 1 | 2 | 3 ------------------------------------------
// TS7: if-branch returns 1, else returns 2 | 3. Joined: 1 | 2 | 3.
#[test]
#[ignore = "typeinfo currently does not propagate `===` number-literal narrowing on a literal-arm number union through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq03 equality-narrowing number-literal-on-triple-union contract"]
fn narrow_equality_eq03_number_literal_on_triple_union() {
    let expr = resolve_alias("Eq03Result");
    assert_number_literal_union(&expr, &[1.0, 2.0, 3.0]);
}

// ----- 4) x === true on boolean -----------------------------------------
// TS7 quirk: `boolean` is treated as `true | false`. if-branch: true,
// else: false. Joined: boolean.
#[test]
#[ignore = "typeinfo currently does not propagate `=== true` narrowing on `boolean` (treated as `true | false`) through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq04 equality-narrowing boolean-true-on-boolean contract"]
fn narrow_equality_eq04_boolean_true_on_boolean() {
    let expr = resolve_alias("Eq04Result");
    assert_primitive(&expr, PrimitiveName::Boolean);
}

// ----- 5) x === null on string | null -----------------------------------
// TS7: if-branch: null, else: string. Joined: string | null.
#[test]
#[ignore = "typeinfo currently does not propagate `=== null` narrowing on a nullable string through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq05 equality-narrowing null-on-nullable-string contract"]
fn narrow_equality_eq05_null_on_nullable_string() {
    let expr = resolve_alias("Eq05Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}

// ----- 6) x === undefined on string | undefined -------------------------
// TS7: if-branch: undefined, else: string. Joined: string | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate `=== undefined` narrowing on an optional string through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq06 equality-narrowing undefined-on-optional-string contract"]
fn narrow_equality_eq06_undefined_on_optional_string() {
    let expr = resolve_alias("Eq06Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 7) x == null (double-equals) on string | null | undefined --------
// TS7 quirk: loose-equality `== null` matches BOTH null and undefined.
// if-branch: null | undefined, else: string. Joined: string | null | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate loose-equality `== null` narrowing (matches both null and undefined) on a nullish-string union through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq07 double-equals-null-on-nullish-string contract"]
fn narrow_equality_eq07_double_equals_null_on_nullish_string() {
    let expr = resolve_alias("Eq07Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 8) x === "a" on string -------------------------------------------
// TS7 quirk: a wider declared type (`string`) is NOT narrowed to the literal
// `"a"`. Both branches see `string`. Joined: string.
#[test]
#[ignore = "typeinfo currently does not faithfully model the TS7 quirk that `x === \"a\"` does NOT narrow a wider declared type `string` to the literal through `ReturnType<typeof fn>`; keep as the future Eq08 equality-narrowing literal-rhs-on-wider-string contract"]
fn narrow_equality_eq08_string_literal_on_string_does_not_narrow() {
    let expr = resolve_alias("Eq08Result");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 9) x === "a" on string | number ----------------------------------
// TS7 quirk: if-branch narrows to `string` (the arm whose primitive covers
// the literal) but NOT down to the literal `"a"`. else keeps the full
// `string | number`. Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not faithfully model the TS7 quirk that `x === \"a\"` on `string | number` narrows the if-branch only down to `string` (not to `\"a\"`) while the else keeps both arms, through `ReturnType<typeof fn>`; keep as the future Eq09 equality-narrowing literal-rhs-on-primitive-union contract"]
fn narrow_equality_eq09_string_literal_on_primitive_union() {
    let expr = resolve_alias("Eq09Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 10) x === y between two unions -----------------------------------
// TS7 quirk: mutual equality between two unions (`x: "a"|"b"`, `y: "b"|"c"`)
// does NOT refine either operand. Both branches return x at its declared
// type. Joined: "a" | "b".
#[test]
#[ignore = "typeinfo currently does not faithfully model the TS7 quirk that mutual `===` between two unions does NOT refine either operand through `ReturnType<typeof fn>`; keep as the future Eq10 equality-narrowing two-unions-mutual-equality contract"]
fn narrow_equality_eq10_two_unions_mutual_equality_does_not_narrow() {
    let expr = resolve_alias("Eq10Result");
    assert_literal_union(&expr, &["a", "b"]);
}

// ----- 11) Impossible compound: x === null && x === undefined -----------
// TS7: the conjunction is never satisfiable; the if-branch is `never`
// (absorbed). else: the FULL original union.
// Joined: string | null | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate impossible-compound `=== null && === undefined` narrowing (if-branch is `never`, absorbed) through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq11 equality-narrowing impossible-compound contract"]
fn narrow_equality_eq11_impossible_compound_absorbs_never() {
    let expr = resolve_alias("Eq11Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
    // never absorbed: the joined union must NOT contain `never`.
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_never = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::Never)));
    assert!(
        !has_never,
        "impossible-compound joined return must absorb never from the unreachable if-branch; got {expr:?}"
    );
}

// ----- 12) Property equality on discriminant ----------------------------
// TS7: equivalent to discriminated-union narrowing via equality. if-branch
// returns s.a (string); else returns s.b (number). Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate property-equality discriminant narrowing (`s.kind === \"a\"`) through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq12 equality-narrowing property-discriminant contract"]
fn narrow_equality_eq12_property_equality_discriminant() {
    let expr = resolve_alias("Eq12Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 13) Equality of `as const` literal --------------------------------
// TS7: `const TAG = "a" as const` has type `"a"`. Comparing against TAG
// works identically to comparing against the literal. if-branch: "a";
// else: "b". Joined: "a" | "b".
#[test]
#[ignore = "typeinfo currently does not propagate equality narrowing against an `as const` literal-RHS variable through `ReturnType<typeof fn>` to the joined return type; keep as the future Eq13 equality-narrowing as-const-literal-rhs contract"]
fn narrow_equality_eq13_as_const_literal_rhs() {
    let expr = resolve_alias("Eq13Result");
    assert_literal_union(&expr, &["a", "b"]);
}

// ----- 14) x === 0 on number --------------------------------------------
// TS7 quirk: the wider declared type `number` is NOT narrowed to the literal
// `0`. Both branches see `number`. Joined: number.
#[test]
#[ignore = "typeinfo currently does not faithfully model the TS7 quirk that `x === 0` does NOT narrow a wider declared type `number` to the literal through `ReturnType<typeof fn>`; keep as the future Eq14 equality-narrowing number-literal-rhs-on-wider-number contract"]
fn narrow_equality_eq14_number_literal_on_number_does_not_narrow() {
    let expr = resolve_alias("Eq14Result");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 15) NaN equality is always false ---------------------------------
// TS7 quirk: `NaN !== NaN` by definition. Comparing against `Number.NaN`
// via a local binding does NOT narrow. Both branches see `number`.
// Joined: number.
#[test]
#[ignore = "typeinfo currently does not faithfully model the TS7 quirk that `x === NaN` (via a `Number.NaN`-bound local) is always false and does NOT narrow, through `ReturnType<typeof fn>`; keep as the future Eq15 equality-narrowing NaN-equality-no-narrowing contract"]
fn narrow_equality_eq15_nan_equality_does_not_narrow() {
    let expr = resolve_alias("Eq15Result");
    assert_primitive(&expr, PrimitiveName::Number);
}
