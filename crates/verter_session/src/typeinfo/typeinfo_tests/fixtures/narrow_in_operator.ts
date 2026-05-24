// @ai-generated - `"prop" in x` narrowing scenarios.
//
// Each fn returns the value(s) reachable inside / outside an `in`-operator
// guard, so ReturnType<typeof fn> encodes the joined narrowing emission.
// All emissions verified out-of-band against tsgo 7.0.0-dev.20260523.1
// via IsExactly probes.
//
// Documented TS7 quirks (NOT fixture bugs):
//   * Io02: When BOTH arms of the union have the discriminant property
//     `a`, the `"a" in x` guard does NOT discriminate — the branch sees the
//     full union and the else is `never` (absorbed). `x.a` in the branch is
//     `string | number`.
//   * Io05: For `{ a?: string }`, `"a" in x` does NOT narrow `a` to
//     non-undefined. The branch returns `x.a` which is `string | undefined`.
//     This matches TS's structural-presence semantics — `in` checks the type
//     declaration, not runtime existence.
//   * Io06: `"prop" in x` REQUIRES a non-`unknown` right operand. For
//     `x: unknown` we must first widen via `typeof x === "object" && x !==
//     null` before the `"a" in x` guard. The branch then narrows to
//     `object & Record<"a", unknown>` (the TS7 emission for `in` on an
//     `object`-typed value).
//   * Io07: `"a" in x` on `object` narrows to `object & Record<"a", unknown>`
//     — same emission as Io06 after its widening guards.
//   * Io12: After reassignment inside the if-branch, `x` is re-narrowed to
//     the static type of the reassigned value. Both the if-branch read and
//     the else-branch read of `x.b` are the literal `2`. Joined: `2`.
//   * Io14: A template-literal-typed key (`key: \`prefixed_${string}\``)
//     does NOT participate in `in`-narrowing — only string-literal-typed
//     keys narrow. Both branches return the same union; joined is the
//     original union.
//   * Io15: `Symbol.iterator in x` narrows for a symbol-typed property.
//     The branch returns the arm whose declaration includes that symbol
//     property.

// ----- 1) "a" in x on {a:string} | {b:number}
// if-branch: first arm -> returns s.a (string).
// else: second arm -> returns s.b (number). Joined: string | number.
export function io01(x: { a: string } | { b: number }) {
  if ("a" in x) return x.a;
  return x.b;
}
export type Io01Result = ReturnType<typeof io01>;

// ----- 2) "a" in x on {a:string} | {a:number;b:1}
// Both arms have `a` — `in` does NOT discriminate. Branch is full union;
// `x.a` is `string | number`. Else is never (absorbed). Joined: string | number.
export function io02(x: { a: string } | { a: number; b: 1 }) {
  if ("a" in x) return x.a;
  return null as never;
}
export type Io02Result = ReturnType<typeof io02>;

// ----- 3) "a" in x else-branch — same shape as Io01 but read the else.
// !("a" in x) selects {b:number}; else (positive) selects {a:string}.
// Joined: string | number.
export function io03(x: { a: string } | { b: number }) {
  if (!("a" in x)) return x.b;
  return x.a;
}
export type Io03Result = ReturnType<typeof io03>;

// ----- 4) "a" in x on intersection — `a` is always present; branch is
// full intersection, else is never (absorbed). Joined: {a:string} & {b:number}.
export function io04(x: { a: string } & { b: number }) {
  if ("a" in x) return x;
  return null as never;
}
export type Io04Result = ReturnType<typeof io04>;

// ----- 5) Optional property `a?: string` — `"a" in x` does NOT narrow `a`
// to non-undefined. Branch reads `x.a` which is `string | undefined`.
// Else is never (absorbed). Joined: string | undefined.
export function io05(x: { a?: string }) {
  if ("a" in x) return x.a;
  return null as never;
}
export type Io05Result = ReturnType<typeof io05>;

