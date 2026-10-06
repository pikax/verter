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

use super::checker_probe_lane_tests::{mismatches, mismatches_in, ProbeProject};

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
