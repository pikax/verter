// @ai-generated - discriminated-union narrowing scenarios.
//
// Each fn returns the value(s) reachable inside / outside a discriminant
// guard, so ReturnType<typeof fn> encodes the joined narrowing emission.
// All emissions verified out-of-band against tsgo 7.0.0-dev.20260523.1
// via IsExactly probes.
//
// Documented TS7 quirks (NOT fixture bugs):
//   * Du09: Destructuring `const { kind } = s` PRESERVES the discriminant
//     correlation between `kind` and `s` in TS7; the if-branch narrows
//     BOTH the destructured `kind` AND the original `s`. The joined return
//     is `string | number` as if the guard had been written directly on
//     `s.kind`.
//   * Du10: When a discriminant is present in only one arm, the
//     `("kind" in s)` guard is required first; `s.kind === "a"` alone
//     would be a type error because `kind` isn't on the other arm.
//   * Du14: After reassignment inside the if-branch (`s = { kind: "b", b: 0 }`),
//     `s` is RE-NARROWED to the new value's static type. The if-body's
//     return type therefore matches the new arm (number), not the original
//     narrowed arm (string). The else-branch reads `s.b` from the original
//     `kind: "b"` arm. Joined: number.
//   * Du15: Template-literal discriminant `kind: \`prefix-${string}\``
//     narrows DOES occur in the if-branch when compared against a concrete
//     `"prefix-foo"` literal — the literal is assignable to the template
//     pattern, so the first arm is selected. The else branch keeps both
//     arms (we excluded one specific value, not the whole pattern).
//     Joined: string | null.

// ----- 1) if (s.kind === "a") on {kind:"a";a:string} | {kind:"b";b:number}
// if-branch: first arm -> returns s.a (string).
// else: second arm -> returns s.b (number).
// Joined: string | number.
export type Du01Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function du01(s: Du01Shape) {
  if (s.kind === "a") return s.a;
  return s.b;
}
export type Du01Result = ReturnType<typeof du01>;

// ----- 2) switch(s.kind) over "a" | "b" -------------------------------
// case "a" -> string; case "b" -> number. Joined: string | number.
export type Du02Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function du02(s: Du02Shape) {
  switch (s.kind) {
    case "a":
      return s.a;
    case "b":
      return s.b;
  }
}
export type Du02Result = ReturnType<typeof du02>;

// ----- 3) switch with default: never exhaustiveness check -------------
// The `never`-aided default contributes `never` to the join (absorbed).
// Joined: string | number.
export type Du03Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function du03(s: Du03Shape) {
  switch (s.kind) {
    case "a":
      return s.a;
    case "b":
      return s.b;
    default: {
      const _exhaustive: never = s;
      return _exhaustive;
    }
  }
}
export type Du03Result = ReturnType<typeof du03>;

// ----- 4) if (s.kind !== "a") -----------------------------------------
// negated if-branch: union-minus-a -> returns s.b (number).
// else: a-arm -> returns s.a (string). Joined: string | number.
export type Du04Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function du04(s: Du04Shape) {
  if (s.kind !== "a") return s.b;
  return s.a;
}
export type Du04Result = ReturnType<typeof du04>;

// ----- 5) Multi-property discriminant kind === "a" && tag === 1 -------
// Original union: three arms. Compound guard narrows to the first arm only.
// if-branch: returns s.a1 (string).
// else: returns null. Joined: string | null.
export type Du05Shape =
  | { kind: "a"; tag: 1; a1: string }
  | { kind: "b"; tag: 1; b1: number }
  | { kind: "a"; tag: 2; a2: boolean };
export function du05(s: Du05Shape) {
  if (s.kind === "a" && s.tag === 1) return s.a1;
  return null;
}
export type Du05Result = ReturnType<typeof du05>;

// ----- 6) Nested discriminants ---------------------------------------
// Outer s.outer === "o1" narrows to the first arm, then inner.kind === "ia"
// narrows the inner union. if-branch: returns s.inner.ia (string).
// else: returns null. Joined: string | null.
export type Du06Inner = { kind: "ia"; ia: string } | { kind: "ib"; ib: number };
export type Du06Shape = { outer: "o1"; inner: Du06Inner } | { outer: "o2"; q: boolean };
export function du06(s: Du06Shape) {
  if (s.outer === "o1" && s.inner.kind === "ia") return s.inner.ia;
  return null;
}
export type Du06Result = ReturnType<typeof du06>;

// ----- 7) Discriminant on number-literal type -------------------------
// if-branch: kind === 1 -> first arm -> returns s.a (string).
// else: kind === 2 -> second arm -> returns s.b (number).
// Joined: string | number.
export type Du07Shape = { kind: 1; a: string } | { kind: 2; b: number };
export function du07(s: Du07Shape) {
  if (s.kind === 1) return s.a;
  return s.b;
}
export type Du07Result = ReturnType<typeof du07>;

