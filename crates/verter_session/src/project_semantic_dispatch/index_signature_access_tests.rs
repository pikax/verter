//! Indexed accesses the checker answers from an index signature: a key no
//! property names reads the index signature that applies to it
//! (TypeScript's `getPropertyTypeForIndexType` over
//! `getApplicableIndexInfo`).
//!
//! A numeric key — a number, a number literal or a numeric string name —
//! applies to a number index, any other index whose key type it is
//! assignable to (a template literal or a symbol index) before a string
//! index, and a string index when nothing else applies; several applicable
//! non-string indexes intersect their types. A symbol key no symbol index
//! takes reads the string index (TS2538, which the checker recovers from
//! with that type).
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --declaration
//! --emitDeclarationOnly --strict` under each of `strictNullChecks` and
//! `noImplicitAny` on and off: `export declare const v: <probe>; export
//! const r = v;` read off the declaration emitted for `r`. The four
//! settings print the same answer for every probe here except where a test
//! says otherwise.

use super::checker_probe_lane_tests::{mismatches, mismatches_in, with_probe, ProbeProject};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;

const FIXTURE: &str = "\
interface SN { [index: number]: string }
interface SS { [key: string]: 1 }
type TN = { [n: number]: 2 };
interface M { a: 'A'; [k: string]: 'A' | 'S' }
interface MN { 0: 'zero'; [n: number]: 'N' | 'zero'; [k: string]: 'N' | 'zero' | 'S' }
interface TL { [k: `data-${string}`]: 'D' }
interface TT { [k: `a${string}`]: 'A'; [k: `${string}b`]: 'B' }
interface SY { [s: symbol]: 'Y' }
interface RO { readonly [i: number]: 'R' }
interface Dict<T> { [k: string]: T }
interface CS { (): 1; [k: string]: 'CS' }
interface SU { [k: string]: 1 | undefined }
interface X extends SS { a: 1 }
interface Z extends X { z: 1 }
interface SNS { [index: number]: 'n'; [k: string]: 'n' | 's' }
interface Y extends SNS { [index: number]: 'n' }
declare const uniq: unique symbol;
";

/// The declarations TypeScript 7.0.2's `lib.es5.d.ts` makes for the
/// wrapper members read below, verbatim.
const LIB: &str = "\
interface String { charAt(pos: number): string; readonly length: number; readonly [index: number]: string; }
";

