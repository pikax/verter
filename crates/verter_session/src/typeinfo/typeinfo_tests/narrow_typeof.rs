//! @ai-generated - typeof-narrowing contracts.
//!
//! Each test pins ONE TS7 emission for a `typeof x === "..."` (or `!==`)
//! discriminator across primitives, unions with unknown, generic T,
//! switch/exhaustive, negated guards, literal-typed RHS, and compound
//! `&&` chains. Each scenario is one function in the fixture; one alias
//! per function via `type NtNNResult = ReturnType<typeof ntNN>`.
//!
//! All emissions verified against tsgo 7.0.0-dev.20260523.1 via IsExactly
//! probes BEFORE encoding the Rust assertions.
//!
//! Each scenario is one `*Result = ReturnType<typeof ntXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.

use super::support::*;

const NARROW_TYPEOF: &str = include_str!("fixtures/narrow_typeof.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/narrow_typeof.ts", NARROW_TYPEOF);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/narrow_typeof.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) typeof === "string" on string | number ------------------------
// TS7: if-branch returns string, else returns number. Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt01 typeof-narrowing string-on-binary-union contract"]
fn narrow_typeof_nt01_string_on_binary_union() {
    let expr = resolve_alias("Nt01StringOnUnionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 2) typeof === "number" on string | number | boolean --------------
// TS7: if-branch returns number, else returns string | boolean.
// Joined: string | number | boolean.
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt02 typeof-narrowing number-on-triple-union contract"]
fn narrow_typeof_nt02_number_on_triple_union() {
    let expr = resolve_alias("Nt02NumberOnTripleResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
}

// ----- 3) typeof === "boolean" on string | boolean ----------------------
// TS7: if-branch returns boolean, else returns string. Joined: string | boolean.
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt03 typeof-narrowing boolean-on-union contract"]
fn narrow_typeof_nt03_boolean_on_union() {
    let expr = resolve_alias("Nt03BooleanOnUnionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
}

// ----- 4) typeof === "object" on Record<string, unknown> | string -------
// TS7 quirk: `typeof null === "object"`, but null is NOT in the original
// union here so it is NOT introduced into the if-branch. if-branch:
// Record<string, unknown>; else: string. Joined: Record<string, unknown> | string.
// We assert the union contains the string primitive arm (the Record<> arm
// projects to an object/intersection — its precise shape is left to the
// resolver and is not the contract under test here).
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt04 typeof-narrowing object-on-union contract (null NOT introduced when absent from original)"]
fn narrow_typeof_nt04_object_on_union_keeps_no_null() {
    let expr = resolve_alias("Nt04ObjectOnUnionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    // The object arm must NOT introduce Null because null is not in original.
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_null = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::Null)));
    assert!(
        !has_null,
        "joined return must NOT contain Null since null was absent from original union; got {expr:?}"
    );
}

// ----- 5) typeof === "function" on (() => void) | string ----------------
// TS7: if-branch is the function type, else is string. Joined: (() => void) | string.
// We assert the union contains both a function arm and a string primitive arm.
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt05 typeof-narrowing function-on-union contract"]
fn narrow_typeof_nt05_function_on_union() {
    let expr = resolve_alias("Nt05FunctionOnUnionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_function = arms.iter().any(|arm| matches!(arm, TypeExpr::Function(_)));
    assert!(
        has_function,
        "joined return must contain a function arm; got {expr:?}"
    );
}

// ----- 6) typeof === "undefined" on string | undefined ------------------
// TS7: if-branch: undefined; else: string. Joined: string | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt06 typeof-narrowing undefined-on-union contract"]
fn narrow_typeof_nt06_undefined_on_union() {
    let expr = resolve_alias("Nt06UndefinedOnUnionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 7) typeof === "bigint" on bigint | string ------------------------
// TS7: if-branch: bigint; else: string. Joined: bigint | string.
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt07 typeof-narrowing bigint-on-union contract"]
fn narrow_typeof_nt07_bigint_on_union() {
    let expr = resolve_alias("Nt07BigintOnUnionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::BigInt);
    assert_union_contains_primitive(&expr, PrimitiveName::String);
}

// ----- 8) typeof === "symbol" on symbol | string ------------------------
// TS7: if-branch: symbol; else: string. Joined: symbol | string.
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt08 typeof-narrowing symbol-on-union contract"]
fn narrow_typeof_nt08_symbol_on_union() {
    let expr = resolve_alias("Nt08SymbolOnUnionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::Symbol);
    assert_union_contains_primitive(&expr, PrimitiveName::String);
}

// ----- 9) typeof === "string" on unknown --------------------------------
// TS7: if-branch narrows to string; else stays unknown. Both branches
// return x, so the joined return is unknown (string is subsumed).
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt09 typeof-narrowing string-on-unknown contract (joined return collapses to unknown)"]
fn narrow_typeof_nt09_string_on_unknown() {
    let expr = resolve_alias("Nt09StringOnUnknownResult");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// ----- 10) typeof === "string" on generic T -----------------------------
// TS7 quirk: in the if-branch x narrows to `T & string`. For ReturnType
// over `nt10StringOnGeneric` with no type argument supplied, T defaults
// to unknown and the joined return is unknown.
#[test]
#[ignore = "typeinfo currently does not propagate `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt10 typeof-narrowing string-on-unbound-generic contract (collapses to unknown when T unspecified)"]
fn narrow_typeof_nt10_string_on_unbound_generic() {
    let expr = resolve_alias("Nt10StringOnGenericResult");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// ----- 11) typeof !== "string" on string | number -----------------------
// TS7: if-branch (negated): number; else: string. Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate negated `typeof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future Nt11 negated-typeof-narrowing on-binary-union contract"]
fn narrow_typeof_nt11_negated_on_binary_union() {
    let expr = resolve_alias("Nt11NegatedOnUnionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 12) switch typeof exhaustive -------------------------------------
// TS7: case "string" -> string, case "number" -> number, case "boolean" ->
// boolean. default is unreachable (`const _exhaustive: never = x`) and
// contributes `never` to the join (absorbed). Joined: string | number | boolean.
#[test]
#[ignore = "typeinfo currently does not propagate switch-on-typeof narrowing across exhaustive case arms with a never-typed default through `ReturnType<typeof fn>`; keep as the future Nt12 switch-typeof-exhaustive contract"]
fn narrow_typeof_nt12_switch_exhaustive() {
    let expr = resolve_alias("Nt12SwitchTypeofResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
    // never absorbed: the joined union must NOT contain `never`.
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_never = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::Never)));
    assert!(
        !has_never,
        "switch-exhaustive joined return must absorb never from the unreachable default; got {expr:?}"
    );
}

// ----- 13) Negated guard with early return ------------------------------
// TS7: `if (typeof x !== "string") return x;` -> the if-body returns
// `number` (negated arm), and the trailing `return x` sees x as `string`
// (only path past the guard). Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate post-guard `typeof`-narrowing across an early-return into the trailing return through `ReturnType<typeof fn>`; keep as the future Nt13 negated-typeof-guard-early-return contract"]
fn narrow_typeof_nt13_negated_guard_early_return() {
    let expr = resolve_alias("Nt13NegatedEarlyReturnResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 14) typeof against a literal-type variable -----------------------
// TS7 quirk: `typeof x === tag` where `tag: "string"` is a literal-typed
// variable does NOT narrow (TS only narrows `typeof` against a STRING-LITERAL
// RHS in the source — not against a variable, even when the variable's
// declared type is the literal `"string"`). Both branches see the full
// original `string | number` union. Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not faithfully model the TS7 quirk that `typeof x === tag` (with tag a literal-typed variable) does NOT narrow through `ReturnType<typeof fn>`; keep as the future Nt14 typeof-vs-literal-variable contract"]
fn narrow_typeof_nt14_compare_literal_var_does_not_narrow() {
    let expr = resolve_alias("Nt14CompareLiteralVarResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 15) Compound typeof + property guard -----------------------------
// TS7: `typeof x === "string" && x.length > 0` -> if-branch x is string
// (length probe requires it); else branch is the FULL original union
// (TS cannot represent "string with length===0" as a distinct arm).
// Both branches return x. Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate compound `typeof && property-guard` narrowing through `ReturnType<typeof fn>`; keep as the future Nt15 compound-typeof-and-property contract"]
fn narrow_typeof_nt15_compound_and_property() {
    let expr = resolve_alias("Nt15CompoundAndResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}
