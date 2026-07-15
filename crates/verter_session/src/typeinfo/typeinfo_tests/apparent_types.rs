//! @ai-generated - apparent-type method-access contracts on primitives.
//!
//! Each test pins ONE TS7 emission for an apparent-type member lookup
//! on a primitive expression (String/Number/Array/Boolean/BigInt/Symbol
//! wrapper interfaces, plus apparent type via generic constraint).
//! Each scenario is one function in the fixture; one alias per function
//! via `type ApNNResult = ReturnType<typeof apNN>`.
//!
//! All emissions verified against tsgo 7.0.0-dev.20260523.1 via IsExactly
//! probes BEFORE encoding the Rust assertions.
//!
//! Each scenario is one `*Result = ReturnType<typeof apXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.
//!
//! These tests are currently ignored because Verter's typeinfo resolver
//! does not yet perform apparent-type member lookup on primitive
//! expressions inside function bodies for `ReturnType<typeof fn>`
//! projection. Apparent-type member resolution requires (a) inferring
//! the call/member-access result type from the lib.d.ts wrapper interface
//! (String, Number, Array, Boolean, BigInt, Symbol) given a primitive
//! receiver, and (b) propagating that result through the function
//! return-type inference to the `ReturnType<...>` projection. Both are
//! missing in the current resolver.
//!
//! Each test name and ignore reason references the future contract.
//! When apparent-type method dispatch lands, drop the `#[ignore]` and
//! the assertions should pass as written.
//!
//! These tests are a forward-looking pin; do NOT weaken them.
//!
//! See also `narrow_typeof.rs` for the typeof-narrowing tier and
//! support.rs for shared host setup helpers.

use super::support::*;

const APPARENT_TYPES: &str = include_str!("fixtures/apparent_types.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/apparent_types.ts", APPARENT_TYPES);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/apparent_types.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) "hello".length -> number ---------------------------------------
// TS7: `.length` on a string literal expression resolves through the
// String wrapper apparent type to `number`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type member lookup (String.prototype.length) on a primitive string expression inside `ReturnType<typeof fn>`; keep as the future Ap01 apparent-type string-length contract"]
fn apparent_types_ap01_string_length() {
    let expr = resolve_alias("Ap01StringLengthResult");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 2) "hello".toUpperCase() -> string --------------------------------
// TS7: `.toUpperCase()` resolves through String.prototype.toUpperCase
// to `string`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (String.prototype.toUpperCase) on a primitive string expression inside `ReturnType<typeof fn>`; keep as the future Ap02 apparent-type string-toUpperCase contract"]
fn apparent_types_ap02_string_to_upper_case() {
    let expr = resolve_alias("Ap02StringToUpperCaseResult");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 3) "hello".charAt(0) -> string ------------------------------------
// TS7: `.charAt(0)` resolves through String.prototype.charAt to `string`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (String.prototype.charAt) on a primitive string expression inside `ReturnType<typeof fn>`; keep as the future Ap03 apparent-type string-charAt contract"]
fn apparent_types_ap03_string_char_at() {
    let expr = resolve_alias("Ap03StringCharAtResult");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 4) "hello".slice(0, 2) -> string ----------------------------------
// TS7: `.slice(0, 2)` resolves through String.prototype.slice to `string`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (String.prototype.slice) on a primitive string expression inside `ReturnType<typeof fn>`; keep as the future Ap04 apparent-type string-slice contract"]
fn apparent_types_ap04_string_slice() {
    let expr = resolve_alias("Ap04StringSliceResult");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 5) (42).toFixed(2) -> string --------------------------------------
// TS7 quirk: Number.prototype.toFixed returns `string`, NOT `number`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (Number.prototype.toFixed -> string) on a primitive number expression inside `ReturnType<typeof fn>`; keep as the future Ap05 apparent-type number-toFixed contract"]
fn apparent_types_ap05_number_to_fixed() {
    let expr = resolve_alias("Ap05NumberToFixedResult");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 6) (42).toString() -> string --------------------------------------
// TS7: Number.prototype.toString returns `string`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (Number.prototype.toString -> string) on a primitive number expression inside `ReturnType<typeof fn>`; keep as the future Ap06 apparent-type number-toString contract"]
fn apparent_types_ap06_number_to_string() {
    let expr = resolve_alias("Ap06NumberToStringResult");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 7) (3.14).toExponential(2) -> string ------------------------------