/// A key no property names reads the index signature that applies to it.
///
/// Measured on TypeScript 7.0.2: `SN[number]`, `SN[0]`, `SN['0']`,
/// `SN['1.5']`, `SN[-1]` and `SN['-1']` are `string`; `SS[string]`,
/// `SS['x']`, `SS[number]` and `SS[0]` are `1`; `TN[number]`, `TN[0]` and
/// `TN['1']` are `2`; `RO[number]` is `"R"`; `Dict<3>['x']` and
/// `Dict<3>[string]` are `3`; `{ [k: string]: 4 }['z']` is `4`.
#[test]
fn a_key_no_property_names_reads_the_applicable_index_signature() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("SN[number]", "string"),
            ("SN[0]", "string"),
            ("SN['0']", "string"),
            ("SN['1.5']", "string"),
            ("SN[-1]", "string"),
            ("SN['-1']", "string"),
            ("SS[string]", "1"),
            ("SS['x']", "1"),
            ("SS[number]", "1"),
            ("SS[0]", "1"),
            ("TN[number]", "2"),
            ("TN[0]", "2"),
            ("TN['1']", "2"),
            ("RO[number]", "\"R\""),
            ("Dict<3>['x']", "3"),
            ("Dict<3>[string]", "3"),
            ("{ [k: string]: 4 }['z']", "4"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A property wins over an index signature that also applies, the numeric
/// spelling of a property name reads it too, and a number index applies
/// before a string index.
///
/// Measured on TypeScript 7.0.2: `M['a']` is `"A"` and `M['b']`,
/// `M[string]` and `M[number]` are `"A" | "S"`; `MN[0]` and `MN['0']` are
/// `"zero"`, `MN[1]` and `MN[number]` `"N" | "zero"`, and `MN['x']` and
/// `MN[string]` `"N" | "S" | "zero"`.
#[test]
fn a_property_wins_over_an_index_signature_and_a_number_index_over_a_string_one() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("M['a']", "\"A\""),
            ("M['b']", "\"A\" | \"S\""),
            ("M[string]", "\"A\" | \"S\""),
            ("M[number]", "\"A\" | \"S\""),
            ("MN[0]", "\"zero\""),
            ("MN['0']", "\"zero\""),
            ("MN[1]", "\"N\" | \"zero\""),
            ("MN[number]", "\"N\" | \"zero\""),
            ("MN['x']", "\"N\" | \"S\" | \"zero\""),
            ("MN[string]", "\"N\" | \"S\" | \"zero\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A union key reads each constituent, a template-literal index applies
/// to the keys its pattern matches (two applicable ones intersect), and a
/// symbol index to symbols.
///
/// Measured on TypeScript 7.0.2: `SN[0 | 1]` is `string`, `SS['x' | 'y']`
/// and `SS['x' | 0]` are `1`, `M['a' | 'b']` is `"A" | "S"`; `TL['data-x']`
/// and ``TL[`data-${string}`]`` are `"D"`, `TT['ax']` is `"A"` and
/// `TT['ab']` `never`; `SS[`p${string}`]` is `1`; `SY[symbol]` and
/// `SY[typeof uniq]` are `"Y"`.
#[test]
fn union_template_and_symbol_keys_read_the_index_signature_that_applies() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("SN[0 | 1]", "string"),
            ("SS['x' | 'y']", "1"),
            ("SS['x' | 0]", "1"),
            ("M['a' | 'b']", "\"A\" | \"S\""),
            ("TL['data-x']", "\"D\""),
            ("TL[`data-${string}`]", "\"D\""),
            ("TT['ax']", "\"A\""),
            ("TT['ab']", "never"),
            ("SS[`p${string}`]", "1"),
            ("SY[symbol]", "\"Y\""),
            ("SY[typeof uniq]", "\"Y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A symbol key no symbol index takes reads the string index — the type
/// the checker recovers with after TS2538 ("Type 'symbol' cannot be used
/// as an index type").
///
/// Measured on TypeScript 7.0.2: `SS[symbol]` and `SS[typeof uniq]` are
/// `1`, each with TS2538.
#[test]
fn a_symbol_key_without_a_symbol_index_reads_the_string_index() {
    let failures = mismatches(FIXTURE, &[("SS[symbol]", "1"), ("SS[typeof uniq]", "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An index signature is read before the members an object's apparent
/// `Function` or `Object` type adds.
///
/// Measured on TypeScript 7.0.2: `SS['toString']`, `SS['constructor']` and
/// `SS['hasOwnProperty']` are `1`; `CS['bind']`, `CS['call']` and `CS['x']`
/// are `"CS"`.
#[test]
fn an_index_signature_is_read_before_apparent_members() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("SS['toString']", "1"),
            ("SS['constructor']", "1"),
            ("SS['hasOwnProperty']", "1"),
            ("CS['bind']", "\"CS\""),
            ("CS['call']", "\"CS\""),
            ("CS['x']", "\"CS\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A key no index signature applies to stays the typed miss: the checker
/// reports it and recovers with its error type.
///
/// Measured on TypeScript 7.0.2: `SN['x']` and `SN['01']` are TS2339
/// ("Property 'x' does not exist on type 'SN'"), `SN[string]` TS2537
/// ("Type 'SN' has no matching index signature for type 'string'"),
/// `SN[symbol]` TS2538, `TL['other']` TS2339 and `TL[string]` TS2537,
/// `SY['x']` TS2339 — each printed as the error type `any`.
#[test]
fn a_key_no_index_signature_applies_to_stays_a_miss() {
    for probe in [
        "SN['x']",
        "SN['01']",
        "SN[string]",
        "SN[symbol]",
        "TL['other']",
        "TL[string]",
        "SY['x']",
    ] {
        let measured = with_probe(FIXTURE, probe, |dispatch, node| {
            render_node(dispatch, node, 0)
        });
        assert_eq!(measured, "Opaque(Miss)", "`{probe}` is the checker's error");
    }
}

/// An index read follows the declaring project's `strictNullChecks` as a
/// property read does: with it off, `undefined` is no type of its own.
///
/// Measured on TypeScript 7.0.2: `SU['x']` is `1 | undefined` with
/// `strictNullChecks` on and `1` with it off (`noImplicitAny` either way).
#[test]
fn an_index_read_follows_the_projects_strict_null_checks() {
    let failures = mismatches(FIXTURE, &[("SU['x']", "1 | undefined")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for options in [
        r#"{ "strict": true, "strictNullChecks": false }"#,
        r#"{ "strict": true, "strictNullChecks": false, "noImplicitAny": false }"#,
    ] {
        let loose = ProbeProject {
            files: &[],
            compiler_options: Some(options),
            ambient_lib: None,
        };
        let failures = mismatches_in(loose, FIXTURE, &[("SU['x']", "1")]);
        assert!(failures.is_empty(), "{options}: {}", failures.join("\n"));
    }
}

/// A primitive reads its global wrapper's index signature, in a project
/// whose library declares one.
///
/// Measured on TypeScript 7.0.2: `string[number]`, `string[0]` and
/// `'abc'[number]` are `string`; `string['x']` is TS2339 ("Property 'x'
/// does not exist on type 'String'").
#[test]
fn a_primitive_reads_its_wrappers_index_signature() {
    let project = ProbeProject {
        files: &[],
        compiler_options: None,
        ambient_lib: Some(LIB),
    };
    let failures = mismatches_in(
        project,
        FIXTURE,
        &[
            ("string[number]", "string"),
            ("string[0]", "string"),
            ("'abc'[number]", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let measured = super::checker_probe_lane_tests::with_probe_in(
        project,
        FIXTURE,
        "string['x']",
        |dispatch, node| render_node(dispatch, node, 0),
    );
    assert_eq!(
        measured, "Opaque(Miss)",
        "`string['x']` is the checker's error"
    );
}

/// An interface that inherits an index signature is one object: a key none
/// of its own or inherited properties names reads its index signatures,
/// own and inherited, and a property it names is read as that property.
///
/// Measured on TypeScript 7.0.2: `X['b']` and `Z['q']` are `1`, `X['a']`
/// and `Z['a']` `1`, `Z['z']` `1`, `Y[0]` `"n"` and `Y['s']` `"n" | "s"`.
#[test]
fn an_inherited_index_signature_is_read_as_the_interfaces_own() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("X['b']", "1"),
            ("Z['q']", "1"),
            ("X['a']", "1"),
            ("Z['a']", "1"),
            ("Z['z']", "1"),
            ("Y[0]", "\"n\""),
            ("Y['s']", "\"n\" | \"s\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The global `Object`'s members are read last: after a property, after an
/// index signature on a surface with no property of its own, and before an
/// index signature on a surface with properties.
///
/// Measured on TypeScript 7.0.2 (whose `Object` declares `toString():
/// string`): `M['toString']`, `X['toString']` and `SN['toString']` are `()
/// => string`, while `SS['toString']` is `1`, `M['zz']` `"A" | "S"` and
/// `X['zz']` `1`.
#[test]
fn the_global_objects_members_are_read_after_an_index_signature_without_properties() {
    let project = ProbeProject {
        files: &[],
        compiler_options: None,
        ambient_lib: Some("interface Object { toString(): string; }"),
    };
    let failures = mismatches_in(
        project,
        FIXTURE,
        &[
            ("M['toString']", "() => string"),
            ("X['toString']", "() => string"),
            ("SN['toString']", "() => string"),
            ("SS['toString']", "1"),
            ("M['zz']", "\"A\" | \"S\""),
            ("X['zz']", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The arms of a union or intersection read their own properties only: the
/// index signatures a composite applies are the composite's, not any one
/// arm's.
///
/// Measured on TypeScript 7.0.2: `(SS & { x: 2 })['x']` is `2` — the
/// property, never intersected with the other arm's index signature; `(SN
/// | SS)[0]` is TS2339 ("Property '0' does not exist on type 'SN | SS'"):
/// the union's index signatures are the key types every arm declares, and
/// `SN` and `SS` share none.
#[test]
fn a_composites_arms_read_their_own_properties_only() {
    let failures = mismatches(FIXTURE, &[("(SS & { x: 2 })['x']", "2")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let measured = with_probe(FIXTURE, "(SN | SS)[0]", |dispatch, node| {
        render_node(dispatch, node, 0)
    });
    assert_eq!(
        measured, "Opaque(Miss)",
        "`(SN | SS)[0]` is the checker's error"
    );
}
