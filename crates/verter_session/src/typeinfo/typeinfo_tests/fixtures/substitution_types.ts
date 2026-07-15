// @ai-generated - substitution-type scenarios.
//
// Each fn pins one TS7 emission for a substitution-type behaviour —
// the internal "T narrowed to T & U" mechanism TS uses to keep
// generic identity while flowing type guards through method calls,
// destructures, conditional types, asserts predicates, and the
// `in`-operator. ReturnType<typeof fn> encodes the joined narrowing.
//
// All emissions verified out-of-band against tsgo 7.0.0-dev.20260523.1
// via IsExactly probes.
//
// Documented TS7 surprises (NOT fixture bugs):
//   * Sb01: With T unspecified, `ReturnType` resolves T to `unknown`.
//     The if-branch's `T & string` collapses to `string` when T=unknown,
//     and the joined `string | unknown` collapses to `unknown`.
//   * Sb04: The RAW return annotation is `T`, but `ReturnType<typeof f>`
//     with no type args resolves T to `unknown`.
//   * Sb06: After `x = (1 as unknown) as T`, the substitution is
//     UN-NARROWED (assignment to a wider site widens). Return goes
//     back to plain T -> unknown.
//   * Sb08: `IfStr<unknown>` does NOT distribute to `"yes" | "no"`.
//     `unknown extends string` is `false` (unknown is NOT a subtype of
//     string), so the conditional resolves immediately to the false-arm:
//     `"no"`. Distribution requires the test position to be a NAKED
//     union type parameter — `unknown` is not a union, so no
//     distribution happens.
//   * Sb14: A default type argument (`<T = string>`) does NOT apply
//     inside `ReturnType<typeof fn>` — the bare function type still
//     leaves T unresolved, which `ReturnType` then collapses to
//     `unknown`. Defaults only apply at value-position call sites.
//   * Sb15: Recursive self-calls are NOT a substitution event. The
//     declared return type `T` stays as `T` -> `unknown`.

// ----- 1) Bare narrowing of generic ------------------------------------
// if-branch: T & string (substitution). else: T. With T=unknown, the
// if-branch collapses to string and the joined return is `string | unknown`
// = unknown.
export function sb01<T>(x: T) {
  if (typeof x === "string") return x;
  return x;
}
export type Sb01Result = ReturnType<typeof sb01>;

// ----- 2) Narrowing in a constrained generic ---------------------------
// if-branch: substitution `T & string` -> `.toUpperCase()` returns string.
// else: T (constrained to `string | number`). Joined: `string | (string | number)`
// = `string | number`.
export function sb02<T extends string | number>(x: T) {
  if (typeof x === "string") return x.toUpperCase();
  return x;
}
export type Sb02Result = ReturnType<typeof sb02>;

// ----- 3) Substitution survives method calls ---------------------------
// `x.toUpperCase()` on T-extends-string returns the apparent type `string`.
// Substitution is preserved across the method call. Joined: string.
export function sb03<T extends string>(x: T) {
  const u = x.toUpperCase();
  return u;
}
export type Sb03Result = ReturnType<typeof sb03>;

// ----- 4) Narrowed substitution to return position ---------------------
// RAW return annotation is `T`. `ReturnType<typeof fn>` with no type args
// resolves T to `unknown`. The narrowed `T & string` in the if-branch is
// returned as `T` at the return position.
export function sb04<T>(x: T): T {
  if (typeof x === "string") return x;
  throw 0;
}
export type Sb04Result = ReturnType<typeof sb04>;

// ----- 5) Compound narrowing via typeof && instanceof ------------------
// `typeof x === "object" && x instanceof Date` narrows x to Date.
// `x.getTime()` returns number. Joined: number.
export function sb05<T>(x: T) {
  if (typeof x === "object" && x instanceof Date) return x.getTime();
  throw 0;
}
export type Sb05Result = ReturnType<typeof sb05>;

