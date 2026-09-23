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

use super::checker_probe_lane_tests::{holds_deferred_conditional, mismatches};

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

/// An access that reads an OPTIONAL property is not decided. The checker
/// reads `Box['opt']` as `3 | undefined` (measured on TypeScript 7.0.2, and
/// `undefined extends Box['opt']` is `"y"`), while the type-level
/// indexed-access read answers the declared `3`; deciding on that read
/// would answer `"n"`. The relation stays undecided instead, and the
/// conditional stays deferred.
#[test]
fn an_access_reading_an_optional_property_stays_undecided() {
    assert!(
        holds_deferred_conditional(FIXTURE, "undefined extends Box['opt'] ? 'y' : 'n'"),
        "a relation over an optional property's read must not be decided on a read without \
         its `undefined`"
    );
}

const NARROW: &str = "\
type Boxed = { k: 'a' };
type Subject = { v: Boxed['k'] };
function isOther(x: Subject): x is { v: 'b' } { return true as boolean as never }
export function makeProps(x: Subject) { return { v: isOther(x) ? x : 'no' } }
";

/// A type-predicate narrow compares the subject with the predicate's type
/// through the same reads: `Subject`'s member `v` reads `'a'` through
/// `Boxed['k']`, which conflicts with the predicate's `v: 'b'` as two unit
/// types, so the checker reduces `Subject & { v: 'b' }` to `never` and the
/// narrowed arm vanishes.
///
/// Measured on TypeScript 7.0.2 (`tsc --declaration --emitDeclarationOnly
/// --strict`): `makeProps` returns `{ v: string; }`.
#[test]
fn a_narrow_over_a_member_read_through_an_indexed_access_decides_as_the_checker() {
    let failures = mismatches(
        NARROW,
        &[("ReturnType<typeof makeProps>", "{ v: string; }")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
