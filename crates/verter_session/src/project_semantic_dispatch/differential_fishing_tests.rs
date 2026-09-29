//! Differential probes across relation and inference cases the corpus and
//! the other differential tests leave out: generic function types and
//! inference from generic sources, overload resolution over unions and
//! optional parameters, template-literal index signatures, `unknown` and
//! `never` in conditional distribution, callable intersections, weak types
//! with methods, readonly arrays and tuples, enums against `number` and
//! `string`, symbol keys, the `{} | null | undefined` edges, `satisfies`,
//! contextually typed object literal methods, and `keyof` over unions and
//! intersections.
//!
//! Every expected answer is TypeScript 7.0.2's (`tsc --ignoreConfig --noEmit
//! --strict --noErrorTruncation` under each `strictNullChecks` ×
//! `noImplicitAny` setting), read off TS2322 for `declare const p: <probe>;
//! const s: never = p;` (a type row) or `const s: never = f();` (a return
//! row) and cross-checked against TS2339 for `p.__nope`. The four answers
//! are listed strict, `strictNullChecks` off, `noImplicitAny` off, both off.
//! An ignored test asserts the measured answer for the rows the lane does
//! not answer yet.

use super::differential_harness_tests::{Matrix, Read};

/// The fixture of [`generic_functions`].
const GENERIC_FUNCTIONS: &str = r##"
declare function id<T>(x: T): T;
declare function pair<T, U>(x: T, y: U): [T, U];
declare function apply<A, R>(f: (a: A) => R, a: A): R;
declare function compose<A, B, C>(f: (b: B) => C, g: (a: A) => B): (a: A) => C;
declare function wrap<T>(x: T): { v: T };
declare function first<T>(xs: readonly T[]): T;
declare function mapArr<T, U>(xs: T[], f: (x: T) => U): U[];
declare function callWith<T>(f: <U>(x: U) => U, x: T): T;
declare function len(s: string): number;
declare function show(n: number): string;
type RT<F> = F extends (...a: any) => infer R ? R : never;
export function r1() { return apply(len, "a"); }
export function r2() { return compose(show, len); }
export function r3() { return mapArr([1, 2], x => x > 1); }
export function r4() { return callWith(id, 3); }
export function r5() { return apply(id, 5); }
export function r6() { return first(["a", "b"] as const); }
export function r7() { return pair(1, "x"); }
export function r8() { const f: (x: string) => string = id; return f; }
export function r9() { return compose(wrap, len); }
export function r10() { return mapArr(["a"], wrap); }
declare function apply2<A, R>(a: A, f: (a: A) => R): R;
export function r11() { return apply2(5, id); }
"##;

