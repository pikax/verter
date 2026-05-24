//! @ai-generated - substitution-type contracts.
//!
//! Each test pins ONE TS7 emission for a substitution-type scenario —
//! the internal "T narrowed to T & U" mechanism TS uses to keep generic
//! identity while flowing type guards through method calls, destructures,
//! conditional types, asserts predicates, the `in`-operator, and recursive
//! self-calls.
//!
//! Substitution types are SUBTLE: emissions often disagree with the
//! intuitive guess (notably Sb08's no-distribution-of-unknown, Sb14's
//! ignore-the-default, and Sb06's un-narrow-on-assignment). Every
//! emission below has been verified against tsgo 7.0.0-dev.20260523.1
//! via IsExactly probes BEFORE encoding the Rust assertion.
//!
//! Each scenario is one `*Result = ReturnType<typeof sbXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.
//!
//! Documented TS7 surprises (NOT fixture bugs):
//!   * Sb01 — With T unspecified via ReturnType, the joined return
//!     collapses to `unknown` (string is subsumed).
//!   * Sb04 — Even with explicit `:T` return annotation, ReturnType
//!     resolves to `unknown`, not `T` and not the substituted `T & string`.
//!   * Sb06 — Re-assigning a wider value to the narrowed binding
//!     UN-narrows it; the substitution is removed.
//!   * Sb08 — `IfStr<unknown>` does NOT distribute. `unknown extends
//!     string` is `false`, so the conditional immediately resolves to
//!     the false-arm `"no"`. Distribution requires a NAKED union type
//!     parameter at the test position — `unknown` is not a union.
//!   * Sb14 — Default type args (`<T = string>`) DO NOT apply inside
//!     `ReturnType<typeof fn>`. ReturnType still resolves T to `unknown`.
//!     Defaults only apply at value-position call sites.
//!   * Sb15 — Recursive self-calls are NOT a substitution event. The
//!     return type stays as the declared `T` -> `unknown`.
//!
//! All scenarios are currently `#[ignore]` because typeinfo does not yet
//! propagate substitution-type semantics through `ReturnType<typeof fn>`.
//! Each test is the future contract for that specific emission.

use super::support::*;

