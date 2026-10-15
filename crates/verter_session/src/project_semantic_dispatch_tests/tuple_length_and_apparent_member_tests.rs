//! Reads the checker answers from a tuple's shape or a value's apparent
//! type: a tuple's `length` is its possible lengths, a position at or past
//! a rest element reads every element from the rest on, and any other key
//! of a primitive, an array or a tuple is a member of its global wrapper
//! (`String`, `Number`, `Boolean`, `Array`, `ReadonlyArray`) — TypeScript's
//! `getApparentType`.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noEmit --strict`: `declare
//! const v: <probe>; export const s: null = v;` read off the TS2322
//! message.

use super::checker_probe_lane_tests::{
    degradation_in, mismatches, mismatches_in, return_failures_in_one_host, with_probe,
    ProbeProject,
};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;

const FIXTURE: &str = "\
type Tup = [1, 'two', 3?];
type RestT = [1, ...string[]];
type Mid = [1, ...string[], 2];
type RoT = readonly [1, 2];
type Lead = [...number[], 'end'];
type Opt = [1?, 2?];
type E = [];
";

/// The declarations TypeScript 7.0.2's `lib.es5.d.ts` makes for the
/// members read below, verbatim: the global wrappers the checker reads a
/// primitive's, an array's and a tuple's members from.
const LIB: &str = "\
interface String { charAt(pos: number): string; readonly length: number; readonly [index: number]: string; }
interface Number { toFixed(fractionDigits?: number): string; }
interface Boolean { valueOf(): boolean; }
interface Array<T> { length: number; [n: number]: T; }
interface ReadonlyArray<T> { readonly length: number; readonly [n: number]: T; }
";

