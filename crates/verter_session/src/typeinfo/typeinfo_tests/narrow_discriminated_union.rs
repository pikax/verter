//! @ai-generated - discriminated-union narrowing contracts.
//!
//! Each test pins ONE TS7 emission for discriminated-union narrowing
//! across `s.kind === "..."` / `switch (s.kind)` / multi-property
//! discriminants — the single most common TS pattern in real codebases
//! (Redux, AST visitors, Vue events). Scenarios cover string-literal,
//! number-literal, boolean-literal, template-literal, and `in`-guard
//! discriminants; nested guards; exhaustiveness checks; per-arm joins;
//! fall-through; shared properties; destructure correlation; and
//! reassignment re-narrowing.
//!
//! All emissions verified against tsgo 7.0.0-dev.20260523.1 via IsExactly
//! probes BEFORE encoding the Rust assertions.
//!
//! Each scenario is one `*Result = ReturnType<typeof duXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.

use super::support::*;

const NARROW_DISCRIMINATED_UNION: &str = include_str!("fixtures/narrow_discriminated_union.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(
        host,
        "/fixtures/narrow_discriminated_union.ts",
        NARROW_DISCRIMINATED_UNION,
    );
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/narrow_discriminated_union.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) if (s.kind === "a") on {kind:"a";a:string} | {kind:"b";b:number}
// TS7: if-branch sees the a-arm (returns string), else sees the b-arm
// (returns number). Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate discriminated-union narrowing on `s.kind === \"a\"` through `ReturnType<typeof fn>` to the joined return type; keep as the future Du01 if-equality-discriminant binary-union contract"]
fn narrow_discriminated_union_du01_if_equality_discriminant() {
    let expr = resolve_alias("Du01Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 2) switch(s.kind) over "a" | "b" --------------------------------
// TS7: case "a" returns string, case "b" returns number. Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate switch-on-discriminant narrowing across case arms through `ReturnType<typeof fn>`; keep as the future Du02 switch-discriminant exhaustive contract"]
fn narrow_discriminated_union_du02_switch_discriminant() {
    let expr = resolve_alias("Du02Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 3) switch with default: never exhaustiveness check ---------------
// TS7: cases return their arm-specific types; default is unreachable
// (`const _exhaustive: never = s`) and contributes `never` (absorbed).
// Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate switch-on-discriminant narrowing across cases with a never-typed default through `ReturnType<typeof fn>`; keep as the future Du03 switch-default-never exhaustiveness contract"]
fn narrow_discriminated_union_du03_switch_default_never() {
    let expr = resolve_alias("Du03Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    // never absorbed: the joined union must NOT contain `never`.
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_never = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::Never)));
    assert!(
        !has_never,
        "switch-default-never joined return must absorb never from the unreachable default; got {expr:?}"
    );
}

// ----- 4) if (s.kind !== "a") ------------------------------------------
// TS7: negated if-branch returns s.b (number); else returns s.a (string).
// Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate negated discriminant narrowing on `s.kind !== \"a\"` through `ReturnType<typeof fn>`; keep as the future Du04 negated-discriminant binary-union contract"]
fn narrow_discriminated_union_du04_negated_discriminant() {
    let expr = resolve_alias("Du04Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 5) Multi-property discriminant kind === "a" && tag === 1 --------
// TS7: compound `&&` guard narrows to the SINGLE first arm
// `{kind:"a"; tag:1; a1:string}`. if-branch returns s.a1 (string).
// else returns null. Joined: string | null.
#[test]
#[ignore = "typeinfo currently does not propagate compound multi-property discriminant narrowing (`s.kind === \"a\" && s.tag === 1`) through `ReturnType<typeof fn>`; keep as the future Du05 multi-property-discriminant contract"]
fn narrow_discriminated_union_du05_multi_property_discriminant() {
    let expr = resolve_alias("Du05Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}

// ----- 6) Nested discriminants -----------------------------------------
// TS7: outer `s.outer === "o1"` narrows to the first outer arm, then
// `s.inner.kind === "ia"` narrows the inner union. if-branch returns
// s.inner.ia (string). else returns null. Joined: string | null.
#[test]
#[ignore = "typeinfo currently does not propagate nested discriminant narrowing (`s.outer === \"o1\" && s.inner.kind === \"ia\"`) through `ReturnType<typeof fn>`; keep as the future Du06 nested-discriminant contract"]
fn narrow_discriminated_union_du06_nested_discriminant() {
    let expr = resolve_alias("Du06Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}

// ----- 7) Discriminant on number-literal type --------------------------
// TS7: if-branch (kind === 1) returns s.a (string); else returns s.b (number).
// Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate number-literal discriminant narrowing (`s.kind === 1`) through `ReturnType<typeof fn>`; keep as the future Du07 number-literal-discriminant contract"]
fn narrow_discriminated_union_du07_number_literal_discriminant() {
    let expr = resolve_alias("Du07Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 8) Discriminant on boolean-literal type -------------------------
// TS7: `if (s.ok)` narrows on truthiness of a boolean-literal-typed property.
// if-branch (ok === true) returns s.data (string); else (ok === false)
// returns s.err (number). Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate boolean-literal truthiness discriminant narrowing (`if (s.ok)`) through `ReturnType<typeof fn>`; keep as the future Du08 boolean-literal-discriminant contract"]
fn narrow_discriminated_union_du08_boolean_literal_discriminant() {
    let expr = resolve_alias("Du08Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 9) Discriminant via property destructure ------------------------
// TS7 quirk: destructuring `const { kind } = s` PROPAGATES the
// discriminated-union correlation between `kind` and `s` — the if-branch
// narrows BOTH the destructured `kind` AND the original `s`. if-branch
// returns s.a (string); else returns s.b (number). Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate destructured-discriminant correlation (`const {kind} = s; if (kind === \"a\")` narrows s) through `ReturnType<typeof fn>`; keep as the future Du09 destructure-correlation contract"]
fn narrow_discriminated_union_du09_destructure_correlation() {
    let expr = resolve_alias("Du09Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 10) Discriminant present in only one arm -----------------------
// TS7: second arm has no `kind` property, so `("kind" in s)` narrows out
// the second arm first; then `s.kind === "a"` selects the first.
// if-branch returns s.a (string); else returns null. Joined: string | null.
#[test]
#[ignore = "typeinfo currently does not propagate `in`-guard combined with discriminant narrowing (`(\"kind\" in s) && s.kind === \"a\"`) through `ReturnType<typeof fn>`; keep as the future Du10 in-guard-plus-discriminant contract"]
fn narrow_discriminated_union_du10_in_guard_plus_discriminant() {
    let expr = resolve_alias("Du10Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}

// ----- 11) Switch returning per-arm types -----------------------------
// TS7: case "a" -> string, case "b" -> number. Joined: string | number.
// Identical structure to scenario 2; pinned as the joined-return contract.
#[test]
#[ignore = "typeinfo currently does not propagate switch-on-discriminant per-arm joined return type through `ReturnType<typeof fn>`; keep as the future Du11 switch-per-arm-join contract"]
fn narrow_discriminated_union_du11_switch_per_arm_join() {
    let expr = resolve_alias("Du11Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 12) Switch with fall-through case "a": case "b": --------------
// TS7: fall-through `case "a": case "b":` block narrows s to the union
// of those two arms; s.payload is `string | number`. case "c" returns
// s.flag (boolean). Joined: string | number | boolean.
#[test]
#[ignore = "typeinfo currently does not propagate switch-fall-through narrowing (`case \"a\": case \"b\":` block sees union of both arms) through `ReturnType<typeof fn>`; keep as the future Du12 switch-fall-through contract"]
fn narrow_discriminated_union_du12_switch_fall_through() {
    let expr = resolve_alias("Du12Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
}

// ----- 13) Discriminated union with shared property ------------------
// TS7: both arms carry `shared: string`. Inside the if-branch s.a (1)
// and s.shared (string) are both accessible; the branch returns
// `{ v: 1; sh: string }`. else returns `{ v: 2; sh: string }`.
// Joined: `{ v: 1; sh: string } | { v: 2; sh: string }`.
#[test]
fn narrow_discriminated_union_du13_shared_property() {
    let expr = resolve_alias("Du13Result");
    // Joined: union of two object arms, each carrying `v` and `sh`.
    assert_union_has_object_arm(&expr, &["sh", "v"]);
}

// ----- 14) Re-narrowing after reassignment ---------------------------
// TS7 quirk: inside the if-branch `s = { kind: "b", b: 0 }` re-narrows s
// to the static type of the new value. The if-body returns s.b (number);
// the else returns s.b (number, from the original "b" arm). Joined: number.
#[test]
#[ignore = "typeinfo currently does not propagate re-narrowing after reassignment inside a discriminated-union branch through `ReturnType<typeof fn>`; keep as the future Du14 reassignment-re-narrowing contract"]
fn narrow_discriminated_union_du14_reassignment_re_narrowing() {
    let expr = resolve_alias("Du14Result");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 15) Tagged-template-style discriminant ------------------------
// TS7 quirk: `kind: \`prefix-${string}\`` is a template-literal type.
// Comparing against the concrete `"prefix-foo"` literal (which IS
// assignable to `\`prefix-${string}\``) selects the first arm in the
// if-branch. if-branch returns s.a (string); else returns null.
// Joined: string | null.
#[test]
#[ignore = "typeinfo currently does not propagate template-literal discriminant narrowing (`s.kind === \"prefix-foo\"` against `kind: \\`prefix-${string}\\``) through `ReturnType<typeof fn>`; keep as the future Du15 template-literal-discriminant contract"]
fn narrow_discriminated_union_du15_template_literal_discriminant() {
    let expr = resolve_alias("Du15Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}
