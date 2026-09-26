//! Differential probes of the library globals: `Array` and
//! `ReadonlyArray` methods, `String`, `Function` (`apply`, `call`,
//! `bind`), `Number`, `Object`, `Promise` with `await`, `Map` and
//! `Set`. Each project registers a library copied from TypeScript's own
//! `lib.es5.d.ts`, `lib.es2015.collection.d.ts`, `lib.es2015.promise.d.ts`
//! and `lib.es2016.array.include.d.ts` (the declarations these rows read,
//! verbatim, comments dropped). A row is a function of the fixture answered
//! as its body-derived return, or a type in TYPE position.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --noLib --declaration
//! --emitDeclarationOnly --strict --noErrorTruncation` over the library as
//! a root file, under each `strictNullChecks` × `noImplicitAny` setting:
//! `declare const p: <probe>; const s: never = p;` (a function row reads
//! `ReturnType<typeof f>`) read off the TS2322 message. A row with one
//! answer answers alike in the four settings; a row with two answers gives
//! the `strictNullChecks` answer then the answer with it off. An ignored
//! test asserts the measured answer for rows the lane does not yet answer as
//! the checker does; "wrong-but-clean" marks a lane answer published
//! complete and undegraded.

use super::differential_harness_tests::{Matrix, Read};

/// Array and readonly array method calls and reads.
const ARRAYS: &str = r##"
export function aMap(xs: number[]) { return xs.map((x) => x > 0); }
export function aMapIndex(xs: string[]) { return xs.map((x, i) => i); }
export function aFilter(xs: (string | number)[]) { return xs.filter((x) => typeof x === "string"); }
export function aFilterGuard(xs: (string | number)[]) { return xs.filter((x): x is string => typeof x === "string"); }
export function aReduce(xs: number[]) { return xs.reduce((a, b) => a + b); }
export function aReduceInit(xs: number[]) { return xs.reduce((acc, x) => acc + x, ""); }
export function aSlice(xs: readonly string[]) { return xs.slice(1); }
export function aJoin(xs: number[]) { return xs.join(","); }
export function aPush(xs: number[]) { return xs.push(1); }
export function aPop(xs: number[]) { return xs.pop(); }
export function aIncludes(xs: string[]) { return xs.includes("a"); }
export function aLength(xs: string[]) { return xs.length; }
export function aIndex(xs: string[]) { return xs[0]; }
export function aEvery(xs: (string | number)[]) { if (xs.every((x): x is number => typeof x === "number")) return xs; throw 0; }
export function aIsArray(x: string | string[]) { if (Array.isArray(x)) return x; throw 0; }
export function aIsArrayElse(x: string | string[]) { if (Array.isArray(x)) throw 0; return x; }
export function aArrayCtor() { return new Array(1, 2); }
export function aTupleMap(t: [1, 2]) { return t.map((x) => x); }
export function aReadonlyMap(xs: readonly number[]) { return xs.map((x) => [x]); }
export function aChain(xs: number[]) { return xs.filter((x) => x > 1).map((x) => "" + x).join(); }
export function aForEachResult(xs: number[]) { return xs.forEach(() => {}); }
"##;