/// A tuple's `length` is the union of its possible lengths, `number` with a
/// rest element.
///
/// Measured on TypeScript 7.0.2: `Tup['length']` is `2 | 3`, `[1,
/// 2]['length']` and `RoT['length']` are `2`, `Opt['length']` is `0 | 1 |
/// 2`, `E['length']` is `0`, `RestT['length']` is `number`, `Partial<[1,
/// 2]>['length']` is `0 | 1 | 2`, `Required<Tup>['length']` is `3`, and
/// `Tup['length'] extends 2 | 3` is `"y"`.
#[test]
fn a_tuple_length_is_its_possible_lengths() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Tup['length']", "2 | 3"),
            ("[1, 2]['length']", "2"),
            ("RoT['length']", "2"),
            ("Opt['length']", "0 | 1 | 2"),
            ("E['length']", "0"),
            ("RestT['length']", "number"),
            ("Partial<[1, 2]>['length']", "0 | 1 | 2"),
            ("Required<Tup>['length']", "3"),
            ("Tup['length'] extends 2 | 3 ? 'y' : 'n'", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A position before a rest element reads that element; one at or past it
/// reads every element from the rest on, the rest contributing its element
/// type.
///
/// Measured on TypeScript 7.0.2: `RestT[1]`, `RestT[5]` and `[1,
/// ...string[]][1]` are `string`, `Partial<[1, ...string[]]>[1]` and
/// `Partial<RestT>[1]` are `string | undefined`, `Partial<[1,
/// ...string[]]>[0]` is `1 | undefined`, `Mid[0]` is `1`, `Mid[1]` and
/// `Mid[2]` are `string | 2`, `Lead[0]` and `Lead[3]` are `number | "end"`,
/// `[1, 2?, ...string[]][1]` is `2 | undefined`, `RestT[number]` is `string
/// | 1` and `Mid[number]` is `string | 1 | 2`.
#[test]
fn a_position_at_or_past_a_rest_element_reads_from_the_rest_on() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("RestT[1]", "string"),
            ("RestT[5]", "string"),
            ("[1, ...string[]][1]", "string"),
            ("Partial<[1, ...string[]]>[1]", "string | undefined"),
            ("Partial<RestT>[1]", "string | undefined"),
            ("Partial<[1, ...string[]]>[0]", "1 | undefined"),
            ("Mid[0]", "1"),
            ("Mid[1]", "string | 2"),
            ("Mid[2]", "string | 2"),
            ("Lead[0]", "number | \"end\""),
            ("Lead[3]", "number | \"end\""),
            ("[1, 2?, ...string[]][1]", "2 | undefined"),
            ("RestT[number]", "string | 1"),
            ("Mid[number]", "string | 1 | 2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Any other key of a primitive, an array or a tuple reads its global
/// wrapper, in a project whose library declares one.
///
/// Measured on TypeScript 7.0.2: `string['length']`,
/// `Partial<string>['length']`, `'abc'['length']`, `string[]['length']` and
/// `(readonly 1[])['length']` are `number`, and `string['charAt'] extends
/// (pos: number) => string`, `number['toFixed'] extends (fractionDigits?:
/// number) => string` and `true['valueOf'] extends () => boolean` are
/// `"y"`.
#[test]
fn a_primitive_or_array_reads_its_global_wrapper() {
    let project = ProbeProject {
        files: &[],
        compiler_options: None,
        ambient_lib: Some(LIB),
    };
    let failures = mismatches_in(
        project,
        FIXTURE,
        &[
            ("string['length']", "number"),
            ("Partial<string>['length']", "number"),
            ("'abc'['length']", "number"),
            ("string[]['length']", "number"),
            ("(readonly 1[])['length']", "number"),
            (
                "string['charAt'] extends (pos: number) => string ? 'y' : 'n'",
                "\"y\"",
            ),
            (
                "number['toFixed'] extends (fractionDigits?: number) => string ? 'y' : 'n'",
                "\"y\"",
            ),
            ("true['valueOf'] extends () => boolean ? 'y' : 'n'", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A literal position past the end of a tuple without a rest element reads
/// `undefined` — the checker's recovery beside its TS2493 error — however
/// wide the position; past the fixed elements of a tuple with one it reads
/// the rest. With a rest element a negative key reads the tuple's number
/// index, and any numeric key of an array reads its element.
///
/// Measured on TypeScript 7.0.2: `RoT[5]`, `RoT[4294967296]`, `Opt[2]` and
/// `E[0]` are `undefined`, `RestT[4294967296]` is `string`, `RestT[-1]` is
/// `string | 1` and `string[][-1]` is `string`.
///
/// Mutation: answering an out-of-range position with the walker's miss
/// leaves the `undefined` rows opaque; reading a position too wide for
/// `usize` as a negative key reads `RestT[4294967296]` as `string | 1` on a
/// 32-bit target; reading a negative key as no numeric key leaves
/// `RestT[-1]` opaque.
#[test]
fn an_out_of_range_tuple_position_reads_undefined() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("RoT[5]", "undefined"),
            ("RoT[4294967296]", "undefined"),
            ("Opt[2]", "undefined"),
            ("E[0]", "undefined"),
            ("RestT[4294967296]", "string"),
            ("RestT[-1]", "string | 1"),
            ("string[][-1]", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A negative key reads the number index of every object but a single
/// tuple without a rest element — a union of tuples included, whichever its
/// arms. Of that single tuple a declared access stays unanswered: the
/// checker reads `undefined` only for an access written over it (its TS2514
/// recovery) and the number index for an instantiated one, and a declared
/// type does not say which it is.
///
/// Measured on TypeScript 7.0.2: `Mixed[-1]` is `string | 1 | 2`,
/// `Two[-1]` is `"a" | 1 | 2` (beside TS2339), `RoT[-1]` is `undefined` and
/// `Last<["a", "b"]>` over `X[-1]` is `"a" | "b"`.
///
/// Mutation: answering `undefined` off every tuple without a rest element
/// reads `Mixed[-1]` as `string | undefined | 1`, `Two[-1]` as `undefined`
/// and `Last<["a", "b"]>` as `undefined`; a union arm the walker leaves
/// unread kept beside the others reads `Mixed[-1]` with an opaque member.
#[test]
fn a_negative_key_reads_the_number_index_unless_the_access_decides() {
    let source = format!(
        "{FIXTURE}type Last<X extends readonly unknown[]> = X[-1];\ntype Mixed = [1, 2] | [1, ...string[]];\ntype Two = [1, 2] | ['a'];\n"
    );
    let failures = mismatches(
        &source,
        &[
            ("Mixed[-1]", "string | 1 | 2"),
            ("Two[-1]", "\"a\" | 1 | 2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for probe in ["RoT[-1]", "Last<[\"a\", \"b\"]>"] {
        let measured = with_probe(&source, probe, |dispatch, node| {
            render_node(dispatch, node, 0)
        });
        assert_eq!(measured, "Opaque(Miss)", "`{probe}` must stay unanswered");
    }
}

/// A position past the end of a tuple that is one arm of an intersection
/// reads the intersection's number index, which needs the global `Array`
/// wrapper; in a project whose library declares none that read is not
/// settled, so the access stays unanswered rather than reading the lone
/// tuple's `undefined`.
///
/// Mutation: projecting the arm as a single tuple when its wrapper is
/// unavailable reads both probes as `undefined`.
#[test]
fn a_composite_past_end_read_without_a_settled_wrapper_stays_unanswered() {
    let source = "type It = readonly [\"a\"] & { x: 1 };\n";
    let measured = with_probe(source, "It[3]", |dispatch, node| {
        render_node(dispatch, node, 0)
    });
    assert_eq!(measured, "Opaque(Miss)", "`It[3]` must stay unanswered");
    let returned = degradation_in(
        ProbeProject::default(),
        "export const IT = [\"a\"] as unknown as readonly [\"a\"] & { x: 1 };\nexport function it3() { return IT[3]; }\n",
        "it3",
    );
    assert!(
        matches!(returned, Ok(Some(_))),
        "`IT[3]` must not answer a definite type: {returned:?}"
    );
}

/// The module an `as const` tuple is imported from, with a global
/// interface whose members are declared through it.
const ROLES: &str = "\
export const ROLES = [\"admin\", \"editor\", \"viewer\"] as const;
declare global {
  interface Account {
    role: (typeof ROLES)[number];
    head: (typeof ROLES)[0];
    opt?: (typeof ROLES)[number];
    nul: (typeof ROLES)[number] | null;
    arr: (typeof ROLES)[number][];
    pair: [(typeof ROLES)[0], (typeof ROLES)[number]];
    nested: { inner: (typeof ROLES)[number] };
    cb: (r: (typeof ROLES)[number]) => (typeof ROLES)[0];
  }
}
";

/// Element reads in a function's return: a literal index of an imported
/// `as const` tuple is that element, a numeric key the union of the
/// elements it can read, a member declared `(typeof ROLES)[number]` or
/// `(typeof ROLES)[0]` the element type it names — wherever the access
/// sits in the declared type: an optional member, a union, an array or
/// tuple element, a nested object member, a function type's parameter or
/// return — and an out-of-range or negative literal `undefined`; a
/// rest-element tuple and a mutable array read as the checker reads them.
///
/// Measured on TypeScript 7.0.2 (`tsc --declaration --emitDeclarationOnly
/// --strict`): the `.d.ts` return type beside each row; `r3`, `rNeg` and
/// `opt2` beside their TS2493 / TS2514 errors; `nestedRole` and `cbRole`,
/// whose `.d.ts` reuses the authored type node, off the TS2322 message of
/// `const s: null = nestedRole(a)`. A negative key reads
/// `undefined` only off a single tuple without a rest element; off a union
/// (`uniNeg`, `mixNeg`, beside TS2339) or a tuple with a rest element it
/// reads the number index.
///
/// Mutation: without the element access of a binding the frame does not
/// own, every literal-index row is an unmodeled position; without the
/// declared positional read `roleIn` keeps an unreduced indexed access in
/// its member; reading only an access that is the whole declared type
/// leaves `optRole`, `nulRole`, `arrRole`, `pairRole`, `nestedRole` and
/// `cbRole` holding one; answering an out-of-range position with a miss
/// leaves `r3`, `rNeg` and `opt2` unmodeled; deciding a negative key per
/// union member reads `mixNeg` as `string | undefined | 1`.
#[test]
fn a_const_tuple_element_read_is_the_element_the_checker_reads() {
    let project = ProbeProject {
        files: &[("roles.ts", ROLES)],
        ..ProbeProject::default()
    };
    let failures = return_failures_in_one_host(
        project,
        r#"import { ROLES } from "./roles";
export const REST = [1, "a"] as unknown as readonly [1, ...string[]];
export const MID = [1, "a", 2] as unknown as readonly [1, ...string[], 2];
export const MUT = ["a", "b"];
export const OPT = [1] as unknown as readonly [1, 2?];
export const UNI = [1, 2] as unknown as readonly [1, 2] | readonly ["a"];
export const MIX = [1, 2] as unknown as readonly [1, 2] | readonly [1, ...string[]];
export function r0() { return ROLES[0]; }
export function r2() { return ROLES[2]; }
export function r3() { return ROLES[3]; }
export function rNeg() { return ROLES[-1]; }
export function rNegOr(i: -1 | 0) { return ROLES[i]; }
export function rBig() { return ROLES[4294967296]; }
export function rNum(i: number) { return ROLES[i]; }
export function rLit(i: 0 | 2) { return ROLES[i]; }
export function rStr() { return ROLES["1"]; }
export function role(a: Account) { return a.role; }
export function head(a: Account) { return a.head; }
export function roleIn(a: Account) { return { role: a.role }; }
export function optRole(a: Account) { return a.opt; }
export function nulRole(a: Account) { return a.nul; }
export function arrRole(a: Account) { return a.arr; }
export function pairRole(a: Account) { return a.pair; }
export function nestedRole(a: Account) { return a.nested; }
export function innerRole(a: Account) { return a.nested.inner; }
export function cbRole(a: Account) { return a.cb; }
export function rest0() { return REST[0]; }
export function rest1() { return REST[1]; }
export function rest9() { return REST[9]; }
export function restNeg() { return REST[-1]; }
export function restBig() { return REST[4294967296]; }
export function uniNeg() { return UNI[-1]; }
export function mixNeg() { return MIX[-1]; }
export function restN(i: number) { return REST[i]; }
export function mid0() { return MID[0]; }
export function mid1() { return MID[1]; }
export function mut0() { return MUT[0]; }
export function mut9() { return MUT[9]; }
export function mutN(i: number) { return MUT[i]; }
export function opt1() { return OPT[1]; }
export function opt2() { return OPT[2]; }
"#,
        &[
            ("r0", "\"admin\""),
            ("r2", "\"viewer\""),
            ("r3", "undefined"),
            ("rNeg", "undefined"),
            ("rNegOr", "\"admin\" | undefined"),
            ("rBig", "undefined"),
            ("rNum", "\"admin\" | \"editor\" | \"viewer\""),
            ("rLit", "\"admin\" | \"viewer\""),
            ("rStr", "\"editor\""),
            ("role", "\"admin\" | \"editor\" | \"viewer\""),
            ("head", "\"admin\""),
            ("roleIn", "{ role: \"admin\" | \"editor\" | \"viewer\"; }"),
            ("optRole", "\"admin\" | \"editor\" | \"viewer\" | undefined"),
            ("nulRole", "\"admin\" | \"editor\" | \"viewer\" | null"),
            ("arrRole", "(\"admin\" | \"editor\" | \"viewer\")[]"),
            (
                "pairRole",
                "[\"admin\", \"admin\" | \"editor\" | \"viewer\"]",
            ),
            (
                "nestedRole",
                "{ inner: \"admin\" | \"editor\" | \"viewer\"; }",
            ),
            ("innerRole", "\"admin\" | \"editor\" | \"viewer\""),
            (
                "cbRole",
                "(r: \"admin\" | \"editor\" | \"viewer\") => \"admin\"",
            ),
            ("rest0", "1"),
            ("rest1", "string"),
            ("rest9", "string"),
            ("restNeg", "string | 1"),
            ("restBig", "string"),
            ("uniNeg", "\"a\" | 1 | 2"),
            ("mixNeg", "string | 1 | 2"),
            ("restN", "string | 1"),
            ("mid0", "1"),
            ("mid1", "string | 2"),
            ("mut0", "string"),
            ("mut9", "string"),
            ("mutN", "string"),
            ("opt1", "2 | undefined"),
            ("opt2", "undefined"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