const SUBSTITUTION_TYPES: &str = include_str!("fixtures/substitution_types.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/substitution_types.ts", SUBSTITUTION_TYPES);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/substitution_types.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) Bare narrowing of generic ------------------------------------
// TS7: if-branch is `T & string` (substitution), else is `T`. With T=unknown
// via ReturnType, the joined return is `string | unknown` which collapses
// to `unknown` (string is subsumed by unknown).
#[test]
#[ignore = "typeinfo currently does not propagate substitution-type narrowing on a bare generic T through `ReturnType<typeof fn>` to the collapsed `unknown` emission; keep as the future Sb01 bare-narrowing-of-generic contract"]
fn substitution_types_sb01_bare_narrowing_of_generic() {
    let expr = resolve_alias("Sb01Result");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// ----- 2) Narrowing in a constrained generic ---------------------------
// TS7: if-branch is `T & string`.toUpperCase() -> string. Else is T
// (constrained to `string | number`). Joined: `string | (string | number)`
// = `string | number`.
#[test]
#[ignore = "typeinfo currently does not propagate substitution-type narrowing across a constrained generic with method-call apparent type through `ReturnType<typeof fn>` to the joined `string | number` emission; keep as the future Sb02 narrowing-in-constrained-generic contract"]
fn substitution_types_sb02_narrowing_in_constrained_generic() {
    let expr = resolve_alias("Sb02Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 3) Substitution survives method calls ---------------------------
// TS7: `x.toUpperCase()` on T-extends-string reads the apparent type
// `string`. The substitution is preserved across the method call.
// Joined: string.
#[test]
#[ignore = "typeinfo currently does not propagate substitution-type apparent-type access across a method call on a constrained generic through `ReturnType<typeof fn>` to `string`; keep as the future Sb03 substitution-survives-method-calls contract"]
fn substitution_types_sb03_substitution_survives_method_calls() {
    let expr = resolve_alias("Sb03Result");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 4) Narrowed substitution to return position ---------------------
// TS7 quirk: RAW return annotation is `T`. With T=unknown via ReturnType,
// the emission is `unknown` — NOT the substitution `T & string` and NOT
// the literal `T`.
#[test]
#[ignore = "typeinfo currently does not propagate the explicit `: T` return annotation on a generic-narrowed body through `ReturnType<typeof fn>` to the T-resolves-to-`unknown` emission; keep as the future Sb04 narrowed-substitution-to-return-position contract"]
fn substitution_types_sb04_narrowed_substitution_to_return_position() {
    let expr = resolve_alias("Sb04Result");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// ----- 5) Compound narrowing via typeof && instanceof ------------------
// TS7: `typeof x === "object" && x instanceof Date` narrows x to Date.
// `x.getTime()` returns number.
#[test]
#[ignore = "typeinfo currently does not propagate compound `typeof && instanceof` narrowing on a bare generic through `ReturnType<typeof fn>` to the `number` emission from `Date.getTime()`; keep as the future Sb05 compound-typeof-and-instanceof contract"]
fn substitution_types_sb05_compound_typeof_and_instanceof() {
    let expr = resolve_alias("Sb05Result");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 6) Narrowing widens after a re-assignment -----------------------
// TS7 quirk: Inside the if-branch x is initially `T & string`. After
// `x = (1 as unknown) as T` the substitution is REMOVED (TS un-narrows
// on assignment to a wider site). The return is the un-narrowed T which
// resolves to `unknown` via ReturnType.
#[test]
#[ignore = "typeinfo currently does not model TS7's un-narrowing-on-assignment-to-wider-site behaviour on a generic substitution through `ReturnType<typeof fn>` to the widened `unknown` emission; keep as the future Sb06 narrowing-widens-after-reassignment contract"]
fn substitution_types_sb06_narrowing_widens_after_reassignment() {
    let expr = resolve_alias("Sb06Result");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// ----- 7) Constraint-flow apparent type --------------------------------
// TS7: `x.len` on T-extends-{len:number} reads the constraint's apparent
// type. Returns number.
#[test]
#[ignore = "typeinfo currently does not propagate constraint-flow apparent-type property access on a constrained generic through `ReturnType<typeof fn>` to `number`; keep as the future Sb07 constraint-flow-apparent-type contract"]
fn substitution_types_sb07_constraint_flow_apparent_type() {
    let expr = resolve_alias("Sb07Result");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 8) Generic in conditional position retains identity -------------
// TS7 quirk: `IfStr<T>` with T=unknown does NOT distribute. `unknown
// extends string` is `false` (unknown is NOT a subtype of string), so
// the conditional resolves immediately to the false-arm: the string
// literal `"no"`. Distribution requires the test position to be a NAKED
// UNION type parameter — `unknown` is not a union, so no distribution
// happens.
#[test]
#[ignore = "typeinfo currently does not model TS7's no-distribution-on-`unknown` rule for conditional types through `ReturnType<typeof fn>` to the literal `\"no\"` emission; keep as the future Sb08 generic-in-conditional-no-distribute-on-unknown contract"]
fn substitution_types_sb08_generic_in_conditional_no_distribute_on_unknown() {
    let expr = resolve_alias("Sb08Result");
    assert_string_literal(&expr, "no");
}

// ----- 9) `asserts x is string` on generic -----------------------------
// TS7: After the assert, x is `T & string`. With T=unknown via ReturnType,
// `unknown & string` collapses to `string`. Joined: string.
#[test]
#[ignore = "typeinfo currently does not propagate `asserts x is string` predicate narrowing on a bare generic through `ReturnType<typeof fn>` to the collapsed `string` emission; keep as the future Sb09 asserts-x-is-string-on-generic contract"]
fn substitution_types_sb09_asserts_x_is_string_on_generic() {
    let expr = resolve_alias("Sb09Result");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 10) `x is T` predicate on generic -------------------------------
// TS7: After `isFoo<T>(x)` the variable x narrows to T. ReturnType
// resolves T to `unknown`. Joined: unknown.
#[test]
#[ignore = "typeinfo currently does not propagate `x is T` predicate narrowing on a bare generic through `ReturnType<typeof fn>` to the T-resolves-to-`unknown` emission; keep as the future Sb10 x-is-T-predicate-on-generic contract"]
fn substitution_types_sb10_x_is_t_predicate_on_generic() {
    let expr = resolve_alias("Sb10Result");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// ----- 11) Generic narrowed via `in` operator --------------------------
// TS7: `"a" in x` narrows T's apparent type to the `{ a: 1 }` arm.
// Else returns the `{ b: 2 }` arm. Joined: `1 | 2`.
#[test]
#[ignore = "typeinfo currently does not propagate `in`-operator narrowing on a constrained-union generic through `ReturnType<typeof fn>` to the literal-union `1 | 2` emission; keep as the future Sb11 generic-narrowed-via-in-operator contract"]
fn substitution_types_sb11_generic_narrowed_via_in_operator() {
    let expr = resolve_alias("Sb11Result");
    assert_number_literal_union(&expr, &[1.0, 2.0]);
}

// ----- 12) Truthiness narrowing on `T extends string | undefined` ------
// TS7: Truthy guard removes `undefined`. With the constraint
// `string | undefined`, truthy reduces to `string`. Joined: string.
#[test]
#[ignore = "typeinfo currently does not propagate truthiness narrowing on a `T extends string | undefined` constrained generic through `ReturnType<typeof fn>` to the `string` emission; keep as the future Sb12 truthiness-on-T-or-undefined contract"]
fn substitution_types_sb12_truthiness_on_t_or_undefined() {
    let expr = resolve_alias("Sb12Result");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 13) Substitution carried across destructure ---------------------
// TS7: Destructuring `{ val }` from T-extends-{val:number} reads `val`
// as the constraint's apparent property type: number.
#[test]
#[ignore = "typeinfo currently does not propagate substitution-type apparent-type access across an object destructure on a constrained generic through `ReturnType<typeof fn>` to `number`; keep as the future Sb13 substitution-carried-across-destructure contract"]
fn substitution_types_sb13_substitution_carried_across_destructure() {
    let expr = resolve_alias("Sb13Result");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 14) Default type arg with narrowing -----------------------------
// TS7 quirk: `<T = string>` default does NOT apply inside `ReturnType<typeof fn>`.
// The bare function type is still unparameterised; ReturnType resolves
// T to `unknown` (not the default). Joined: unknown.
#[test]
#[ignore = "typeinfo currently does not model TS7's defaults-do-not-apply-inside-ReturnType rule; the emission must be the un-narrowed `unknown`, NOT the default `string`. Keep as the future Sb14 default-type-arg-ignored-by-return-type contract"]
fn substitution_types_sb14_default_type_arg_ignored_by_return_type() {
    let expr = resolve_alias("Sb14Result");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// ----- 15) Recursive generic substitution ------------------------------
// TS7: Self-recursion `f(x)` is NOT a substitution event. The declared
// return type `T` stays as `T` -> `unknown` via ReturnType.
#[test]
#[ignore = "typeinfo currently does not propagate the recursive-call return type on a bare generic through `ReturnType<typeof fn>` to the un-substituted T-resolves-to-`unknown` emission; keep as the future Sb15 recursive-generic-substitution contract"]
fn substitution_types_sb15_recursive_generic_substitution() {
    let expr = resolve_alias("Sb15Result");
    assert_primitive(&expr, PrimitiveName::Unknown);
}