/// The library every project registers and the checker reads as its only lib
/// file.
const GLOBALS_LIB: &str = r##"interface Object { toString(): string; hasOwnProperty(v: PropertyKey): boolean; }
interface Function { apply(this: Function, thisArg: any, argArray?: any): any; readonly length: number; }
interface CallableFunction extends Function {
  apply<T, R>(this: (this: T) => R, thisArg: T): R;
  apply<T, A extends any[], R>(this: (this: T, ...args: A) => R, thisArg: T, args: A): R;
  call<T, A extends any[], R>(this: (this: T, ...args: A) => R, thisArg: T, ...args: A): R;
  bind<T>(this: T, thisArg: ThisParameterType<T>): OmitThisParameter<T>;
  bind<T, A extends any[], B extends any[], R>(this: (this: T, ...args: [...A, ...B]) => R, thisArg: T, ...args: A): (...args: B) => R;
}
interface NewableFunction extends Function {}
interface IArguments { [index: number]: any; length: number; }
interface String {
  charAt(pos: number): string;
  indexOf(searchString: string, position?: number): number;
  slice(start?: number, end?: number): string;
  split(separator: string, limit?: number): string[];
  toUpperCase(): string;
  concat(...strings: string[]): string;
  readonly length: number;
  readonly [index: number]: string;
}
interface StringConstructor { new (value?: any): String; (value?: any): string; readonly prototype: String; fromCharCode(...codes: number[]): string; }
declare var String: StringConstructor;
interface Boolean { valueOf(): boolean; }
interface Number { toFixed(fractionDigits?: number): string; }
interface RegExp {}
interface ConcatArray<T> { readonly length: number; readonly [n: number]: T; join(separator?: string): string; slice(start?: number, end?: number): T[]; }
interface ReadonlyArray<T> {
  readonly length: number;
  concat(...items: ConcatArray<T>[]): T[];
  concat(...items: (T | ConcatArray<T>)[]): T[];
  join(separator?: string): string;
  slice(start?: number, end?: number): T[];
  indexOf(searchElement: T, fromIndex?: number): number;
  every<S extends T>(predicate: (value: T, index: number, array: readonly T[]) => value is S, thisArg?: any): this is readonly S[];
  every(predicate: (value: T, index: number, array: readonly T[]) => unknown, thisArg?: any): boolean;
  some(predicate: (value: T, index: number, array: readonly T[]) => unknown, thisArg?: any): boolean;
  forEach(callbackfn: (value: T, index: number, array: readonly T[]) => void, thisArg?: any): void;
  map<U>(callbackfn: (value: T, index: number, array: readonly T[]) => U, thisArg?: any): U[];
  filter<S extends T>(predicate: (value: T, index: number, array: readonly T[]) => value is S, thisArg?: any): S[];
  filter(predicate: (value: T, index: number, array: readonly T[]) => unknown, thisArg?: any): T[];
  reduce(callbackfn: (previousValue: T, currentValue: T, currentIndex: number, array: readonly T[]) => T): T;
  reduce(callbackfn: (previousValue: T, currentValue: T, currentIndex: number, array: readonly T[]) => T, initialValue: T): T;
  reduce<U>(callbackfn: (previousValue: U, currentValue: T, currentIndex: number, array: readonly T[]) => U, initialValue: U): U;
  includes(searchElement: T, fromIndex?: number): boolean;
  readonly [n: number]: T;
}
interface Array<T> {
  length: number;
  push(...items: T[]): number;
  pop(): T | undefined;
  concat(...items: ConcatArray<T>[]): T[];
  concat(...items: (T | ConcatArray<T>)[]): T[];
  join(separator?: string): string;
  slice(start?: number, end?: number): T[];
  indexOf(searchElement: T, fromIndex?: number): number;
  every<S extends T>(predicate: (value: T, index: number, array: T[]) => value is S, thisArg?: any): this is S[];
  every(predicate: (value: T, index: number, array: T[]) => unknown, thisArg?: any): boolean;
  some(predicate: (value: T, index: number, array: T[]) => unknown, thisArg?: any): boolean;
  forEach(callbackfn: (value: T, index: number, array: T[]) => void, thisArg?: any): void;
  map<U>(callbackfn: (value: T, index: number, array: T[]) => U, thisArg?: any): U[];
  filter<S extends T>(predicate: (value: T, index: number, array: T[]) => value is S, thisArg?: any): S[];
  filter(predicate: (value: T, index: number, array: T[]) => unknown, thisArg?: any): T[];
  reduce(callbackfn: (previousValue: T, currentValue: T, currentIndex: number, array: T[]) => T): T;
  reduce(callbackfn: (previousValue: T, currentValue: T, currentIndex: number, array: T[]) => T, initialValue: T): T;
  reduce<U>(callbackfn: (previousValue: U, currentValue: T, currentIndex: number, array: T[]) => U, initialValue: U): U;
  includes(searchElement: T, fromIndex?: number): boolean;
  [n: number]: T;
}
interface ArrayConstructor {
  new <T>(...items: T[]): T[];
  <T>(...items: T[]): T[];
  isArray(arg: any): arg is any[];
  readonly prototype: any[];
}
declare var Array: ArrayConstructor;
interface PromiseLike<T> {
  then<TResult1 = T, TResult2 = never>(onfulfilled?: ((value: T) => TResult1 | PromiseLike<TResult1>) | undefined | null, onrejected?: ((reason: any) => TResult2 | PromiseLike<TResult2>) | undefined | null): PromiseLike<TResult1 | TResult2>;
}
interface Promise<T> {
  then<TResult1 = T, TResult2 = never>(onfulfilled?: ((value: T) => TResult1 | PromiseLike<TResult1>) | undefined | null, onrejected?: ((reason: any) => TResult2 | PromiseLike<TResult2>) | undefined | null): Promise<TResult1 | TResult2>;
  catch<TResult = never>(onrejected?: ((reason: any) => TResult | PromiseLike<TResult>) | undefined | null): Promise<T | TResult>;
}
interface PromiseConstructor {
  readonly prototype: Promise<any>;
  new <T>(executor: (resolve: (value: T | PromiseLike<T>) => void, reject: (reason?: any) => void) => void): Promise<T>;
  all<T extends readonly unknown[] | []>(values: T): Promise<{ -readonly [P in keyof T]: Awaited<T[P]>; }>;
  race<T extends readonly unknown[] | []>(values: T): Promise<Awaited<T[number]>>;
  reject<T = never>(reason?: any): Promise<T>;
  resolve(): Promise<void>;
  resolve<T>(value: T): Promise<Awaited<T>>;
  resolve<T>(value: T | PromiseLike<T>): Promise<Awaited<T>>;
}
declare var Promise: PromiseConstructor;
type Awaited<T> = T extends null | undefined ? T :
  T extends object & { then(onfulfilled: infer F, ...args: infer _): any; } ?
    F extends ((value: infer V, ...args: infer _) => any) ?
      Awaited<V> :
    never :
  T;