// ----- 8) Discriminant on boolean-literal type ------------------------
// `if (s.ok)` narrows on truthiness of a boolean-literal-typed property.
// if-branch: ok === true -> first arm -> returns s.data (string).
// else: ok === false -> second arm -> returns s.err (number).
// Joined: string | number.
export type Du08Shape = { ok: true; data: string } | { ok: false; err: number };
export function du08(s: Du08Shape) {
  if (s.ok) return s.data;
  return s.err;
}
export type Du08Result = ReturnType<typeof du08>;

// ----- 9) Discriminant via property destructure -----------------------
// `const { kind } = s; if (kind === "a") return s.a;` — TS7 PROPAGATES the
// correlation between the destructured `kind` and the original `s`, so
// the if-branch narrows BOTH. Joined: string | number.
export type Du09Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function du09(s: Du09Shape) {
  const { kind } = s;
  if (kind === "a") return s.a;
  return s.b;
}
export type Du09Result = ReturnType<typeof du09>;

// ----- 10) Discriminant present in only one arm -----------------------
// Second arm has no `kind` property — `("kind" in s)` narrows out the
// second arm first, then `s.kind === "a"` narrows further to the first.
// if-branch: returns s.a (string). else: returns null. Joined: string | null.
export type Du10Shape = { kind: "a"; a: string } | { b: number };
export function du10(s: Du10Shape) {
  if ("kind" in s && s.kind === "a") return s.a;
  return null;
}
export type Du10Result = ReturnType<typeof du10>;

// ----- 11) Switch returning per-arm types ----------------------------
// Identical structure to scenario 2 but documented as the joined-return
// contract: case "a" returns string, case "b" returns number.
// Joined: string | number.
export type Du11Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function du11(s: Du11Shape) {
  switch (s.kind) {
    case "a":
      return s.a;
    case "b":
      return s.b;
  }
}
export type Du11Result = ReturnType<typeof du11>;

// ----- 12) Switch with fall-through case "a": case "b": --------------
// Fall-through narrowing: inside the joined `case "a": case "b":` body,
// `s` is narrowed to `{kind:"a"|"b"; payload:string|number}`. The block
// returns s.payload which is `string | number`. The "c" case returns boolean.
// Joined: string | number | boolean.
export type Du12Shape =
  | { kind: "a"; payload: string }
  | { kind: "b"; payload: number }
  | { kind: "c"; flag: boolean };
export function du12(s: Du12Shape) {
  switch (s.kind) {
    case "a":
    case "b":
      return s.payload;
    case "c":
      return s.flag;
  }
}
export type Du12Result = ReturnType<typeof du12>;

// ----- 13) Discriminated union with shared property ------------------
// Both arms carry `shared: string`. After narrowing, the arm-specific value
// retains access to the shared property. Each branch returns an object
// with both arm-specific and shared values.
// Joined: { v: 1; sh: string } | { v: 2; sh: string }.
export type Du13Shape = { kind: "a"; shared: string; a: 1 } | { kind: "b"; shared: string; b: 2 };
export function du13(s: Du13Shape) {
  if (s.kind === "a") return { v: s.a, sh: s.shared };
  return { v: s.b, sh: s.shared };
}
export type Du13Result = ReturnType<typeof du13>;

// ----- 14) Re-narrowing after reassignment ---------------------------
// Inside the if-branch we reassign `s` to a new arm. TS7 re-narrows `s`
// to the static type of the reassigned value (the `{kind:"b"; b:number}`
// arm). The if-body returns s.b (number); the else returns s.b (number,
// from the original "b" arm). Joined: number.
export type Du14Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function du14(s: Du14Shape) {
  if (s.kind === "a") {
    s = { kind: "b", b: 0 };
    return s.b;
  }
  return s.b;
}
export type Du14Result = ReturnType<typeof du14>;

// ----- 15) Tagged-template-style discriminant -----------------------
// `kind: \`prefix-${string}\`` is a template-literal type. Comparing
// against the concrete `"prefix-foo"` literal (which IS assignable to
// `\`prefix-${string}\``) narrows the if-branch to the first arm only.
// if-branch: returns s.a (string). else: returns null.
// Joined: string | null.
export type Du15Shape =
  | { kind: `prefix-${string}`; a: string }
  | { kind: `other-${string}`; b: number };
export function du15(s: Du15Shape) {
  if (s.kind === "prefix-foo") return s.a;
  return null;
}
export type Du15Result = ReturnType<typeof du15>;
