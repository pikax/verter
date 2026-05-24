// @ai-generated - apparent-type method-access scenarios on primitives.
//
// Each fn invokes one apparent-type member (String.prototype.*,
// Number.prototype.*, Array.prototype.*, Boolean.prototype.*,
// BigInt.prototype.*, Symbol.prototype.*, or via a generic constraint),
// so `ReturnType<typeof fn>` encodes the resulting primitive/array
// emission. Primitives have no own methods; TS resolves the member by
// looking up the "apparent type" (the wrapper interface in lib.d.ts:
// String, Number, Array<T>, Boolean, BigInt, Symbol).
//
// All emissions verified out-of-band against tsgo 7.0.0-dev.20260523.1
// via IsExactly probes BEFORE encoding the Rust assertions.
//
// Documented TS7 emissions (NOT fixture bugs):
//   * Ap05/Ap06/Ap07/Ap11/Ap13: `.toFixed`, `.toString`, `.toExponential`
//     are declared on Number.prototype / Boolean.prototype /
//     BigInt.prototype returning `string` (NOT `number`). `(42).toString()`
//     is string, full stop.
//   * Ap12: `Boolean.prototype.valueOf()` returns `boolean` (it unwraps
//     the wrapper). `true.toString()` returns string; `true.valueOf()`
//     returns boolean.
//   * Ap14: `Symbol("x").description` is declared as `string | undefined`
//     in lib.es2019.symbol.d.ts — descriptions are optional.
//   * Ap15: With `function f<T extends string>(x: T) { return x.length; }`
//     the apparent type of `x` inside the body is `T & string`'s apparent
//     type (= String). `x.length` resolves to `number`. ReturnType<typeof f>
//     for an unspecified T is still `number` because `.length` is invariant
//     over the constraint.

// ----- 1) "hello".length -> number ---------------------------------------
export function ap01StringLength() {
  return "hello".length;
}
export type Ap01StringLengthResult = ReturnType<typeof ap01StringLength>;

// ----- 2) "hello".toUpperCase() -> string --------------------------------
export function ap02StringToUpperCase() {
  return "hello".toUpperCase();
}
export type Ap02StringToUpperCaseResult = ReturnType<typeof ap02StringToUpperCase>;

// ----- 3) "hello".charAt(0) -> string ------------------------------------
export function ap03StringCharAt() {
  return "hello".charAt(0);
}
export type Ap03StringCharAtResult = ReturnType<typeof ap03StringCharAt>;

// ----- 4) "hello".slice(0, 2) -> string ----------------------------------
export function ap04StringSlice() {
  return "hello".slice(0, 2);
}
export type Ap04StringSliceResult = ReturnType<typeof ap04StringSlice>;

// ----- 5) (42).toFixed(2) -> string --------------------------------------
// TS7 quirk: returns string, NOT number.
export function ap05NumberToFixed() {
  return (42).toFixed(2);
}
export type Ap05NumberToFixedResult = ReturnType<typeof ap05NumberToFixed>;

// ----- 6) (42).toString() -> string --------------------------------------
export function ap06NumberToString() {
  return (42).toString();
}
export type Ap06NumberToStringResult = ReturnType<typeof ap06NumberToString>;

// ----- 7) (3.14).toExponential(2) -> string ------------------------------
export function ap07NumberToExponential() {
  return (3.14).toExponential(2);
}
export type Ap07NumberToExponentialResult = ReturnType<typeof ap07NumberToExponential>;

// ----- 8) [1, 2, 3].length -> number -------------------------------------
export function ap08ArrayLength() {
  return [1, 2, 3].length;
}
export type Ap08ArrayLengthResult = ReturnType<typeof ap08ArrayLength>;

// ----- 9) [1, 2, 3].map(x => x * 2) -> number[] --------------------------
export function ap09ArrayMap() {
  return [1, 2, 3].map((x) => x * 2);
}
export type Ap09ArrayMapResult = ReturnType<typeof ap09ArrayMap>;

// ----- 10) [1, 2, 3].filter(x => x > 1) -> number[] ----------------------
export function ap10ArrayFilter() {
  return [1, 2, 3].filter((x) => x > 1);
}
export type Ap10ArrayFilterResult = ReturnType<typeof ap10ArrayFilter>;

// ----- 11) true.toString() -> string -------------------------------------
export function ap11BooleanToString() {
  return true.toString();
}
export type Ap11BooleanToStringResult = ReturnType<typeof ap11BooleanToString>;

// ----- 12) false.valueOf() -> boolean ------------------------------------
// TS7 quirk: `.valueOf()` unwraps the wrapper and returns the primitive.
export function ap12BooleanValueOf() {
  return false.valueOf();
}
export type Ap12BooleanValueOfResult = ReturnType<typeof ap12BooleanValueOf>;

// ----- 13) 123n.toString() -> string -------------------------------------
export function ap13BigintToString() {
  return 123n.toString();
}
export type Ap13BigintToStringResult = ReturnType<typeof ap13BigintToString>;

// ----- 14) Symbol("x").description -> string | undefined -----------------
// TS7 quirk: descriptions are declared `string | undefined` in
// lib.es2019.symbol.d.ts (creation argument is optional).
export function ap14SymbolDescription() {
  return Symbol("x").description;
}
export type Ap14SymbolDescriptionResult = ReturnType<typeof ap14SymbolDescription>;

// ----- 15) Apparent type via generic constraint -> number ----------------
// `T extends string` -> apparent type of x is String -> `.length` is number.
export function ap15GenericConstraintLength<T extends string>(x: T) {
  return x.length;
}
export type Ap15GenericConstraintLengthResult = ReturnType<typeof ap15GenericConstraintLength>;