// TS7: Number.prototype.toExponential returns `string`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (Number.prototype.toExponential -> string) on a primitive number expression inside `ReturnType<typeof fn>`; keep as the future Ap07 apparent-type number-toExponential contract"]
fn apparent_types_ap07_number_to_exponential() {
    let expr = resolve_alias("Ap07NumberToExponentialResult");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 8) [1, 2, 3].length -> number -------------------------------------
// TS7: `.length` on a number-literal array resolves through Array<T>
// apparent type to `number`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type member lookup (Array<T>.length -> number) on an array-literal expression inside `ReturnType<typeof fn>`; keep as the future Ap08 apparent-type array-length contract"]
fn apparent_types_ap08_array_length() {
    let expr = resolve_alias("Ap08ArrayLengthResult");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 9) [1, 2, 3].map(x => x * 2) -> number[] --------------------------
// TS7: `.map(x => x * 2)` resolves through Array<number>.map<U>(cb) with
// U inferred as `number` -> emits `number[]`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (Array<T>.map<U> with U inferred from callback) on an array-literal expression inside `ReturnType<typeof fn>`; keep as the future Ap09 apparent-type array-map contract"]
fn apparent_types_ap09_array_map() {
    let expr = resolve_alias("Ap09ArrayMapResult");
    assert_array_of_primitive(&expr, PrimitiveName::Number);
}

// ----- 10) [1, 2, 3].filter(x => x > 1) -> number[] ----------------------
// TS7: `.filter(x => x > 1)` resolves through Array<number>.filter ->
// `number[]` (no type-predicate narrowing here).
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (Array<T>.filter -> T[]) on an array-literal expression inside `ReturnType<typeof fn>`; keep as the future Ap10 apparent-type array-filter contract"]
fn apparent_types_ap10_array_filter() {
    let expr = resolve_alias("Ap10ArrayFilterResult");
    assert_array_of_primitive(&expr, PrimitiveName::Number);
}

// ----- 11) true.toString() -> string -------------------------------------
// TS7: Boolean.prototype.toString returns `string`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (Boolean.prototype.toString -> string) on a primitive boolean expression inside `ReturnType<typeof fn>`; keep as the future Ap11 apparent-type boolean-toString contract"]
fn apparent_types_ap11_boolean_to_string() {
    let expr = resolve_alias("Ap11BooleanToStringResult");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 12) false.valueOf() -> boolean ------------------------------------
// TS7 quirk: Boolean.prototype.valueOf unwraps the wrapper -> `boolean`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (Boolean.prototype.valueOf -> boolean) on a primitive boolean expression inside `ReturnType<typeof fn>`; keep as the future Ap12 apparent-type boolean-valueOf contract"]
fn apparent_types_ap12_boolean_value_of() {
    let expr = resolve_alias("Ap12BooleanValueOfResult");
    assert_primitive(&expr, PrimitiveName::Boolean);
}

// ----- 13) 123n.toString() -> string -------------------------------------
// TS7: BigInt.prototype.toString returns `string`.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type method dispatch (BigInt.prototype.toString -> string) on a primitive bigint expression inside `ReturnType<typeof fn>`; keep as the future Ap13 apparent-type bigint-toString contract"]
fn apparent_types_ap13_bigint_to_string() {
    let expr = resolve_alias("Ap13BigintToStringResult");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 14) Symbol("x").description -> string | undefined -----------------
// TS7 quirk: descriptions are `string | undefined` (Symbol creation
// argument is optional per lib.es2019.symbol.d.ts).
#[test]
#[ignore = "typeinfo currently does not perform apparent-type member lookup (Symbol.prototype.description -> string | undefined) on a Symbol(...) expression inside `ReturnType<typeof fn>`; keep as the future Ap14 apparent-type symbol-description contract"]
fn apparent_types_ap14_symbol_description() {
    let expr = resolve_alias("Ap14SymbolDescriptionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 15) Apparent type via generic constraint -> number ----------------
// TS7: `T extends string` -> apparent type of `x: T` is the String wrapper
// -> `x.length` resolves to `number`. The constraint apparent-type lookup
// is independent of the (unspecified) T binding in ReturnType<typeof f>.
#[test]
#[ignore = "typeinfo currently does not perform apparent-type member lookup through a generic constraint (T extends string -> apparent type String -> .length: number) inside `ReturnType<typeof fn>`; keep as the future Ap15 apparent-type generic-constraint contract"]
fn apparent_types_ap15_generic_constraint_length() {
    let expr = resolve_alias("Ap15GenericConstraintLengthResult");
    assert_primitive(&expr, PrimitiveName::Number);
}