interface Map<K, V> {
  clear(): void;
  delete(key: K): boolean;
  forEach(callbackfn: (value: V, key: K, map: Map<K, V>) => void, thisArg?: any): void;
  get(key: K): V | undefined;
  has(key: K): boolean;
  set(key: K, value: V): this;
  readonly size: number;
}
interface MapConstructor {
  new (): Map<any, any>;
  new <K, V>(entries?: readonly (readonly [K, V])[] | null): Map<K, V>;
  readonly prototype: Map<any, any>;
}
declare var Map: MapConstructor;
interface Set<T> {
  add(value: T): this;
  clear(): void;
  delete(value: T): boolean;
  forEach(callbackfn: (value: T, value2: T, set: Set<T>) => void, thisArg?: any): void;
  has(value: T): boolean;
  readonly size: number;
}
interface SetConstructor {
  new <T = any>(values?: readonly T[] | null): Set<T>;
  readonly prototype: Set<any>;
}
declare var Set: SetConstructor;
type ThisParameterType<T> = T extends (this: infer U, ...args: never) => any ? U : unknown;
type OmitThisParameter<T> = unknown extends ThisParameterType<T> ? T : T extends (...args: infer A) => infer R ? (...args: A) => R : T;
type ReturnType<T extends (...args: any) => any> = T extends (...args: any) => infer R ? R : any;
type Parameters<T extends (...args: any) => any> = T extends (...args: infer P) => any ? P : never;
type PropertyKey = string | number | symbol;
interface TemplateStringsArray extends ReadonlyArray<string> { readonly raw: readonly string[]; }
"##;

