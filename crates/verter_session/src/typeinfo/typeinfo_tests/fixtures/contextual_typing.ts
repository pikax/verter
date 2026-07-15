// @ai-generated - contextual-typing scenarios.
//
// Each fn pins ONE TS7 emission for a contextual-typing scenario
// (parameter inference from contextual signature, object-literal flow,
// `as const`, `satisfies`, type-parameter constraints, etc.). Each fn
// returns the value whose contextually-typed shape encodes the
// emission, so `ReturnType<typeof fn>` directly captures the contract.
// All emissions verified out-of-band against tsgo 7.0.0-dev.20260523.1
// via IsExactly probes.
//
// Documented TS7 quirks (NOT fixture bugs):
//   * Ct09: A `const` initialized from a discriminated-union typed
//     declaration is NARROWED in the return position to the arm of
//     the assigned literal — even though the declared type is the
//     full union. `Ct09Result` is the narrow arm `{kind:"a"; a:1}`,
//     NOT the declared union.
//   * Ct11: `as const` adds a `readonly` modifier to every property.
//     Emission is `{ readonly a: 1 }`, not `{ a: 1 }`.
//   * Ct13: `as` cast on an object literal narrows to the cast target,
//     dropping excess properties. Emission is `{ a: 1 }`, NOT the
//     source literal's wider shape `{ a: 1; b: 2 }`.
//   * Ct14: `satisfies T` validates the source against T but evaluates
//     to the wider satisfies target shape — emission is
//     `{ a: number; b: string }`, NOT the narrow literal
//     `{ a: 1; b: "x" }`.
//   * Ct10: A tuple-typed const declaration produces a tuple emission
//     `[string, number]`, NOT a widened union array `(string|number)[]`.

// ----- 1) Callback parameter from contextual signature ----------------
// `Array<number>.map<U>(cb: (x: number) => U)` flows `number` into the
// callback parameter `x`. `x.toFixed(2)` returns `string`. The map call
// returns `string[]`.
export function ct01() {
  return [1, 2, 3].map((x) => x.toFixed(2));
}
export type Ct01Result = ReturnType<typeof ct01>;

// ----- 2) Same callback, named to characterize the return type ---------
// Identical structure to Ct01; pinned as the read-the-return-type
// contract: the function's return type is `string[]`.
export function ct02() {
  return [1, 2, 3].map((x) => x.toFixed(2));
}
export type Ct02Result = ReturnType<typeof ct02>;

// ----- 3) Object literal assignment from typed target ------------------
// `const o: { a: 1; b: 2 } = { a: 1, b: 2 }` — declared type is the
// literal shape. `typeof o` is `{ a: 1; b: 2 }` (literal types preserved).
export function ct03() {
  const o: { a: 1; b: 2 } = { a: 1, b: 2 };
  return o;
}
export type Ct03Result = ReturnType<typeof ct03>;

// ----- 4) Object literal in function call ------------------------------
// `ct04(o: { tag: "x" })` contextually types the argument literal as
// `{ tag: "x" }`. Returning `o` publishes that contextually-typed shape.
export function ct04(o: { tag: "x" }) {
  return o;
}
export function ct04Call() {
  return ct04({ tag: "x" });
}
export type Ct04Result = ReturnType<typeof ct04Call>;

// ----- 5) Return-type contextual flow ----------------------------------
// `const fn05: () => 42 = () => 42` — declared return is `42`. Calling
// `fn05()` produces `42`, NOT `number`.
export const fn05: () => 42 = () => 42;
export function ct05() {
  return fn05();
}
export type Ct05Result = ReturnType<typeof ct05>;

// ----- 6) Parenthesized expression preserves context -------------------
// Wrapping the arrow in parens does NOT erase contextual typing.
// `const fn06: () => 42 = (() => 42)` still returns `42`.
export const fn06: () => 42 = () => 42;
export function ct06() {
  return fn06();
}
export type Ct06Result = ReturnType<typeof ct06>;

