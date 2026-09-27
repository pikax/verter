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
interface SS2 { [key: string]: 2 }
interface NS { [n: number]: 'N'; [k: string]: 'S' }
type Fn = () => void;
type U = SS | SN;
type UU = SS | SS2;
type I = SS & { a: 1 };
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

/// The members of the library's `Object` and `Function` a composite's arms
/// read, as `lib.es5.d.ts` declares them.
const APPARENT_LIB: &str = "\
interface Object { toString(): string; }
interface Function { bind(this: Function, thisArg: any, ...argArray: any[]): any; prototype: any; readonly length: number; readonly name: string; }
";

/// An indexed access over a union or an intersection object reads the
/// composite as the checker does (`getIndexedAccessType`): every arm
/// string-index-only reads the composite's `string` index; a property an
/// arm declares reads the arms' properties — a union member without one
/// reading its apparent member or its applicable index signature, an
/// intersection reading only the arms that declare it; otherwise the
/// composite's own index signatures — an intersection's every arm's,
/// intersected, a union's the key types every arm declares, unioned.
///
/// Measured on TypeScript 7.0.2: `(SS | { x: 2 })['x']`, `(SS |
/// SS2)['x']`, `(SS | { x: 2 } | SS2)['x']` and `(SS | SS2)[number]` are
/// `1 | 2`; `(SS & SN)[0]`, `(SS & SN)[number]`, `(SNS | SN)[0]`, `(SN |
/// { 0: 'z' })[0]` and `(SS & SN & { a: 1 })[5]` are `string`; `(SS &
/// SN)['x']`, `(SS & { y: 1 })['x']` and `(SS & { a: 'A' })['b']` are
/// `1`; `(SS & SS2)['x']` is `never`; `(SNS | SS)[0]` is `"n" | "s" | 1`;
/// `(SS & { x: 2 })['x']` is `2`; `(TL & SS)['data-x']` is `"D"`; `(SS |
/// SS2)[symbol]` is `1 | 2` with TS2538. `(SS | { y: 2 })['x']` and `(SN
/// | SS)[0]` are TS2339: no arm but one declares the key, and the arms
/// share no index key type.
#[test]
fn a_composite_object_reads_its_properties_then_its_index_signatures() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("(SS | { x: 2 })['x']", "1 | 2"),
            ("(SS | SS2)['x']", "1 | 2"),
            ("(SS | { x: 2 } | SS2)['x']", "1 | 2"),
            ("(SS | SS2)[number]", "1 | 2"),
            ("(SS & SN)[0]", "string"),
            ("(SS & SN)[number]", "string"),
            ("(SNS | SN)[0]", "string"),
            ("(SN | { 0: 'z' })[0]", "string"),
            ("(SS & SN & { a: 1 })[5]", "string"),
            ("(SS & SN)['x']", "1"),
            ("(SS & { y: 1 })['x']", "1"),
            ("(SS & { a: 'A' })['b']", "1"),
            ("(SS & SS2)['x']", "never"),
            ("(SNS | SS)[0]", "\"n\" | \"s\" | 1"),
            ("(SS & { x: 2 })['x']", "2"),
            ("(TL & SS)['data-x']", "\"D\""),
            ("(SS | SS2)[symbol]", "1 | 2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for probe in ["(SS | { y: 2 })['x']", "(SN | SS)[0]"] {
        let measured = with_probe(FIXTURE, probe, |dispatch, node| {
            render_node(dispatch, node, 0)
        });
        assert_eq!(measured, "Opaque(Miss)", "`{probe}` is the checker's error");
    }
}

