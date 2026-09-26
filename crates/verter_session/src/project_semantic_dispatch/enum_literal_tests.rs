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

/// Element access on an enum object: a numeric enum's reverse mapping reads
/// the member's name as `string`, and a string key reads the member.
///
/// Measured on TypeScript 7.0.2: `eRev` (`E[E.A]`) is `string`, `sRev`
/// (`S["P"]`) is `S`; `(typeof E)[0]` and `(typeof E)[number]` are
/// `string`.
#[test]
#[ignore = "element access on an enum object: the numeric reverse mapping and a string-keyed member read"]
fn an_enum_object_element_access_reads_the_reverse_mapping_and_members() {
    assert_rows(
        &[("enums.ts", ENUMS)],
        REVERSE,
        &[
            ("ReturnType<typeof eRev>", "string"),
            ("ReturnType<typeof sRev>", "S"),
            ("(typeof E)[0]", "string"),
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
