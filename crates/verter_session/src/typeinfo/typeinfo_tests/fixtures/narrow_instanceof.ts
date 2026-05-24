// @ai-generated - instanceof-narrowing scenarios.
//
// Each fn returns the value(s) reachable inside / outside an instanceof
// guard, so ReturnType<typeof fn> encodes the joined narrowing emission.
// All emissions verified out-of-band against tsgo 7.0.0-dev.20260523.1
// via IsExactly probes.
//
// Documented TS7 quirks (NOT fixture bugs):
//   * In04: For `x: InA | InB` where `class InB extends InA`, the parameter
//     type `InA | InB` collapses to `InA` (union subsumption: `InB` is a
//     subtype of `InA`). The if-branch sees `InA`, the else is unreachable
//     (`never`, absorbed). Joined return: `InA`.
//   * In06: For an abstract class `In6A` and `const x: In6A = new In6B()`,
//     `x instanceof In6A` does NOT widen `x` further inside the if-branch —
//     it stays as `In6A`. The else is `never` (absorbed). Joined: `In6A`.
//   * In10: For `const x: A & { tag: 1 }`, the if-branch keeps the FULL
//     intersection `A & { tag: 1 }` (instanceof A narrows the A side, the
//     `{ tag: 1 }` side is preserved). Joined: `(A & { tag: 1 }) | null`.
//   * In11: For a generic ctor `T extends new (...args: any[]) => any` not
//     instantiated at the ReturnType call site, `T` resolves to its
//     constraint, so `InstanceType<T>` resolves to `any`. The if-branch
//     narrows `x: unknown` to `any` and the else returns null. Joined:
//     `any | null` (which collapses to `any` in TS but is checked as
//     `any | null` via IsExactly).
//   * In13: `x instanceof Array` is a TS7 special case — the if-branch
//     narrows to `any[]` (not `unknown[]`). This is the historical TS
//     behaviour preserved in TS7.
//   * In14: `x instanceof Promise` narrows to `Promise<any>` (not
//     `Promise<unknown>`) — same historical preservation as In13.

class InA {
  a = 1;
}
class InB {
  b = 2;
}

// ----- 1) x instanceof A on A | B ---------------------------------------
// if-branch: A. else: B. Joined: A | B.
export function in01InstanceOfBinaryUnion(x: InA | InB) {
  if (x instanceof InA) return x;
  return x;
}
export type In01InstanceOfBinaryUnionResult = ReturnType<typeof in01InstanceOfBinaryUnion>;

// ----- 2) x instanceof A on A | string ----------------------------------
// if-branch: A. else: string. Joined: A | string.
export function in02InstanceOfWithPrimitive(x: InA | string) {
  if (x instanceof InA) return x;
  return x;
}
export type In02InstanceOfWithPrimitiveResult = ReturnType<typeof in02InstanceOfWithPrimitive>;

// ----- 3) x instanceof A on unknown -------------------------------------
// if-branch: A. else: unknown. Joined: unknown (A is subsumed).
export function in03InstanceOfOnUnknown(x: unknown) {
  if (x instanceof InA) return x;
  return x;
}
export type In03InstanceOfOnUnknownResult = ReturnType<typeof in03InstanceOfOnUnknown>;

// ----- 4) Subclass union — x: A | B where B extends A -------------------
// The parameter type `In4A | In4B` collapses to `In4A` (subsumption).
// Joined return: In4A.
class In4A {
  a4 = 1;
}
class In4B extends In4A {
  b4 = 2;
}
export function in04InstanceOfSubclassUnion(x: In4A | In4B) {
  if (x instanceof In4A) return x;
  return x;
}
export type In04InstanceOfSubclassUnionResult = ReturnType<typeof in04InstanceOfSubclassUnion>;

// ----- 5) Already-narrowed declared type — const x: B; x instanceof A ---
// The if-branch keeps x as B (B already satisfies A). else is never.
// Joined: B.
class In5A {
  a5 = 1;
}
class In5B extends In5A {
  b5 = 2;
}
export function in05InstanceOfAlreadyNarrowed() {
  const x: In5B = new In5B();
  if (x instanceof In5A) return x;
  return x;
}
export type In05InstanceOfAlreadyNarrowedResult = ReturnType<typeof in05InstanceOfAlreadyNarrowed>;

// ----- 6) Abstract class — `abstract class A`, x: A = new B() -----------
// instanceof A does NOT widen `x` further; the if-branch keeps `A`.
// else is never (absorbed). Joined: A.
abstract class In6A {
  a6 = 1;
  abstract foo(): void;
}
class In6B extends In6A {
  b6 = 2;
  foo() {}
}
export function in06InstanceOfAbstract() {
  const x: In6A = new In6B();
  if (x instanceof In6A) return x;
  return x;
}
export type In06InstanceOfAbstractResult = ReturnType<typeof in06InstanceOfAbstract>;

