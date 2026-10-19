//! Enum literal types: every member of an enum has its own nominal
//! literal type (`E.A`), whose value is its base; the enum's type is the
//! union of its members' literal types (the one member's literal type for
//! a one-member enum), and a union holding every member of an enum prints
//! as the enum.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on these
//! fixtures with `tsc --ignoreConfig --declaration --emitDeclarationOnly
//! --strict` under all four `strictNullChecks` x `noImplicitAny` settings,
//! read off the emitted `.d.ts` (a type probe `P` is read as the return of
//! `function p() { const __probe: P = null as any; return __probe; }`).
//! The four settings agree on every row except where a row says otherwise.

use super::checker_probe_lane_tests::{mismatches_in, ProbeProject};
use super::differential_harness_tests::{Matrix, Read};

const ENUMS: &str = "\
export enum E { A, B }
export enum E3 { A, B, C }
export enum N { X = 1, Y = 5, Z }
export enum S { P = \"p\", Q = \"q\" }
export enum H { N0 = 0, S1 = \"s\" }
export const enum CE { A, B }
export declare enum DE { A, B }
export declare enum DI { A = 1, B }
export declare const enum DCE { A, B }
export enum M { A = 1 }
export enum M { B = 2 }
export enum Single { Only }
export enum SingleS { Only = \"o\" }
export enum Comp { A = \"x\".length, B = 2 }
export enum Cexpr { A = 1, B = A << 1, C = A | B }
export enum F { A = 1 }
export enum G { A = 1 }
export namespace NS { export enum Inner { X = 1, Y = 2 } }
";

fn enum_project<'a>(files: &'a [(&'static str, &'static str)]) -> ProbeProject<'a> {
    ProbeProject {
        files,
        compiler_options: None,
        ambient_lib: None,
    }
}

fn assert_rows(files: &[(&'static str, &'static str)], source: &str, rows: &[(&str, &str)]) {
    let failures = mismatches_in(enum_project(files), source, rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const USE_ENUMS: &str =
    "import { E, E3, N, S, H, CE, DE, DI, DCE, M, Single, SingleS, Comp, Cexpr, F, G, NS } from './enums';\n";

/// An enum member's type is its own literal type, printed `Enum.Member`
/// (`NS.Inner.X` for a namespace's enum); a one-member enum's type IS its
/// member's literal type and prints as the enum; the enum's type is the
/// union of its members, and a union holding every member of an enum
/// prints as the enum wherever its first member stands.
///
/// Measured on TypeScript 7.0.2: `E` is `E`, `E.A` `E.A`, `E.A | E.B`
/// `E`, `E3.A | E3.B` `E3.A | E3.B`, `E3.C | E3.A | E3.B | 7` `7 | E3`,
/// `Single` and `Single.Only` `Single`, `S.P` `S.P`, `CE.A` `CE.A`,
/// `DI.B` `DI.B`, `M` `M`, `M.B` `M.B`, `N.Z` `N.Z`, `Comp.A` `Comp.A`,
/// `NS.Inner.X` `NS.Inner.X`, `NS.Inner.X | NS.Inner.Y` `NS.Inner`, `typeof
/// E.A` `E.A`, `keyof typeof E` `"A" | "B"`, `` `${E.A}` `` `"0"`, `E.A[]`
/// `E.A[]`, `{ a: E.A | E.B }` `{ a: E; }`, `E.A | 0` `0 | E.A`, `S.P |
/// "p"` `"p" | S.P`.
///
/// Mutation: printing a union member by member (no enum grouping) answers
/// `E.A | E.B` for `E.A | E.B`; lowering `E.A` to its value answers `0`.
#[test]
fn an_enum_member_is_its_own_literal_type_and_prints_as_the_checker_prints_it() {
    assert_rows(
        &[("enums.ts", ENUMS)],
        USE_ENUMS,
        &[
            ("E", "E"),
            ("E.A", "E.A"),
            ("E.A | E.B", "E"),
            ("E3.A | E3.B", "E3.A | E3.B"),
            ("E3.C | E3.A | E3.B | 7", "7 | E3"),
            ("Single", "Single"),
            ("Single.Only", "Single"),
            ("S.P", "S.P"),
            ("CE.A", "CE.A"),
            ("DI.B", "DI.B"),
            ("M", "M"),
            ("M.B", "M.B"),
            ("N.Z", "N.Z"),
            ("Comp.A", "Comp.A"),
            ("NS.Inner.X", "NS.Inner.X"),
            ("NS.Inner.X | NS.Inner.Y", "NS.Inner"),
            ("typeof E.A", "E.A"),
            ("keyof typeof E", "\"A\" | \"B\""),
            ("`${E.A}`", "\"0\""),
            ("E.A[]", "E.A[]"),
            ("{ a: E.A | E.B }", "{ a: E; }"),
            ("E.A | 0", "0 | E.A"),
            ("S.P | \"p\"", "\"p\" | S.P"),
        ],
    );
}

/// An enum member's literal is nominal. It relates to any other type as
/// its value does; `number` is assignable to a numeric member and to a
/// numeric enum, a number literal only to a member of its value (to any
/// member whose value is not a constant); no string literal and no
/// `string` is assignable to a string member; a member of one enum is
/// never assignable to another enum's, whatever their values.
///
/// Measured on TypeScript 7.0.2, `X extends Y ? "y" : "n"`: `E.A`/`0` y;
/// `0`/`E.A` y; `1`/`E.A` n; `E`/`number` y; `number`/`E` y;
/// `number`/`E.A` y; `S`/`string` y; `string`/`S` n; `S.P`/`"p"` y;
/// `"p"`/`S.P` n; `"p"`/`S` n; `E.A`/`E` y; `E`/`E.A` n; `F.A`/`G.A` n;
/// `F`/`G` n; `F.A`/`G` n; `1`/`F` y; `2`/`F` n; `0`/`E` y; `5`/`E` n;
/// `1.5`/`E` n; `E`/`0 | 1` y; `0 | 1`/`E` y; `H`/`string` n;
/// `H`/`string | number` y; `H.S1`/`string` y; `E3`/`E3.A | E3.B | E3.C`
/// y; `E`/`E3` n; `Cexpr.C`/`3` y; `CE.A`/`0` y; `M`/`1 | 2` y;
/// `Single`/`0` y; `0`/`Single` y; `E.A`/`string` n; `S.P`/`number` n; and
/// over computed members (`Comp.A`, `DE.A`, a member of a `declare enum`
/// without an initializer): `5`/`Comp` y; `5`/`Comp.A` y; `Comp`/`Comp.A`
/// n; `Comp.A`/`number` y; `Comp.A`/`3` n; `Comp.B`/`2` y; `2`/`Comp.B` y;
/// `DE.A`/`0` n; `0`/`DE.A` y; `7`/`DE.A` y; `DE.A`/`DE.B` n; `1`/`DCE.B`
/// y; `5`/`DCE.B` n.
///
/// Mutation: relating an enum member's literal to another enum's member by
/// its value answers `F.A`/`G.A` y; refusing `number` into a numeric
/// member answers `number`/`E.A` n.
#[test]
fn an_enum_member_relates_nominally_and_through_its_value() {
    let rows: Vec<(String, &str)> = [
        ("E.A", "0", "y"),
        ("0", "E.A", "y"),
        ("1", "E.A", "n"),
        ("E", "number", "y"),
        ("number", "E", "y"),
        ("number", "E.A", "y"),
        ("S", "string", "y"),
        ("string", "S", "n"),
        ("S.P", "\"p\"", "y"),
        ("\"p\"", "S.P", "n"),
        ("\"p\"", "S", "n"),
        ("E.A", "E", "y"),
        ("E", "E.A", "n"),
        ("F.A", "G.A", "n"),
        ("F", "G", "n"),
        ("F.A", "G", "n"),
        ("1", "F", "y"),
        ("2", "F", "n"),
        ("0", "E", "y"),
        ("5", "E", "n"),
        ("1.5", "E", "n"),
        ("E", "0 | 1", "y"),
        ("0 | 1", "E", "y"),
        ("H", "string", "n"),
        ("H", "string | number", "y"),
        ("H.S1", "string", "y"),
        ("E3", "E3.A | E3.B | E3.C", "y"),
        ("E", "E3", "n"),
        ("Cexpr.C", "3", "y"),
        ("CE.A", "0", "y"),
        ("M", "1 | 2", "y"),
        ("Single", "0", "y"),
        ("0", "Single", "y"),
        ("E.A", "string", "n"),
        ("S.P", "number", "n"),
        ("5", "Comp", "y"),
        ("5", "Comp.A", "y"),
        ("Comp", "Comp.A", "n"),
        ("Comp.A", "number", "y"),
        ("Comp.A", "3", "n"),
        ("Comp.B", "2", "y"),
        ("2", "Comp.B", "y"),
        ("DE.A", "0", "n"),
        ("0", "DE.A", "y"),
        ("7", "DE.A", "y"),
        ("DE.A", "DE.B", "n"),
        ("1", "DCE.B", "y"),
        ("5", "DCE.B", "n"),
    ]
    .into_iter()
    .map(|(source, target, answer)| {
        (
            format!("{source} extends {target} ? \"y\" : \"n\""),
            if answer == "y" { "\"y\"" } else { "\"n\"" },
        )
    })
    .collect();
    let rows: Vec<(&str, &str)> = rows.iter().map(|(p, a)| (p.as_str(), *a)).collect();
    assert_rows(&[("enums.ts", ENUMS)], USE_ENUMS, &rows);
}

const VALUE_READS: &str = "\
import { E, E3, N, S, H, CE, DE, DI, M, Single, SingleS, Comp, Cexpr, NS } from './enums';
export function eA() { return E.A; }
export function eAB(b: boolean) { return b ? E.A : E.B; }
export function e3AB(b: boolean) { return b ? E3.A : E3.B; }
export function eLet() { let x = E.A; return x; }
export function eConst() { const x = E.A; return x; }
export function eObj() { return { e: E.A }; }
export function eArr() { return [E.A]; }
export function eSingle() { return Single.Only; }
export function eSingleS() { return SingleS.Only; }
export function sP() { return S.P; }
export function hS() { return H.S1; }
export function ceA() { return CE.A; }
export function deA() { return DE.A; }
export function diB() { return DI.B; }
export function mB() { return M.B; }
export function nZ() { return N.Z; }
export function compA() { return Comp.A; }
export function cexprC() { return Cexpr.C; }
export function eAnnA(): E.A { return E.A; }
export function nsX() { return NS.Inner.X; }
export function propRead() { const o: typeof E = E; return o.A; }
export function letAssign() { let x = E3.A; x = E3.B; return x; }
export function tern(b: boolean) { return b ? E3.A : 0; }
";

/// A read of an enum member is its FRESH literal: it widens to its enum
/// wherever a bare literal widens (a return, an object or array member, a
/// `let`), and a `const` keeps it. A `let` initialized with a member is
/// typed by the enum and holds the member, so a read of it is the member.
///
/// Measured on TypeScript 7.0.2: `eA` is `E`, `eAB` `E` (both members of
/// `E`), `e3AB` `E3.A | E3.B`, `eLet` `E.A`, `eConst` `E`, `eObj` `{ e: E;
/// }`, `eArr` `E[]`, `eSingle` `Single`, `eSingleS` `SingleS`, `sP` `S`,
/// `hS` `H`, `ceA` `CE`, `deA` `DE`, `diB` `DI`, `mB` `M`, `nZ` `N`,
/// `compA` `Comp`, `cexprC` `Cexpr`, `eAnnA` `E.A`, `nsX` `NS.Inner`,
/// `propRead` `E`, `letAssign` `E3.B`, `tern` `0 | E3.A`.
///
/// Mutation: widening an enum member's literal to its value's primitive
/// answers `eA` `number`; not reducing a `let` over its widened enum
/// answers `eLet` `E`.
#[test]
fn a_fresh_enum_member_widens_to_its_enum() {
    assert_rows(
        &[("enums.ts", ENUMS)],
        VALUE_READS,
        &[
            ("ReturnType<typeof eA>", "E"),
            ("ReturnType<typeof eAB>", "E"),
            ("ReturnType<typeof e3AB>", "E3.A | E3.B"),
            ("ReturnType<typeof eLet>", "E.A"),
            ("ReturnType<typeof eConst>", "E"),
            ("ReturnType<typeof eObj>", "{ e: E; }"),
            ("ReturnType<typeof eArr>", "E[]"),
            ("ReturnType<typeof eSingle>", "Single"),
            ("ReturnType<typeof eSingleS>", "SingleS"),
            ("ReturnType<typeof sP>", "S"),
            ("ReturnType<typeof hS>", "H"),
            ("ReturnType<typeof ceA>", "CE"),
            ("ReturnType<typeof deA>", "DE"),
            ("ReturnType<typeof diB>", "DI"),
            ("ReturnType<typeof mB>", "M"),
            ("ReturnType<typeof nZ>", "N"),
            ("ReturnType<typeof compA>", "Comp"),
            ("ReturnType<typeof cexprC>", "Cexpr"),
            ("ReturnType<typeof eAnnA>", "E.A"),
            ("ReturnType<typeof nsX>", "NS.Inner"),
            ("ReturnType<typeof propRead>", "E"),
            ("ReturnType<typeof letAssign>", "E3.B"),
            ("ReturnType<typeof tern>", "0 | E3.A"),
        ],
    );
}

const NARROWING: &str = "\
import { E, E3, S, Single } from './enums';
export function nEq(x: E3) { if (x === E3.A) { return x; } throw 0; }
export function nNeq(x: E3) { if (x === E3.A) { throw 0; } return x; }
export function neq(x: E3) { if (x !== E3.A) { return x; } throw 0; }
export function orEq(x: E3) { if (x === E3.A || x === E3.B) { return x; } throw 0; }
export function orElse(x: E3) { if (x === E3.A || x === E3.B) { throw 0; } return x; }
export function eqS(x: S) { if (x === S.P) { throw 0; } return x; }
export function eqLit(x: E3) { if (x === 0) { return x; } throw 0; }
export function nSw(x: E) { switch (x) { case E.A: return 1; case E.B: return 2; default: return x; } }
export function swExh(x: E) { switch (x) { case E.A: return 1; case E.B: return 2; } }
export function swPart(x: E3) { switch (x) { case E3.A: return 1; case E3.B: return 2; } }
export function swRest(x: E3) { switch (x) { case E3.A: throw 0; default: return x; } }
export function eqSingle(x: Single) { if (x === Single.Only) { return x; } return x; }
";

/// An equality with an enum member narrows by the member (`x === E3.A` is
/// `E3.A`, and the other edge the remaining members); a `switch` over an
/// enum's every member is exhaustive, so its default edge is `never` and
/// no path falls off the end.
///
/// Measured on TypeScript 7.0.2: `nEq` is `E3.A`, `nNeq` `E3.B | E3.C`,
/// `neq` `E3.B | E3.C`, `orEq` `E3.A | E3.B`, `orElse` `E3.C`, `eqS`
/// `S.Q`, `eqLit` `E3.A`, `nSw` `1 | 2`, `swExh` `1 | 2`, `swPart` `1 | 2
/// | undefined` (`1 | 2` with `strictNullChecks` off), `swRest` `E3.B |
/// E3.C`, `eqSingle` `Single`.
///
/// Mutation: not reading a free member path as an equality operand
/// answers `nEq` the unnarrowed `E3` behind a typed gap.
#[test]
fn an_enum_member_narrows_equality_and_switch() {
    assert_rows(
        &[("enums.ts", ENUMS)],
        NARROWING,
        &[
            ("ReturnType<typeof nEq>", "E3.A"),
            ("ReturnType<typeof nNeq>", "E3.B | E3.C"),
            ("ReturnType<typeof neq>", "E3.B | E3.C"),
            ("ReturnType<typeof orEq>", "E3.A | E3.B"),
            ("ReturnType<typeof orElse>", "E3.C"),
            ("ReturnType<typeof eqS>", "S.Q"),
            ("ReturnType<typeof eqLit>", "E3.A"),
            ("ReturnType<typeof nSw>", "1 | 2"),
            ("ReturnType<typeof swExh>", "1 | 2"),
            ("ReturnType<typeof swPart>", "1 | 2 | undefined"),
            ("ReturnType<typeof swRest>", "E3.B | E3.C"),
            ("ReturnType<typeof eqSingle>", "Single"),
        ],
    );
    let loose = mismatches_in(
        ProbeProject {
            files: &[("enums.ts", ENUMS)],
            compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
            ambient_lib: None,
        },
        NARROWING,
        &[("ReturnType<typeof swPart>", "1 | 2")],
    );
    assert!(loose.is_empty(), "{}", loose.join("\n"));
}

const CONSTANTS_DEP: &str = "export const dk = 7;\nexport enum DE { X = 3, Y = \"y\" }\n";
const CONSTANTS: &str = "\
import { dk, DE } from './dep';
const k = 5;
const ks = \"s\";
const kAnn: number = 9;
let kl = 4;
export namespace NS { export const nk = 11; export enum Inner { A = nk, B = NS.nk + 1 } }
export enum R { A = k, B = ks, C = kAnn, D = kl, E = dk, F = DE.X, G = DE.Y, I = DE.X + 1 }
export enum M1 { A = 1 }
export enum M1 { B = A + 1, C = M1.A * 10 }
export enum Other { P = R.A + 100, Q }
export enum Flags { None = 0, A = 1 << 0, B = 1 << 1, AB = A | B, C = Flags.AB << 1, D = Flags[\"C\"] * 2, Neg = ~A, Shr = -8 >> 1, Ushr = -8 >>> 28, Pow = 2 ** 10, Div = 1 / 4 }
export enum Str { A = \"a\", B = A + \"b\", C = `c${1 + 1}`, D = \"d\" + 1, E = `${B}-${Flags.A}` }
export declare enum DE2 { A, B }
export declare const enum DCE { A, B }
";

/// A member's value is the checker's constant evaluation of its
/// initializer: literals, templates, the unary and binary arithmetic and
/// bitwise operators, and references to members of the enum itself, of
/// another declaration of a merged enum, of another enum, and to
/// unannotated `const` variables — in the file, an enclosing namespace or
/// another module. An annotated or mutable variable is no constant, so a
/// member initialized with one is computed; so is a member without an
/// initializer in an ambient non-`const` enum.
///
/// Measured on TypeScript 7.0.2 (`X extends Y ? "y" : "n"`, each `y`
/// except where noted): `R.A`/`5`, `R.B`/`"s"`, `1`/`R.C` (computed),
/// `1`/`R.D` (computed), `R.E`/`7`, `R.F`/`3`, `R.G`/`"y"`, `R.I`/`4`,
/// `M1.B`/`2`, `M1.C`/`10`, `Other.P`/`105`, `Other.Q`/`106`,
/// `NS.Inner.A`/`11`, `NS.Inner.B`/`12`, `Flags.AB`/`3`, `Flags.C`/`6`,
/// `Flags.D`/`12`, `Flags.Neg`/`-2`, `Flags.Shr`/`-4`, `Flags.Ushr`/`15`,
/// `Flags.Pow`/`1024`, `Flags.Div`/`0.25`, `Str.B`/`"ab"`, `Str.C`/`"c2"`,
/// `Str.D`/`"d1"`, `Str.E`/`"ab-1"`, `7`/`DE2.A` (computed), `5`/`DCE.B`
/// `n`; the declaration file prints `C,` and `D,` without a value.
///
/// Mutation: evaluating no reference outside the declaration answers
/// `R.A`/`5` `n`; auto-numbering an ambient enum's bare members answers
/// `7`/`DE2.A` `n`.
#[test]
fn an_enum_member_value_is_the_checkers_constant_evaluation() {
    let yes = |probe: &'static str| (probe, "\"y\"");
    assert_rows(
        &[("dep.ts", CONSTANTS_DEP), ("enums.ts", CONSTANTS)],
        "import { R, M1, Other, NS, Flags, Str, DE2, DCE } from './enums';\n",
        &[
            yes("R.A extends 5 ? \"y\" : \"n\""),
            yes("R.B extends \"s\" ? \"y\" : \"n\""),
            yes("1 extends R.C ? \"y\" : \"n\""),
            yes("1 extends R.D ? \"y\" : \"n\""),
            yes("R.E extends 7 ? \"y\" : \"n\""),
            yes("R.F extends 3 ? \"y\" : \"n\""),
            yes("R.G extends \"y\" ? \"y\" : \"n\""),
            yes("R.I extends 4 ? \"y\" : \"n\""),
            yes("M1.B extends 2 ? \"y\" : \"n\""),
            yes("M1.C extends 10 ? \"y\" : \"n\""),
            yes("Other.P extends 105 ? \"y\" : \"n\""),
            yes("Other.Q extends 106 ? \"y\" : \"n\""),
            yes("NS.Inner.A extends 11 ? \"y\" : \"n\""),
            yes("NS.Inner.B extends 12 ? \"y\" : \"n\""),
            yes("Flags.AB extends 3 ? \"y\" : \"n\""),
            yes("Flags.C extends 6 ? \"y\" : \"n\""),
            yes("Flags.D extends 12 ? \"y\" : \"n\""),
            yes("Flags.Neg extends -2 ? \"y\" : \"n\""),
            yes("Flags.Shr extends -4 ? \"y\" : \"n\""),
            yes("Flags.Ushr extends 15 ? \"y\" : \"n\""),
            yes("Flags.Pow extends 1024 ? \"y\" : \"n\""),
            yes("Flags.Div extends 0.25 ? \"y\" : \"n\""),
            yes("Str.B extends \"ab\" ? \"y\" : \"n\""),
            yes("Str.C extends \"c2\" ? \"y\" : \"n\""),
            yes("Str.D extends \"d1\" ? \"y\" : \"n\""),
            yes("Str.E extends \"ab-1\" ? \"y\" : \"n\""),
            yes("7 extends DE2.A ? \"y\" : \"n\""),
            ("5 extends DCE.B ? \"y\" : \"n\"", "\"n\""),
        ],
    );
}

const REVERSE: &str = "\
import { E, S } from './enums';
export function eRev() { return E[E.A]; }
export function sRev() { return S[\"P\"]; }
";

/// A numeric enum's object reads its reverse mapping: `(typeof E)[0]` and
/// `(typeof E)[number]` are `string`, measured on TypeScript 7.0.2.
#[test]
fn an_enum_object_type_reads_its_reverse_mapping() {
    assert_rows(
        &[("enums.ts", ENUMS)],
        REVERSE,
        &[
            ("(typeof E)[0]", "string"),
            ("(typeof E)[number]", "string"),
        ],
    );
}

/// Element access on an enum object value: a numeric enum's reverse mapping
/// reads the member's name as `string`, and a string key reads the member.
///
/// Measured on TypeScript 7.0.2: `eRev` (`E[E.A]`) is `string`, `sRev`
/// (`S["P"]`) is `S`.
#[test]
#[ignore = "element access on an enum object: the numeric reverse mapping and a string-keyed member read"]
fn an_enum_object_element_access_reads_the_reverse_mapping_and_members() {
    assert_rows(
        &[("enums.ts", ENUMS)],
        REVERSE,
        &[
            ("ReturnType<typeof eRev>", "string"),
            ("ReturnType<typeof sRev>", "S"),
        ],
    );
}

/// Operators over an enum type the lane keeps as an unreduced reference:
/// the conditional utilities, a template over the enum, and the union and
/// intersection reductions against its members.
///
/// Measured on TypeScript 7.0.2: `Exclude<E3, E3.A>` is `E3.B | E3.C`,
/// `Exclude<E, E.A>` `E.B`, `` `${S}` `` `"p" | "q"`, `E | number`
/// `number`, `E & E.A` `E.A`, `F.A & G.A` `never`.
#[test]
fn operators_over_an_enum_type_reduce_as_the_checker_reduces_them() {
    assert_rows(
        &[("enums.ts", ENUMS)],
        USE_ENUMS,
        &[
            ("Exclude<E3, E3.A>", "E3.B | E3.C"),
            ("Exclude<E, E.A>", "E.B"),
            ("`${S}`", "\"p\" | \"q\""),
            ("E | number", "number"),
            ("E & E.A", "E.A"),
            ("F.A & G.A", "never"),
        ],
    );
}

/// Enum members referencing members declared after them, themselves, and
/// the spellings `Infinity` and `NaN`.
const REFERENCE_ORDER: &str = "\
enum Cyc { A = B, B = A }
enum Fwd { A = B, B = 1 }
enum Self1 { A = A, B }
enum SelfQ { A = SelfQ.A }
declare enum AmbFwd { A = B, B = 1 }
const enum CFwd { A = B, B = 1, C = CFwd.D, D = 2 }
enum W { A = W.B + 1, B = 5 }
enum EI { Infinity = 5, A = Infinity }
enum FI { A = Infinity, Infinity = 5 }
enum GI { A = Infinity, B = NaN, C = -Infinity }
";

/// A member of an enum body referencing a member declared after it reads
/// `0`, the value the checker gives a member used before its declaration
/// (TS2651), so `enum Cyc { A = B, B = A }` is `0` twice; one referencing
/// itself is used before it is assigned (TS2565) and computed; in an
/// ambient enum a later member is not yet computed, so the reference is
/// computed too. `Infinity` and `NaN` name a member of the enum first
/// (declared before or after the reference), and the global numbers
/// otherwise.
///
/// Measured on TypeScript 7.0.2 (`declare const p: <probe>; const s: never
/// = p;`), alike under every `strictNullChecks` × `noImplicitAny` setting:
/// `Cyc.A extends 0`, `Cyc.B extends 0`, `Fwd.A extends 0`, `1 extends
/// Self1.A`, `1 extends Self1.B`, `1 extends SelfQ.A`, `1 extends
/// AmbFwd.A`, `CFwd.A extends 0`, `CFwd.C extends 0`, `W.A extends 1`,
/// `EI.A extends 5`, `FI.A extends 0` are each `"y"`; `1 extends GI.A` and
/// `1 extends GI.B` are `"n"`.
///
/// Mutation: evaluating a later member's own value instead of `0` answers
/// `Fwd.A extends 0` `"n"`; folding `Infinity` by its spelling answers
/// `EI.A extends 5` `"n"`.
#[test]
fn a_reference_to_a_member_declared_after_it_reads_as_the_checker_reads_it() {
    let yes = |probe: &'static str| (probe, "\"y\"");
    let failures = Matrix::new(REFERENCE_ORDER).types(&[
        yes("Cyc.A extends 0 ? \"y\" : \"n\""),
        yes("Cyc.B extends 0 ? \"y\" : \"n\""),
        yes("Fwd.A extends 0 ? \"y\" : \"n\""),
        yes("1 extends Self1.A ? \"y\" : \"n\""),
        yes("1 extends Self1.B ? \"y\" : \"n\""),
        yes("1 extends SelfQ.A ? \"y\" : \"n\""),
        yes("1 extends AmbFwd.A ? \"y\" : \"n\""),
        yes("CFwd.A extends 0 ? \"y\" : \"n\""),
        yes("CFwd.C extends 0 ? \"y\" : \"n\""),
        yes("W.A extends 1 ? \"y\" : \"n\""),
        yes("EI.A extends 5 ? \"y\" : \"n\""),
        yes("FI.A extends 0 ? \"y\" : \"n\""),
        ("1 extends GI.A ? \"y\" : \"n\"", "\"n\""),
        ("1 extends GI.B ? \"y\" : \"n\"", "\"n\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A module declaring its own `Infinity` and `NaN`: an enum initializer
/// naming them reads those constants, not the global numbers.
const SHADOWED_NUMBERS: &str = "\
const Infinity = 9;
const NaN = 8;
enum K { A = Infinity, B = NaN, C = -Infinity }
";

/// `Infinity` and `NaN` resolve as every value reference does before the
/// global numbers apply: a module's own `const Infinity = 9` is `9`.
///
/// Measured on TypeScript 7.0.2, alike under every setting: `K.A extends
/// 9`, `K.B extends 8`, `K.C extends -9` are each `"y"`.
///
/// Mutation: folding `Infinity` and `NaN` by their spelling answers each
/// `"n"`.
#[test]
fn a_shadowed_infinity_or_nan_reads_the_declaration_in_scope() {
    let yes = |probe: &'static str| (probe, "\"y\"");
    let failures = Matrix::new(SHADOWED_NUMBERS).types(&[
        yes("K.A extends 9 ? \"y\" : \"n\""),
        yes("K.B extends 8 ? \"y\" : \"n\""),
        yes("K.C extends -9 ? \"y\" : \"n\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Infinity` and `NaN` naming the library's global declarations are the
/// global numbers: `enum GI { A = Infinity, B = NaN }` beside a library
/// declaring `declare var Infinity: number;` holds the constants, not
/// computed members.
///
/// Measured on TypeScript 7.0.2 (its own library declares both), alike
/// under every setting: `1 extends GI.A` and `1 extends GI.B` are `"n"`,
/// `GI.C extends number` is `"y"`.
///
/// Mutation: reading a reference that resolves to the library's `var` as
/// a variable answers `1 extends GI.A` `"y"` (computed).
#[test]
fn infinity_and_nan_naming_the_library_globals_are_the_global_numbers() {
    let failures = Matrix::new("enum GI { A = Infinity, B = NaN, C = -Infinity }\n")
        .lib("declare var Infinity: number;\ndeclare var NaN: number;\n")
        .types(&[
            ("1 extends GI.A ? \"y\" : \"n\"", "\"n\""),
            ("1 extends GI.B ? \"y\" : \"n\"", "\"n\""),
            ("GI.C extends number ? \"y\" : \"n\"", "\"y\""),
        ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Enum members referencing declarations of the same file written after
/// them: another enum, another declaration of a merged enum, another
/// namespace block, a `const`; and the same in ambient contexts.
const LATER_DECLARATIONS: &str = "\
enum F { A = G.A, B = 1 }
enum G { A = F.A }
enum M { A = B }
enum M { B = 1 }
enum Q { A = c }
const c = 5;
namespace P { export enum P1 { A = P.P2.A } }
namespace P { export enum P2 { A = 4 } }
enum R { A = S.B + 1 }
enum S { B = 7 }
declare enum D1 { A = D2.A }
declare enum D2 { A = 1 }
declare const enum C1 { A = C2.A }
declare const enum C2 { A = 2 }
declare namespace NS { enum N1 { A = N2.A } enum N2 { A = 3 } }
";

/// A member referencing a member of ANOTHER enum declaration (another enum,
/// another declaration of a merged enum, one in a later namespace block)
/// declared after it in the same file reads `0` (TS2651), and one
/// referencing a `const` declared after it is computed (TS2448) — the
/// checker's declared-before-use rule across declarations. A use in an
/// ambient context reads every declaration as declared before it.
///
/// Measured on TypeScript 7.0.2, alike under every setting: `F.A extends
/// 0`, `G.A extends 0`, `M.A extends 0`, `1 extends Q.A`, `5 extends Q.A`,
/// `P.P1.A extends 0`, `R.A extends 1`, `D1.A extends 1`, `1 extends D1.A`,
/// `C1.A extends 2`, `NS.N1.A extends 3` are each `"y"`; `1 extends NS.N1.A`
/// is `"n"`.
///
/// Mutation: reading every declaration of the file as declared before its
/// use answers `F.A`, `G.A`, `M.A`, `1 extends Q.A`, `P.P1.A` and `R.A`
/// `"n"`; ignoring the ambient context answers `D1.A`, `C1.A` and `NS.N1.A`
/// `"n"`.
#[test]
fn a_reference_to_a_later_declaration_reads_as_the_checker_reads_it() {
    let yes = |probe: &'static str| (probe, "\"y\"");
    let failures = Matrix::new(LATER_DECLARATIONS).types(&[
        yes("F.A extends 0 ? \"y\" : \"n\""),
        yes("G.A extends 0 ? \"y\" : \"n\""),
        yes("M.A extends 0 ? \"y\" : \"n\""),
        yes("1 extends Q.A ? \"y\" : \"n\""),
        yes("5 extends Q.A ? \"y\" : \"n\""),
        yes("P.P1.A extends 0 ? \"y\" : \"n\""),
        yes("R.A extends 1 ? \"y\" : \"n\""),
        yes("D1.A extends 1 ? \"y\" : \"n\""),
        yes("1 extends D1.A ? \"y\" : \"n\""),
        yes("C1.A extends 2 ? \"y\" : \"n\""),
        yes("NS.N1.A extends 3 ? \"y\" : \"n\""),
        ("1 extends NS.N1.A ? \"y\" : \"n\"", "\"n\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Enums in namespaces naming the namespaces' own members, exported or not:
/// a `const` (one spelled `Infinity` / `NaN`), an enum, a `const` of an
/// enclosing namespace, and a `const` of another block of a merged
/// namespace.
const NAMESPACE_MEMBERS: &str = "\
namespace N { const Infinity = 2; const NaN = 3; export enum E { A = Infinity, B = NaN } }
namespace N2 { const k = 4; export enum E { A = k } }
namespace N2 { export enum F { A = k } }
namespace N4 { const k = 1; enum H { X = 1 } export const e = 2; export enum E { A = k, B = H.X, C = e } }
namespace Outer { const o = 6; export namespace Inner { export enum E { A = o } } }
";

/// An enum in a namespace names the namespace's own non-exported members —
/// a `const` (one spelled `Infinity` or `NaN` shadows the global number),
/// an enum — and exported ones, in its own block or an enclosing one; a
/// non-exported member of another block of a merged namespace is not in
/// scope (TS2304), so the member naming it is computed.
///
/// Measured on TypeScript 7.0.2, alike under every setting: `N.E.A extends
/// 2`, `N.E.B extends 3`, `N2.E.A extends 4`, `1 extends N2.F.A`, `N4.E.A
/// extends 1`, `N4.E.B extends 1`, `N4.E.C extends 2` and
/// `Outer.Inner.E.A extends 6` are each `"y"`.
///
/// Mutation: resolving no private member of an enclosing namespace answers
/// `N.E.A`, `N.E.B`, `N2.E.A`, `N4.E.A`, `N4.E.B` and `Outer.Inner.E.A`
/// `"n"`; reading a private member outside its block answers `1 extends
/// N2.F.A` `"n"`.
#[test]
fn an_enum_reads_its_namespaces_non_exported_constants() {
    let yes = |probe: &'static str| (probe, "\"y\"");
    let failures = Matrix::new(NAMESPACE_MEMBERS).types(&[
        yes("N.E.A extends 2 ? \"y\" : \"n\""),
        yes("N.E.B extends 3 ? \"y\" : \"n\""),
        yes("N2.E.A extends 4 ? \"y\" : \"n\""),
        yes("1 extends N2.F.A ? \"y\" : \"n\""),
        yes("N4.E.A extends 1 ? \"y\" : \"n\""),
        yes("N4.E.B extends 1 ? \"y\" : \"n\""),
        yes("N4.E.C extends 2 ? \"y\" : \"n\""),
        yes("Outer.Inner.E.A extends 6 ? \"y\" : \"n\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A namespace's non-exported member is no member of the namespace read
/// from outside it: `typeof N4.k` and `typeof N4.H` name nothing (TS2339),
/// while the exported `typeof N4.e` is `2`.
///
/// Measured on TypeScript 7.0.2, alike under every setting: `typeof N4.e`
/// is `2`, `typeof N4.k` and `typeof N4.H` are the error type (`any`).
///
/// Mutation: reading a qualified member without the export check answers
/// `typeof N4.k` a clean `1` — a member the checker says does not exist.
#[test]
fn a_namespaces_non_exported_member_is_not_read_from_outside() {
    let matrix = Matrix::new(NAMESPACE_MEMBERS);
    let mut failures = matrix.types(&[("typeof N4.e", "2")]);
    let rows: Vec<(Read<'_>, Vec<&str>)> = ["typeof N4.k", "typeof N4.H"]
        .into_iter()
        .map(|probe| (Read::Type(probe), vec!["any"; 4]))
        .collect();
    for ((read, _), verdicts) in rows.iter().zip(matrix.verdicts(&rows)) {
        let Read::Type(probe) = read else {
            unreachable!("type rows")
        };
        failures.extend(
            verdicts
                .into_iter()
                .filter(|verdict| !verdict.matched && verdict.class == "WRONG-CLEAN")
                .map(|verdict| format!("{probe}: {}", verdict.lane)),
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `blocks` script files each declaring one block of the global enum
/// `BenchFlag`: `F0 = 1`, then `F<i> = BenchFlag.F<i-1> * 2 + i % 3`, with
/// the values TypeScript 7.0.2 gives those members.
fn bench_flag_blocks(blocks: usize) -> (Vec<(String, String)>, Vec<f64>) {
    let mut files = Vec::with_capacity(blocks);
    let mut values = Vec::with_capacity(blocks);
    for index in 0..blocks {
        let (initializer, value) = if index == 0 {
            ("1".to_owned(), 1.0)
        } else {
            let carry = index % 3;
            (
                format!("BenchFlag.F{} * 2 + {carry}", index - 1),
                values[index - 1] * 2.0 + carry as f64,
            )
        };
        files.push((
            format!("enum-{index}.ts"),
            format!("enum BenchFlag {{ F{index} = {initializer} }}\n"),
        ));
        values.push(value);
    }
    (files, values)
}

/// The template-literal print of a union of number values: `"1" | "3"`.
fn template_print(values: &[f64]) -> String {
    values
        .iter()
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// A global enum declared in one block per script file is ONE enum whose
/// members are every block's, each block's member computed from the
/// previous block's: `` `${BenchFlag}` `` is the template of every member
/// value, and `BenchFlag.F<i>` each member's own value.
///
/// Measured on TypeScript 7.0.2 (`tsc --ignoreConfig --strict` over the
/// block files in name order, reading `` `${BenchFlag}` `` and
/// `BenchFlag.F<last>` off a TS2322 against `never`): at 10 blocks `"1" |
/// "1096" | "136" | "16" | "273" | "3" | "33" | "548" | "68" | "8"`; at 25
/// blocks the 25 values up to `"35951176"`; at 50 blocks the 50 values up
/// to `"1206321328759953"`, which a double holds exactly; the last member
/// prints as itself.
///
/// Mutation: reading the enum's first block alone answers `"1"`.
#[test]
fn a_global_enum_merged_across_files_carries_every_members_value() {
    for blocks in [10, 25, 50] {
        let (files, values) = bench_flag_blocks(blocks);
        let tsc_last = match blocks {
            10 => 1096.0,
            25 => 35_951_176.0,
            _ => 1_206_321_328_759_953.0,
        };
        assert_eq!(values[blocks - 1], tsc_last, "the recurrence tsc measured");
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, source)| (name.as_str(), source.as_str()))
            .collect();
        let expected = template_print(&values);
        let last = format!("BenchFlag.F{}", blocks - 1);
        let extends_value = format!("{last} extends {} ? \"y\" : \"n\"", values[blocks - 1]);
        let rows = [
            ("`${BenchFlag}`", expected.as_str()),
            (last.as_str(), last.as_str()),
            (extends_value.as_str(), "\"y\""),
        ];
        let project = ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        };
        let failures = mismatches_in(project, "export {};\n", &rows);
        assert!(
            failures.is_empty(),
            "{blocks} blocks:\n{}",
            failures.join("\n")
        );
    }
}

/// Assembling a global enum merged across files, and looking up which
/// block declares each member an initializer reads, grows with the number
/// of blocks: going from 20 to 40 blocks adds twice the work going from
/// 10 to 20 adds.
///
/// Mutation: finding a member's block by walking, and counting, the blocks
/// before it makes the work added from 20 to 40 blocks about four times
/// that added from 10 to 20.
#[test]
fn assembling_a_merged_enum_grows_linearly_with_its_blocks() {
    let work = |blocks: usize| {
        let (files, values) = bench_flag_blocks(blocks);
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, source)| (name.as_str(), source.as_str()))
            .collect();
        let expected = template_print(&values);
        let before = super::enum_type::enum_assembly_work_for_tests();
        let project = ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        };
        let failures = mismatches_in(
            project,
            "export {};\n",
            &[("`${BenchFlag}`", expected.as_str())],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        super::enum_type::enum_assembly_work_for_tests() - before
    };
    let (quarter, half, full) = (work(10), work(20), work(40));
    assert!(quarter > 0, "the enum is assembled");
    assert_eq!(
        full - half,
        2 * (half - quarter),
        "assembly work over 10, 20 and 40 blocks: {quarter}, {half}, {full}"
    );
}

// The three merged-enum fixture files live in `crate::for_tests` so the
// `verter_lsp` real-provider diagnostics lane proves the SAME program this
// engine lane does (see the consts' doc comment for the measured tsc codes).
use crate::for_tests::{MERGED_A, MERGED_B, MERGED_C};

/// Every block of a global enum merged across files evaluates as the
/// checker evaluates it: auto-increment continues after a constant member
/// computed from another block (`C` is 7), a string member concatenates
/// another block's, a `const enum` merges like any other, a namespace
/// merged with the enum adds no member but its exported value is a property
/// of the enum object, and a member initialized with `Math.random()` is
/// computed — its template is `string`.
///
/// Measured on TypeScript 7.0.2 (`tsc --ignoreConfig --strict` over
/// `a.ts b.ts c.ts`, each read off a TS2322 against `never`):
/// `` `${AutoInc}` `` is `"3" | "6" | "7"`, `` `${Str}` `` `"a" | "ab" |
/// "abc"`, `` `${CE}` `` `"1" | "2" | "3"`, `` `${NE}` `` `"1" | "2"`,
/// `` `${Rand}` `` `string`, `AutoInc.C` `AutoInc.C`, `Rand` `Rand`,
/// `Rand.F` `Rand.F`, `1 extends Rand.F`, `AutoInc.C extends 7` and
/// `CE.C extends 3` `"y"`, `keyof typeof NE` `"A" | "B" | "x"`, `typeof
/// NE.x` `2`. These
/// declarations raise no diagnostic. Two blocks omitting their first
/// initializer raise TS2432 and still number from `0` (`` `${Two}` `` is
/// `"0" | "1"`); a bare member after `Math.random()` raises TS1061 and is
/// computed (`1 extends Bad.S` is `"y"`, `` `${Bad}` `` `string`).
///
/// Mutation: reading each enum's first block alone answers `` `${AutoInc}`
/// `` `"3"` and `AutoInc.C` an unresolved member; an enum object of its
/// members alone answers `keyof typeof NE` `"A" | "B"`.
#[test]
fn global_enum_blocks_evaluate_as_the_checker_evaluates_them() {
    let yes = |probe: &'static str| (probe, "\"y\"");
    let files = [("a.ts", MERGED_A), ("b.ts", MERGED_B), ("c.ts", MERGED_C)];
    assert_rows(
        &files,
        "export {};\n",
        &[
            ("`${AutoInc}`", "\"3\" | \"6\" | \"7\""),
            ("`${Str}`", "\"a\" | \"ab\" | \"abc\""),
            ("`${CE}`", "\"1\" | \"2\" | \"3\""),
            ("`${NE}`", "\"1\" | \"2\""),
            ("keyof typeof NE", "\"A\" | \"B\" | \"x\""),
            ("typeof NE.x", "2"),
            ("`${Rand}`", "string"),
            ("AutoInc.C", "AutoInc.C"),
            ("Rand", "Rand"),
            ("Rand.F", "Rand.F"),
            yes("1 extends Rand.F ? \"y\" : \"n\""),
            yes("AutoInc.C extends 7 ? \"y\" : \"n\""),
            yes("CE.C extends 3 ? \"y\" : \"n\""),
            ("`${Two}`", "\"0\" | \"1\""),
            yes("1 extends Bad.S ? \"y\" : \"n\""),
            ("`${Bad}`", "string"),
        ],
    );
}

/// Editing one block of a global enum merged across files changes the
/// enum exactly as a cold build of the edited program answers it.
///
/// Measured on TypeScript 7.0.2: rewriting `enum-3.ts` of the 10-block
/// program to `F3 = BenchFlag.F2 * 3` makes `F3` `24` and every later
/// member follow it — `` `${BenchFlag}` `` is `"1" | "3" | "8" | "24" |
/// "49" | "100" | "200" | "401" | "804" | "1608"`.
///
/// Mutation: a warm read keeping the enum's pre-edit members answers the
/// unedited union.
#[test]
fn editing_one_block_of_a_merged_enum_matches_a_cold_build() {
    use super::checker_probe_lane_tests::{probe_host, with_probe_on_host};
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
    let (files, before) = bench_flag_blocks(10);
    let edited_source = "enum BenchFlag { F3 = BenchFlag.F2 * 3 }\n";
    let mut after = before.clone();
    after[3] = after[2] * 3.0;
    for index in 4..after.len() {
        after[index] = after[index - 1] * 2.0 + (index % 3) as f64;
    }
    assert_eq!(after[9], 1608.0, "the edited recurrence tsc measured");
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, source)| (name.as_str(), source.as_str()))
        .collect();
    let edited: Vec<(&str, &str)> = borrowed
        .iter()
        .map(|&(name, source)| {
            (
                name,
                if name == "enum-3.ts" {
                    edited_source
                } else {
                    source
                },
            )
        })
        .collect();
    let project = ProbeProject {
        files: &borrowed,
        compiler_options: None,
        ambient_lib: None,
    };
    let edited_project = ProbeProject {
        files: &edited,
        ..project
    };
    // The template's print, and whether it is the checker's `expected`.
    let read =
        |host: &std::sync::Arc<crate::VerterHost>, project: ProbeProject<'_>, expected: &[f64]| {
            let expected = checker_syntax::parse(&template_print(expected))
                .expect("a union of string literals parses");
            with_probe_on_host(
                host,
                project,
                "export {};\n",
                "`${BenchFlag}`",
                |dispatch, node| {
                    (
                        render_node(dispatch, node, 0),
                        checker_syntax::matches_node(dispatch, node, &expected, 0),
                    )
                },
            )
        };
    let warm = probe_host(project);
    let (unedited, matched) = read(&warm, project, &before);
    assert!(matched, "before the edit: {unedited}");
    crate::u6_flow_shape_corpus_tests::upsert(
        &warm,
        "/wb/enum-3.ts",
        edited_source,
        crate::FileLanguage::script_ts(),
    );
    let (cold, matched) = read(&probe_host(edited_project), edited_project, &after);
    assert!(matched, "a cold build of the edited program: {cold}");
    let (warm_after, _) = read(&warm, edited_project, &after);
    assert_eq!(
        warm_after, cold,
        "the edited warm host answers as a cold build"
    );
    assert_ne!(warm_after, unedited, "the edit changes the answer");
}

/// A global enum is merged wherever its blocks are declared: the enum a
/// script's namespace exports (`namespace N { export enum E {} }` in two
/// scripts is one `N.E`), a module's `declare global` block of a script's
/// enum, and a module's `declare global { namespace N { enum E {} } }`
/// block of a script's `N.E`, whichever file declares first; a module's
/// `declare module "./a"` block merges into `./a`'s enum; a module's own
/// enum and its `declare global` block of the same name stay two enums; an
/// enum declared with a namespace in one file carries the namespace's
/// exported values.
///
/// Measured on TypeScript 7.0.2 (`tsc --ignoreConfig --strict` over the
/// fixture files, each read off a TS2322 against `never`; the fixtures
/// raise no other diagnostic): over the two namespace blocks
/// `` `${N.E}` `` is `"1" | "2" | "3"`, `keyof typeof N.E` `"A" | "B" |
/// "C"` and `N.E.C extends 3` `"y"`; over the script block and the
/// `declare global` block, in either file order, `` `${E}` `` is `"1" |
/// "2" | "6"`, `keyof typeof E` `"A" | "B" | "C"`, `E.B` `E.B` and `E.C
/// extends 6` `"y"`; over the script namespace and the `declare global`
/// namespace, in either file order, `` `${N.E}` `` is `"1" | "2"`, `keyof
/// typeof N.E` `"A" | "B"` and `N.E.B extends 2` `"y"`; over `./a`'s enum
/// and its `declare module` block, `` `${E}` `` is `"1" | "2"`, `keyof
/// typeof E` `"A" | "B"` and `E.B extends 2` `"y"`; over a script's `E`
/// and a module declaring its own `E { X = 9 }` beside a `declare global`
/// block, the global `` `${E}` `` is `"1" | "2"` and `keyof typeof E`
/// `"A" | "B"` while the module's
/// `` `${ME}` `` is `"9"` and `keyof typeof ME` `"X"`; over one file
/// declaring `NE` and its namespace, `keyof typeof NE` is `"A" | "f" |
/// "x"` and `typeof NE.x` `2`.
///
/// Mutation: merging only a script's top-level enum answers `` `${N.E}`
/// `` `"1"` and `` `${E}` `` `"1"`; leaving a `declare global` enum
/// unindexed leaves the module-first `E` unresolved; dropping a block
/// nested in a `declare global` namespace or a `declare module` block
/// answers `"1"`; reading the global block as the module's own `E`
/// answers `` `${ME}` `` `"2" | "9"`; an enum object of its members alone
/// answers `keyof typeof NE` `"A"`.
#[test]
fn a_global_enum_merges_every_block_and_its_namespace() {
    let mut failures = Vec::new();
    let namespace_blocks = [
        ("a.ts", "namespace N { export enum E { A = 1 } }\n"),
        (
            "b.ts",
            "namespace N { export enum E { B = N.E.A + 1, C } }\n",
        ),
    ];
    failures.extend(mismatches_in(
        enum_project(&namespace_blocks),
        "export {};\n",
        &[
            ("`${N.E}`", "\"1\" | \"2\" | \"3\""),
            ("keyof typeof N.E", "\"A\" | \"B\" | \"C\""),
            ("N.E.C extends 3 ? \"y\" : \"n\"", "\"y\""),
        ],
    ));
    let script = "enum E { A = 1 }\n";
    let augmentation = "export {};\ndeclare global { enum E { B = 2, C = E.A + 5 } }\n";
    for files in [
        [("a.ts", script), ("m.ts", augmentation)],
        [("g.ts", augmentation), ("z.ts", script)],
    ] {
        failures.extend(mismatches_in(
            enum_project(&files),
            "export {};\n",
            &[
                ("`${E}`", "\"1\" | \"2\" | \"6\""),
                ("keyof typeof E", "\"A\" | \"B\" | \"C\""),
                ("E.B", "E.B"),
                ("E.C extends 6 ? \"y\" : \"n\"", "\"y\""),
            ],
        ));
    }
    let namespace_script = "namespace N { export enum E { A = 1 } }\n";
    let namespace_augmentation =
        "export {};\ndeclare global { namespace N { enum E { B = 2 } } }\n";
    for files in [
        [("a.ts", namespace_script), ("m.ts", namespace_augmentation)],
        [("g.ts", namespace_augmentation), ("z.ts", namespace_script)],
    ] {
        failures.extend(mismatches_in(
            enum_project(&files),
            "export {};\n",
            &[
                ("`${N.E}`", "\"1\" | \"2\""),
                ("keyof typeof N.E", "\"A\" | \"B\""),
                ("N.E.B extends 2 ? \"y\" : \"n\"", "\"y\""),
            ],
        ));
    }
    let module_augmentation = [
        ("a.ts", "export enum E { A = 1 }\n"),
        (
            "m.ts",
            "import \"./a\";\ndeclare module \"./a\" { enum E { B = 2 } }\n",
        ),
    ];
    failures.extend(mismatches_in(
        enum_project(&module_augmentation),
        "import { E } from './a';\n",
        &[
            ("`${E}`", "\"1\" | \"2\""),
            ("keyof typeof E", "\"A\" | \"B\""),
            ("E.B extends 2 ? \"y\" : \"n\"", "\"y\""),
        ],
    ));
    let shadowing = [
        ("a.ts", "enum E { A = 1 }\n"),
        (
            "m.ts",
            "export enum E { X = 9 }\ndeclare global { enum E { B = 2 } }\n",
        ),
    ];
    failures.extend(mismatches_in(
        enum_project(&shadowing),
        "import { E as ME } from './m';\n",
        &[
            ("`${E}`", "\"1\" | \"2\""),
            ("keyof typeof E", "\"A\" | \"B\""),
            ("`${ME}`", "\"9\""),
            ("keyof typeof ME", "\"X\""),
        ],
    ));
    let with_namespace = "enum NE { A = 1 }\n\
        namespace NE { export const x = 2; export function f() { return 1 as const } }\n";
    failures.extend(mismatches_in(
        enum_project(&[("a.ts", with_namespace)]),
        "export {};\n",
        &[
            ("keyof typeof NE", "\"A\" | \"f\" | \"x\""),
            ("typeof NE.x", "2"),
        ],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A block entering or leaving a merged enum changes the enum on a warm
/// host exactly as a cold build of the edited program answers it: a
/// script's namespace starting, then stopping, to export its `E`, and a
/// module's `declare module "./a"` block of `./a`'s enum appearing, having
/// its member's value edited and disappearing. An edit to a file declaring
/// no block of the enum keeps the warm answer, re-evaluating no
/// initializer.
///
/// Measured on TypeScript 7.0.2 (`tsc --ignoreConfig --strict`, read off a
/// TS2322 against `never`; no other diagnostic): `` `${N.E}` `` is `"1"`
/// while `b.ts`'s `namespace N { enum E { B = N.E.A + 1 } }` keeps its
/// enum private and `"1" | "2"` once it exports it; `./a`'s `` `${E}` `` is
/// `"1"` without the augmentation, `"1" | "2"` with `declare module "./a" {
/// enum E { B = 2 } }` and `"1" | "3"` once `B = 3`.
///
/// Mutation: discovering a dotted enum's blocks without observing the
/// exported membership of `N.E` leaves the warm `` `${N.E}` `` at `"1"`
/// after `b.ts` exports its enum; observing the augmentation index only
/// when a block contributes leaves the warm `` `${E}` `` at `"1"` after
/// the augmentation appears.
#[test]
fn an_enum_block_entering_or_leaving_its_enum_matches_a_cold_build() {
    use super::checker_probe_lane_tests::{probe_host, with_probe_on_host};
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
    // The probe's print, and whether it is the checker's `checker`.
    let read = |host: &std::sync::Arc<crate::VerterHost>,
                project: ProbeProject<'_>,
                source: &str,
                probe: &str,
                checker: &str| {
        let expected = checker_syntax::parse(checker).expect("a union of string literals parses");
        with_probe_on_host(host, project, source, probe, |dispatch, node| {
            (
                render_node(dispatch, node, 0),
                checker_syntax::matches_node(dispatch, node, &expected, 0),
            )
        })
    };
    let mut failures = Vec::new();
    // Each edit of `path` to `edited`, in turn, on one warm host, against a
    // cold build of the edited program and the checker's answer.
    let mut walk = |files: &[(&'static str, &'static str)],
                    source: &str,
                    probe: &str,
                    path: &'static str,
                    edits: &[(&'static str, &'static str)],
                    initial: &str| {
        let warm = probe_host(enum_project(files));
        let mut program: Vec<(&str, &str)> = files.to_vec();
        let (first, matched) = read(&warm, enum_project(&program), source, probe, initial);
        if !matched {
            failures.push(format!("`{probe}` before any edit: {first}"));
        }
        for (edited, checker) in edits {
            for entry in program.iter_mut().filter(|(name, _)| *name == path) {
                entry.1 = edited;
            }
            crate::u6_flow_shape_corpus_tests::upsert(
                &warm,
                &format!("/wb/{path}"),
                edited,
                crate::FileLanguage::script_ts(),
            );
            let (cold, matched) = read(
                &probe_host(enum_project(&program)),
                enum_project(&program),
                source,
                probe,
                checker,
            );
            let (warm_after, _) = read(&warm, enum_project(&program), source, probe, checker);
            if !matched {
                failures.push(format!(
                    "`{probe}` cold after `{edited}`: the checker answers `{checker}`, \
                     the lane measured `{cold}`"
                ));
            }
            if warm_after != cold {
                failures.push(format!(
                    "`{probe}` warm after `{edited}`: `{warm_after}`, a cold build `{cold}`"
                ));
            }
        }
    };
    walk(
        &[
            ("a.ts", "namespace N { export enum E { A = 1 } }\n"),
            ("b.ts", "namespace N { enum E { B = N.E.A + 1 } }\n"),
        ],
        "export {};\n",
        "`${N.E}`",
        "b.ts",
        &[
            (
                "namespace N { export enum E { B = N.E.A + 1 } }\n",
                "\"1\" | \"2\"",
            ),
            ("namespace N { enum E { B = N.E.A + 1 } }\n", "\"1\""),
        ],
        "\"1\"",
    );
    walk(
        &[
            ("a.ts", "export enum E { A = 1 }\n"),
            ("m.ts", "import \"./a\";\nexport {};\n"),
        ],
        "import { E } from './a';\n",
        "`${E}`",
        "m.ts",
        &[
            (
                "import \"./a\";\ndeclare module \"./a\" { enum E { B = 2 } }\n",
                "\"1\" | \"2\"",
            ),
            (
                "import \"./a\";\ndeclare module \"./a\" { enum E { B = 3 } }\n",
                "\"1\" | \"3\"",
            ),
            ("import \"./a\";\nexport {};\n", "\"1\""),
        ],
        "\"1\"",
    );
    // An edit to a file declaring no block of the enum keeps the warm
    // answer without evaluating an initializer again.
    let files = [
        ("a.ts", "namespace N { export enum E { A = 1 } }\n"),
        ("b.ts", "namespace N { export enum E { B = N.E.A + 1 } }\n"),
        ("u.ts", "const unrelated = 1;\n"),
    ];
    let warm = probe_host(enum_project(&files));
    let both = "\"1\" | \"2\"";
    let (before, matched) = read(
        &warm,
        enum_project(&files),
        "export {};\n",
        "`${N.E}`",
        both,
    );
    if !matched {
        failures.push(format!("`${{N.E}}` over two exported blocks: {before}"));
    }
    crate::u6_flow_shape_corpus_tests::upsert(
        &warm,
        "/wb/u.ts",
        "const unrelated = 2;\n",
        crate::FileLanguage::script_ts(),
    );
    let work = super::enum_type::enum_initializer_work_for_tests();
    let (after, _) = read(
        &warm,
        enum_project(&files),
        "export {};\n",
        "`${N.E}`",
        both,
    );
    let reread_work = super::enum_type::enum_initializer_work_for_tests() - work;
    if after != before {
        failures.push(format!(
            "an unrelated edit moved `${{N.E}}`: {before} -> {after}"
        ));
    }
    if reread_work != 0 {
        failures.push(format!(
            "an unrelated edit re-evaluated {reread_work} initializer steps"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
