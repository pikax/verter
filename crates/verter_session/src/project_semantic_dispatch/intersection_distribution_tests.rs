//! An intersection over a union distributes, as TypeScript's
//! `getIntersectionType` does: `A & (B | C)` is `(A & B) | (A & C)`, each
//! constituent reduced on its own — disjoint scalars to `never`, a literal
//! over its primitive, a primitive over `{}` — and redundant supertypes are
//! removed first (`string & (string | number)` is `string`). A result whose
//! constituents are object intersections keeps the printed origin
//! (`(QA | QB) & Z`), and a member read or a relation reads the distributed
//! type.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noEmit --strict`: `declare
//! const v: <probe>; export const s: null = v;` read off the TS2322
//! message; a `never` answer, which is assignable to `null` and so draws
//! no message, is read as `[<probe>] extends [never] ? 'y' : 'n'` being
//! `"y"`.

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
interface QA { qa: 1 }
interface QB { qb: 2 }
type Z = { z: 1 };
type T1 = [1 & (1 | 2)];
type O1 = { a: 1 & (1 | 2) };
";

/// Scalar constituents distribute and reduce.
///
/// Measured on TypeScript 7.0.2: `1 & (1 | 2)` is `1`, `('a' | 'b') & ('b' |
/// 'c')` is `"b"`, `string & ('a' | 1)` is `"a"`, `(1 | 2 | 3) & (2 | 3 | 4)
/// & (3 | 4)` is `3`, `(1 | 'a') & (number | boolean)` is `1`, `(true | 1) &
/// boolean` is `true`, `string & (string | number)` is `string`, `{} & (1 |
/// 2)` is `1 | 2`, `QA & QA` is `QA`, `1 & unknown` is `1`, and `number &
/// string` is `never`.
#[test]
fn an_intersection_over_scalar_unions_distributes_and_reduces() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("1 & (1 | 2)", "1"),
            ("('a' | 'b') & ('b' | 'c')", "\"b\""),
            ("string & ('a' | 1)", "\"a\""),
            ("(1 | 2 | 3) & (2 | 3 | 4) & (3 | 4)", "3"),
            ("(1 | 'a') & (number | boolean)", "1"),
            ("(true | 1) & boolean", "true"),
            ("string & (string | number)", "string"),
            ("{} & (1 | 2)", "1 | 2"),
            ("QA & QA", "QA"),
            ("1 & unknown", "1"),
            ("number & string", "never"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Object constituents distribute too; the print keeps the written origin
/// when every constituent stays an intersection.
///
/// Measured on TypeScript 7.0.2: `('a' | QA) & string` is `"a" | (QA &
/// string)`, `(1 | QA) & 1` is `1 | (QA & 1)`, `(QA | 1) & QB` prints `(1 |
/// QA) & QB`, and `(QA | QB) & Z` prints `(QA | QB) & Z`.
#[test]
fn an_intersection_over_object_unions_distributes_and_keeps_its_origin() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("('a' | QA) & string", "\"a\" | (QA & string)"),
            ("(1 | QA) & 1", "1 | (QA & 1)"),
            ("(QA | 1) & QB", "(1 | QA) & QB"),
            ("(QA | QB) & Z", "(QA | QB) & Z"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A member read, a `keyof` and a relation read the distributed type.
///
/// Measured on TypeScript 7.0.2: `({ k: 1 } & { k: 1 | 2 })['k']` is `1`,
/// `((QA | QB) & Z)['z']` is `1`, `keyof ((QA | QB) & Z)` is `"z"`, `(1 & (1
/// | 2)) extends 1` and `1 extends (1 & (1 | 2))` are `"y"`, `((QA | QB) &
/// Z) extends QA & Z` is `"n"` and `QA & Z extends ((QA | QB) & Z)` is
/// `"y"`.
#[test]
fn reads_and_relations_see_the_distributed_intersection() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("({ k: 1 } & { k: 1 | 2 })['k']", "1"),
            ("((QA | QB) & Z)['z']", "1"),
            ("keyof ((QA | QB) & Z)", "\"z\""),
            ("(1 & (1 | 2)) extends 1 ? 'y' : 'n'", "\"y\""),
            ("1 extends (1 & (1 | 2)) ? 'y' : 'n'", "\"y\""),
            ("((QA | QB) & Z) extends QA & Z ? 'y' : 'n'", "\"n\""),
            ("QA & Z extends ((QA | QB) & Z) ? 'y' : 'n'", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A written intersection relates as the type the checker constructs from
/// it wherever it stands — a tuple element, an object member, an array
/// element, a relation operand, a member read.
///
/// Measured on TypeScript 7.0.2: `[1 & (1 | 2)] extends [1]` and the
/// reverse, `{ a: 1 & (1 | 2) } extends { a: 1 }` and the reverse, `{ a:
/// number & string } extends { a: never }`, `(number & string) extends
/// never`, `[number & string] extends [never]`, `(1 & (1 | 2))[] extends
/// 1[]`, `(string & ('a' | 1)) extends 'a'` and the reverse, `{ a: string
/// & ('a' | 1) } extends { a: 'a' }`, `T1[0] extends 1` and `1 extends
/// O1['a']` are `"y"`, and `2 extends O1['a']` is `"n"`.
#[test]
fn a_written_intersection_relates_as_the_type_it_constructs() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("[1 & (1 | 2)] extends [1] ? 'y' : 'n'", "\"y\""),
            ("[1] extends [1 & (1 | 2)] ? 'y' : 'n'", "\"y\""),
            ("{ a: 1 & (1 | 2) } extends { a: 1 } ? 'y' : 'n'", "\"y\""),
            ("{ a: 1 } extends { a: 1 & (1 | 2) } ? 'y' : 'n'", "\"y\""),
            (
                "{ a: number & string } extends { a: never } ? 'y' : 'n'",
                "\"y\"",
            ),
            ("(number & string) extends never ? 'y' : 'n'", "\"y\""),
            ("[number & string] extends [never] ? 'y' : 'n'", "\"y\""),
            ("(1 & (1 | 2))[] extends 1[] ? 'y' : 'n'", "\"y\""),
            ("(string & ('a' | 1)) extends 'a' ? 'y' : 'n'", "\"y\""),
            ("'a' extends (string & ('a' | 1)) ? 'y' : 'n'", "\"y\""),
            (
                "{ a: string & ('a' | 1) } extends { a: 'a' } ? 'y' : 'n'",
                "\"y\"",
            ),
            ("T1[0] extends 1 ? 'y' : 'n'", "\"y\""),
            ("1 extends O1['a'] ? 'y' : 'n'", "\"y\""),
            ("2 extends O1['a'] ? 'y' : 'n'", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