// ----- 6) "a" in x on unknown — REQUIRES widening to `object` first.
// `typeof x === "object" && x !== null && "a" in x` narrows the branch to
// `object & Record<"a", unknown>`. Else returns null.
// Joined: (object & Record<"a", unknown>) | null.
export function io06(x: unknown) {
  if (typeof x === "object" && x !== null && "a" in x) return x;
  return null;
}
export type Io06Result = ReturnType<typeof io06>;

// ----- 7) "a" in x on object — narrows to object & Record<"a", unknown>.
// Else returns null. Joined: (object & Record<"a", unknown>) | null.
export function io07(x: object) {
  if ("a" in x) return x;
  return null;
}
export type Io07Result = ReturnType<typeof io07>;

// ----- 8) compound "a" in x && "b" in x — narrows to the arm with both
// keys. Else returns null. Joined: {a:1; b:2} | null.
export function io08(x: { a: string } | { b: number } | { a: 1; b: 2 }) {
  if ("a" in x && "b" in x) return x;
  return null;
}
export type Io08Result = ReturnType<typeof io08>;

// ----- 9) !("a" in x) negated — narrows out arms with `a`.
// Joined: string | number.
export function io09(x: { a: string } | { b: number }) {
  if (!("a" in x)) return x.b;
  return x.a;
}
export type Io09Result = ReturnType<typeof io09>;

// ----- 10) Three-arm: {a:1}|{a:2;b:1}|{c:1}. Branch = {a:1} | {a:2;b:1}.
// Branch returns x.a which is `1 | 2`. Else returns x.c (literal 1).
// Joined: 1 | 2.
export function io10(x: { a: 1 } | { a: 2; b: 1 } | { c: 1 }) {
  if ("a" in x) return x.a;
  return x.c;
}
export type Io10Result = ReturnType<typeof io10>;

// ----- 11) Generic constrained to Record<string, unknown>.
// At the ReturnType call site, T resolves to its constraint:
// Record<string, unknown>. Branch returns x (a T value). Else throws
// (never, absorbed). Joined: Record<string, unknown>.
export function io11<T extends Record<string, unknown>>(x: T) {
  if ("a" in x) return x;
  throw 0;
}
export type Io11Result = ReturnType<typeof io11>;

// ----- 12) Reassignment in branch — re-narrows to assigned value's type.
// Inside the if-branch we reassign x to `{ b: 2 }`. The post-assignment
// read of `x.b` is the literal `2`. Else also reads `x.b` from the
// original `{b:2}` arm — also `2`. Joined: 2.
export function io12(x: { a: 1 } | { b: 2 }) {
  if ("a" in x) {
    x = { b: 2 };
    return x.b;
  }
  return x.b;
}
export type Io12Result = ReturnType<typeof io12>;

// ----- 13) Class instance vs object literal — branch narrows to the
// class instance arm (presence of `a` field). Else returns x (the literal
// arm). Joined: Io13C | {b:2}.
export class Io13C {
  a = 1;
}
export function io13(x: Io13C | { b: 2 }) {
  if ("a" in x) return x;
  return x;
}
export type Io13Result = ReturnType<typeof io13>;

// ----- 14) Template-literal-typed key — TS7 does NOT narrow.
// `key: \`prefixed_${string}\`` is a template-literal type containing a
// generic `${string}` placeholder. The `key in x` guard does not narrow
// the operand. Both branches return the same union.
// Joined: {prefixed_a:1} | {other:2}.
export function io14(x: { prefixed_a: 1 } | { other: 2 }) {
  const key = "prefixed_a" as `prefixed_${string}`;
  if (key in x) return x;
  return x;
}
export type Io14Result = ReturnType<typeof io14>;

// ----- 15) Symbol.iterator in x — narrows for symbol property.
// Branch sees the iterable arm; else returns null.
// Joined: { [Symbol.iterator](): Iterator<number> } | null.
export function io15(x: { [Symbol.iterator](): Iterator<number> } | { b: 2 }) {
  if (Symbol.iterator in x) return x;
  return null;
}
export type Io15Result = ReturnType<typeof io15>;