/// A composite's arms read the members the apparent `Function` and
/// `Object` types add before the composite's index signatures — unless
/// every arm is string-index-only, when the key reads the composite's
/// `string` index.
///
/// Measured on TypeScript 7.0.2 (with the library's `Object` and
/// `Function`): `(SS | { x: 2 })['toString']` and `(SS & { y: 1
/// })['toString']` are `() => string`, `(SS | { toString: 5
/// })['toString']` is `5 | (() => string)`, `(SS | SS2)['toString']` is
/// `1 | 2`, `(SS & SS2)['toString']` and `(SS & CS)['bind']` are `never`,
/// `(CS & { y: 1 })['bind'] extends Function['bind']` and `(SS &
/// Fn)['bind'] extends Function['bind']` are `"y"`, `(SS & Fn)['x']` is
/// `1`, `NS['toString']` is `() => string` and `NS['q']` is `"S"`.
#[test]
fn a_composites_arms_read_apparent_members_before_its_index_signatures() {
    let project = ProbeProject {
        files: &[],
        compiler_options: None,
        ambient_lib: Some(APPARENT_LIB),
    };
    let failures = mismatches_in(
        project,
        FIXTURE,
        &[
            ("(SS | { x: 2 })['toString']", "() => string"),
            ("(SS & { y: 1 })['toString']", "() => string"),
            ("(SS | { toString: 5 })['toString']", "5 | (() => string)"),
            ("(SS | SS2)['toString']", "1 | 2"),
            ("(SS & SS2)['toString']", "never"),
            ("(SS & CS)['bind']", "never"),
            (
                "(CS & { y: 1 })['bind'] extends Function['bind'] ? 'y' : 'n'",
                "\"y\"",
            ),
            (
                "(SS & Fn)['bind'] extends Function['bind'] ? 'y' : 'n'",
                "\"y\"",
            ),
            ("(SS & Fn)['x']", "1"),
            ("NS['toString']", "() => string"),
            ("NS['q']", "\"S\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The members of the library's `Object`, `String`, `Array` and
/// `ReadonlyArray` the arms below read, as `lib.es5.d.ts` declares them.
const ARRAY_LIB: &str = "\
interface Object { toString(): string; }
interface String { readonly length: number; readonly [index: number]: string; }
interface Array<T> { length: number; [n: number]: T; }
interface ReadonlyArray<T> { readonly length: number; readonly [n: number]: T; }
";

/// A tuple, an array, a nested union or a nested intersection arm is read
/// as the checker reads it: a tuple declares its fixed positions and
/// `length`, an array none of its positions, both reading the rest
/// through `Array`'s number index; a union arm flattens into its union, an
/// intersection distributes over a union arm, and an intersection arm of a
/// union is one object.
///
/// Measured on TypeScript 7.0.2 (all four settings): `([1] | SS)[0]`,
/// `([1] | SS)['length']`, `(string[] & SS)['x']`, `((SS | SN) & { a: 1
/// })['a']` and `([1] & SS)[0]` and `([1] & SS)[5]` are `1`;
/// `(string[] | SS)['length']` and `(string[] & SS)['length']` are
/// `number`; `(string[] & SS)[0]`, `(string[] & SS)[number]` and
/// `(string[] | { 0: 'z' })[0]` are `string`; `((SS | SS2) & { a: 1
/// })['b']`, `([1, 2] | SS)[1]`, `((SS & SN) | SS2)['x']`, `((SS & SN) |
/// SS2)[0]` and `([1] | { 0: 2 })[0]` are `1 | 2`; `([1, 2] | [3])[0]` is
/// `1 | 3`; `([1] | string[])[0]` is `string | 1`; `(UU | { x: 2 })['x']`
/// over `type UU = SS | SS2` is `1 | 2`. `([1] | SS)['x']`, `(string[] |
/// SS)[0]`, `(string[] | SS)['x']`, `((SS | SN) & { a: 1 })['b']`,
/// `([1] | SS)[5]`, `(U | { x: 2 })['x']` and `(U & { a: 1 })['b']` over
/// `type U = SS | SN`, `(I | SN)['a']` and `(I | SN)[0]` over `type I = SS
/// & { a: 1 }`, and `(string | SS)[0]` are TS2339, `(readonly string[] |
/// SS)[number]` and `([1] | SS)[number]` TS2537 — the error type.
#[test]
fn tuple_array_and_nested_composite_arms_read_as_the_checker_reads_them() {
    let project = ProbeProject {
        files: &[],
        compiler_options: None,
        ambient_lib: Some(ARRAY_LIB),
    };
    let failures = mismatches_in(
        project,
        FIXTURE,
        &[
            ("([1] | SS)[0]", "1"),
            ("([1] | SS)['length']", "1"),
            ("(string[] & SS)['x']", "1"),
            ("((SS | SN) & { a: 1 })['a']", "1"),
            ("([1] & SS)[0]", "1"),
            ("([1] & SS)[5]", "1"),
            ("(string[] | SS)['length']", "number"),
            ("(string[] & SS)['length']", "number"),
            ("(string[] & SS)[0]", "string"),
            ("(string[] & SS)[number]", "string"),
            ("(string[] | { 0: 'z' })[0]", "string"),
            ("((SS | SS2) & { a: 1 })['b']", "1 | 2"),
            ("([1, 2] | SS)[1]", "1 | 2"),
            ("((SS & SN) | SS2)['x']", "1 | 2"),
            ("((SS & SN) | SS2)[0]", "1 | 2"),
            ("([1] | { 0: 2 })[0]", "1 | 2"),
            ("([1, 2] | [3])[0]", "1 | 3"),
            ("([1] | string[])[0]", "string | 1"),
            ("(UU | { x: 2 })['x']", "1 | 2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for probe in [
        "([1] | SS)['x']",
        "(string[] | SS)[0]",
        "(string[] | SS)['x']",
        "((SS | SN) & { a: 1 })['b']",
        "([1] | SS)[5]",
        "(U | { x: 2 })['x']",
        "(U & { a: 1 })['b']",
        "(I | SN)['a']",
        "(I | SN)[0]",
        "(string | SS)[0]",
        "(readonly string[] | SS)[number]",
        "([1] | SS)[number]",
    ] {
        let measured = super::checker_probe_lane_tests::with_probe_in(
            project,
            FIXTURE,
            probe,
            |dispatch, node| render_node(dispatch, node, 0),
        );
        assert_eq!(measured, "Opaque(Miss)", "`{probe}` is the checker's error");
    }
}