// ----- 6) Narrowing widens after re-assignment -------------------------
// Inside the if-branch x is initially `T & string`. After
// `x = (1 as unknown) as T` the substitution is removed (TS un-narrows
// on assignment to a wider site). Return is plain T -> unknown.
export function sb06<T>(x: T) {
  if (typeof x === "string") {
    x = 1 as unknown as T;
    return x;
  }
  throw 0;
}
export type Sb06Result = ReturnType<typeof sb06>;

// ----- 7) Constraint-flow apparent type --------------------------------
// `x.len` on T-extends-{len:number} reads through the constraint's
// apparent type. Returns number.
export function sb07<T extends { len: number }>(x: T) {
  return x.len;
}
export type Sb07Result = ReturnType<typeof sb07>;

// ----- 8) Generic in conditional position retains identity -------------
// `IfStr<T>` with T=unknown does NOT distribute (unknown is NOT a naked
// union type parameter). `unknown extends string` is false, so the
// conditional resolves immediately to the false-arm: "no".
export type IfStr<T> = T extends string ? "yes" : "no";
export function sb08<T>() {
  return null as unknown as IfStr<T>;
}
export type Sb08Result = ReturnType<typeof sb08>;

// ----- 9) `asserts x is string` on generic -----------------------------
// After the assert, x is `T & string`. With T=unknown via ReturnType,
// `unknown & string` collapses to `string`. Joined: string.
export function assertIsString(x: any): asserts x is string {}
export function sb09<T>(x: T) {
  assertIsString(x);
  return x;
}
export type Sb09Result = ReturnType<typeof sb09>;

// ----- 10) `x is T` predicate on generic -------------------------------
// After `isFoo<T>(x)` the variable x narrows to T. ReturnType resolves T
// to unknown. Joined: unknown.
export function isFoo<T>(x: unknown): x is T {
  return true;
}
export function sb10<T>(x: unknown) {
  if (isFoo<T>(x)) return x;
  throw 0;
}
export type Sb10Result = ReturnType<typeof sb10>;

// ----- 11) Generic narrowed via `in` operator --------------------------
// `"a" in x` narrows T's apparent type to the `{ a: 1 }` arm. Else
// returns the `{ b: 2 }` arm. Joined: 1 | 2.
export function sb11<T extends { a: 1 } | { b: 2 }>(x: T) {
  if ("a" in x) return x.a;
  return x.b;
}
export type Sb11Result = ReturnType<typeof sb11>;

// ----- 12) Truthiness narrowing on `T extends string | undefined` ------
// Truthy guard removes `undefined`. With the constraint `string | undefined`,
// truthy reduces to `string`. Joined: string.
export function sb12<T extends string | undefined>(x: T) {
  if (x) return x;
  throw 0;
}
export type Sb12Result = ReturnType<typeof sb12>;

// ----- 13) Substitution carried across destructure ---------------------
// Destructuring `{ val }` from T-extends-{val:number} reads `val` as the
// constraint's apparent property type: number. Joined: number.
export function sb13<T extends { val: number }>(x: T) {
  const { val } = x;
  return val;
}
export type Sb13Result = ReturnType<typeof sb13>;

// ----- 14) Default type arg with narrowing -----------------------------
// SURPRISE: `<T = string>` default does NOT apply inside `ReturnType<typeof fn>`.
// The bare function type is still unparameterised; ReturnType resolves T
// to `unknown` (not the default). Joined: unknown.
export function sb14<T = string>(x: T) {
  if (typeof x === "string") return x;
  return x;
}
export type Sb14Result = ReturnType<typeof sb14>;

// ----- 15) Recursive generic substitution ------------------------------
// Self-recursion `f(x)` is NOT a substitution event. Declared return
// type T stays as T -> unknown.
export function sb15<T>(x: T): T {
  return sb15(x);
}
export type Sb15Result = ReturnType<typeof sb15>;
