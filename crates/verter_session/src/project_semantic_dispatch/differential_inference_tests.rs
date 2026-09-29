//! Differential probes of inference and relation corners: bare `infer`
//! selection, type parameter constraints and defaults, `NoInfer`, `const`
//! type parameters, variadic tuples, tail-recursive conditional types,
//! `infer` with an `extends` constraint, mapped type modifiers, key
//! remapping, homomorphic mapped types over arrays and tuples, and
//! `Awaited` / `Promise` inference.
//!
//! Every expected answer is TypeScript 7.0.2's (`tsc --ignoreConfig --noEmit
//! --strict --noErrorTruncation` under each `strictNullChecks` ×
//! `noImplicitAny` setting; the `Promise` rows with `--noLib` over the
//! library the global-library differential tests register), read off
//! TS2322 for `declare const p: <probe>; const s: never = p;` (a type row)
//! or `const s: never = f();` (a return row) and cross-checked against
//! TS2339 for `p.__nope`. The four answers are listed strict,
//! `strictNullChecks` off, `noImplicitAny` off, both off. An ignored test
//! asserts the measured answer for the rows the lane does not answer yet.

use super::differential_global_library_tests::GLOBALS_LIB;
use super::differential_harness_tests::{Matrix, Read};

/// The fixture of [`bare_infer_selection`].
const BARE_INFER_SELECTION: &str = r##"
type InferSel<T> = T extends infer X ? { label: string } : T;
type Id<T> = T extends infer X ? X : never;
type NoDist<T> = [T] extends [infer X] ? X : never;
declare function k<T>(x: T): InferSel<T>;
declare function kid<T>(x: T): Id<T>;
export function b1() { return k(1); }
export function b2() { return k(null! as never); }
export function b3() { return kid("a" as "a" | "b"); }
"##;