// ----- 7) else-branch reachability — if (instanceof) return; else x ----
// if-branch returns null; else returns `x` narrowed to NOT-A = B.
// Joined: B | null.
class In7A {
  a7 = 1;
}
class In7B {
  b7 = 2;
}
export function in07InstanceOfElseReachability(x: In7A | In7B) {
  if (x instanceof In7A) return null;
  return x;
}
export type In07InstanceOfElseReachabilityResult = ReturnType<
  typeof in07InstanceOfElseReachability
>;

// ----- 8) instanceof on interface union — x: I; if (x instanceof A) ----
// I is an interface, A implements I. The if-branch narrows x to A.
// else returns null. Joined: A | null.
interface In8I {
  i8: string;
}
class In8A implements In8I {
  i8 = "a";
  extra = 1;
}
export function in08InstanceOfInterfaceUnion(x: In8I) {
  if (x instanceof In8A) return x;
  return null;
}
export type In08InstanceOfInterfaceUnionResult = ReturnType<typeof in08InstanceOfInterfaceUnion>;

// ----- 9) Negated narrowing — if (!(x instanceof A)) return; return x --
// if-branch (negated) returns null; trailing return sees x as A (the
// only path past the guard). Joined: A | null.
class In9A {
  a9 = 1;
}
class In9B {
  b9 = 2;
}
export function in09InstanceOfNegated(x: In9A | In9B) {
  if (!(x instanceof In9A)) return null;
  return x;
}
export type In09InstanceOfNegatedResult = ReturnType<typeof in09InstanceOfNegated>;

// ----- 10) Intersection — x: A & { tag: 1 }; if (x instanceof A) -------
// The if-branch keeps the FULL intersection `A & { tag: 1 }`. else returns
// null. Joined: (A & { tag: 1 }) | null.
class In10A {
  a10 = 1;
}
export function in10InstanceOfIntersection() {
  const x: In10A & { tag: 1 } = Object.assign(new In10A(), { tag: 1 as const });
  if (x instanceof In10A) return x;
  return null;
}
export type In10InstanceOfIntersectionResult = ReturnType<typeof in10InstanceOfIntersection>;

// ----- 11) Generic ctor T extends new (...args: any[]) => any ----------
// At the ReturnType call site, T resolves to its constraint, so
// InstanceType<T> is `any`. The if-branch narrows `x: unknown` to `any`,
// the else returns null. Joined: any | null.
export function in11InstanceOfGenericCtor<T extends new (...args: any[]) => any>(
  x: unknown,
  ctor: T,
) {
  if (x instanceof ctor) return x;
  return null;
}
export type In11InstanceOfGenericCtorResult = ReturnType<typeof in11InstanceOfGenericCtor>;

// ----- 12) Chained instanceof — A else if B else C --------------------
// case A returns "a"; case B returns "b"; else returns "c". Joined:
// "a" | "b" | "c".
class In12A {
  a12 = 1;
}
class In12B {
  b12 = 2;
}
class In12C {
  c12 = 3;
}
export function in12InstanceOfChained(x: In12A | In12B | In12C) {
  if (x instanceof In12A) return "a" as const;
  if (x instanceof In12B) return "b" as const;
  return "c" as const;
}
export type In12InstanceOfChainedResult = ReturnType<typeof in12InstanceOfChained>;

// ----- 13) instanceof Array — narrows to any[] ------------------------
// TS7 special case: `x instanceof Array` narrows to `any[]` (NOT
// `unknown[]`), preserving historical TS behaviour. else returns null.
// Joined: any[] | null.
export function in13InstanceOfArray(x: unknown) {
  if (x instanceof Array) return x;
  return null;
}
export type In13InstanceOfArrayResult = ReturnType<typeof in13InstanceOfArray>;

// ----- 14) instanceof Promise — narrows to Promise<any> ---------------
// TS7 special case: `x instanceof Promise` narrows to `Promise<any>` (NOT
// `Promise<unknown>`). else returns null. Joined: Promise<any> | null.
export function in14InstanceOfPromise(x: unknown) {
  if (x instanceof Promise) return x;
  return null;
}
export type In14InstanceOfPromiseResult = ReturnType<typeof in14InstanceOfPromise>;

// ----- 15) x: A | null | undefined; if (x instanceof A) ---------------
// if-branch narrows to A (null/undefined are not instances of any class).
// else returns null. Joined: A | null.
class In15A {
  a15 = 1;
}
export function in15InstanceOfNullable(x: In15A | null | undefined) {
  if (x instanceof In15A) return x;
  return null;
}
export type In15InstanceOfNullableResult = ReturnType<typeof in15InstanceOfNullable>;
