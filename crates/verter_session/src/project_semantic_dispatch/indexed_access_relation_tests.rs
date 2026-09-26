//! Relations through an indexed access. An indexed access over a type that
//! is not generic is the property type it reads — the checker resolves
//! `Box['lit']` to `2` where it is written — so the relation engine decides
//! a pair whose source or target is one by the type it reads, and a
//! conditional selects its branch and a call its overload on that verdict.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture: `declare const v: <probe>; export const s: null = v;`
//! read off the TS2322 message, and `tsc --declaration
//! --emitDeclarationOnly --strict` for an inferred function return.

use super::checker_probe_lane_tests::{mismatches, mismatches_in, ProbeProject};

const FIXTURE: &str = "\
interface Box { lit: 2; n: number; s: string; u: 1 | 2; o: { k: 'v' }; opt?: 3 }
type Obj = { a: 'x'; nested: { b: 5 } };
type Tup = [1, 'two'];
interface XB { b: 'bb' }
interface XD extends XB { d: 'dd' }
type AB = { a: 1 } & { b: 2 };
declare function pick(x: Box['lit']): 'lit';
declare function pick(x: number): 'num';
export function pickTwo() { return pick(2); }
export function pickThree() { return pick(3); }
";

/// The relation engine decides a pair with an indexed-access TARGET by the
/// type the access reads.
///
/// Measured on TypeScript 7.0.2: `2 extends Box['lit']` is `"y"`,
/// `3 extends Box['lit']` and `string extends Box['lit']` are `"n"`,
/// `number extends Box['n']` and `1 extends Box['u']` are `"y"`,
/// `2 extends Box['lit' | 's']` is `"y"`, `5 extends Obj['nested']['b']`
/// is `"y"`, `'two' extends Tup[1]` is `"y"`, `string extends
/// string[][number]` is `"y"` while `number extends string[][number]` is
/// `"n"`, `'bb' extends XD['b']` (a member `XD` inherits) is `"y"` and
/// `2 extends AB['b']` is `"y"`.
#[test]
fn an_indexed_access_target_relates_as_the_type_it_reads() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("2 extends Box['lit'] ? 'y' : 'n'", "\"y\""),
            ("3 extends Box['lit'] ? 'y' : 'n'", "\"n\""),
            ("string extends Box['lit'] ? 'y' : 'n'", "\"n\""),
            ("number extends Box['n'] ? 'y' : 'n'", "\"y\""),
            ("1 extends Box['u'] ? 'y' : 'n'", "\"y\""),
            ("2 extends Box['lit' | 's'] ? 'y' : 'n'", "\"y\""),
            ("5 extends Obj['nested']['b'] ? 'y' : 'n'", "\"y\""),
            ("'two' extends Tup[1] ? 'y' : 'n'", "\"y\""),
            ("string extends string[][number] ? 'y' : 'n'", "\"y\""),
            ("number extends string[][number] ? 'y' : 'n'", "\"n\""),
            ("'bb' extends XD['b'] ? 'y' : 'n'", "\"y\""),
            ("2 extends AB['b'] ? 'y' : 'n'", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The same for an indexed-access SOURCE, for an access on both sides, for
/// an object type read through an access, and for an access nested in a
/// member the relation descends into. A union read is not a naked type
/// parameter, so it does not distribute.
///
/// Measured on TypeScript 7.0.2: `Box['lit'] extends 2` and
/// `Box['lit'] extends number` are `"y"`, `Box['u'] extends 1` is `"n"`,
/// `Box['s'] extends Box['lit']` is `"n"`, `Box['lit'] extends Box['n']` is
/// `"y"`, `{ k: 'v' } extends Box['o']` and `Box['o'] extends { k: string }`
/// are `"y"`, `XD['d'] extends 'dd'` is `"y"`, `AB['a'] extends 2` is `"n"`
/// and `{ x: XD['d'] } extends { x: 'dd' }` is `"y"`.
#[test]
fn an_indexed_access_source_relates_as_the_type_it_reads() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Box['lit'] extends 2 ? 'y' : 'n'", "\"y\""),
            ("Box['lit'] extends number ? 'y' : 'n'", "\"y\""),
            ("Box['u'] extends 1 ? 'y' : 'n'", "\"n\""),
            ("Box['s'] extends Box['lit'] ? 'y' : 'n'", "\"n\""),
            ("Box['lit'] extends Box['n'] ? 'y' : 'n'", "\"y\""),
            ("{ k: 'v' } extends Box['o'] ? 'y' : 'n'", "\"y\""),
            ("Box['o'] extends { k: string } ? 'y' : 'n'", "\"y\""),
            ("XD['d'] extends 'dd' ? 'y' : 'n'", "\"y\""),
            ("AB['a'] extends 2 ? 'y' : 'n'", "\"n\""),
            ("{ x: XD['d'] } extends { x: 'dd' } ? 'y' : 'n'", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Overload resolution relates each argument to its parameter through the
/// same authority, so a parameter typed by an indexed access selects its
/// overload.
///
/// Measured on TypeScript 7.0.2 over `pick(x: Box['lit']): 'lit'` and
/// `pick(x: number): 'num'`: `pick(2)` is `"lit"` and `pick(3)` is
/// `"num"`.
#[test]
fn a_parameter_typed_by_an_indexed_access_selects_its_overload() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof pickTwo>", "\"lit\""),
            ("ReturnType<typeof pickThree>", "\"num\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const OPTIONAL: &str = "\
interface Box { opt?: 3; req: 4; u?: 1 | 2; un?: 3 | undefined }
type Tup = [1, 2?];
type G<T> = T['opt' & keyof T];
type Get<T, K extends keyof T> = T[K];
type K = keyof Box;
";

/// An access that reads an OPTIONAL property or tuple element reads its
/// type plus `undefined` under `strictNullChecks` — through a key union,
/// `keyof`, an alias of `keyof`, an intersection with `keyof`, a generic
/// alias and the homomorphic utilities — and the relations decide on that
/// read. With `strictNullChecks` off the read erases `undefined`;
/// `exactOptionalPropertyTypes` does not change it.
///
/// Measured on TypeScript 7.0.2 over this fixture (`tsc --ignoreConfig
/// --noEmit`, then with `--strictNullChecks false`, then with
/// `--exactOptionalPropertyTypes`), strict / loose: `Box['opt']`,
/// `Box['un']`, `G<Box>`, `Get<Box, 'opt'>`, `Pick<Box, 'opt'>['opt']`,
/// `Readonly<Box>['opt']`, `Box['opt' & keyof Box]` and `{ o?: 3 }['o']`
/// are `3 | undefined` / `3`; `Box['opt' | 'req']` is `3 | 4 |
/// undefined` / `3 | 4`; `Box['u']` is `1 | 2 | undefined` / `1 | 2`;
/// `Partial<Box>['req']` is `4 | undefined` / `4`; `Tup[1]` is `2 |
/// undefined` / `2`; `Tup[number]` is `1 | 2 | undefined` / `1 | 2`;
/// `Box[keyof Box]` and `Box[K]` are `1 | 2 | 3 | 4 | undefined` / `1 |
/// 2 | 3 | 4`; `Required<Box>['opt']` is `3`; `undefined extends
/// Box['opt']` and `3 extends Box['opt']` are `"y"`; `Box['opt'] extends
/// 3` is `"n"` / `"y"`. The exact-optional answers equal the strict ones.
#[test]
fn an_access_reading_an_optional_property_reads_its_undefined() {
    let strict = ProbeProject::default();
    let loose = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
        ambient_lib: None,
    };
    let exact = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "exactOptionalPropertyTypes": true }"#),
        ambient_lib: None,
    };
    for (project, strict_null_checks) in [(strict, true), (loose, false), (exact, true)] {
        let read = |on: &'static str, off: &'static str| if strict_null_checks { on } else { off };
        let failures = mismatches_in(
            project,
            OPTIONAL,
            &[
                ("Box['opt']", read("3 | undefined", "3")),
                ("Box['un']", read("3 | undefined", "3")),
                ("G<Box>", read("3 | undefined", "3")),
                ("Get<Box, 'opt'>", read("3 | undefined", "3")),
                ("Pick<Box, 'opt'>['opt']", read("3 | undefined", "3")),
                ("Readonly<Box>['opt']", read("3 | undefined", "3")),
                ("Box['opt' & keyof Box]", read("3 | undefined", "3")),
                ("{ o?: 3 }['o']", read("3 | undefined", "3")),
                ("Box['opt' | 'req']", read("3 | 4 | undefined", "3 | 4")),
                ("Box['u']", read("1 | 2 | undefined", "1 | 2")),
                ("Partial<Box>['req']", read("4 | undefined", "4")),
                ("Tup[1]", read("2 | undefined", "2")),
                ("Tup[number]", read("1 | 2 | undefined", "1 | 2")),
                (
                    "Box[keyof Box]",
                    read("1 | 2 | 3 | 4 | undefined", "1 | 2 | 3 | 4"),
                ),
                ("Box[K]", read("1 | 2 | 3 | 4 | undefined", "1 | 2 | 3 | 4")),
                ("Required<Box>['opt']", "3"),
                ("undefined extends Box['opt'] ? 'y' : 'n'", "\"y\""),
                ("3 extends Box['opt'] ? 'y' : 'n'", "\"y\""),
                ("Box['opt'] extends 3 ? 'y' : 'n'", read("\"n\"", "\"y\"")),
            ],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

const INTERSECTED: &str = "\
interface QA { qa: 1 }
interface QB { qb: 2 }
interface H { projectOnly?: number; onClick?: (payload: QA) => void; both?: { qa: 1 }; req: QA; gen?: string }
type Div = H & { projectOnly?: string; onClick?: (payload: QB) => void; both?: { qb: 2 }; req?: QB };
type NU = { a: { qa: 1 } | null } & { a: { qb: 2 } | null | undefined };
type GT<T> = { a?: T } & { a?: { qa: 1 } };
";

/// A member read through an intersection is the intersection of its
/// constituents' reads, distributed over their `null` / `undefined` arms:
/// a nullish arm every constituent carries stays, any other one meets a
/// disjoint arm and drops.
///
/// Measured on TypeScript 7.0.2 over this fixture, strict / with
/// `--strictNullChecks false`: `Div['onClick']` is `(((payload: QA) =>
/// void) & ((payload: QB) => void)) | undefined` / `((payload: QA) => void) &
/// ((payload: QB) => void)`; `Div['projectOnly']` is `undefined` / `never`;
/// `Div['both']` is `({ qa: 1; } & { qb: 2; }) | undefined` / `{ qa: 1; } &
/// { qb: 2; }`; `NU['a']` is `({ qa: 1; } & { qb: 2; }) | null` / `{ qa:
/// 1; } & { qb: 2; }`; `GT<string>['a']` is `(string & { qa: 1; }) |
/// undefined` / `string & { qa: 1; }`; `Div['gen']` is `string |
/// undefined` / `string`; `Div['both'] extends { qa: 1; qb: 2 }` is `"n"` /
/// `"y"`.
#[test]
fn an_intersection_member_read_distributes_its_nullish_arms() {
    let strict = ProbeProject::default();
    let loose = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
        ambient_lib: None,
    };
    for (project, strict_null_checks) in [(strict, true), (loose, false)] {
        let read = |on: &'static str, off: &'static str| if strict_null_checks { on } else { off };
        let failures = mismatches_in(
            project,
            INTERSECTED,
            &[
                (
                    "Div['onClick']",
                    read(
                        "(((payload: QA) => void) & ((payload: QB) => void)) | undefined",
                        "((payload: QA) => void) & ((payload: QB) => void)",
                    ),
                ),
                ("Div['projectOnly']", read("undefined", "never")),
                (
                    "Div['both']",
                    read(
                        "({ qa: 1; } & { qb: 2; }) | undefined",
                        "{ qa: 1; } & { qb: 2; }",
                    ),
                ),
                (
                    "NU['a']",
                    read(
                        "({ qa: 1; } & { qb: 2; }) | null",
                        "{ qa: 1; } & { qb: 2; }",
                    ),
                ),
                (
                    "GT<string>['a']",
                    read("(string & { qa: 1; }) | undefined", "string & { qa: 1; }"),
                ),
                ("Div['gen']", read("string | undefined", "string")),
                (
                    "Div['both'] extends { qa: 1; qb: 2 } ? 'y' : 'n'",
                    read("\"n\"", "\"y\""),
                ),
            ],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

/// An intersection source none of whose constituents is assignable to an
/// object target on its own relates as the object its constituents
/// compose.
///
/// Measured on TypeScript 7.0.2 over the fixture above: `QA & QB extends {
/// qa: 1; qb: 2 }` and `Div['req'] extends { qa: 1; qb: 2 }` are `"y"`,
/// `QA & QB extends { qa: 1; qb: 3 }` is `"n"`.
#[test]
fn an_intersection_source_relates_as_the_object_it_composes() {
    let failures = mismatches(
        INTERSECTED,
        &[
            ("QA & QB extends { qa: 1; qb: 2 } ? 'y' : 'n'", "\"y\""),
            ("Div['req'] extends { qa: 1; qb: 2 } ? 'y' : 'n'", "\"y\""),
            ("QA & QB extends { qa: 1; qb: 3 } ? 'y' : 'n'", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const NARROW: &str = "\
type Boxed = { k: 'a' };
type Subject = { v: Boxed['k'] };
function isOther(x: Subject): x is { v: 'b' } { return true as boolean as never }
export function makeProps(x: Subject) { return { v: isOther(x) ? x : 'no' } }
type OptionalBoxed = { k?: 'a' };
type OptionalSubject = { v: OptionalBoxed['k'] };
function isOptionalOther(x: OptionalSubject): x is { v: 'b' } { return true as boolean as never }
export function makeOptionalProps(x: OptionalSubject) { return { v: isOptionalOther(x) ? x : 'no' } }
type N1 = { v: number };
function is1(x: N1): x is { v: 'b' } { return true as boolean as never }
export function n1(x: N1) { return { v: is1(x) ? x : 'no' } }
type N2 = { v: 'a' | 'c' };
function is2(x: N2): x is { v: 'b' | 'd' } { return true as boolean as never }
export function n2(x: N2) { return { v: is2(x) ? x : 'no' } }
type N3 = { v: 'a' | 'b' };
function is3(x: N3): x is { v: 'b' | 'd' } { return true as boolean as never }
export function n3(x: N3) { return { v: is3(x) ? x : 'no' } }
type N4 = { v: string };
function is4(x: N4): x is { v: number } { return true as boolean as never }
export function n4(x: N4) { return { v: is4(x) ? x : 'no' } }
type N5 = { v?: 'a' };
function is5(x: N5): x is { v: 'b' } { return true as boolean as never }
export function n5(x: N5) { return { v: is5(x) ? x : 'no' } }
type N6 = { v: boolean };
function is6(x: N6): x is { v: 1 } { return true as boolean as never }
export function n6(x: N6) { return { v: is6(x) ? x : 'no' } }
type N7 = { v?: 'a' };
function is7(x: N7): x is { v?: 'b' } { return true as boolean as never }
export function n7(x: N7) { return { v: is7(x) ? x : 'no' } }
type N8 = { v: 'a' | null };
function is8(x: N8): x is { v: 'b' | undefined } { return true as boolean as never }
export function n8(x: N8) { return { v: is8(x) ? x : 'no' } }
";

/// A type-predicate narrow compares the subject with the predicate's type
/// through the same reads, and the checker reduces `Subject & Predicate`
/// to `never` on a shared member that is an EMPTY DISCRIMINANT — not
/// optional on both sides, a literal type on at least one side (a unit
/// type, `boolean`, or a union of those), and no value in common — so the
/// narrowed arm vanishes: `Subject`'s member `v` reads `'a'` through
/// `Boxed['k']` and `'a' | undefined` through `OptionalBoxed['k']`, both
/// empty against the predicate's `v: 'b'`.
///
/// Measured on TypeScript 7.0.2 (`tsc --ignoreConfig --noEmit`, and with
/// `--strictNullChecks false`, the same answers): `makeProps`,
/// `makeOptionalProps`, `n1` (`number` against `'b'`), `n2` (`'a' | 'c'`
/// against `'b' | 'd'`), `n5` (optional `'a'` against required `'b'`),
/// `n6` (`boolean` against `1`) and `n8` (`'a' | null` against `'b' |
/// undefined`) return `{ v: string; }`; the intersection is kept by `n3`
/// (a shared `'b'`) — `{ v: string | (N3 & { v: 'b' | 'd'; }); }` — by
/// `n4` (no literal side) — `{ v: string | (N4 & { v: number; }); }` — and
/// by `n7` (optional on both sides) — `{ v: string | (N7 & { v?: 'b';
/// }); }` (the checker keeps the predicate's quote style; the prints below
/// spell the same literals with double quotes).
#[test]
fn a_narrow_over_a_member_read_through_an_indexed_access_decides_as_the_checker() {
    let loose = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
        ambient_lib: None,
    };
    for project in [ProbeProject::default(), loose] {
        let failures = mismatches_in(
            project,
            NARROW,
            &[
                ("ReturnType<typeof makeProps>", "{ v: string; }"),
                ("ReturnType<typeof makeOptionalProps>", "{ v: string; }"),
                ("ReturnType<typeof n1>", "{ v: string; }"),
                ("ReturnType<typeof n2>", "{ v: string; }"),
                (
                    "ReturnType<typeof n3>",
                    "{ v: string | (N3 & { v: \"b\" | \"d\"; }); }",
                ),
                (
                    "ReturnType<typeof n4>",
                    "{ v: string | (N4 & { v: number; }); }",
                ),
                ("ReturnType<typeof n5>", "{ v: string; }"),
                ("ReturnType<typeof n6>", "{ v: string; }"),
                (
                    "ReturnType<typeof n7>",
                    "{ v: string | (N7 & { v?: \"b\"; }); }",
                ),
                ("ReturnType<typeof n8>", "{ v: string; }"),
            ],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

const MERGED_METHODS: &str = "\
interface MM { f(): 'first' }
interface MM { f(): 'second' }
interface MDX { f(x: string): string }
interface MDX { f(y: string): string }
";

/// A merged method read through an indexed access is its overload group:
/// the callable its signatures make, never an empty object, so a relation
/// over it and a signature utility over it read those signatures.
///
/// Measured on TypeScript 7.0.2: `MM['f'] extends () => 'second'` is `"y"`,
/// `MM['f'] extends () => 'third'` is `"n"`, `ReturnType<MM['f']>` is
/// `"second"` and `Parameters<MDX['f']>[0]` is `string`.
#[test]
fn a_merged_method_read_through_an_indexed_access_is_its_overload_group() {
    let failures = mismatches(
        MERGED_METHODS,
        &[
            ("MM['f'] extends () => 'second' ? 'y' : 'n'", "\"y\""),
            ("MM['f'] extends () => 'third' ? 'y' : 'n'", "\"n\""),
            ("ReturnType<MM['f']>", "\"second\""),
            ("Parameters<MDX['f']>[0]", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
