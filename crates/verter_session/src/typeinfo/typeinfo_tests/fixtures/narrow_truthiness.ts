// @ai-generated - truthiness-narrowing scenarios.
//
// Each fn returns the value(s) reachable inside / outside an `if (x)` /
// `if (!x)` truthiness guard, so ReturnType<typeof fn> encodes the joined
// narrowing emission. All emissions verified out-of-band against tsgo
// 7.0.0-dev.20260523.1 via IsExactly probes.
//
// Documented TS7 quirks (NOT fixture bugs):
//   * Tr04: `if (x)` on a wider declared `string` does NOT narrow the else
//     branch to `""`. TS keeps `x` as `string` in BOTH branches when the
//     declared type is the wider primitive. Joined: string.
//   * Tr05: `if (x)` on `0 | 1 | 2` DOES narrow — if-branch narrows to
//     `1 | 2` (the truthy arms) and else narrows to `0` (the falsy arm).
//     Joined: 0 | 1 | 2.
//   * Tr06: `if (x)` on `"" | "a" | "b"` DOES narrow — if-branch narrows
//     to `"a" | "b"` and else narrows to `""`. Joined: "" | "a" | "b".
//   * Tr07: `if (x)` on `boolean` (= `true | false`) narrows — if-branch
//     to `true` and else to `false`. Joined: boolean.
//   * Tr11: `if (x)` on `unknown` narrows the if-branch to `{}` (the
//     non-nullish truthy upper bound) and leaves the else as `unknown`.
//     The joined return collapses to `unknown` because `{}` is subsumed.
//   * Tr12: `if (x)` on `object | null` DOES narrow out `null` in the
//     if-branch (because `null` is structurally falsy in TS's flow rules),
//     producing if-branch `object` and else `null`. Joined: object | null.
//     (TS does NOT narrow plain `object` further, unlike string/number,
//     because `object` has no non-empty falsy subset to split off.)
//   * Tr14: `if (x)` on `number | undefined` narrows the if-branch to
//     `number` (keeping `0` because `0` is a number, not a separate arm)
//     and leaves the else as `number | undefined` (since `0` is falsy too).
//     Joined: number | undefined.

// ----- 1) if (x) on string | undefined ----------------------------------
// if-branch: string. else: undefined. Joined: string | undefined.
export function tr01StringOrUndefined(x: string | undefined) {
  if (x) return x;
  return x;
}
export type Tr01Result = ReturnType<typeof tr01StringOrUndefined>;

// ----- 2) if (x) on string | null ---------------------------------------
// if-branch: string. else: null. Joined: string | null.
export function tr02StringOrNull(x: string | null) {
  if (x) return x;
  return x;
}
export type Tr02Result = ReturnType<typeof tr02StringOrNull>;

// ----- 3) if (x) on string | null | undefined ---------------------------
// if-branch: string. else: null | undefined. Joined: string | null | undefined.
export function tr03StringOrNullish(x: string | null | undefined) {
  if (x) return x;
  return x;
}
export type Tr03Result = ReturnType<typeof tr03StringOrNullish>;

// ----- 4) if (x) on string (no nullable) --------------------------------
// TS7 quirk: a wider declared `string` is NOT narrowed in the else branch
// to `""`. Both branches see `string`. Joined: string.
export function tr04StringNoNullable(x: string) {
  if (x) return x;
  return x;
}
export type Tr04Result = ReturnType<typeof tr04StringNoNullable>;

// ----- 5) if (x) on 0 | 1 | 2 -------------------------------------------
// if-branch: 1 | 2 (truthy arms). else: 0 (falsy arm). Joined: 0 | 1 | 2.
export function tr05NumberLiteralUnion(x: 0 | 1 | 2) {
  if (x) return x;
  return x;
}
export type Tr05Result = ReturnType<typeof tr05NumberLiteralUnion>;