// ----- 7) `as` cast erases context -------------------------------------
// `const fn07: () => number = () => 42 as number` — the `as` cast on
// the body widens to `number`. The function's declared signature is
// `() => number`. Calling `fn07()` returns `number`.
export function ct07() {
  const fn07: () => number = () => 42 as number;
  return fn07();
}
export type Ct07Result = ReturnType<typeof ct07>;

// ----- 8) JSX-like attribute contextual typing -------------------------
// Emulates a JSX attribute pass-through via a regular function call:
// `take(p: Props)` flows the `Props` shape onto the literal argument.
// `take({ count: 1 })` returns `p.count`, which is `1`.
export function ct08() {
  type Props = { count: 1 };
  function take(p: Props) {
    return p.count;
  }
  return take({ count: 1 });
}
export type Ct08Result = ReturnType<typeof ct08>;

// ----- 9) Discriminated union contextual (with narrowing) --------------
// `const x: U = { kind: "a", a: 1 }` is NARROWED to the assigned arm in
// the return position. Emission is the narrow arm `{ kind: "a"; a: 1 }`,
// NOT the declared union — TS7 narrowing applies here.
export function ct09() {
  const x: { kind: "a"; a: 1 } | { kind: "b"; b: 2 } = { kind: "a", a: 1 };
  return x;
}
export type Ct09Result = ReturnType<typeof ct09>;

// ----- 10) Array literal contextually typed as tuple -------------------
// `const t: [string, number] = ["a", 1]` — the array literal is
// contextually typed as a tuple. Emission is `[string, number]`,
// NOT `(string | number)[]`.
export function ct10() {
  const t: [string, number] = ["a", 1];
  return t;
}
export type Ct10Result = ReturnType<typeof ct10>;

// ----- 11) `as const` overrides contextual widening --------------------
// `{ a: 1 } as const` produces `{ readonly a: 1 }`. Note the `readonly`
// modifier — `as const` does NOT just preserve the literal type, it
// also marks every property readonly.
export function ct11() {
  return { a: 1 } as const;
}
export type Ct11Result = ReturnType<typeof ct11>;

// ----- 12) Function expression argument type from contextual signature -
// `const e: E<number> = (f) => f(1)` flows `E<number>` onto `e`, which
// in turn contextually types the callback `f` parameter `x` as `number`.
// `e(x => x.toFixed(2))` invokes `f` with a `number`, gets `string`.
// The outer call also returns `string`.
export function ct12() {
  type E<T> = (f: (x: T) => string) => string;
  const e: E<number> = (f) => f(1);
  return e((x) => x.toFixed(2));
}
export type Ct12Result = ReturnType<typeof ct12>;

// ----- 13) Object literal `as` cast narrows shape ----------------------
// `{ a: 1, b: 2 } as { a: 1 }` casts to the narrower target shape.
// `typeof o` is `{ a: 1 }`, NOT the source literal's `{ a: 1; b: 2 }`.
export function ct13() {
  const o = { a: 1, b: 2 } as { a: 1 };
  return o;
}
export type Ct13Result = ReturnType<typeof ct13>;

// ----- 14) `satisfies` operator validates against wider type -----------
// `satisfies T` validates the source against T but evaluates to the
// WIDER satisfies target shape — emission is `{ a: number; b: string }`,
// NOT the narrow literal `{ a: 1; b: "x" }`. The `const` keyword does
// NOT preserve literal types in this case.
export function ct14() {
  const o = { a: 1, b: "x" } satisfies { a: number; b: string };
  return o;
}
export type Ct14Result = ReturnType<typeof ct14>;

// ----- 15) Contextual type via type parameter constraint ---------------
// `call15<T>(f: (x: T) => T, x: T): T` — TS infers `T` from the second
// argument (`1` → `number`), then contextually types the callback
// parameter `x` as `number`. `x + 1` returns `number`. The outer call
// returns `T = number`.
export function call15<T>(f: (x: T) => T, x: T): T {
  return f(x);
}
export function ct15() {
  return call15((x) => x + 1, 1);
}
export type Ct15Result = ReturnType<typeof ct15>;