/// A bare `infer` extends selects its true branch for a check that is not generic and distributes over a union or `never`.
#[test]
fn bare_infer_selection() {
    let failures = Matrix::new(BARE_INFER_SELECTION).four(&[
        (
            Read::Type(r#"InferSel<never>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"InferSel<string>"#),
            r#"{ label: string; }"#,
            r#"{ label: string; }"#,
            r#"{ label: string; }"#,
            r#"{ label: string; }"#,
        ),
        (
            Read::Type(r#"Id<1 | 2>"#),
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
        ),
        (
            Read::Type(r#"Id<never>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"NoDist<1 | 2>"#),
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
        ),
        (
            Read::Type(r#"NoDist<never>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"[string] extends [infer X] ? X : 0"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Return(r#"b1"#),
            r#"{ label: string; }"#,
            r#"{ label: string; }"#,
            r#"{ label: string; }"#,
            r#"{ label: string; }"#,
        ),
        (
            Read::Return(r#"b2"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Return(r#"b3"#),
            r#""a" | "b""#,
            r#""a" | "b""#,
            r#""a" | "b""#,
            r#""a" | "b""#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`constraints_and_defaults`].
const CONSTRAINTS_AND_DEFAULTS: &str = r##"
declare function d1<T = string>(): T;
declare function d2<T extends number = 1>(x?: T): T;
declare function d3<T, U = T[]>(x: T): U;
declare function d4<T extends { a: unknown } = { a: 1 }>(x?: T): T["a"];
declare function d5<T extends string>(x: T): T;
declare function d6<T extends readonly unknown[]>(x: T): T;
declare function d7<K extends keyof O, O = { a: 1; b: 2 }>(k: K): O[K];
declare function d8<T extends string | number>(x: T): T[];
declare function d9<T extends object = {}>(x?: T): T;
type WithDef<T, U = T> = [T, U];
type Cons<T extends string = "x"> = { v: T };
export function e1() { return d1(); }
export function e2() { return d2(); }
export function e3() { return d2(5); }
export function e4() { return d3(1); }
export function e5() { return d4(); }
export function e6() { return d4({ a: "z" }); }
export function e7() { return d5("q"); }
export function e8() { return d6([1, 2]); }
export function e9() { return d7("a"); }
export function e10() { return d8(1); }
export function e11() { return d9(); }
"##;

/// Type parameters with constraints and defaults infer, fall back and relate as the checker's do.
#[test]
fn constraints_and_defaults() {
    let failures = Matrix::new(CONSTRAINTS_AND_DEFAULTS).four(&[
        (
            Read::Return(r#"e1"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Return(r#"e2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Return(r#"e3"#),
            r#"5"#,
            r#"5"#,
            r#"5"#,
            r#"5"#,
        ),
        (
            Read::Return(r#"e4"#),
            r#"number[]"#,
            r#"number[]"#,
            r#"number[]"#,
            r#"number[]"#,
        ),
        (
            Read::Return(r#"e5"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Return(r#"e6"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Return(r#"e7"#),
            r#""q""#,
            r#""q""#,
            r#""q""#,
            r#""q""#,
        ),
        (
            Read::Return(r#"e8"#),
            r#"number[]"#,
            r#"number[]"#,
            r#"number[]"#,
            r#"number[]"#,
        ),
        (
            Read::Return(r#"e9"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Return(r#"e10"#),
            r#"1[]"#,
            r#"1[]"#,
            r#"1[]"#,
            r#"1[]"#,
        ),
        (
            Read::Return(r#"e11"#),
            r#"{}"#,
            r#"{}"#,
            r#"{}"#,
            r#"{}"#,
        ),
        (
            Read::Type(r#"[WithDef<1>] extends [[1, 1]] ? ([[1, 1]] extends [WithDef<1>] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[WithDef<1, 2>] extends [[1, 2]] ? ([[1, 2]] extends [WithDef<1, 2>] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[Cons] extends [{ v: "x" }] ? ([{ v: "x" }] extends [Cons] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[Cons<"y">] extends [{ v: "y" }] ? ([{ v: "y" }] extends [Cons<"y">] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"WithDef<1>[1]"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"Cons["v"]"#),
            r#""x""#,
            r#""x""#,
            r#""x""#,
            r#""x""#,
        ),
        (
            Read::Type(r#"[<T extends string = "a">() => T] extends [() => string] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[<T extends string>(x: T) => T] extends [(x: "a") => "a"] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`no_infer`].
const NO_INFER: &str = r##"
declare function ni1<T>(a: T, b: NoInfer<T>): T;
declare function ni2<T>(a: NoInfer<T>, b: T): T;
declare function ni3<T extends string>(values: T[], def: NoInfer<T>): T;
declare function ni4<T>(x: T, f: (v: NoInfer<T>) => void): T;
declare function ni5<T>(x: NoInfer<T>): T;
export function n1() { return ni1(1, 1); }
export function n2() { return ni2("a", "a"); }
export function n3() { return ni3(["x", "y"], "x"); }
export function n4() { return ni4(3, (v) => {}); }
export function n5() { return ni5(1); }
"##;

/// `NoInfer` blocks inference through the parameter it wraps.
#[test]
fn no_infer() {
    let failures = Matrix::new(NO_INFER).four(&[
        (
            Read::Return(r#"n1"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Return(r#"n2"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Return(r#"n4"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Return(r#"n5"#),
            r#"unknown"#,
            r#"unknown"#,
            r#"unknown"#,
            r#"unknown"#,
        ),
        (
            Read::Type(r#"NoInfer<string>"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Type(r#"[NoInfer<1>] extends [number] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`const_type_parameters`].
const CONST_TYPE_PARAMETERS: &str = r##"
declare function c1<const T>(x: T): T;
declare function c2<const T extends readonly unknown[]>(x: T): T;
declare function c3<const T extends unknown[]>(x: T): T;
declare function c4<const T extends { a: unknown }>(x: T): T;
declare function c5<const T>(...xs: T[]): T;
declare function c6<const T extends readonly string[]>(x: [...T]): T;
declare function c7<const T extends Record<string, unknown>>(x: T): T;
export function k1() { return c1([1, "a"]); }
export function k2() { return c1({ a: 1, b: [true] }); }
export function k3() { return c2([1, 2]); }
export function k4() { return c3([1, 2]); }
export function k5() { return c4({ a: "x" }); }
export function k6() { return c5(1, 2); }
export function k7() { return c6(["a", "b"]); }
export function k8() { const arr = [1, 2]; return c1(arr); }
export function k9() { return c1(1); }
export function k10() { return c1(null); }
export function k11() { return c7({ n: { m: [1] } }); }
export function k12() { return c2([]); }
"##;

/// `const` type parameters infer literal, readonly arrays, tuples and objects.
#[test]
fn const_type_parameters() {
    let failures = Matrix::new(CONST_TYPE_PARAMETERS).four(&[
        (
            Read::Return(r#"k1"#),
            r#"readonly [1, "a"]"#,
            r#"readonly [1, "a"]"#,
            r#"readonly [1, "a"]"#,
            r#"readonly [1, "a"]"#,
        ),
        (
            Read::Return(r#"k2"#),
            r#"{ readonly a: 1; readonly b: readonly [true]; }"#,
            r#"{ readonly a: 1; readonly b: readonly [true]; }"#,
            r#"{ readonly a: 1; readonly b: readonly [true]; }"#,
            r#"{ readonly a: 1; readonly b: readonly [true]; }"#,
        ),
        (
            Read::Return(r#"k3"#),
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
        ),
        (
            Read::Return(r#"k5"#),
            r#"{ readonly a: "x"; }"#,
            r#"{ readonly a: "x"; }"#,
            r#"{ readonly a: "x"; }"#,
            r#"{ readonly a: "x"; }"#,
        ),
        (
            Read::Return(r#"k6"#),
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
        ),
        (
            Read::Return(r#"k8"#),
            r#"number[]"#,
            r#"number[]"#,
            r#"number[]"#,
            r#"number[]"#,
        ),
        (Read::Return(r#"k9"#), r#"1"#, r#"1"#, r#"1"#, r#"1"#),
        (
            Read::Return(r#"k10"#),
            r#"null"#,
            r#"any"#,
            r#"null"#,
            r#"any"#,
        ),
        (
            Read::Return(r#"k11"#),
            r#"{ readonly n: { readonly m: readonly [1]; }; }"#,
            r#"{ readonly n: { readonly m: readonly [1]; }; }"#,
            r#"{ readonly n: { readonly m: readonly [1]; }; }"#,
            r#"{ readonly n: { readonly m: readonly [1]; }; }"#,
        ),
        (
            Read::Return(r#"k12"#),
            r#"readonly []"#,
            r#"readonly []"#,
            r#"readonly []"#,
            r#"readonly []"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`variadic_tuples`].
const VARIADIC_TUPLES: &str = r##"
type Push<T extends unknown[], U> = [...T, U];
type Unshift<T extends unknown[], U> = [U, ...T];
type Concat<A extends unknown[], B extends unknown[]> = [...A, ...B];
type Last<T extends unknown[]> = T extends [...infer _, infer L] ? L : never;
type Init<T extends unknown[]> = T extends [...infer I, unknown] ? I : never;
type Head<T> = T extends [infer H, ...unknown[]] ? H : never;
type Mid<T> = T extends [unknown, ...infer M, unknown] ? M : never;
declare function tail<T extends unknown[]>(xs: [unknown, ...T]): T;
declare function concat<A extends unknown[], B extends unknown[]>(a: [...A], b: [...B]): [...A, ...B];
declare function pushF<T extends unknown[], U>(t: [...T], u: U): [...T, U];
declare function args<A extends unknown[]>(f: (...a: A) => void): A;
declare function fn2(a: string, b?: number): void;
export function v1() { return tail([1, "a", true]); }
export function v2() { return concat([1], ["a"]); }
export function v3() { return pushF([1, 2], "x"); }
export function v4() { return args(fn2); }
export function v5() { return args((a: 1, ...r: string[]) => {}); }
"##;

/// Variadic tuple types spread, infer and instantiate as the checker's do.
#[test]
fn variadic_tuples() {
    let failures = Matrix::new(VARIADIC_TUPLES).four(&[
        (
            Read::Type(r#"Push<[1, 2], 3>"#),
            r#"[1, 2, 3]"#,
            r#"[1, 2, 3]"#,
            r#"[1, 2, 3]"#,
            r#"[1, 2, 3]"#,
        ),
        (
            Read::Type(r#"Unshift<[1], 0>"#),
            r#"[0, 1]"#,
            r#"[0, 1]"#,
            r#"[0, 1]"#,
            r#"[0, 1]"#,
        ),
        (
            Read::Type(r#"Concat<[1], [2, 3]>"#),
            r#"[1, 2, 3]"#,
            r#"[1, 2, 3]"#,
            r#"[1, 2, 3]"#,
            r#"[1, 2, 3]"#,
        ),
        (
            Read::Type(r#"Concat<[1], string[]>"#),
            r#"[1, ...string[]]"#,
            r#"[1, ...string[]]"#,
            r#"[1, ...string[]]"#,
            r#"[1, ...string[]]"#,
        ),
        (
            Read::Type(r#"Concat<string[], [1]>"#),
            r#"[...string[], 1]"#,
            r#"[...string[], 1]"#,
            r#"[...string[], 1]"#,
            r#"[...string[], 1]"#,
        ),
        (
            Read::Type(r#"Last<[1, 2, 3]>"#),
            r#"3"#,
            r#"3"#,
            r#"3"#,
            r#"3"#,
        ),
        (
            Read::Type(r#"Last<[]>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"Last<string[]>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"Init<[1, 2, 3]>"#),
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
        ),
        (
            Read::Type(r#"Mid<[1, 2, 3, 4]>"#),
            r#"[2, 3]"#,
            r#"[2, 3]"#,
            r#"[2, 3]"#,
            r#"[2, 3]"#,
        ),
        (
            Read::Type(r#"Head<string[]>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"[...[1, 2], ...number[]]"#),
            r#"[1, 2, ...number[]]"#,
            r#"[1, 2, ...number[]]"#,
            r#"[1, 2, ...number[]]"#,
            r#"[1, 2, ...number[]]"#,
        ),
        (
            Read::Type(r#"[1, ...[2, ...[3]]]"#),
            r#"[1, 2, 3]"#,
            r#"[1, 2, 3]"#,
            r#"[1, 2, 3]"#,
            r#"[1, 2, 3]"#,
        ),
        (
            Read::Type(r#"[[1, 2]] extends [[...number[]]] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Return(r#"v4"#),
            r#"[a: string, b?: number | undefined]"#,
            r#"[a: string, b?: number]"#,
            r#"[a: string, b?: number | undefined]"#,
            r#"[a: string, b?: number]"#,
        ),
        (
            Read::Return(r#"v5"#),
            r#"[a: 1, ...r: string[]]"#,
            r#"[a: 1, ...r: string[]]"#,
            r#"[a: 1, ...r: string[]]"#,
            r#"[a: 1, ...r: string[]]"#,
        ),
        (
            Read::Type(r#"Push<string[], 1>"#),
            r#"[...string[], 1]"#,
            r#"[...string[], 1]"#,
            r#"[...string[], 1]"#,
            r#"[...string[], 1]"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`recursive_conditionals`].
const RECURSIVE_CONDITIONALS: &str = r##"
type Rep<N extends number, Acc extends unknown[] = []> = Acc["length"] extends N ? Acc : Rep<N, [...Acc, 0]>;
type Len<T extends unknown[]> = T["length"];
type TrimLeft<S extends string> = S extends ` ${infer R}` ? TrimLeft<R> : S;
type Count<S extends string, Acc extends unknown[] = []> = S extends `${string}${infer R}` ? Count<R, [...Acc, 0]> : Acc["length"];
type Rev<T extends unknown[], Acc extends unknown[] = []> = T extends [infer H, ...infer R] ? Rev<R, [H, ...Acc]> : Acc;
type Deep<T, N extends unknown[] = []> = N["length"] extends 50 ? T : Deep<{ v: T }, [...N, 0]>;
"##;

/// Tail-recursive conditional types evaluate to the checker's depth.
#[test]
fn recursive_conditionals() {
    let failures = Matrix::new(RECURSIVE_CONDITIONALS).four(&[
        (Read::Type(r#"Len<Rep<5>>"#), r#"5"#, r#"5"#, r#"5"#, r#"5"#),
        (
            Read::Type(r#"Len<Rep<100>>"#),
            r#"100"#,
            r#"100"#,
            r#"100"#,
            r#"100"#,
        ),
        (Read::Type(r#"Count<"">"#), r#"0"#, r#"0"#, r#"0"#, r#"0"#),
        (
            Read::Type(r#"Len<Rep<50>>"#),
            r#"50"#,
            r#"50"#,
            r#"50"#,
            r#"50"#,
        ),
        (
            Read::Type(r#"[Deep<1>] extends [Deep<number>] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`infer_with_constraints`].
const INFER_WITH_CONSTRAINTS: &str = r##"
type FirstStr<T> = T extends [infer S extends string, ...unknown[]] ? S : never;
type NumPart<T> = T extends `${infer N extends number}` ? N : never;
type BoolPart<T> = T extends `${infer B extends boolean}` ? B : never;
type Get<T> = T extends { a: infer A extends string } ? A : "no";
type RetStr<F> = F extends () => (infer R extends string) ? R : "no";
"##;

/// The fixture of [`mapped_modifiers`].
const MAPPED_MODIFIERS: &str = r##"
interface M { readonly a: 1; b?: 2; readonly c?: 3 }
type Mut<T> = { -readonly [K in keyof T]: T[K] };
type Req<T> = { [K in keyof T]-?: T[K] };
type Opt<T> = { [K in keyof T]+?: T[K] };
type RO<T> = { +readonly [K in keyof T]: T[K] };
type MutReq<T> = { -readonly [K in keyof T]-?: T[K] };
"##;

/// Mapped type modifiers add and remove `readonly` and `?`.
#[test]
fn mapped_modifiers() {
    let failures = Matrix::new(MAPPED_MODIFIERS).four(&[
        (
            Read::Type(r#"{ -readonly [K in keyof M]: M[K] }"#),
            r#"{ a: 1; b?: 2 | undefined; c?: 3 | undefined; }"#,
            r#"{ a: 1; b?: 2; c?: 3; }"#,
            r#"{ a: 1; b?: 2 | undefined; c?: 3 | undefined; }"#,
            r#"{ a: 1; b?: 2; c?: 3; }"#,
        ),
        (
            Read::Type(r#"{ [K in keyof M]-?: M[K] }"#),
            r#"{ readonly a: 1; b: 2; readonly c: 3; }"#,
            r#"{ readonly a: 1; b: 2; readonly c: 3; }"#,
            r#"{ readonly a: 1; b: 2; readonly c: 3; }"#,
            r#"{ readonly a: 1; b: 2; readonly c: 3; }"#,
        ),
        (
            Read::Type(r#"{ [K in keyof M]+?: M[K] }"#),
            r#"{ readonly a?: 1 | undefined; b?: 2 | undefined; readonly c?: 3 | undefined; }"#,
            r#"{ readonly a?: 1; b?: 2; readonly c?: 3; }"#,
            r#"{ readonly a?: 1 | undefined; b?: 2 | undefined; readonly c?: 3 | undefined; }"#,
            r#"{ readonly a?: 1; b?: 2; readonly c?: 3; }"#,
        ),
        (
            Read::Type(r#"{ +readonly [K in keyof M]: M[K] }"#),
            r#"{ readonly a: 1; readonly b?: 2 | undefined; readonly c?: 3 | undefined; }"#,
            r#"{ readonly a: 1; readonly b?: 2; readonly c?: 3; }"#,
            r#"{ readonly a: 1; readonly b?: 2 | undefined; readonly c?: 3 | undefined; }"#,
            r#"{ readonly a: 1; readonly b?: 2; readonly c?: 3; }"#,
        ),
        (
            Read::Type(r#"{ -readonly [K in keyof M]-?: M[K] }"#),
            r#"{ a: 1; b: 2; c: 3; }"#,
            r#"{ a: 1; b: 2; c: 3; }"#,
            r#"{ a: 1; b: 2; c: 3; }"#,
            r#"{ a: 1; b: 2; c: 3; }"#,
        ),
        (
            Read::Type(r#"Req<{ a?: string | undefined }>["a"]"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Type(r#"Req<{ a?: undefined }>["a"]"#),
            r#"never"#,
            r#"undefined"#,
            r#"never"#,
            r#"undefined"#,
        ),
        (
            Read::Type(r#"[Req<M>] extends [{ readonly a: 1; b: 2; readonly c: 3 }] ? ([{ readonly a: 1; b: 2; readonly c: 3 }] extends [Req<M>] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[MutReq<M>] extends [{ a: 1; b: 2; c: 3 }] ? ([{ a: 1; b: 2; c: 3 }] extends [MutReq<M>] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[Mut<M>] extends [{ a: 1 }] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[Opt<{ a: 1 }>] extends [{ a: 1 }] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[RO<{ a: 1 }>] extends [{ a: 1 }] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`key_remapping`].
const KEY_REMAPPING: &str = r##"
type Getters<T> = { [K in keyof T as `get${Capitalize<string & K>}`]: () => T[K] };
type Filter<T, U> = { [K in keyof T as T[K] extends U ? K : never]: T[K] };
type Prefix<T> = { [K in keyof T as `p_${K & string}`]: T[K] };
type Ex<T, U> = T extends U ? never : T;
type Drop<T, D> = { [K in keyof T as Ex<K, D>]: T[K] };
"##;

/// Mapped types remap their keys with `as`.
#[test]
fn key_remapping() {
    let failures = Matrix::new(KEY_REMAPPING).four(&[
        (
            Read::Type(r#"Getters<{ name: string }>["getName"]"#),
            r#"() => string"#,
            r#"() => string"#,
            r#"() => string"#,
            r#"() => string"#,
        ),
        (
            Read::Type(r#"keyof Prefix<{ a: 1; b: 2 }>"#),
            r#""p_a" | "p_b""#,
            r#""p_a" | "p_b""#,
            r#""p_a" | "p_b""#,
            r#""p_a" | "p_b""#,
        ),
        (
            Read::Type(r#"keyof Filter<{ a: 1; b: "x"; c: 2 }, number>"#),
            r#""a" | "c""#,
            r#""a" | "c""#,
            r#""a" | "c""#,
            r#""a" | "c""#,
        ),
        (
            Read::Type(r#"{ [K in "a" | "b" as Uppercase<K>]: K }"#),
            r#"{ A: "a"; B: "b"; }"#,
            r#"{ A: "a"; B: "b"; }"#,
            r#"{ A: "a"; B: "b"; }"#,
            r#"{ A: "a"; B: "b"; }"#,
        ),
        (
            Read::Type(r#"{ [K in "a" as 1]: K }"#),
            r#"{ 1: "a"; }"#,
            r#"{ 1: "a"; }"#,
            r#"{ 1: "a"; }"#,
            r#"{ 1: "a"; }"#,
        ),
        (
            Read::Type(r#"{ [K in "a" | "b" as K extends "a" ? "x" : "x"]: K }"#),
            r#"{ x: "a" | "b"; }"#,
            r#"{ x: "a" | "b"; }"#,
            r#"{ x: "a" | "b"; }"#,
            r#"{ x: "a" | "b"; }"#,
        ),
        (
            Read::Type(r#"keyof Filter<{ a?: 1; b: 2 }, number>"#),
            r#""b""#,
            r#""a" | "b""#,
            r#""b""#,
            r#""a" | "b""#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`homomorphic_arrays_and_tuples`].
const HOMOMORPHIC_ARRAYS_AND_TUPLES: &str = r##"
type Box<T> = { [K in keyof T]: { v: T[K] } };
type Nullable<T> = { [K in keyof T]: T[K] | null };
type ROM<T> = { readonly [K in keyof T]: T[K] };
type Str<T> = { [K in keyof T]: string };
type MutM<T> = { -readonly [K in keyof T]: T[K] };
declare function hm<T extends unknown[]>(x: T): Box<T>;
declare function hmc<const T extends readonly unknown[]>(x: T): Box<T>;
export function h1() { return hm([1, "a"]); }
export function h2() { return hmc([1, "a"]); }
"##;

/// Homomorphic mapped types map arrays and tuples element-wise.
#[test]
fn homomorphic_arrays_and_tuples() {
    let failures = Matrix::new(HOMOMORPHIC_ARRAYS_AND_TUPLES).four(&[
        (
            Read::Type(r#"Box<[1, 2]>"#),
            r#"[{ v: 1; }, { v: 2; }]"#,
            r#"[{ v: 1; }, { v: 2; }]"#,
            r#"[{ v: 1; }, { v: 2; }]"#,
            r#"[{ v: 1; }, { v: 2; }]"#,
        ),
        (
            Read::Type(r#"Box<number[]>"#),
            r#"{ v: number; }[]"#,
            r#"{ v: number; }[]"#,
            r#"{ v: number; }[]"#,
            r#"{ v: number; }[]"#,
        ),
        (
            Read::Type(r#"Nullable<[1, "a"]>"#),
            r#"[1 | null, "a" | null]"#,
            r#"[1, "a"]"#,
            r#"[1 | null, "a" | null]"#,
            r#"[1, "a"]"#,
        ),
        (
            Read::Type(r#"ROM<[1, 2]>"#),
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
        ),
        (
            Read::Type(r#"ROM<number[]>"#),
            r#"readonly number[]"#,
            r#"readonly number[]"#,
            r#"readonly number[]"#,
            r#"readonly number[]"#,
        ),
        (
            Read::Type(r#"Str<[1, 2, 3]>"#),
            r#"[string, string, string]"#,
            r#"[string, string, string]"#,
            r#"[string, string, string]"#,
            r#"[string, string, string]"#,
        ),
        (
            Read::Type(r#"Box<readonly [1]>"#),
            r#"readonly [{ v: 1; }]"#,
            r#"readonly [{ v: 1; }]"#,
            r#"readonly [{ v: 1; }]"#,
            r#"readonly [{ v: 1; }]"#,
        ),
        (
            Read::Type(r#"Box<[a: 1, b?: 2]>"#),
            r#"[a: { v: 1; }, b?: { v: 2 | undefined; } | undefined]"#,
            r#"[a: { v: 1; }, b?: { v: 2; }]"#,
            r#"[a: { v: 1; }, b?: { v: 2 | undefined; } | undefined]"#,
            r#"[a: { v: 1; }, b?: { v: 2; }]"#,
        ),
        (
            Read::Type(r#"Box<[1, ...string[]]>"#),
            r#"[{ v: 1; }, ...{ v: string; }[]]"#,
            r#"[{ v: 1; }, ...{ v: string; }[]]"#,
            r#"[{ v: 1; }, ...{ v: string; }[]]"#,
            r#"[{ v: 1; }, ...{ v: string; }[]]"#,
        ),
        (
            Read::Type(r#"MutM<readonly string[]>"#),
            r#"string[]"#,
            r#"string[]"#,
            r#"string[]"#,
            r#"string[]"#,
        ),
        (Read::Type(r#"Box<[]>"#), r#"[]"#, r#"[]"#, r#"[]"#, r#"[]"#),
        (
            Read::Type(r#"Str<readonly [1]>"#),
            r#"readonly [string]"#,
            r#"readonly [string]"#,
            r#"readonly [string]"#,
            r#"readonly [string]"#,
        ),
        (
            Read::Return(r#"h1"#),
            r#"{ v: string | number; }[]"#,
            r#"{ v: string | number; }[]"#,
            r#"{ v: string | number; }[]"#,
            r#"{ v: string | number; }[]"#,
        ),
        (
            Read::Return(r#"h2"#),
            r#"readonly [{ v: 1; }, { v: "a"; }]"#,
            r#"readonly [{ v: 1; }, { v: "a"; }]"#,
            r#"readonly [{ v: 1; }, { v: "a"; }]"#,
            r#"readonly [{ v: 1; }, { v: "a"; }]"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`awaited_and_promises`].
const AWAITED_AND_PROMISES: &str = r##"
declare function pr<T>(x: T): Promise<T>;
declare const p1: Promise<number>;
declare const p2: Promise<Promise<string>>;
declare function aw<T>(x: T): Awaited<T>;
declare const thenable: { then(f: (v: "t") => void): void };
declare const cond: boolean;
export async function a1() { return 1; }
export async function a2() { return p1; }
export async function a3() { return await p2; }
export function a4() { return aw(p1); }
export function a5() { return pr(1); }
export async function a6() { const x = await p1; return x; }
export function a7() { return p1.then((x) => [x]); }
export function a8() { return Promise.resolve(p2); }
export function a9() { return Promise.all([p1, 1] as const); }
export async function a10() { return await thenable; }
export function a11() { return aw(thenable); }
export async function a12() { if (cond) return 1; return p1; }
"##;

/// `Awaited` and `Promise` inference through the library's declarations.
#[test]
fn awaited_and_promises() {
    let failures = Matrix::new(AWAITED_AND_PROMISES).lib(GLOBALS_LIB).four(&[
        (
            Read::Type(r#"Awaited<Promise<number>>"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Type(r#"Awaited<Promise<Promise<1>>>"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"Awaited<number | Promise<string>>"#),
            r#"string | number"#,
            r#"string | number"#,
            r#"string | number"#,
            r#"string | number"#,
        ),
        (
            Read::Type(r#"Awaited<null>"#),
            r#"null"#,
            r#"null"#,
            r#"null"#,
            r#"null"#,
        ),
        (
            Read::Type(r#"Awaited<PromiseLike<"x">>"#),
            r#""x""#,
            r#""x""#,
            r#""x""#,
            r#""x""#,
        ),
        (
            Read::Type(r#"Awaited<{ then(): void }>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"Awaited<Promise<number> | Promise<string>>"#),
            r#"string | number"#,
            r#"string | number"#,
            r#"string | number"#,
            r#"string | number"#,
        ),
        (
            Read::Type(r#"Awaited<any>"#),
            r#"any"#,
            r#"any"#,
            r#"any"#,
            r#"any"#,
        ),
        (
            Read::Type(r#"Awaited<never>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Return(r#"a1"#),
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
        ),
        (
            Read::Return(r#"a2"#),
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
        ),
        (
            Read::Return(r#"a3"#),
            r#"Promise<string>"#,
            r#"Promise<string>"#,
            r#"Promise<string>"#,
            r#"Promise<string>"#,
        ),
        (
            Read::Return(r#"a4"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Return(r#"a5"#),
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
        ),
        (
            Read::Return(r#"a6"#),
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
        ),
        (
            Read::Return(r#"a10"#),
            r#"Promise<"t">"#,
            r#"Promise<"t">"#,
            r#"Promise<"t">"#,
            r#"Promise<"t">"#,
        ),
        (
            Read::Return(r#"a11"#),
            r#""t""#,
            r#""t""#,
            r#""t""#,
            r#""t""#,
        ),
        (
            Read::Return(r#"a12"#),
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
            r#"Promise<number>"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`hype_repros`].
const HYPE_REPROS: &str = r##"
type U2 = "a" | 1;
type Nv = never;
type A = any;
type Un = unknown;
type W<T> = [T] extends [string] ? 1 : 2;
type IsStr<T> = T extends string ? "yes" : "no";
type Dist3<P> = P extends "a" ? P : 2;
type Dist<P> = P extends "a" ? [P] : 2;
type PE<M, L> = { type: "ParsingError"; message: M; lineNumber: L };
type E<X> = X extends PE<any, any> ? "err" : "ok";
type F<X> = [X] extends [PE<infer M, any>] ? M : "ok";
type Push<T extends any[], E> = [...T, E];
type Eat<T> = T extends `${infer A}${infer B}` ? B : "";
type A8<I extends string> = I extends `${infer A}${infer B}` ? A8<Eat<I>> : "done";
type T8<I extends string> = I extends `${infer A}${infer B}` ? T8<B> : "done";
type Box<X> = { v: X };
type InferExt<T> = T extends [infer X extends string] ? X : "nope";
"##;

/// Reduced repros from the HypeScript type-retrieval run.
#[test]
fn hype_repros() {
    let failures = Matrix::new(HYPE_REPROS).four(&[
        (
            Read::Type(r#"IsStr<U2>"#),
            r#""no" | "yes""#,
            r#""no" | "yes""#,
            r#""no" | "yes""#,
            r#""no" | "yes""#,
        ),
        (
            Read::Type(r#"IsStr<Nv>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"IsStr<A>"#),
            r#""no" | "yes""#,
            r#""no" | "yes""#,
            r#""no" | "yes""#,
            r#""no" | "yes""#,
        ),
        (
            Read::Type(r#"A extends Un ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"A extends string ? 1 : 2"#),
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
        ),
        (
            Read::Type(r#"Nv extends string ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"U2 extends string ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (Read::Type(r#"W<Nv>"#), r#"1"#, r#"1"#, r#"1"#, r#"1"#),
        (
            Read::Type(r#"Dist3<"a" | "b">"#),
            r#""a" | 2"#,
            r#""a" | 2"#,
            r#""a" | 2"#,
            r#""a" | 2"#,
        ),
        (
            Read::Type(r#"Dist<"a" | "b">"#),
            r#"2 | ["a"]"#,
            r#"2 | ["a"]"#,
            r#"2 | ["a"]"#,
            r#"2 | ["a"]"#,
        ),
        (
            Read::Type(r#"[never] extends [infer P] ? (P extends 0 ? 1 : 2) : 3"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"Nv extends infer P ? (P extends 0 ? 1 : 2) : 3"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"E<null>"#),
            r#""ok""#,
            r#""err""#,
            r#""ok""#,
            r#""err""#,
        ),
        (
            Read::Type(r#"E<undefined>"#),
            r#""ok""#,
            r#""err""#,
            r#""ok""#,
            r#""err""#,
        ),
        (
            Read::Type(r#"PE<1, 2> extends PE<any, any> ? "err" : "ok""#),
            r#""err""#,
            r#""err""#,
            r#""err""#,
            r#""err""#,
        ),
        (
            Read::Type(r#"null extends PE<any, any> ? "err" : "ok""#),
            r#""ok""#,
            r#""err""#,
            r#""ok""#,
            r#""err""#,
        ),
        (
            Read::Type(r#"Push<[1], 2>"#),
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
        ),
        (
            Read::Type(r#"A8<"aa"> extends "done" ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`template_escapes`].
const TEMPLATE_ESCAPES: &str = r##"
type NL = `a\n`;
type P<S> = S extends `${infer A}\n${infer B}` ? [A, B] : 0;
type L<S extends string, N extends 0[] = []> = S extends `${string}${infer R}` ? L<R, [...N, 0]> : N["length"];
type Q<S> = S extends `${infer A}\\${infer B}` ? [A, B] : 0;
type C = `a\
b`;
type Tab<S> = S extends `a${string}\t` ? 1 : 0;
export function v1() { return `a\n${"q"}` as const; }
export function v2() { return `\u{41}\x42\\${"c"}` as const; }
export function v3(k: string) { return `a\t${k}` as const; }
export function v4() { return [`x\ty`] as const; }
"##;

/// A template literal reads its cooked text in type and value position, an invalid escape its raw text.
#[test]
fn template_escapes() {
    let failures = Matrix::new(TEMPLATE_ESCAPES).four(&[
        (
            Read::Type(r#"NL"#),
            r#""a\n""#,
            r#""a\n""#,
            r#""a\n""#,
            r#""a\n""#,
        ),
        (
            Read::Type(r#"NL extends "a\n" ? 1 : 0"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#""a\\n" extends NL ? 1 : 0"#),
            r#"0"#,
            r#"0"#,
            r#"0"#,
            r#"0"#,
        ),
        (
            Read::Type(r#"P<"p\nq">"#),
            r#"["p", "q"]"#,
            r#"["p", "q"]"#,
            r#"["p", "q"]"#,
            r#"["p", "q"]"#,
        ),
        (Read::Type(r#"P<"p\\nq">"#), r#"0"#, r#"0"#, r#"0"#, r#"0"#),
        (
            Read::Type(r#"Q<"x\\y">"#),
            r#"["x", "y"]"#,
            r#"["x", "y"]"#,
            r#"["x", "y"]"#,
            r#"["x", "y"]"#,
        ),
        (
            Read::Type(r#"C"#),
            r#""ab""#,
            r#""ab""#,
            r#""ab""#,
            r#""ab""#,
        ),
        (
            Read::Type(r#"`a${string}\u{42}`"#),
            r#"`a${string}B`"#,
            r#"`a${string}B`"#,
            r#"`a${string}B`"#,
            r#"`a${string}B`"#,
        ),
        (
            Read::Type(r#""aZB" extends `a${string}\u{42}` ? 1 : 0"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#""aZB" extends `a${string}\x42` ? 1 : 0"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (Read::Type(r#"Tab<"a1\t">"#), r#"1"#, r#"1"#, r#"1"#, r#"1"#),
        (
            Read::Type(r#"Tab<"a1\\t">"#),
            r#"0"#,
            r#"0"#,
            r#"0"#,
            r#"0"#,
        ),
        (
            Read::Type(r#"`a\`b${string}`"#),
            r#"`a\`b${string}`"#,
            r#"`a\`b${string}`"#,
            r#"`a\`b${string}`"#,
            r#"`a\`b${string}`"#,
        ),
        (
            Read::Type(r#""a`b1" extends `a\`b${string}` ? 1 : 0"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#""a${b}1" extends `a\${b}${string}` ? 1 : 0"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"`x${string}\\`"#),
            r#"`x${string}\\`"#,
            r#"`x${string}\\`"#,
            r#"`x${string}\\`"#,
            r#"`x${string}\\`"#,
        ),
        (
            Read::Type(r#"`a\t${string}`"#),
            r#"`a\t${string}`"#,
            r#"`a\t${string}`"#,
            r#"`a\t${string}`"#,
            r#"`a\t${string}`"#,
        ),
        (
            Read::Type(r#"`a\u{zz}b`"#),
            r#""a\\u{zz}b""#,
            r#""a\\u{zz}b""#,
            r#""a\\u{zz}b""#,
            r#""a\\u{zz}b""#,
        ),
        (
            Read::Return(r#"v1"#),
            r#""a\nq""#,
            r#""a\nq""#,
            r#""a\nq""#,
            r#""a\nq""#,
        ),
        (
            Read::Return(r#"v2"#),
            r#""AB\\c""#,
            r#""AB\\c""#,
            r#""AB\\c""#,
            r#""AB\\c""#,
        ),
        (
            Read::Return(r#"v3"#),
            r#"`a\t${string}`"#,
            r#"`a\t${string}`"#,
            r#"`a\t${string}`"#,
            r#"`a\t${string}`"#,
        ),
        (
            Read::Return(r#"v4"#),
            r#"readonly ["x\ty"]"#,
            r#"readonly ["x\ty"]"#,
            r#"readonly ["x\ty"]"#,
            r#"readonly ["x\ty"]"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The checker keeps an array literal argument's element literals when the parameter's element type is a type parameter with a primitive constraint (`isLiteralOfContextualType`): `ni3(["x", "y"], "x")` is `"x" | "y"`.
#[test]
#[ignore = "WRONG-CLEAN in the lane: a call argument is evaluated before its parameter's contextual type is known, so the array literal's elements arrive widened; contextual literal retention for nested argument literals is not modelled yet"]
fn pinned_array_literal_arguments_keep_literals_under_a_primitive_constraint() {
    let failures = Matrix::new(NO_INFER).four(&[(
        Read::Return(r#"n3"#),
        r#""x" | "y""#,
        r#""x" | "y""#,
        r#""x" | "y""#,
        r#""x" | "y""#,
    )]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A `const` type parameter inferred through a rest parameter keeps the argument tuple's literals.
#[test]
#[ignore = "typed gap in the lane: a rest parameter over a const type parameter is an UnrepresentableCallee"]
fn pinned_const_type_parameters_through_rest_parameters() {
    let failures = Matrix::new(CONST_TYPE_PARAMETERS).four(&[
        (
            Read::Return(r#"k4"#),
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
        ),
        (
            Read::Return(r#"k7"#),
            r#"["a", "b"]"#,
            r#"["a", "b"]"#,
            r#"["a", "b"]"#,
            r#"["a", "b"]"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A call infers a variadic tuple type parameter from the arguments it spreads over.
#[test]
#[ignore = "typed gap in the lane: a variadic tuple parameter is an UnrepresentableCallee"]
fn pinned_variadic_tuple_inference_in_calls() {
    let failures = Matrix::new(VARIADIC_TUPLES).four(&[
        (
            Read::Return(r#"v1"#),
            r#"[string, boolean]"#,
            r#"[string, boolean]"#,
            r#"[string, boolean]"#,
            r#"[string, boolean]"#,
        ),
        (
            Read::Return(r#"v2"#),
            r#"[number, string]"#,
            r#"[number, string]"#,
            r#"[number, string]"#,
            r#"[number, string]"#,
        ),
        (
            Read::Return(r#"v3"#),
            r#"[number, number, string]"#,
            r#"[number, number, string]"#,
            r#"[number, number, string]"#,
            r#"[number, number, string]"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A tail-recursive conditional type evaluates 998 and 999 levels deep under the checker's tail-recursion limit of 1000.
#[test]
#[ignore = "budget (the budgets agent): the connected work limit trips before the checker's tail-recursion depth (PROJECTION_WORK_LIMIT)"]
fn pinned_tail_recursive_conditional_near_the_depth_limit() {
    let failures = Matrix::new(RECURSIVE_CONDITIONALS).four(&[
        (
            Read::Type(r#"Len<Rep<998>>"#),
            r#"998"#,
            r#"998"#,
            r#"998"#,
            r#"998"#,
        ),
        (
            Read::Type(r#"Len<Rep<999>>"#),
            r#"999"#,
            r#"999"#,
            r#"999"#,
            r#"999"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A recursive alias re-instantiated with other arguments evaluates to its base case.
#[test]
#[ignore = "recursion (the budgets agent's R2): an instantiation is cut at the first re-entry of its declaration whatever its arguments, publishing Opaque(RecursiveRef)"]
fn pinned_recursive_alias_instantiation() {
    let failures = Matrix::new(RECURSIVE_CONDITIONALS).four(&[
        (
            Read::Type(r#"TrimLeft<"   x">"#),
            r#""x""#,
            r#""x""#,
            r#""x""#,
            r#""x""#,
        ),
        (
            Read::Type(r#"TrimLeft<"                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    x">"#),
            r#""x""#,
            r#""x""#,
            r#""x""#,
            r#""x""#,
        ),
        (
            Read::Type(r#"Count<"abcd">"#),
            r#"4"#,
            r#"4"#,
            r#"4"#,
            r#"4"#,
        ),
        (
            Read::Type(r#"Rev<[1, 2, 3]>"#),
            r#"[3, 2, 1]"#,
            r#"[3, 2, 1]"#,
            r#"[3, 2, 1]"#,
            r#"[3, 2, 1]"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `infer X extends C` infers `X` and relates it to `C` (a template literal hole converts a numeric or boolean string).
#[test]
#[ignore = "typed gap in the lane (UNDECIDED_CONDITIONAL): an `infer` declaration with an extends constraint is an out-of-scope pattern"]
fn pinned_infer_with_an_extends_constraint() {
    let failures = Matrix::new(INFER_WITH_CONSTRAINTS).four(&[
        (
            Read::Type(r#"FirstStr<["x", 1]>"#),
            r#""x""#,
            r#""x""#,
            r#""x""#,
            r#""x""#,
        ),
        (
            Read::Type(r#"FirstStr<[1, "x"]>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"NumPart<"42">"#),
            r#"42"#,
            r#"42"#,
            r#"42"#,
            r#"42"#,
        ),
        (
            Read::Type(r#"NumPart<"x">"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"BoolPart<"true">"#),
            r#"true"#,
            r#"true"#,
            r#"true"#,
            r#"true"#,
        ),
        (
            Read::Type(r#"Get<{ a: "q" }>"#),
            r#""q""#,
            r#""q""#,
            r#""q""#,
            r#""q""#,
        ),
        (
            Read::Type(r#"Get<{ a: 1 }>"#),
            r#""no""#,
            r#""no""#,
            r#""no""#,
            r#""no""#,
        ),
        (
            Read::Type(r#"RetStr<() => "s">"#),
            r#""s""#,
            r#""s""#,
            r#""s""#,
            r#""s""#,
        ),
        (
            Read::Type(r#"RetStr<() => 1>"#),
            r#""no""#,
            r#""no""#,
            r#""no""#,
            r#""no""#,
        ),
        (
            Read::Type(r#"NumPart<"1.5">"#),
            r#"1.5"#,
            r#"1.5"#,
            r#"1.5"#,
            r#"1.5"#,
        ),
        (
            Read::Type(r#"NumPart<"-1">"#),
            r#"-1"#,
            r#"-1"#,
            r#"-1"#,
            r#"-1"#,
        ),
        (
            Read::Type(r#"NumPart<"0x10">"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A key-remapped mapped type relates both ways to the object type it produces.
#[test]
#[ignore = "typed gap in the lane (UNDECIDED_CONDITIONAL): the relation over a remapped mapped type is undecided"]
fn pinned_key_remapped_mapped_type_relations() {
    let failures = Matrix::new(KEY_REMAPPING).four(&[
        (
            Read::Type(r#"[Getters<{ name: string }>] extends [{ getName: () => string }] ? ([{ getName: () => string }] extends [Getters<{ name: string }>] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[Filter<{ a: 1; b: "x"; c: 2 }, number>] extends [{ a: 1; c: 2 }] ? ([{ a: 1; c: 2 }] extends [Filter<{ a: 1; b: "x"; c: 2 }, number>] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[Prefix<{ a: 1 }>] extends [{ p_a: 1 }] ? ([{ p_a: 1 }] extends [Prefix<{ a: 1 }>] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[Drop<{ a: 1; b: 2 }, "a">] extends [{ b: 2 }] ? ([{ b: 2 }] extends [Drop<{ a: 1; b: 2 }, "a">] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The library's `Promise` relates to `PromiseLike`, its `then` infers a callback's return, `Promise.resolve` unwraps a nested promise and `Promise.all` awaits each element.
#[test]
#[ignore = "typed gap / print gap in the lane: the library's `Promise` is a builtin carrier the relation cannot expand (UNDECIDED_CONDITIONAL, UnrepresentableCallee), and a named application keeps an `Awaited<…>` argument unreduced in print"]
fn pinned_library_promise_relations_and_calls() {
    let failures = Matrix::new(AWAITED_AND_PROMISES).lib(GLOBALS_LIB).four(&[
        (
            Read::Type(r#"[Promise<1>] extends [PromiseLike<number>] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[Promise<number>] extends [Promise<1>] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Return(r#"a7"#),
            r#"Promise<number[]>"#,
            r#"Promise<number[]>"#,
            r#"Promise<number[]>"#,
            r#"Promise<number[]>"#,
        ),
        (
            Read::Return(r#"a8"#),
            r#"Promise<string>"#,
            r#"Promise<string>"#,
            r#"Promise<string>"#,
            r#"Promise<string>"#,
        ),
        (
            Read::Return(r#"a9"#),
            r#"Promise<[number, 1]>"#,
            r#"Promise<[number, 1]>"#,
            r#"Promise<[number, 1]>"#,
            r#"Promise<[number, 1]>"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An `infer` nested in a generic application or constrained by `extends` infers through the pattern; without `strictNullChecks` `null` relates to the pattern and its `infer` falls back to `unknown`.
#[test]
#[ignore = "typed gap in the lane (UNDECIDED_CONDITIONAL): an `infer` nested below a generic application, or constrained, is an out-of-scope pattern"]
fn pinned_hype_nested_infer_patterns() {
    let failures = Matrix::new(HYPE_REPROS).four(&[
        (
            Read::Type(r#"F<null>"#),
            r#""ok""#,
            r#"unknown"#,
            r#""ok""#,
            r#"unknown"#,
        ),
        (
            Read::Type(r#"F<undefined>"#),
            r#""ok""#,
            r#"unknown"#,
            r#""ok""#,
            r#"unknown"#,
        ),
        (
            Read::Type(r#"Box<"a"> extends Box<infer P> ? P : 0"#),
            r#""a""#,
            r#""a""#,
            r#""a""#,
            r#""a""#,
        ),
        (
            Read::Type(r#"InferExt<["a"]>"#),
            r#""a""#,
            r#""a""#,
            r#""a""#,
            r#""a""#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A recursive alias over a template pattern reaches its base case.
#[test]
#[ignore = "recursion (the budgets agent's R2): an instantiation is cut at the first re-entry of its declaration, publishing Opaque(RecursiveRef)"]
fn pinned_hype_recursive_alias() {
    let failures = Matrix::new(HYPE_REPROS).four(&[(
        Read::Type(r#"T8<"aa">"#),
        r#""done""#,
        r#""done""#,
        r#""done""#,
        r#""done""#,
    )]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A recursive template literal length counter counts cooked characters.
#[test]
#[ignore = "recursion (the budgets agent's R2): an instantiation is cut at the first re-entry of its declaration, publishing Opaque(RecursiveRef)"]
fn pinned_recursive_template_length() {
    let failures = Matrix::new(TEMPLATE_ESCAPES).four(&[
        (Read::Type(r#"L<"p\nq">"#), r#"3"#, r#"3"#, r#"3"#, r#"3"#),
        (Read::Type(r#"L<"p\\nq">"#), r#"4"#, r#"4"#, r#"4"#, r#"4"#),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
