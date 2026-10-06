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
//! function.

use super::differential_harness_tests::{Matrix, Read, ALL};

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
/// permissively.
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
