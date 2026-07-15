// @ai-generated - equality-narrowing scenarios.
//
// Each fn returns the value(s) reachable inside / outside an equality guard,
// so ReturnType<typeof fn> encodes the joined narrowing emission.
// All emissions verified out-of-band against tsgo 7.0.0-dev.20260523.1
// via IsExactly probes.
//
// Documented TS7 quirks (NOT fixture bugs):
//   * Eq08: `x === "a"` on `x: string` does NOT narrow the if-branch down to
//     the literal `"a"`. TS keeps `x` as `string` in both branches when the
//     declared type is wider than the literal RHS. Joined: string.
//   * Eq09: `x === "a"` on `x: string | number` narrows the if-branch to
//     `string` (the union arm containing the literal) but NOT down to the
//     literal `"a"`. The else branch keeps the FULL union `string | number`
//     because TS cannot represent "string excluding 'a'" as a distinct arm.
//     Joined: string | number.
//   * Eq10: `x === y` between two unions (`x: "a"|"b"`, `y: "b"|"c"`) does
//     NOT narrow either operand. Both branches return x at its original
//     type. Joined: "a" | "b".
//   * Eq14: `x === 0` on `x: number` does NOT narrow to the literal `0`.
//     Both branches see the original `number`. Joined: number.
//   * Eq15: `x === NaN` is always false (`NaN !== NaN`) and TS7 reports it
//     as TS2845 if written as a literal `NaN` RHS. To get a 0-diagnostic
//     fixture we bind `Number.NaN` to a local and compare against the
//     local. Either way, no narrowing occurs. Joined: number.

// ----- 1) x === "a" on "a" | "b" ---------------------------------------
// if-branch: "a". else: "b". Joined: "a" | "b".
export function eq01StringLiteralOnUnion(x: "a" | "b") {
  if (x === "a") return x;
  return x;
}
export type Eq01Result = ReturnType<typeof eq01StringLiteralOnUnion>;

// ----- 2) x !== "a" (negated) on "a" | "b" -----------------------------
// if-branch (negated): "b". else: "a". Joined: "a" | "b".
export function eq02NegatedStringLiteralOnUnion(x: "a" | "b") {
  if (x !== "a") return x;
  return x;
}
export type Eq02Result = ReturnType<typeof eq02NegatedStringLiteralOnUnion>;

// ----- 3) x === 1 on 1 | 2 | 3 -----------------------------------------
// if-branch: 1. else: 2 | 3. Joined: 1 | 2 | 3.
export function eq03NumberLiteralOnTriple(x: 1 | 2 | 3) {
  if (x === 1) return x;
  return x;
}
export type Eq03Result = ReturnType<typeof eq03NumberLiteralOnTriple>;

// ----- 4) x === true on boolean ---------------------------------------
// boolean = true | false. if-branch: true. else: false.
// Joined: boolean (= true | false).
export function eq04BooleanTrueOnBoolean(x: boolean) {
  if (x === true) return x;
  return x;
}
export type Eq04Result = ReturnType<typeof eq04BooleanTrueOnBoolean>;

// ----- 5) x === null on string | null ---------------------------------
// if-branch: null. else: string. Joined: string | null.
export function eq05NullOnNullableString(x: string | null) {
  if (x === null) return x;
  return x;
}
export type Eq05Result = ReturnType<typeof eq05NullOnNullableString>;

// ----- 6) x === undefined on string | undefined -----------------------
// if-branch: undefined. else: string. Joined: string | undefined.
export function eq06UndefinedOnOptionalString(x: string | undefined) {
  if (x === undefined) return x;
  return x;
}
export type Eq06Result = ReturnType<typeof eq06UndefinedOnOptionalString>;

// ----- 7) x == null (double-equals) on string | null | undefined ------
// `x == null` matches BOTH null and undefined (loose equality). if-branch:
// null | undefined. else: string. Joined: string | null | undefined.
export function eq07DoubleEqualsNullOnNullish(x: string | null | undefined) {
  if (x == null) return x;
  return x;
}
export type Eq07Result = ReturnType<typeof eq07DoubleEqualsNullOnNullish>;

// ----- 8) x === "a" on string -----------------------------------------
// TS7 quirk: a wider declared type (`string`) is NOT narrowed to the literal
// `"a"`. Both branches see `string`. Joined: string.
export function eq08StringLiteralOnString(x: string) {
  if (x === "a") return x;
  return x;
}
export type Eq08Result = ReturnType<typeof eq08StringLiteralOnString>;

// ----- 9) x === "a" on string | number --------------------------------
// TS7 quirk: if-branch narrows to `string` (the union arm whose primitive
// covers the literal) but NOT down to the literal `"a"`. The else branch
// keeps the FULL union. Joined: string | number.
export function eq09StringLiteralOnPrimitiveUnion(x: string | number) {
  if (x === "a") return x;
  return x;
}
export type Eq09Result = ReturnType<typeof eq09StringLiteralOnPrimitiveUnion>;

// ----- 10) x === y between two unions ---------------------------------
// TS7 quirk: mutual equality between two unions does NOT refine either
// operand. Both branches return x at its declared type. Joined: "a" | "b".
export function eq10TwoUnionsMutualEquality(x: "a" | "b", y: "b" | "c") {
  if (x === y) return x;
  return x;
}
export type Eq10Result = ReturnType<typeof eq10TwoUnionsMutualEquality>;

// ----- 11) Impossible compound: x === null && x === undefined ---------
// The conjunction is structurally never satisfiable. if-branch is `never`
// (absorbed). else: the original union. Joined: string | null | undefined.
export function eq11ImpossibleCompound(x: string | null | undefined) {
  if (x === null && x === undefined) return x;
  return x;
}
export type Eq11Result = ReturnType<typeof eq11ImpossibleCompound>;

// ----- 12) Property equality on discriminant --------------------------
// Equality narrowing on a discriminated-union tag. if-branch: returns s.a
// (string). else: returns s.b (number). Joined: string | number.
export type Eq12Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function eq12PropertyEqualityDiscriminant(s: Eq12Shape) {
  if (s.kind === "a") return s.a;
  return s.b;
}
export type Eq12Result = ReturnType<typeof eq12PropertyEqualityDiscriminant>;

// ----- 13) Equality of `as const` literal -----------------------------
// `const TAG = "a" as const` has type `"a"`. Comparing against TAG works
// identically to comparing against the literal. Joined: "a" | "b".
export function eq13AsConstLiteralRhs(x: "a" | "b") {
  const TAG = "a" as const;
  if (x === TAG) return x;
  return x;
}
export type Eq13Result = ReturnType<typeof eq13AsConstLiteralRhs>;

// ----- 14) x === 0 on number ------------------------------------------
// TS7 quirk: the wider declared type `number` is NOT narrowed to the
// literal `0`. Both branches see `number`. Joined: number.
export function eq14NumberLiteralOnNumber(x: number) {
  if (x === 0) return x;
  return x;
}
export type Eq14Result = ReturnType<typeof eq14NumberLiteralOnNumber>;

// ----- 15) NaN equality is always false -------------------------------
// `NaN !== NaN` by definition, so `x === NaN` (or `x === NAN` via a
// `Number.NaN`-bound local to dodge TS2845's "always false" lint) never
// narrows. Both branches see `number`. Joined: number.
export function eq15NaNEqualityNoNarrowing(x: number) {
  const NAN = Number.NaN;
  if (x === NAN) return x;
  return x;
}
export type Eq15Result = ReturnType<typeof eq15NaNEqualityNoNarrowing>;
