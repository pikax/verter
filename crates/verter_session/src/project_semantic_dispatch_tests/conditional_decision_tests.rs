//! How a conditional type is decided: deferred where the checker defers
//! it, decided by the permissive and restrictive tests where its operands
//! hold a type parameter the checker does not defer on, and related as a
//! deferred type where it stays one.
//!
//! Every expected answer is TypeScript 7.0.2's, measured with `tsc
//! --ignoreConfig --noEmit --strict --noErrorTruncation` under all four
//! `strictNullChecks` x `noImplicitAny` settings (alike in every setting
//! here): the type a generic function's body holds is read off TS2322 for
//! `const s: never = null! as <probe>` inside that function, and a call's
//! answer off `const s: never = f()`, with no diagnostic in the calling
//! function. Rows over installed packages are measured the same way with
//! `--moduleResolution bundler --module esnext --target es2020
//! --skipLibCheck`, in a directory whose `node_modules` is the one the
//! matrix links.

use super::differential_harness_tests::{Matrix, Read, Setting, ALL};

/// Generic functions holding each conditional; overloads choosing between a
/// parameter a deferred conditional argument may relate to and `unknown`;
/// and method overloads choosing between a deferred conditional parameter
/// and `unknown`.
const SOURCE: &str = r##"
declare function pickNum(n: number): "hit"; declare function pickNum(n: unknown): "other";
declare function pick12(n: 1 | 2): "hit"; declare function pick12(n: unknown): "other";
declare function pickStr(s: string): "hit"; declare function pickStr(s: unknown): "other";
declare function pick1(n: 1): "hit"; declare function pick1(n: unknown): "other";
export function a1<T>() { return null! as (any extends T ? 1 : 2); }
export function a2<T>() { return null! as (T extends any ? 1 : 2); }
export function a3<T>() { return null! as (any extends [T] ? 1 : 2); }
export function b1<T extends string>() { return null! as ({ v: T } extends { v: string } ? 1 : 2); }
export function b2<T extends string>() { return null! as ([T] extends [string] ? 1 : 2); }
export function b3<T>() { return null! as ({ v: T } extends { v: unknown } ? 1 : 2); }
export function b4<T>() { return null! as ({ v: T } extends { v: number } ? 1 : 2); }
export function b5<T extends string>() { return null! as ({ v: T } extends { v: number } ? 1 : 2); }
export function b6<T extends string>() { return null! as ({ v: T } extends { v: T } ? 1 : 2); }
export function b7<T>() { return null! as ({ v: T } extends {} ? 1 : 2); }
export function b9<T>() { return null! as ({ v: T[] } extends { v: string[] } ? 1 : 2); }
export function b10<T>() { return null! as ({ v: [T] } extends { v: [] } ? 1 : 2); }
export function b12<T>() { return null! as ({ v: T } extends { v: never } ? 1 : 2); }
export function i1<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: number } ? 1 : 2); }
export function i2<T extends { a: string }>() { return null! as ({ a: T["a"] } extends { a: string } ? 1 : 2); }
export function i3<T extends { a: 1 }>() { return null! as (T["a"] extends number ? 1 : 2); }
export function i4<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: unknown } ? 1 : 2); }
export function i5<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: T["a"] } ? 1 : 2); }
export function i6<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: never } ? 1 : 2); }
export function i7<T extends { a: 1 }>() { return null! as ({ a: [T["a"]] } extends { a: [] } ? 1 : 2); }
export function i8<T>() { return null! as ({ a: T[keyof T] } extends { a: string } ? 1 : 2); }
export function k1<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: {} } ? 1 : 2); }
export function k2<T extends { a: { b: 1 } }>() { return null! as ({ a: T["a"]["b"] } extends { a: {} } ? 1 : 2); }
export function k3<T extends { a: 1 | 2 }>() { return null! as ({ a: T["a"] } extends { a: {} } ? 1 : 2); }
export function n1<T>() { return null! as ({ v: T; f: <U>(x: U) => T } extends { v: string; f: unknown } ? 1 : 2); }
export function n2<T>() { return null! as ({ v: 1; f: <U>(x: U) => U } extends { v: number; f: unknown } ? 1 : 2); }
export function n3<T>() { return null! as ({ v: T; f: <T>(x: T) => T } extends { v: string; f: unknown } ? 1 : 2); }
export function n4<T>() { return null! as ({ f: <T>(x: T) => T } extends { f: (x: string) => string } ? 1 : 2); }
export function n5<T>() { return null! as ({ f: <U>(x: U) => T } extends { f: (x: string) => string } ? 1 : 2); }
export function n6<T>() { return null! as ({ f: <T>(x: T) => T; v: T } extends { f: (x: string) => string; v: unknown } ? 1 : 2); }
export function n7<T>() { return null! as ({ f: <U>(x: U) => U } extends { f: (x: string) => number } ? 1 : 2); }
export function n8<T>() { return null! as ({ v: T; f: <U>(x: U) => U } extends { v: unknown; f: (x: string) => number } ? 1 : 2); }
export function c1<T>(x: T extends string ? 1 : 2) { return pickNum(x); }
export function c2<T>(x: T extends string ? 1 : 2) { return pick12(x); }
export function c3<T>(x: T extends string ? 1 : 2) { return pickStr(x); }
export function c4<T>(x: T extends string ? 1 : 2) { return pick1(x); }
export function c5<T>(x: T extends string ? "a" : "b") { return pickStr(x); }
export function c6<T extends string>(x: T extends string ? 1 : 2) { return pick1(x); }
export function c7<T extends number>(x: T extends string ? 1 : 2) { return pick1(x); }
export function c8<T extends string>(x: [T] extends [string] ? 1 : 2) { return pick1(x); }
export function c9<T extends number>(x: T extends string ? 1 : never) { return pickStr(x); }
export function e1<T>(n: 1, o: { pick(x: T extends string ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e2<T>(n: 1, o: { pick(x: T extends string ? number : number): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e3<T extends string>(n: 1, o: { pick(x: T extends string ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e4<T>(x: T extends string ? 1 : 2, o: { pick(x: T extends string ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(x); }
export function e5<T>(x: T extends string ? 1 : 2, o: { pick(x: T extends string ? number : number): "hit"; pick(x: unknown): "other" }) { return o.pick(x); }
export function e6<T>(n: 1, o: { pick(x: T extends string ? 1 : T): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e7<T>(n: never, o: { pick(x: T extends string ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e8<T>(n: 1, o: { pick(x: { v: T } extends { v: number } ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e9<T>(n: 1, o: { pick(x: { v: T } extends { v: unknown } ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e10<T>(n: 1, o: { pick(x: T extends unknown ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e11<T>(n: 2, o: { pick(x: [T] extends [T] ? 2 : 1): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e12<T extends string>(n: 2, o: { pick(x: { v: T } extends { v: string } ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e13<T>(n: 1, o: { pick(x: T extends infer U ? 1 : 1): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e14<T>(n: 2, o: { pick(x: T extends unknown ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e15<T>(n: 1, o: { pick(x: T extends T ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e16<T extends string>(n: 1, o: { pick(x: [T] extends [string] ? 1 : 1): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e17<T>(n: 1, o: { pick(x: { v: T } extends { v: number } ? number : number): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function h1<T>(n: 1) { const o = null! as { pick(x: T extends unknown ? 1 : 2): "hit"; pick(x: unknown): "other" }; return o.pick(n); }
export function h2<T>(n: 1) { const o = null! as { pick(x: T extends string ? number : number): "hit"; pick(x: unknown): "other" }; return o.pick(n); }
export function h3<T>(n: 1) { const o = null! as { pick(x: [T] extends [string] ? 1 : 1): "hit"; pick(x: unknown): "other" }; return o.pick(n); }
"##;

/// Each generic function's measured answer.
const RETURNS: &[(&str, &str)] = &[
    ("a1", "any extends T ? 1 : 2"),
    ("a2", "T extends any ? 1 : 2"),
    ("a3", "1 | 2"),
    ("b1", "{ v: T; } extends { v: string; } ? 1 : 2"),
    ("b2", "[T] extends [string] ? 1 : 2"),
    ("b3", "1"),
    ("b4", "{ v: T; } extends { v: number; } ? 1 : 2"),
    ("b5", "{ v: T; } extends { v: number; } ? 1 : 2"),
    ("b6", "1"),
    ("b7", "1"),
    ("b9", "{ v: T[]; } extends { v: string[]; } ? 1 : 2"),
    ("b10", "2"),
    ("b12", "{ v: T; } extends { v: never; } ? 1 : 2"),
    ("i1", "{ a: T[\"a\"]; } extends { a: number; } ? 1 : 2"),
    ("i2", "{ a: T[\"a\"]; } extends { a: string; } ? 1 : 2"),
    ("i3", "T[\"a\"] extends number ? 1 : 2"),
    ("i4", "1"),
    ("i5", "1"),
    ("i6", "{ a: T[\"a\"]; } extends { a: never; } ? 1 : 2"),
    ("i7", "2"),
    ("i8", "{ a: T[keyof T]; } extends { a: string; } ? 1 : 2"),
    (
        "n1",
        "{ v: T; f: <U>(x: U) => T; } extends { v: string; f: unknown; } ? 1 : 2",
    ),
    ("n2", "1"),
    (
        "n3",
        "{ v: T; f: <T>(x: T) => T; } extends { v: string; f: unknown; } ? 1 : 2",
    ),
    ("n4", "1"),
    (
        "n5",
        "{ f: <U>(x: U) => T; } extends { f: (x: string) => string; } ? 1 : 2",
    ),
    ("n6", "1"),
    ("n7", "2"),
    ("n8", "2"),
];

/// `any extends T ? 1 : 2` stays deferred (the `any` check is read only
/// once neither operand is generic, and `[T]` is not generic, so `any
/// extends [T]` is `1 | 2`); `{ v: T } extends { v: string }` with `T
/// extends string` stays deferred (the restrictive instantiation drops the
/// constraint, so the true branch is not proven, and the permissive one
/// holds, so the false branch is not either), while `{ v: T } extends { v:
/// unknown }` and `{ v: T } extends { v: T }` hold restrictively (the
/// wildcard of the permissive test relating to itself). The wildcard relates
/// to `never` as well, where `any` does not, so `{ v: T } extends { v: never
/// }` stays deferred; `{ v: [T] } extends { v: [] }` fails even
/// permissively. A type parameter a generic signature declares is bound
/// only inside that signature: the function's `T` written in a signature
/// that declares `U` (or that declares a `T` of its own beside it) stays
/// free there and defers the conditional, while a signature's own
/// parameters never do: `{ f: <U>(x: U) => U }` against `{ f: (x: string)
/// => number }` is plainly false.
#[test]
fn conditionals_are_deferred_and_decided_as_the_checker_decides_them() {
    let failures = Matrix::new(SOURCE).settings(&ALL).returns(RETURNS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Each call's measured answer: `"hit"` when the argument relates to the
/// first overload's parameter.
const CALLS: &[(&str, &str)] = &[
    ("c1", "\"hit\""),
    ("c2", "\"hit\""),
    ("c3", "\"other\""),
    ("c4", "\"other\""),
    ("c5", "\"hit\""),
    ("c6", "\"hit\""),
    ("c7", "\"other\""),
    ("c8", "\"other\""),
    ("c9", "\"other\""),
    ("e1", "\"other\""),
    ("e3", "\"other\""),
    ("e4", "\"hit\""),
    ("e5", "\"hit\""),
    ("e6", "\"other\""),
    ("e7", "\"hit\""),
    ("e8", "\"other\""),
    ("e9", "\"hit\""),
    ("e11", "\"hit\""),
    ("e12", "\"other\""),
    ("e13", "\"other\""),
    ("e14", "\"other\""),
    ("e15", "\"other\""),
    ("e16", "\"hit\""),
    ("e17", "\"hit\""),
    ("h3", "\"hit\""),
];

/// A deferred conditional source relates through its branches (`T extends
/// string ? 1 : 2` to `number` and to `1 | 2`, not to `string` or `1`),
/// through the conditional at its check type's constraint (`1` under `T
/// extends string`, `2` under `T extends number`), never through a
/// constraint that is `never`, and branch to branch against a conditional
/// of the same `extends` type. A deferred conditional target takes a source
/// relating to each branch it may still take (`[T] extends [string] ? 1 :
/// 1`), the false branch skipped where the restrictive instantiation holds;
/// never when it declares an `infer` or its branches or `extends` type read
/// its distributed check type (`T extends string ? 1 : T`, `T extends T ? 1
/// : 2`).
#[test]
fn deferred_conditionals_relate_as_the_checker_relates_them() {
    let failures = Matrix::new(SOURCE).settings(&ALL).returns(CALLS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An indexed access of a type parameter is generic, and relates as its
/// constraint: the same access of the parameter's constraint, and `unknown`
/// for the unconstrained parameter the restrictive test reads. Without
/// `strictNullChecks` `unknown` relates to `{}`, so the restrictive test
/// holds and the access's constraint (`1` for `T["a"]` under `T extends {
/// a: 1 }`, each member of a union constraint, and through an access of an
/// access too) decides the true branch; with it the conditional stays
/// deferred.
#[test]
fn indexed_accesses_of_a_type_parameter_relate_through_its_constraint() {
    let failures = Matrix::new(SOURCE).settings(&ALL).nullness(&[
        (
            Read::Return("k1"),
            "{ a: T[\"a\"]; } extends { a: {}; } ? 1 : 2",
            "1",
        ),
        (
            Read::Return("k2"),
            "{ a: T[\"a\"][\"b\"]; } extends { a: {}; } ? 1 : 2",
            "1",
        ),
        (
            Read::Return("k3"),
            "{ a: T[\"a\"]; } extends { a: {}; } ? 1 : 2",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Whether a distributive conditional target reads its check type is read
/// by the checker off where the conditional is written (a block between the
/// type parameter's declaration and the conditional counts as a reading),
/// which the conditional's type does not carry. A source that relates to
/// each branch of such a target (`1` to `T extends unknown ? 1 : 2`, which
/// the checker takes in a parameter list, `"hit"`, and refuses in a
/// function body, `"other"`) is therefore undecided, never answered; a
/// target that is not distributive is related in a body too (`h3` above).
#[test]
fn a_distributive_target_whose_reading_is_unknown_is_not_answered() {
    let rows = [
        ("e2", "\"hit\""),
        ("e10", "\"hit\""),
        ("h1", "\"other\""),
        ("h2", "\"other\""),
    ];
    let reads: Vec<(Read<'_>, Vec<&str>)> = rows
        .iter()
        .map(|(function, answer)| (Read::Return(function), vec![*answer; ALL.len()]))
        .collect();
    let verdicts = Matrix::new(SOURCE).settings(&ALL).verdicts(&reads);
    for ((read, _), row) in reads.iter().zip(&verdicts) {
        for verdict in row {
            assert!(
                !verdict.matched && verdict.class == "GAP",
                "`{}` is undecided, not {} ({})",
                read.text(),
                verdict.class,
                verdict.lane
            );
        }
    }
}

/// Instantiated conditionals of each lattice row and distribution shape,
/// constrained generic operands, and `keyof T` arguments to overloads.
const CLOSED_SOURCE: &str = r##"
declare function pickStr(s: string): "hit"; declare function pickStr(s: unknown): "other";
type D<T> = T extends string ? "y" : "n";
type W<T> = [T] extends [string] ? "y" : "n";
type Bx<T> = T extends any ? { v: T } : never;
type N<T> = [T] extends [never] ? "never" : "other";
type IsAny<T> = 0 extends 1 & T ? true : false;
type K<T> = keyof T extends never ? "empty" : "keys";
export function d1() { return null! as D<string | number>; }
export function d2() { return null! as W<string | number>; }
export function d3() { return null! as D<never>; }
export function d4() { return null! as W<never>; }
export function d5() { return null! as D<any>; }
export function d6() { return null! as W<any>; }
export function d7() { return null! as Bx<1 | 2>; }
export function d8() { return null! as N<never>; }
export function d9() { return null! as IsAny<any>; }
export function d10() { return null! as IsAny<unknown>; }
export function d11() { return null! as D<boolean>; }
export function d12() { return null! as K<{}>; }
export function d13() { return null! as K<{ a: 1 }>; }
export function d14() { return null! as D<unknown>; }
export function d18<T extends string>() { return null! as (T extends string ? 1 : 2); }
declare const RefSymbol: unique symbol;
declare const RawSymbol: unique symbol;
declare const Marker: unique symbol;
interface Acc<T = any, S = T> { get value(): T; set value(_: S); [RefSymbol]: true; }
type ShallowAcc<T = any, S = T> = Acc<T, S> & { [Marker]?: true };
export function a1() { return null! as (Acc<number> extends Acc<infer V, unknown> ? V : 0); }
export function a2() { return null! as (Acc<number> extends ShallowAcc<infer V, unknown> ? V : 0); }
export function a3() { return null! as (Acc<number> extends Acc<number, unknown> ? 1 : 2); }
export function a4() { return null! as (Acc<number> extends { value: string } ? 1 : 2); }
export function a5() { return null! as Acc<number>["value"]; }
export function w1() { return null! as ({ a: 1 } extends { [RawSymbol]?: true } ? 1 : 2); }
export function w2() { return null! as ({ a: 1 } extends { a?: 1; b?: 2 } ? 1 : 2); }
export function w3() { return null! as ({} extends { [RawSymbol]?: true } ? 1 : 2); }
export function k5<T>(x: T extends string ? "a" : "b") { return pickStr(x); }
export function k6<T>(x: T extends string ? "a" : 1) { return pickStr(x); }
"##;

/// A union check distributes and a wrapped one does not (`D<string |
/// number>` is `"y" | "n"`, `W<string | number>` `"n"`); `never` is the
/// empty union to a distributive check and an ordinary type to a wrapped
/// one; `any` takes both branches of a distributive check and relates to a
/// wrapped one. A check holding a type parameter its constraint would
/// decide (`T extends string`) stays deferred. An interface's `get` / `set`
/// pair is one property of its getter's type, so a reference to it relates
/// and infers through that property whatever its setter takes. A weak
/// target (every property optional) takes no source that has properties
/// and shares none of them. A deferred conditional argument relates to a
/// parameter both its branches relate to, never to one only its true
/// branch does.
#[test]
fn instantiated_and_constrained_conditionals_decide_as_the_checker_decides_them() {
    let failures = Matrix::new(CLOSED_SOURCE).settings(&ALL).returns(&[
        ("d1", "\"n\" | \"y\""),
        ("d2", "\"n\""),
        ("d3", "never"),
        ("d4", "\"y\""),
        ("d5", "\"n\" | \"y\""),
        ("d6", "\"y\""),
        ("d7", "{ v: 1; } | { v: 2; }"),
        ("d8", "\"never\""),
        ("d9", "true"),
        ("d10", "false"),
        ("d11", "\"n\""),
        ("d12", "\"empty\""),
        ("d13", "\"keys\""),
        ("d14", "\"n\""),
        ("d18", "T extends string ? 1 : 2"),
        ("a1", "number"),
        ("a2", "number"),
        ("a3", "1"),
        ("a4", "2"),
        ("a5", "number"),
        ("w1", "2"),
        ("w2", "1"),
        ("w3", "1"),
        ("k5", "\"hit\""),
        ("k6", "\"other\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Vue's ref unwrapping read through the real `vue` package, whose
/// `UnwrapRef` and `Ref` reach the probe through the `vue` → runtime-dom →
/// runtime-core → reactivity re-export chain. `Ref` declares its value as a
/// `get value()` / `set value()` accessor pair.
const VUE_SOURCE: &str = r#"
import { ref, proxyRefs, type UnwrapRef, type ShallowUnwrapRef, type Ref, type ShallowRef } from 'vue'
declare function r<T>(value: T): UnwrapRef<T>
const count = ref(0)
export function unwrapNumber() { return null! as UnwrapRef<number>; }
export function unwrapString() { return null! as UnwrapRef<string>; }
export function refZero() { return ref(0); }
export function unwrapInferred() { return r(0); }
export function unwrapRefNumber() { return null! as UnwrapRef<Ref<number>>; }
export function refAsShallow() { return null! as (Ref<number> extends ShallowRef<infer V, unknown> ? V : 0); }
export function exposedRefs() { return null! as ShallowUnwrapRef<{ count: typeof count }>; }
export function returnedRefs() { return null! as ShallowUnwrapRef<{ a: Ref<number>; b: string }>; }
export function proxied() { return proxyRefs({ count }); }
"#;

/// Each answer is the checker's, alike in all four settings: a primitive
/// and a ref decide their unwrapping, `ref(0)` is a `Ref<number, number>`,
/// and the refs a component exposes or returns unwrap to their values — a
/// `Ref` relates to `ShallowRef<infer V>` and `Ref<infer V>` through its
/// accessor's read type.
fn vue_ref_unwrapping_in(settings: &[Setting]) -> Vec<String> {
    Matrix::new(VUE_SOURCE)
        .settings(settings)
        .installed_packages()
        .returns(&[
            ("unwrapNumber", "number"),
            ("unwrapString", "string"),
            ("refZero", "Ref<number, number>"),
            ("unwrapInferred", "number"),
            ("unwrapRefNumber", "number"),
            ("refAsShallow", "number"),
            ("exposedRefs", "{ count: number; }"),
            ("returnedRefs", "{ a: number; b: string; }"),
            ("proxied", "{ count: number; }"),
        ])
}

/// [`vue_ref_unwrapping_in`] the two settings with `noImplicitAny`; each
/// project loads the real packages, so the other two are a test of their
/// own.
#[test]
fn vue_ref_unwrapping_decides_through_the_real_re_export_chain() {
    let failures = vue_ref_unwrapping_in(&ALL[..2]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// [`vue_ref_unwrapping_in`] the two settings without `noImplicitAny`.
#[test]
fn vue_ref_unwrapping_decides_without_no_implicit_any() {
    let failures = vue_ref_unwrapping_in(&ALL[2..]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Vue's `RefUnwrapBailTypes`, declared empty by reactivity and augmented
/// by `declare module "@vue/reactivity"` blocks in runtime-core and
/// runtime-dom, read from the dependencies `vue`'s runtime-dom installs.
const BAIL_SOURCE: &str = r#"
import type { RefUnwrapBailTypes, UnwrapRef } from '@vue/reactivity'
import type { VNode, ComponentInternalInstance } from '@vue/runtime-core'
import type {} from '@vue/runtime-dom'
type Bail = RefUnwrapBailTypes[keyof RefUnwrapBailTypes];
type CoreBail = RefUnwrapBailTypes["runtimeCoreBailTypes"];
export function coreKey() { return null! as ("runtimeCoreBailTypes" extends keyof RefUnwrapBailTypes ? 1 : 2); }
export function domKey() { return null! as ("runtimeDOMBailTypes" extends keyof RefUnwrapBailTypes ? 1 : 2); }
export function vnodeCore() { return null! as (VNode extends CoreBail ? 1 : 2); }
export function holderCore() { return null! as ({ $: ComponentInternalInstance } extends CoreBail ? 1 : 2); }
export function plainCore() { return null! as ({ a: 1 } extends CoreBail ? 1 : 2); }
export function vnode() { return null! as (VNode extends Bail ? 1 : 2); }
export function unwrapVNode() { return null! as (UnwrapRef<VNode> extends VNode ? 1 : 2); }
"#;

/// A package's `declare module "<package>"` block augments the module file
/// the specifier resolves to from the augmenting file, so the bail-type
/// interface holds both the runtime-core and the runtime-dom member, the
/// runtime-core member reads as the checker reads it, and a `VNode` bails
/// out of ref unwrapping through the written bail arm it relates to.
#[test]
fn package_augmentations_merge_into_the_module_they_resolve_to() {
    let failures = Matrix::new(BAIL_SOURCE)
        .settings(&ALL)
        .installed_packages_of(&["vue", "@vue/runtime-dom"])
        .returns(&[
            ("coreKey", "1"),
            ("domKey", "1"),
            ("vnodeCore", "1"),
            ("holderCore", "1"),
            ("plainCore", "2"),
            ("unwrapVNode", "1"),
        ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The runtime-dom member is `DomType<Node | Window>`, which reads
/// `typeof globalThis`: with no standard library registered the lane cannot
/// read it, so a conditional whose `extends` is the whole bail union, not a
/// union written with an arm the check relates to, stays unavailable —
/// never a branch the missing member could overturn (the checker answers
/// `1`).
#[test]
fn a_bail_union_with_an_unreadable_member_is_not_answered() {
    let reads: Vec<(Read<'_>, Vec<&str>)> = ["vnode"]
        .into_iter()
        .map(|function| (Read::Return(function), vec!["1"]))
        .collect();
    let verdicts = Matrix::new(BAIL_SOURCE)
        .settings(&ALL)
        .installed_packages_of(&["vue", "@vue/runtime-dom"])
        .verdicts(&reads);
    for ((read, _), row) in reads.iter().zip(&verdicts) {
        for verdict in row {
            assert!(
                !verdict.matched && verdict.class == "GAP",
                "`{}` is unavailable, not {} ({})",
                read.text(),
                verdict.class,
                verdict.lane
            );
        }
    }
}

/// An interface a sibling file augments through `declare module "./base"`,
/// read by an indexed access as a conditional's `extends` operand.
const AUGMENTED_FILES: &[(&str, &str)] = &[
    ("base.ts", "export interface Bag {}\n"),
    (
        "aug.ts",
        "import type {} from './base'\ninterface Local { req: string }\ndeclare module './base' { interface Bag { core: Local | { $: number } } }\n",
    ),
];

const AUGMENTED_SOURCE: &str = r#"
import type { Bag } from './base'
import type {} from './aug'
type Core = Bag["core"];
export function m1() { return null! as ({ a: 1 } extends Bag["core"] ? 1 : 2); }
export function m2() { return null! as ({ $: 1 } extends Bag["core"] ? 1 : 2); }
export function m3() { return null! as ({ a: 1 } extends Bag[keyof Bag] ? 1 : 2); }
export function m4() { return null! as ({ a: 1 } extends Core ? 1 : 2); }
export function m5() { return null! as ({ req: "x" } extends Core ? 1 : 2); }
"#;

/// A relation reads an indexed access operand as the member's TYPE: a union
/// member read through an augmented interface (`Local | { $: number }`),
/// directly or through an alias, relates arm by arm — never as the empty
/// one-level surface its arms share, which every object would satisfy.
#[test]
fn indexed_members_of_an_augmented_interface_relate_as_their_type() {
    let failures = Matrix::new(AUGMENTED_SOURCE)
        .settings(&ALL)
        .files(AUGMENTED_FILES)
        .returns(&[
            ("m1", "2"),
            ("m2", "1"),
            ("m3", "2"),
            ("m4", "2"),
            ("m5", "1"),
        ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