/// Array methods resolve through the library's `Array<T>` / `ReadonlyArray<T>`:
/// `reduce` with and without a seed, `slice`, `join`, `push`, `pop`,
/// `includes`, `length`, element reads, `Array.isArray` narrowing, the `Array`
/// constructor, `forEach`, and member signatures read in TYPE position.
#[test]
fn array_methods_resolve_as_the_checker_resolves_them() {
    let matrix = Matrix::new(ARRAYS).lib(GLOBALS_LIB);
    let mut failures = matrix.same(&[
        (Read::Return("aReduce"), "number"),
        (Read::Return("aReduceInit"), "string"),
        (Read::Return("aSlice"), "string[]"),
        (Read::Return("aJoin"), "string"),
        (Read::Return("aPush"), "number"),
        (Read::Return("aIncludes"), "boolean"),
        (Read::Return("aLength"), "number"),
        (Read::Return("aIndex"), "string"),
        (Read::Return("aIsArray"), "string[]"),
        (Read::Return("aIsArrayElse"), "string"),
        (Read::Return("aArrayCtor"), "number[]"),
        (Read::Return("aForEachResult"), "void"),
        (Read::Type("ReturnType<number[]['map']>"), "unknown[]"),
        (Read::Type("ReturnType<string[]['slice']>"), "string[]"),
        (Read::Type("string[]['length']"), "number"),
        (Read::Type("Parameters<number[]['push']>"), "number[]"),
    ]);
    failures.extend(matrix.nullness(&[(Read::Return("aPop"), "number | undefined", "number")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `xs.map((x) => x > 0)` is `boolean[]`: `map<U>` infers `U` from the
/// callback's return, typed under the element type, over an array, a tuple
/// and a readonly array alike (TypeScript 7.0.2, all four settings alike).
#[test]
fn an_array_method_callback_return_infers_as_the_checker_infers_it() {
    let matrix = Matrix::new(ARRAYS).lib(GLOBALS_LIB);
    let failures = matrix.returns(&[
        ("aMap", "boolean[]"),
        ("aMapIndex", "number[]"),
        ("aTupleMap", "(1 | 2)[]"),
        ("aReadonlyMap", "number[][]"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `xs.filter((x) => typeof x === "string")` is `string[]`: the arrow's
/// inferred type predicate `x is string` selects the `filter<S extends T>`
/// overload. Wrong-but-clean: the lane answers `(string | number)[]`.
///
/// What the lane gives:
/// - `aFilter`: the checker answers `string[]`; the lane measured `(string |
///   number)[]`.
#[test]
#[ignore = "filter resolves its guard overload with a callback whose type predicate is inferred"]
fn wrong_clean_filter_takes_the_callbacks_inferred_type_predicate() {
    let matrix = Matrix::new(ARRAYS).lib(GLOBALS_LIB);
    let failures = matrix.returns(&[("aFilter", "string[]")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An explicitly annotated guard callback selects `filter<S extends T>`
/// (`string[]`) and makes `every` a `this is S[]` predicate that narrows the
/// receiver (`number[]`); a `filter`/`map`/`join` chain is `string`.
///
/// What the lane gives:
/// - `aFilterGuard`: the checker answers `string[]`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnrepresentableCallee.
/// - `aEvery`: the checker answers `number[]`; the lane measured `(string |
///   number)[]` degraded by FlowGap(GuardNarrowing).
/// - `aChain`: the checker answers `string`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnmodeledPosition.
#[test]
#[ignore = "a type-guard callback resolves filter / every to their generic overloads"]
fn a_guard_callback_resolves_the_generic_overload() {
    let matrix = Matrix::new(ARRAYS).lib(GLOBALS_LIB);
    let failures = matrix.returns(&[
        ("aFilterGuard", "string[]"),
        ("aEvery", "number[]"),
        ("aChain", "string"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// String, function, number and object members.
const STRINGS_AND_FUNCTIONS: &str = r##"
export function sLen(s: string) { return s.length; }
export function sCharAt(s: string) { return s.charAt(0); }
export function sSplit(s: string) { return s.split(","); }
export function sIndex(s: string) { return s[0]; }
export function sUpper(s: "ab") { return s.toUpperCase(); }
export function sLitLen() { return "abc".length; }
export function sStringCall() { return String(1); }
export function sStringNew() { return new String("x"); }
export function sFromCharCode() { return String.fromCharCode(65); }
export function sTemplateTag() { function tag(s: TemplateStringsArray, ...v: number[]) { return s.raw; } return tag`a${1}`; }
export function fnApply() { function add(a: number, b: number) { return a + b; } return add.apply(undefined, [1, 2]); }
export function fnCall() { function add(a: number, b: number) { return a + b; } return add.call(undefined, 1, 2); }
export function fnBind() { function add(a: number, b: number) { return a + b; } return add.bind(undefined, 1); }
export function fnLength(f: () => void) { return f.length; }
export function numFixed(n: number) { return n.toFixed(2); }
export function objToString(o: { a: 1 }) { return o.toString(); }
export function objHas(o: { a: 1 }) { return o.hasOwnProperty("a"); }
"##;

/// A primitive's member resolves through its wrapper interface (`length`,
/// `charAt`, `split`, element reads, `toUpperCase`, `toFixed`), `String(…)` and
/// `new String(…)` resolve the constructor's call and construct signatures, a
/// tagged template receives `TemplateStringsArray`, and a function's `length`
/// and an object's `toString` / `hasOwnProperty` resolve through `Function` /
/// `Object`.
#[test]
fn wrapper_members_resolve_as_the_checker_resolves_them() {
    let matrix = Matrix::new(STRINGS_AND_FUNCTIONS).lib(GLOBALS_LIB);
    let failures = matrix.returns(&[
        ("sLen", "number"),
        ("sCharAt", "string"),
        ("sSplit", "string[]"),
        ("sIndex", "string"),
        ("sUpper", "string"),
        ("sStringCall", "string"),
        ("sStringNew", "String"),
        ("sFromCharCode", "string"),
        ("sTemplateTag", "readonly string[]"),
        ("fnLength", "number"),
        ("numFixed", "string"),
        ("objToString", "string"),
        ("objHas", "boolean"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `"abc".length` is `number`.
///
/// What the lane gives:
/// - `sLitLen`: the checker answers `number`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by FlowGap(UnmodeledExpression).
#[test]
#[ignore = "a member read on a string literal expression resolves through String"]
fn a_member_read_of_a_literal_expression_resolves() {
    let matrix = Matrix::new(STRINGS_AND_FUNCTIONS).lib(GLOBALS_LIB);
    let failures = matrix.returns(&[("sLitLen", "number")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Under `strictBindCallApply` `add.apply(undefined, [1, 2])` and
/// `add.call(undefined, 1, 2)` are `number` and `add.bind(undefined, 1)` is
/// `(b: number) => number`, through `CallableFunction`'s generic `this`-typed
/// signatures.
///
/// What the lane gives:
/// - `fnApply`: the checker answers `number`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnrepresentableCallee.
/// - `fnCall`: the checker answers `number`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnrepresentableCallee.
/// - `fnBind`: the checker answers `(b: number) => number`; the lane measured
///   `<opaque UnmodeledPosition>` degraded by UnrepresentableCallee.
#[test]
#[ignore = "apply, call and bind resolve through CallableFunction's generic signatures"]
fn function_apply_call_and_bind_resolve_through_callable_function() {
    let matrix = Matrix::new(STRINGS_AND_FUNCTIONS).lib(GLOBALS_LIB);
    let failures = matrix.returns(&[
        ("fnApply", "number"),
        ("fnCall", "number"),
        ("fnBind", "(b: number) => number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Promises, `await`, `async` functions, maps and sets.
const PROMISES_AND_COLLECTIONS: &str = r##"
declare const p: Promise<number>;
declare const pp: Promise<Promise<string>>;
export async function pAwait() { return await p; }
export async function pAwaitNested() { return await pp; }
export async function pReturnPromise() { return p; }
export async function pReturnValue() { return 1; }
export function pThen() { return p.then((v) => "" + v); }
export function pThenNoArg() { return p.then(); }
export function pCatch() { return p.catch(() => "e" as const); }
export function pResolve() { return Promise.resolve("x"); }
export function pResolveVoid() { return Promise.resolve(); }
export function pAll() { return Promise.all([p, "s"] as const); }
export function pRace() { return Promise.race([p, pp]); }
export function pNew() { return new Promise<boolean>((res) => res(true)); }
export function pReject() { return Promise.reject(new Error0()); }
export function mNew() { return new Map<string, number>(); }
export function mEntries() { return new Map([["a", 1]]); }
export function mGet(m: Map<string, number>) { return m.get("a"); }
export function mSet(m: Map<string, number>) { return m.set("a", 1); }
export function mSize(m: Map<string, number>) { return m.size; }
export function mUntyped() { return new Map(); }
export function sNew() { return new Set([1, 2]); }
export function sHas(s: Set<string>) { return s.has("a"); }
export function sAdd(s: Set<string>) { return s.add("a"); }
export function sEmpty() { return new Set(); }
class Error0 {}
"##;

/// `await` unwraps a promise (nested ones too), an `async` function's return is
/// a promise of its awaited return, `new Promise`, `Promise.reject`, `new Map`
/// / `new Set` (typed, inferred from entries or values, or untyped) and
/// `Awaited` resolve as the checker resolves them.
#[test]
fn promises_and_collections_resolve_as_the_checker_resolves_them() {
    let matrix = Matrix::new(PROMISES_AND_COLLECTIONS).lib(GLOBALS_LIB);
    let failures = matrix.same(&[
        (Read::Return("pAwait"), "Promise<number>"),
        (Read::Return("pAwaitNested"), "Promise<string>"),
        (Read::Return("pReturnPromise"), "Promise<number>"),
        (Read::Return("pReturnValue"), "Promise<number>"),
        (Read::Return("pResolveVoid"), "Promise<void>"),
        (Read::Return("pNew"), "Promise<boolean>"),
        (Read::Return("pReject"), "Promise<never>"),
        (Read::Return("mNew"), "Map<string, number>"),
        (Read::Return("mEntries"), "Map<string, number>"),
        (Read::Return("mUntyped"), "Map<any, any>"),
        (Read::Return("sNew"), "Set<number>"),
        (Read::Return("sEmpty"), "Set<any>"),
        (Read::Type("Awaited<Promise<number>>"), "number"),
        (Read::Type("Awaited<typeof pp>"), "string"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Method calls on `Promise<number>`, `Map<string, number>` and `Set<string>`
/// resolve through the instantiated interface: `p.then((v) => "" + v)` is
/// `Promise<string>`, `p.then()` `Promise<number>`, `p.catch(() => "e" as
/// const)` `Promise<number | "e">`, `m.get` `number | undefined`, `m.set` the
/// map, `s.has` `boolean`, `s.add` the set.
///
/// What the lane gives:
/// - `pThen`: the checker answers `Promise<string>`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnrepresentableCallee.
/// - `pThenNoArg`: the checker answers `Promise<number>`; the lane measured
///   `<opaque UnmodeledPosition>` degraded by UnrepresentableCallee.
/// - `pCatch`: the checker answers `Promise<number | "e">`; the lane measured
///   `<opaque UnmodeledPosition>` degraded by UnrepresentableCallee.
/// - `mGet`: the checker answers `number | undefined` (strict), `number`
///   (strictNullChecks off), `number | undefined` (noImplicitAny off), `number`
///   (both off); the lane measured `<opaque UnmodeledPosition>` degraded by
///   UnrepresentableCallee.
/// - `mSet`: the checker answers `Map<string, number>`; the lane measured
///   `<opaque UnmodeledPosition>` degraded by UnrepresentableCallee.
/// - `sHas`: the checker answers `boolean`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnrepresentableCallee.
/// - `sAdd`: the checker answers `Set<string>`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnrepresentableCallee.
#[test]
#[ignore = "a method call on an instantiated library generic interface resolves its signature"]
fn a_method_call_on_a_library_generic_instance_resolves() {
    let matrix = Matrix::new(PROMISES_AND_COLLECTIONS).lib(GLOBALS_LIB);
    let mut failures = matrix.returns(&[
        ("pThen", "Promise<string>"),
        ("pThenNoArg", "Promise<number>"),
        ("pCatch", "Promise<number | \"e\">"),
        ("mSet", "Map<string, number>"),
        ("sHas", "boolean"),
        ("sAdd", "Set<string>"),
    ]);
    failures.extend(matrix.nullness(&[(Read::Return("mGet"), "number | undefined", "number")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `ReturnType<Map<string, 1>['get']>` is `1 | undefined` (`1` without
/// `strictNullChecks`) and `Parameters<Set<number>['add']>` is `[value:
/// number]`, each read off the global interface.
#[test]
fn a_member_of_a_library_generic_instance_reads_in_type_position() {
    let matrix = Matrix::new(PROMISES_AND_COLLECTIONS).lib(GLOBALS_LIB);
    let mut failures = matrix.types(&[("Parameters<Set<number>['add']>", "[value: number]")]);
    failures.extend(matrix.nullness(&[(
        Read::Type("ReturnType<Map<string, 1>['get']>"),
        "1 | undefined",
        "1",
    )]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Promise.resolve("x")` is `Promise<string>`: an application's arguments
/// print as the types they resolve to, and `Awaited<string>` is `string`
/// (`Promise<Awaited<string>>` prints `Promise<string>`, `Map<Awaited<Promise<1>>,
/// keyof { a: 1; b: 2 }>` prints `Map<1, "a" | "b">`).
#[test]
fn promise_resolve_awaits_its_argument() {
    let matrix = Matrix::new(PROMISES_AND_COLLECTIONS).lib(GLOBALS_LIB);
    let mut failures = matrix.returns(&[("pResolve", "Promise<string>")]);
    failures.extend(matrix.types(&[
        ("Promise<Awaited<string>>", "Promise<string>"),
        (
            "Map<Awaited<Promise<1>>, keyof { a: 1; b: 2 }>",
            "Map<1, \"a\" | \"b\">",
        ),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Promise.all([p, "s"] as const)` is `Promise<[number, "s"]>` (a homomorphic
/// mapped type over the tuple) and `Promise.race([p, pp])` is `Promise<string |
/// number>`.
///
/// What the lane gives:
/// - `pAll`: the checker answers `Promise<[number, "s"]>`; the lane measured
///   `Promise<<unreduced mapped>>`.
/// - `pRace`: the checker answers `Promise<string | number>`; the lane measured
///   `Promise<Awaited<<unreduced indexed access>>>`.
#[test]
#[ignore = "Promise.all and Promise.race resolve their mapped and indexed return types"]
fn promise_all_and_race_map_their_tuple_argument() {
    let matrix = Matrix::new(PROMISES_AND_COLLECTIONS).lib(GLOBALS_LIB);
    let failures = matrix.returns(&[
        ("pAll", "Promise<[number, \"s\"]>"),
        ("pRace", "Promise<string | number>"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `m.size` over `m: Map<string, number>` is `number`: the member of the
/// global `Map` interface, never the receiver.
#[test]
fn a_map_size_read_reads_as_the_checker_reads_it() {
    let matrix = Matrix::new(PROMISES_AND_COLLECTIONS).lib(GLOBALS_LIB);
    let failures = matrix.returns(&[("mSize", "number")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