/// Generic function types relate and infer.
#[test]
fn generic_functions() {
    let failures = Matrix::new(GENERIC_FUNCTIONS).four(&[
        (
            Read::Type(r#"[<T>(x: T) => T] extends [(x: string) => string] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[(x: string) => string] extends [<T>(x: T) => T] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[<T>(x: T) => T] extends [<U>(x: U) => U] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[<T>(x: T) => T[]] extends [(x: number) => number[]] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[<T>(x: T) => T[]] extends [(x: number) => string[]] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(
                r#"[<T extends string>(x: T) => T] extends [(x: number) => number] ? 1 : 2"#,
            ),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(
                r#"[<T>(x: T, y: T) => void] extends [(x: string, y: number) => void] ? 1 : 2"#,
            ),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[<T>(x: T[]) => T] extends [(x: string[]) => string] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[typeof id] extends [(x: 1) => 1] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[typeof wrap] extends [(x: number) => { v: string }] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"RT<typeof pair<1, 2>>"#),
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
        ),
        (
            Read::Return(r#"r1"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Return(r#"r2"#),
            r#"(a: string) => string"#,
            r#"(a: string) => string"#,
            r#"(a: string) => string"#,
            r#"(a: string) => string"#,
        ),
        (
            Read::Return(r#"r3"#),
            r#"boolean[]"#,
            r#"boolean[]"#,
            r#"boolean[]"#,
            r#"boolean[]"#,
        ),
        (
            Read::Return(r#"r4"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Return(r#"r5"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Return(r#"r6"#),
            r#""a" | "b""#,
            r#""a" | "b""#,
            r#""a" | "b""#,
            r#""a" | "b""#,
        ),
        (
            Read::Return(r#"r7"#),
            r#"[number, string]"#,
            r#"[number, string]"#,
            r#"[number, string]"#,
            r#"[number, string]"#,
        ),
        (
            Read::Return(r#"r8"#),
            r#"(x: string) => string"#,
            r#"(x: string) => string"#,
            r#"(x: string) => string"#,
            r#"(x: string) => string"#,
        ),
        (
            Read::Return(r#"r9"#),
            r#"(a: string) => { v: number; }"#,
            r#"(a: string) => { v: number; }"#,
            r#"(a: string) => { v: number; }"#,
            r#"(a: string) => { v: number; }"#,
        ),
        (
            Read::Return(r#"r10"#),
            r#"{ v: string; }[]"#,
            r#"{ v: string; }[]"#,
            r#"{ v: string; }[]"#,
            r#"{ v: string; }[]"#,
        ),
        (
            Read::Return(r#"r11"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`overloads`].
const OVERLOADS: &str = r##"
declare function ov(x: string): "s";
declare function ov(x: number): "n";
declare function ov(x: string | number): "u";
declare function op(x: string, y?: number): "one";
declare function op(x: string, y: number, z: boolean): "three";
declare function opt(a?: string): "a";
declare function opt(a: number): "b";
declare function lit(x: "a"): 1;
declare function lit(x: string): 2;
declare function rest(...xs: number[]): "r";
declare function rest(x: string, ...xs: string[]): "s";
declare const sn: string | number;
declare const s: string;
type RT<F> = F extends (...a: any) => infer R ? R : never;
type PS<F> = F extends (...a: infer P) => any ? P : never;
export function o1() { return ov("x"); }
export function o2() { return ov(1); }
export function o3() { return ov(sn); }
export function o4() { return op("x"); }
export function o5() { return op("x", 1); }
export function o6() { return op("x", 1, true); }
export function o7() { return opt(); }
export function o8() { return opt(1); }
export function o9() { return lit("a"); }
export function o10() { return lit(s); }
export function o11() { return rest(); }
export function o12() { return rest("a", "b"); }
"##;

/// Overload resolution picks the checker's signature for unions and optional parameters.
#[test]
fn overloads() {
    let failures = Matrix::new(OVERLOADS).four(&[
        (
            Read::Return(r#"o1"#),
            r#""s""#,
            r#""s""#,
            r#""s""#,
            r#""s""#,
        ),
        (
            Read::Return(r#"o2"#),
            r#""n""#,
            r#""n""#,
            r#""n""#,
            r#""n""#,
        ),
        (
            Read::Return(r#"o3"#),
            r#""u""#,
            r#""u""#,
            r#""u""#,
            r#""u""#,
        ),
        (
            Read::Return(r#"o4"#),
            r#""one""#,
            r#""one""#,
            r#""one""#,
            r#""one""#,
        ),
        (
            Read::Return(r#"o5"#),
            r#""one""#,
            r#""one""#,
            r#""one""#,
            r#""one""#,
        ),
        (
            Read::Return(r#"o6"#),
            r#""three""#,
            r#""three""#,
            r#""three""#,
            r#""three""#,
        ),
        (
            Read::Return(r#"o7"#),
            r#""a""#,
            r#""a""#,
            r#""a""#,
            r#""a""#,
        ),
        (
            Read::Return(r#"o8"#),
            r#""b""#,
            r#""b""#,
            r#""b""#,
            r#""b""#,
        ),
        (Read::Return(r#"o9"#), r#"1"#, r#"1"#, r#"1"#, r#"1"#),
        (Read::Return(r#"o10"#), r#"2"#, r#"2"#, r#"2"#, r#"2"#),
        (
            Read::Return(r#"o11"#),
            r#""r""#,
            r#""r""#,
            r#""r""#,
            r#""r""#,
        ),
        (
            Read::Return(r#"o12"#),
            r#""s""#,
            r#""s""#,
            r#""s""#,
            r#""s""#,
        ),
        (
            Read::Type(r#"RT<typeof ov>"#),
            r#""u""#,
            r#""u""#,
            r#""u""#,
            r#""u""#,
        ),
        (
            Read::Type(r#"PS<typeof op>"#),
            r#"[x: string, y: number, z: boolean]"#,
            r#"[x: string, y: number, z: boolean]"#,
            r#"[x: string, y: number, z: boolean]"#,
            r#"[x: string, y: number, z: boolean]"#,
        ),
        (
            Read::Type(r#"[typeof ov] extends [(x: string) => "s"] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[typeof ov] extends [(x: boolean) => string] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[typeof opt] extends [() => "a"] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`template_index_signatures`].
const TEMPLATE_INDEX_SIGNATURES: &str = r##"
type TK = { [k: `data-${string}`]: number };
type TK2 = { [k: `on${string}`]: () => void; [k: string]: unknown };
type MK = { [P in `data-${string}`]: number };
"##;

/// Index signatures keyed by template literal types.
#[test]
fn template_index_signatures() {
    let failures = Matrix::new(TEMPLATE_INDEX_SIGNATURES).four(&[
        (
            Read::Type(r#"[{ "data-x": 1 }] extends [TK] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ "data-x": "s" }] extends [TK] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[{ other: "s" }] extends [TK] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"TK["data-a"]"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Type(r#"keyof TK"#),
            r#"`data-${string}`"#,
            r#"`data-${string}`"#,
            r#"`data-${string}`"#,
            r#"`data-${string}`"#,
        ),
        (
            Read::Type(r#"[TK] extends [{ [k: string]: number }] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ [k: string]: number }] extends [TK] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[MK] extends [TK] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"MK["data-q"]"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Type(r#"[MK] extends [{ [k: `data-${string}`]: string }] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"TK2["onclick"]"#),
            r#"() => void"#,
            r#"() => void"#,
            r#"() => void"#,
            r#"() => void"#,
        ),
        (
            Read::Type(r#"TK2["foo"]"#),
            r#"unknown"#,
            r#"unknown"#,
            r#"unknown"#,
            r#"unknown"#,
        ),
        (
            Read::Type(r#"[{ onx: () => void; y: 1 }] extends [TK2] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ onx: 1 }] extends [TK2] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`unknown_never_distribution`].
const UNKNOWN_NEVER_DISTRIBUTION: &str = r##"
type D<T> = T extends string ? "s" : "o";
type ND<T> = [T] extends [string] ? "s" : "o";
type IsNever<T> = [T] extends [never] ? true : false;
type U<T> = T extends unknown ? [T] : never;
type D2<T> = T extends {} ? 1 : 2;
"##;

/// `unknown` and `never` in distributive and non-distributive conditional types.
#[test]
fn unknown_never_distribution() {
    let failures = Matrix::new(UNKNOWN_NEVER_DISTRIBUTION).four(&[
        (
            Read::Type(r#"D<never>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"ND<never>"#),
            r#""s""#,
            r#""s""#,
            r#""s""#,
            r#""s""#,
        ),
        (
            Read::Type(r#"D<unknown>"#),
            r#""o""#,
            r#""o""#,
            r#""o""#,
            r#""o""#,
        ),
        (
            Read::Type(r#"D<any>"#),
            r#""o" | "s""#,
            r#""o" | "s""#,
            r#""o" | "s""#,
            r#""o" | "s""#,
        ),
        (
            Read::Type(r#"IsNever<never>"#),
            r#"true"#,
            r#"true"#,
            r#"true"#,
            r#"true"#,
        ),
        (
            Read::Type(r#"IsNever<unknown>"#),
            r#"false"#,
            r#"false"#,
            r#"false"#,
            r#"false"#,
        ),
        (
            Read::Type(r#"U<string | number>"#),
            r#"[string] | [number]"#,
            r#"[string] | [number]"#,
            r#"[string] | [number]"#,
            r#"[string] | [number]"#,
        ),
        (
            Read::Type(r#"U<unknown>"#),
            r#"[unknown]"#,
            r#"[unknown]"#,
            r#"[unknown]"#,
            r#"[unknown]"#,
        ),
        (
            Read::Type(r#"U<never>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"D<string | unknown>"#),
            r#""o""#,
            r#""o""#,
            r#""o""#,
            r#""o""#,
        ),
        (
            Read::Type(r#"unknown extends string ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"never extends string ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (Read::Type(r#"D2<unknown>"#), r#"2"#, r#"1"#, r#"2"#, r#"1"#),
        (Read::Type(r#"D2<null>"#), r#"2"#, r#"1"#, r#"2"#, r#"1"#),
        (
            Read::Type(r#"D2<string | undefined>"#),
            r#"1 | 2"#,
            r#"1"#,
            r#"1 | 2"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"D<string | null>"#),
            r#""o" | "s""#,
            r#""s""#,
            r#""o" | "s""#,
            r#""s""#,
        ),
        (
            Read::Type(r#"D<string | any>"#),
            r#""o" | "s""#,
            r#""o" | "s""#,
            r#""o" | "s""#,
            r#""o" | "s""#,
        ),
        (
            Read::Type(r#"U<1 | "a">"#),
            r#"["a"] | [1]"#,
            r#"["a"] | [1]"#,
            r#"["a"] | [1]"#,
            r#"["a"] | [1]"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`callable_intersections`].
const CALLABLE_INTERSECTIONS: &str = r##"
type F1 = (x: string) => "s";
type F2 = (x: number) => "n";
type FI = F1 & F2;
type RT<F> = F extends (...a: any) => infer R ? R : never;
type PS<F> = F extends (...a: infer P) => any ? P : never;
declare const fi: FI;
declare const tagged: F1 & { tag: 1 };
export function i1() { return fi("a"); }
export function i2() { return fi(1); }
export function i3() { return tagged.tag; }
export function i4() { return tagged("q"); }
"##;

/// Intersections of callable types.
#[test]
fn callable_intersections() {
    let failures = Matrix::new(CALLABLE_INTERSECTIONS).four(&[
        (
            Read::Return(r#"i1"#),
            r#""s""#,
            r#""s""#,
            r#""s""#,
            r#""s""#,
        ),
        (
            Read::Return(r#"i2"#),
            r#""n""#,
            r#""n""#,
            r#""n""#,
            r#""n""#,
        ),
        (Read::Return(r#"i3"#), r#"1"#, r#"1"#, r#"1"#, r#"1"#),
        (
            Read::Return(r#"i4"#),
            r#""s""#,
            r#""s""#,
            r#""s""#,
            r#""s""#,
        ),
        (
            Read::Type(r#"RT<FI>"#),
            r#""n""#,
            r#""n""#,
            r#""n""#,
            r#""n""#,
        ),
        (
            Read::Type(r#"PS<FI>"#),
            r#"[x: number]"#,
            r#"[x: number]"#,
            r#"[x: number]"#,
            r#"[x: number]"#,
        ),
        (
            Read::Type(r#"[FI] extends [F1] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[FI] extends [F2] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[F1] extends [FI] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[FI] extends [(x: boolean) => string] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[FI] extends [(x: string | number) => string] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[F1 & { tag: 1 }] extends [{ tag: 1 }] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[(() => 1) & (() => 2)] extends [() => 1] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`weak_types`].
const WEAK_TYPES: &str = r##"
interface W { a?: number; m?(): void }
interface WM { m?(): void }
interface Req { m(): void }
"##;

/// Weak-type detection over targets with optional methods.
#[test]
fn weak_types() {
    let failures = Matrix::new(WEAK_TYPES).four(&[
        (
            Read::Type(r#"[{ b: 1 }] extends [W] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[{ a: 1 }] extends [W] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ m(): void }] extends [WM] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ b: 1 }] extends [WM] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[{}] extends [W] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ b: 1; m: () => void }] extends [WM] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ b: 1 }] extends [Req] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[() => void] extends [WM] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[{ m?(): void; n: 1 }] extends [W] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ m: 1 }] extends [WM] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[{ b: 1 } & { a: 2 }] extends [W] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`readonly_arrays`].
const READONLY_ARRAYS: &str = r##"
type Head<T> = T extends readonly [infer H, ...unknown[]] ? H : never;
type RO<T> = { readonly [K in keyof T]: T[K] };
type Mut<T> = { -readonly [K in keyof T]: T[K] };
export function ro1() { return [1, 2] as const; }
export function ro2() { const t = ["a", 1] as const; return t[0]; }
"##;

/// Readonly arrays and tuples.
#[test]
fn readonly_arrays() {
    let failures = Matrix::new(READONLY_ARRAYS).four(&[
        (
            Read::Type(r#"[readonly number[]] extends [number[]] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[number[]] extends [readonly number[]] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[readonly [1, 2]] extends [readonly number[]] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[readonly [1, 2]] extends [[1, 2]] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[[1, 2]] extends [readonly [number, number]] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"(readonly [1, "a"])[number]"#),
            r#""a" | 1"#,
            r#""a" | 1"#,
            r#""a" | 1"#,
            r#""a" | 1"#,
        ),
        (
            Read::Type(r#"(readonly string[])[0]"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Type(r#"[readonly number[]] extends [readonly unknown[]] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[readonly [x: 1, y?: 2]] extends [readonly [1]] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[readonly [1, ...string[]]] extends [readonly [1]] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"Head<readonly [1, 2]>"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (Read::Type(r#"Head<[3]>"#), r#"3"#, r#"3"#, r#"3"#, r#"3"#),
        (
            Read::Type(r#"RO<[1, 2]>"#),
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
        ),
        (
            Read::Type(r#"Mut<readonly [1, 2]>"#),
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
            r#"[1, 2]"#,
        ),
        (
            Read::Return(r#"ro1"#),
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
            r#"readonly [1, 2]"#,
        ),
        (
            Read::Return(r#"ro2"#),
            r#""a""#,
            r#""a""#,
            r#""a""#,
            r#""a""#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`enum_assignability`].
const ENUM_ASSIGNABILITY: &str = r##"
enum E { A, B }
enum S { X = "x", Y = "y" }
enum M { P = 1, Q = "q" }
const enum C { Z = 3 }
export function en1() { return E.A; }
export function en2() { return S.X; }
"##;

/// Enum types against number and string.
#[test]
fn enum_assignability() {
    let failures = Matrix::new(ENUM_ASSIGNABILITY).four(&[
        (
            Read::Type(r#"[E] extends [number] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[number] extends [E] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[1] extends [E] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[5] extends [E] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[E.A] extends [0] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[S] extends [string] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[string] extends [S] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"["x"] extends [S] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[S.X] extends ["x"] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[M] extends [string | number] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[M.Q] extends [string] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[E.A] extends [E.B] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[C] extends [number] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"keyof typeof E"#),
            r#""A" | "B""#,
            r#""A" | "B""#,
            r#""A" | "B""#,
            r#""A" | "B""#,
        ),
        (
            Read::Type(r#"`${S}`"#),
            r#""x" | "y""#,
            r#""x" | "y""#,
            r#""x" | "y""#,
            r#""x" | "y""#,
        ),
        (
            Read::Type(r#"`${E}`"#),
            r#""0" | "1""#,
            r#""0" | "1""#,
            r#""0" | "1""#,
            r#""0" | "1""#,
        ),
        (Read::Return(r#"en1"#), r#"E"#, r#"E"#, r#"E"#, r#"E"#),
        (Read::Return(r#"en2"#), r#"S"#, r#"S"#, r#"S"#, r#"S"#),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`symbol_keys`].
const SYMBOL_KEYS: &str = r##"
declare const sym: unique symbol;
declare const sym2: unique symbol;
interface HasSym { [sym]: number; x: 1 }
type SymIdx = { [k: symbol]: string };
type Ex<T, U> = T extends U ? T : never;
export function sy1() { const o = { [sym]: 1 }; return o[sym]; }
declare const h: HasSym;
export function sy2() { return h[sym]; }
"##;

/// Unique symbol and symbol-keyed members.
#[test]
fn symbol_keys() {
    let failures = Matrix::new(SYMBOL_KEYS).four(&[
        (
            Read::Type(r#"HasSym[typeof sym]"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Type(r#"[keyof HasSym] extends ["x" | typeof sym] ? (["x" | typeof sym] extends [keyof HasSym] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ [sym]: 1 }] extends [{ [sym]: number }] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ [sym]: 1 }] extends [{ [sym2]: number }] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[HasSym] extends [SymIdx] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[{ [sym]: "a" }] extends [SymIdx] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{ [sym]: 1 }] extends [SymIdx] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"SymIdx[typeof sym]"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Type(r#"[Ex<keyof HasSym, symbol>] extends [typeof sym] ? ([typeof sym] extends [Ex<keyof HasSym, symbol>] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[typeof sym] extends [symbol] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[symbol] extends [typeof sym] ? 1 : 2"#),
            r#"2"#,
            r#"2"#,
            r#"2"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"Ex<{ [K in keyof HasSym]: K }[keyof HasSym], string>"#),
            r#""x""#,
            r#""x""#,
            r#""x""#,
            r#""x""#,
        ),
        (
            Read::Return(r#"sy1"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Return(r#"sy2"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`empty_object_nullish`].
const EMPTY_OBJECT_NULLISH: &str = r##"
type NN<T> = T & {};
"##;

/// `{}`, `null`, `undefined` and `unknown` at the edges.
#[test]
fn empty_object_nullish() {
    let failures = Matrix::new(EMPTY_OBJECT_NULLISH).four(&[
        (
            Read::Type(r#"[unknown] extends [{} | null | undefined] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{} | null | undefined] extends [unknown] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[null] extends [{}] ? 1 : 2"#),
            r#"2"#,
            r#"1"#,
            r#"2"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[undefined] extends [{}] ? 1 : 2"#),
            r#"2"#,
            r#"1"#,
            r#"2"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[string | null] extends [{}] ? 1 : 2"#),
            r#"2"#,
            r#"1"#,
            r#"2"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[{}] extends [object] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[object] extends [{}] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"NN<string | null>"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Type(r#"NN<unknown>"#),
            r#"{}"#,
            r#"{}"#,
            r#"{}"#,
            r#"{}"#,
        ),
        (
            Read::Type(r#"NN<null>"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"[void] extends [{} | null | undefined] ? 1 : 2"#),
            r#"1"#,
            r#"2"#,
            r#"1"#,
            r#"2"#,
        ),
        (
            Read::Type(r#"[{} | null | undefined] extends [{}] ? 1 : 2"#),
            r#"2"#,
            r#"1"#,
            r#"2"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"[unknown] extends [{}] ? 1 : 2"#),
            r#"2"#,
            r#"1"#,
            r#"2"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`satisfies`].
const SATISFIES: &str = r##"
export function sa1() { return { a: 1 } satisfies { a: number }; }
export function sa2() { return "x" satisfies string; }
export function sa3() { const o = { a: "x" } satisfies { a: "x" | "y" }; return o.a; }
export function sa4() { return [1, 2] satisfies [number, number]; }
export function sa5() { const o = { f: (x) => x } satisfies { f: (x: string) => string }; return o.f; }
export function sa6() { return { k: 1 } as const satisfies { k: number }; }
export function sa7() { const t = [1, "a"] satisfies (number | string)[]; return t; }
export function sa8() { const x = { a: 1 } satisfies { [k: string]: number }; return x; }
export function sa9() { const n = 1 satisfies number; return n; }
"##;

/// `satisfies` checks against a type and keeps the expression's own.
#[test]
fn satisfies() {
    let failures = Matrix::new(SATISFIES).four(&[
        (
            Read::Return(r#"sa1"#),
            r#"{ a: number; }"#,
            r#"{ a: number; }"#,
            r#"{ a: number; }"#,
            r#"{ a: number; }"#,
        ),
        (
            Read::Return(r#"sa2"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Return(r#"sa3"#),
            r#""x""#,
            r#""x""#,
            r#""x""#,
            r#""x""#,
        ),
        (
            Read::Return(r#"sa4"#),
            r#"[number, number]"#,
            r#"[number, number]"#,
            r#"[number, number]"#,
            r#"[number, number]"#,
        ),
        (
            Read::Return(r#"sa5"#),
            r#"(x: string) => string"#,
            r#"(x: string) => string"#,
            r#"(x: string) => string"#,
            r#"(x: string) => string"#,
        ),
        (
            Read::Return(r#"sa6"#),
            r#"{ readonly k: 1; }"#,
            r#"{ readonly k: 1; }"#,
            r#"{ readonly k: 1; }"#,
            r#"{ readonly k: 1; }"#,
        ),
        (
            Read::Return(r#"sa7"#),
            r#"(string | number)[]"#,
            r#"(string | number)[]"#,
            r#"(string | number)[]"#,
            r#"(string | number)[]"#,
        ),
        (
            Read::Return(r#"sa8"#),
            r#"{ a: number; }"#,
            r#"{ a: number; }"#,
            r#"{ a: number; }"#,
            r#"{ a: number; }"#,
        ),
        (
            Read::Return(r#"sa9"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`contextual_methods`].
const CONTEXTUAL_METHODS: &str = r##"
declare function cap<T>(o: { m(x: string): T }): T;
declare function make<T>(o: { init(): T; use(v: T): void }): T;
declare function cap2<T>(o: { a: T; m(x: T): void }): T;
export function c1() { return cap({ m(x) { return x; } }); }
export function c2() { return make({ init() { return 1; }, use(v) {} }); }
export function c3() { return cap({ m: (x) => [x] }); }
export function c4() { return cap({ m(x) { return x === "a" ? 1 : 2; } }); }
export function c5() { return cap2({ a: "s", m(x) {} }); }
export function c6() { const o = { n: 1, m() { return this.n; } }; return o.m(); }
export function c7() { return cap({ m(x: "q") { return x; } }); }
export function c8() { return cap({ m(x) { return { x }; } }); }
"##;

/// Object literal methods typed by their context.
#[test]
fn contextual_methods() {
    let failures = Matrix::new(CONTEXTUAL_METHODS).four(&[
        (
            Read::Return(r#"c1"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Return(r#"c2"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Return(r#"c3"#),
            r#"string[]"#,
            r#"string[]"#,
            r#"string[]"#,
            r#"string[]"#,
        ),
        (
            Read::Return(r#"c4"#),
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
        ),
        (
            Read::Return(r#"c5"#),
            r#"string"#,
            r#"string"#,
            r#"string"#,
            r#"string"#,
        ),
        (
            Read::Return(r#"c6"#),
            r#"number"#,
            r#"number"#,
            r#"number"#,
            r#"number"#,
        ),
        (
            Read::Return(r#"c7"#),
            r#""q""#,
            r#""q""#,
            r#""q""#,
            r#""q""#,
        ),
        (
            Read::Return(r#"c8"#),
            r#"{ x: string; }"#,
            r#"{ x: string; }"#,
            r#"{ x: string; }"#,
            r#"{ x: string; }"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`keyof_unions_intersections`].
const KEYOF_UNIONS_INTERSECTIONS: &str = r##"
interface A { a: 1; c: 1 }
interface B { b: 2; c: 2 }
type KU<T> = T extends unknown ? keyof T : never;
"##;

/// `keyof` over unions and intersections.
#[test]
fn keyof_unions_intersections() {
    let failures = Matrix::new(KEYOF_UNIONS_INTERSECTIONS).four(&[
        (
            Read::Type(r#"keyof (A | B)"#),
            r#""c""#,
            r#""c""#,
            r#""c""#,
            r#""c""#,
        ),
        (
            Read::Type(r#"keyof (A & B)"#),
            r#"string | number | symbol"#,
            r#"string | number | symbol"#,
            r#"string | number | symbol"#,
            r#"string | number | symbol"#,
        ),
        (
            Read::Type(r#"keyof (A | { [k: string]: 1 })"#),
            r#""a" | "c""#,
            r#""a" | "c""#,
            r#""a" | "c""#,
            r#""a" | "c""#,
        ),
        (
            Read::Type(r#"keyof (A & { [k: string]: 1 })"#),
            r#"string | number"#,
            r#"string | number"#,
            r#"string | number"#,
            r#"string | number"#,
        ),
        (
            Read::Type(r#"(A | B)["c"]"#),
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
            r#"1 | 2"#,
        ),
        (
            Read::Type(r#"(A & B)["c"]"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"keyof ({ a: 1 } | { a: 2; b: 3 })"#),
            r#""a""#,
            r#""a""#,
            r#""a""#,
            r#""a""#,
        ),
        (
            Read::Type(r#"keyof never"#),
            r#"string | number | symbol"#,
            r#"string | number | symbol"#,
            r#"string | number | symbol"#,
            r#"string | number | symbol"#,
        ),
        (
            Read::Type(r#"keyof unknown"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"[keyof (A | never)] extends ["a" | "c"] ? (["a" | "c"] extends [keyof (A | never)] ? 1 : 3) : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
        (
            Read::Type(r#"keyof ({} | A)"#),
            r#"never"#,
            r#"never"#,
            r#"never"#,
            r#"never"#,
        ),
        (
            Read::Type(r#"{ [K in keyof (A & B)]: K }"#),
            r#"{}"#,
            r#"{}"#,
            r#"{}"#,
            r#"{}"#,
        ),
        (
            Read::Type(r#"KU<A | B>"#),
            r#""a" | "b" | "c""#,
            r#""a" | "b" | "c""#,
            r#""a" | "b" | "c""#,
            r#""a" | "b" | "c""#,
        ),
        (
            Read::Type(r#"[A & B] extends [never] ? 1 : 2"#),
            r#"1"#,
            r#"1"#,
            r#"1"#,
            r#"1"#,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A conditional whose check type is a type parameter is deferred until the
/// parameter is known (`getConditionalType`'s `isDeferredType`), even where
/// every instantiation would select the true branch: the checker gives the
/// return of `g` over `T extends unknown ? 1 : 2` as that conditional. The
/// lane prints no deferred conditional, so the row is an honest gap in
/// every setting — never the clean `1` an early selection publishes.
#[test]
fn a_conditional_over_a_generic_check_is_not_selected_early() {
    let matrix = Matrix::new(
        "export function g<T>(x: T) { const v: T extends unknown ? 1 : 2 = null as any; return v; }\n\
         export function h() { return g(3); }\n",
    );
    let rows = [
        (Read::Return("g"), vec!["T extends unknown ? 1 : 2"; 4]),
        (Read::Return("h"), vec!["1"; 4]),
    ];
    let verdicts = matrix.verdicts(&rows);
    for verdict in &verdicts[0] {
        assert!(
            !verdict.matched && verdict.class == "GAP",
            "the deferred conditional is a gap, never a clean answer: {} {}",
            verdict.class,
            verdict.lane
        );
    }
    for verdict in &verdicts[1] {
        assert!(
            verdict.matched,
            "the instantiated call selects `1`: {}",
            verdict.lane
        );
    }
}