// ----- 6) if (x) on "" | "a" | "b" --------------------------------------
// if-branch: "a" | "b" (truthy arms). else: "" (falsy arm). Joined: "" | "a" | "b".
export function tr06StringLiteralUnion(x: "" | "a" | "b") {
  if (x) return x;
  return x;
}
export type Tr06Result = ReturnType<typeof tr06StringLiteralUnion>;

// ----- 7) if (x) on false | true ----------------------------------------
// if-branch: true. else: false. Joined: boolean (= true | false).
export function tr07BooleanUnion(x: false | true) {
  if (x) return x;
  return x;
}
export type Tr07Result = ReturnType<typeof tr07BooleanUnion>;

// ----- 8) if (!x) (negated) on string | undefined -----------------------
// if-branch (negated): undefined. else: string. Joined: string | undefined.
export function tr08NegatedStringOrUndefined(x: string | undefined) {
  if (!x) return x;
  return x;
}
export type Tr08Result = ReturnType<typeof tr08NegatedStringOrUndefined>;

// ----- 9) Property truthiness guard -------------------------------------
// if-branch returns obj.foo (string). else returns obj.foo (undefined).
// Joined: string | undefined.
export function tr09PropertyTruthiness(obj: { foo: string | undefined }) {
  if (obj.foo) return obj.foo;
  return obj.foo;
}
export type Tr09Result = ReturnType<typeof tr09PropertyTruthiness>;

// ----- 10) Early-return guard -------------------------------------------
// `if (!x) return;` -> after the if, x narrowed to string. The pre-return
// inside the if returns `undefined` (no value). Trailing return: `string`.
// Joined: string | undefined.
export function tr10EarlyReturnGuard(x: string | undefined) {
  if (!x) return;
  return x;
}
export type Tr10Result = ReturnType<typeof tr10EarlyReturnGuard>;

// ----- 11) Truthiness on unknown ----------------------------------------
// TS7 quirk: `if (x)` on `unknown` narrows the if-branch to `{}` (the
// non-nullish truthy upper bound) and leaves the else as `unknown`.
// Joined: {} | unknown = unknown.
export function tr11Unknown(x: unknown) {
  if (x) return x;
  return x;
}
export type Tr11Result = ReturnType<typeof tr11Unknown>;

// ----- 12) Truthiness on object | null ----------------------------------
// if-branch narrows out null -> object. else: null. Joined: object | null.
export function tr12ObjectOrNull(x: object | null) {
  if (x) return x;
  return x;
}
export type Tr12Result = ReturnType<typeof tr12ObjectOrNull>;

// ----- 13) Truthiness compound && chain ---------------------------------
// `if (x && x.length > 0)` on `string | undefined` -> if-branch x is
// `string`. else: full original `string | undefined` (TS cannot represent
// "string with length === 0" as a distinct arm). Joined: string | undefined.
export function tr13CompoundAndChain(x: string | undefined) {
  if (x && x.length > 0) return x;
  return x;
}
export type Tr13Result = ReturnType<typeof tr13CompoundAndChain>;

// ----- 14) Truthiness on number | undefined -----------------------------
// TS7 quirk: if-branch narrows to `number` (TS does NOT narrow out `0`
// because `0` is a number, not a separate arm). else: `number | undefined`
// (since `0` is falsy too). Joined: number | undefined.
export function tr14NumberOrUndefined(x: number | undefined) {
  if (x) return x;
  return x;
}
export type Tr14Result = ReturnType<typeof tr14NumberOrUndefined>;

// ----- 15) Optional chaining truthiness ---------------------------------
// `if (obj?.foo)` on `obj: { foo: string } | undefined` -> in the
// if-branch obj is `{ foo: string }` so `obj.foo` is `string`. The else
// path returns `undefined` explicitly. Joined: string | undefined.
export function tr15OptionalChainTruthiness(obj: { foo: string } | undefined) {
  if (obj?.foo) return obj.foo;
  return undefined;
}
export type Tr15Result = ReturnType<typeof tr15OptionalChainTruthiness>;
