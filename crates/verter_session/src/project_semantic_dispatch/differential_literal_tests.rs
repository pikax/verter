//! Differential probes of literal widening and freshness, `as const`
//! and enums: `const` and `let` initializers, returned literals, object
//! and array literals, templates, const assertions over nested, spread and
//! template values, and numeric, string, const, ambient and single-member
//! enums. A row is a function of the fixture answered as its body-derived
//! return, or a type in TYPE position.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --declaration --emitDeclarationOnly
//! --strict --noErrorTruncation` under each `strictNullChecks` ×
//! `noImplicitAny` setting: `declare const p: <probe>; const s: never =
//! p;` (a function row reads `ReturnType<typeof f>`) read off the TS2322
//! message; the four settings agree on every row. An ignored test asserts
//! the measured answer for rows the lane does not yet answer as the checker
//! does; "wrong-but-clean" marks a lane answer published complete and
//! undegraded.

use super::differential_harness_tests::{Matrix, Read};

/// Literals held by `const` and `let`, returned, and nested in object and array
/// literals.
const WIDENING: &str = r##"
declare function cond(): boolean;
export function wConst() { const a = 1; return a; }
export function wConstStr() { const s = "s"; return s; }
export function wConstBool() { const b = true; return b; }
export function wConstBig() { const n = 10n; return n; }
export function wConstNeg() { const n = -1; return n; }
export function wLet() { let a = 1; return a; }
export function wLetUnion() { let a = cond() ? 1 : "s"; return a; }
export function wConstUnion() { const a = cond() ? 1 : "s"; return a; }
export function wReturnLit() { return "lit"; }
export function wReturnTwoLits(c: boolean) { if (c) return "a"; return "b"; }
export function wReturnMixedLits(c: boolean) { if (c) return "a"; return 1; }
export function wObjLit() { return { a: 1, b: "s", c: true }; }
export function wObjConstProp() { const o = { a: 1 }; return o.a; }
export function wArrLit() { return [1, 2]; }
export function wArrMixed() { return [1, "a", true]; }
export function wNested() { return { a: [{ b: "x" }] }; }
export function wConstInObj() { const k = "k"; return { k }; }
export function wConstInArr() { const k = "k"; return [k]; }
export function wTernaryLit(c: boolean) { return c ? 1 : 2; }
export function wTemplate() { return `a${1}`; }
export function wTemplateConst() { const t = `a${"b"}`; return t; }
export function wNegate() { return -1; }
export function wParenLit() { return (("p")); }
export function wTypeofLit() { const x = "a"; return typeof x; }
export function wNumericSep() { return 1_000; }
export function wLitMethod() { return "abc" as "abc"; }
"##;

/// A fresh literal widens where the checker widens it: a `let`, a returned
/// literal alone, an object or array literal's members; a `const` keeps the
/// literal (a union of literals too) but its read returned alone widens;
/// several returned literals of one kind stay a union; an assertion to a
/// literal type does not widen.
#[test]
fn literals_widen_as_the_checker_widens_them() {
    let matrix = Matrix::new(WIDENING);
    let failures = matrix.returns(&[
        ("wConst", "number"),
        ("wConstStr", "string"),
        ("wConstBool", "boolean"),
        ("wConstBig", "bigint"),
        ("wConstNeg", "number"),
        ("wLet", "number"),
        ("wLetUnion", "string | number"),
        ("wConstUnion", "\"s\" | 1"),
        ("wReturnLit", "string"),
        ("wReturnTwoLits", "\"a\" | \"b\""),
        ("wReturnMixedLits", "\"a\" | 1"),
        ("wObjLit", "{ a: number; b: string; c: boolean; }"),
        ("wObjConstProp", "number"),
        ("wArrLit", "number[]"),
        ("wArrMixed", "(string | number | boolean)[]"),
        ("wNested", "{ a: { b: string; }[]; }"),
        ("wConstInObj", "{ k: string; }"),
        ("wConstInArr", "string[]"),
        ("wTernaryLit", "1 | 2"),
        ("wTemplate", "string"),
        ("wTemplateConst", "string"),
        ("wNegate", "number"),
        ("wParenLit", "string"),
        ("wNumericSep", "number"),
        ("wLitMethod", "\"abc\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `typeof x` in expression position is `"bigint" | "boolean" | "function" |
/// "number" | "object" | "string" | "symbol" | "undefined"`.
///
/// What the lane gives:
/// - `wTypeofLit`: the checker answers `"bigint" | "boolean" | "function" |
///   "number" | "object" | "string" | "symbol" | "undefined"`; the lane
///   measured `<opaque UnmodeledPosition>` degraded by
///   FlowGap(UnmodeledExpression).
#[test]
#[ignore = "a typeof expression is the union of the typeof result strings"]
fn a_typeof_expression_is_the_typeof_result_union() {
    let matrix = Matrix::new(WIDENING);
    let failures = matrix.returns(&[
        ("wTypeofLit", "\"bigint\" | \"boolean\" | \"function\" | \"number\" | \"object\" | \"string\" | \"symbol\" | \"undefined\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Const assertions over scalars, objects, arrays, templates and spreads.
const AS_CONST: &str = r##"
export function cStr() { return "a" as const; }
export function cObj() { return { a: 1, b: { c: "x" } } as const; }
export function cArr() { return [1, "a"] as const; }
export function cNestedArr() { return [[1], [2]] as const; }
export function cLocal() { const o = { a: 1 } as const; return o; }
export function cLocalProp() { const o = { a: 1 } as const; return o.a; }
export function cTemplate() { return `x${1}` as const; }
export function cSpread() { const base = { a: 1 } as const; return { ...base, b: 2 }; }
export function cSpreadConst() { const base = { a: 1 } as const; return { ...base, b: 2 } as const; }
export function cArrSpread() { const t = [1, 2] as const; return [...t, 3]; }
export function cArrSpreadConst() { const t = [1, 2] as const; return [...t, 3] as const; }
export function cBool() { return true as const; }
export function cNeg() { return -1 as const; }
export function cParen() { return ({ a: 1 }) as const; }
export function cEmptyArr() { return [] as const; }
export function cSatisfiesConst() { return { a: 1 } as const satisfies { a: number }; }
export function cReadonlyIndex() { const t = ["x", "y"] as const; return t[0]; }
export function cReadonlyLen() { const t = ["x", "y"] as const; return t.length; }
"##;

/// `as const` makes literals non-widening and objects and arrays deeply
/// readonly (arrays as readonly tuples), through a local, parentheses and
/// `satisfies`; an array spread of a const tuple into a non-const array literal
/// widens, into a const one it stays a tuple; element and `length` reads of a
/// const tuple are literals.
#[test]
fn const_assertions_apply_as_the_checker_applies_them() {
    let matrix = Matrix::new(AS_CONST);
    let failures = matrix.returns(&[
        ("cStr", "\"a\""),
        (
            "cObj",
            "{ readonly a: 1; readonly b: { readonly c: \"x\"; }; }",
        ),
        ("cArr", "readonly [1, \"a\"]"),
        ("cNestedArr", "readonly [readonly [1], readonly [2]]"),
        ("cLocal", "{ readonly a: 1; }"),
        ("cLocalProp", "1"),
        ("cArrSpread", "number[]"),
        ("cArrSpreadConst", "readonly [1, 2, 3]"),
        ("cBool", "true"),
        ("cNeg", "-1"),
        ("cParen", "{ readonly a: 1; }"),
        ("cEmptyArr", "readonly []"),
        ("cSatisfiesConst", "{ readonly a: 1; }"),
        ("cReadonlyIndex", "\"x\""),
        ("cReadonlyLen", "2"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `\`x${1}\` as const` is `"x1"`. Wrong-but-clean: the lane answers `string`.
///
/// What the lane gives:
/// - `cTemplate`: the checker answers `"x1"`; the lane measured `string`.
#[test]
#[ignore = "a template literal expression under as const is its literal string type"]
fn wrong_clean_a_const_template_is_its_literal() {
    let matrix = Matrix::new(AS_CONST);
    let failures = matrix.returns(&[("cTemplate", "\"x1\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `{ ...base, b: 2 }` over `const base = { a: 1 } as const` is `{ a: 1; b:
/// number; }` (`{ readonly a: 1; readonly b: 2; }` under `as const`); the lane
/// publishes an unevaluated spread program.
///
/// What the lane gives:
/// - `cSpread`: the checker answers `{ a: 1; b: number; }`; the lane measured
///   `<unrendered ObjectSpreadProgram(ObjectSpreadProgram { effects:
///   [Spread(S>`.
/// - `cSpreadConst`: the checker answers `{ readonly a: 1; readonly b: 2; }`;
///   the lane measured `<unrendered ObjectSpreadProgram(ObjectSpreadProgram {
///   effects: [Spread(S>`.
#[test]
#[ignore = "an object literal spreading a const object is the merged object type"]
fn an_object_spread_of_a_const_object_is_its_merged_object() {
    let matrix = Matrix::new(AS_CONST);
    let failures = matrix.returns(&[
        ("cSpread", "{ a: 1; b: number; }"),
        ("cSpreadConst", "{ readonly a: 1; readonly b: 2; }"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Numeric, string, const, ambient and single-member enums.
const ENUMS: &str = r##"
enum Color { Red, Green = 5, Blue }
enum Dir { Up = "UP", Down = "DOWN" }
const enum Flag { A = 1 << 0, B = 1 << 1, AB = A | B }
declare enum Amb { P, Q }
enum Single { Only }
export function eMember() { return Color.Red; }
export function eMemberLet() { let c = Color.Green; return c; }
export function eMemberConst() { const c = Color.Green; return c; }
export function eBlue() { return Color.Blue; }
export function eStr() { return Dir.Up; }
export function eStrLet() { let d = Dir.Down; return d; }
export function eFlag() { return Flag.AB; }
export function eSingle() { return Single.Only; }
export function eSingleLet() { let s = Single.Only; return s; }
export function eAmbient() { return Amb.Q; }
export function eCompare(c: Color) { if (c === Color.Red) return c; throw 0; }
export function eCompareElse(c: Color) { if (c === Color.Red) throw 0; return c; }
export function eSwitch(d: Dir) { switch (d) { case Dir.Up: return d; default: return d; } }
export function eObj() { return Color; }
export function eKeyof() { let k: keyof typeof Color = "Red"; return k; }
export function eArr() { return [Color.Red, Color.Blue]; }
export function eTernary(c: boolean) { return c ? Dir.Up : Dir.Down; }
"##;

/// An enum member read returns its enum type when returned alone (the member
/// type from a `let` or `const`), a comparison narrows to the member, a
/// template over a member is its value, `keyof typeof` lists member names, and
/// member unions reduce to the enum.
#[test]
fn enum_values_and_types_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(ENUMS);
    let failures = matrix.same(&[
        (Read::Return("eMember"), "Color"),
        (Read::Return("eMemberLet"), "Color.Green"),
        (Read::Return("eMemberConst"), "Color"),
        (Read::Return("eBlue"), "Color"),
        (Read::Return("eStr"), "Dir"),
        (Read::Return("eStrLet"), "Dir.Down"),
        (Read::Return("eFlag"), "Flag"),
        (Read::Return("eSingle"), "Single"),
        (Read::Return("eSingleLet"), "Single"),
        (Read::Return("eAmbient"), "Amb"),
        (Read::Return("eCompare"), "Color.Red"),
        (Read::Return("eCompareElse"), "Color.Green | Color.Blue"),
        (Read::Return("eSwitch"), "Dir"),
        (Read::Return("eKeyof"), "\"Red\""),
        (Read::Return("eArr"), "Color[]"),
        (Read::Return("eTernary"), "Dir"),
        (Read::Type("Color"), "Color"),
        (Read::Type("Color.Green"), "Color.Green"),
        (Read::Type("`${Color.Blue}`"), "\"6\""),
        (Read::Type("`${Dir.Up}`"), "\"UP\""),
        (
            Read::Type("keyof typeof Color"),
            "\"Blue\" | \"Green\" | \"Red\"",
        ),
        (Read::Type("keyof typeof Dir"), "\"Down\" | \"Up\""),
        (Read::Type("(typeof Dir)['Up']"), "Dir.Up"),
        (Read::Type("Flag.AB"), "Flag.AB"),
        (Read::Type("[Color.Red] extends [0] ? 1 : 2"), "1"),
        (Read::Type("[Dir.Up] extends ['UP'] ? 1 : 2"), "1"),
        (Read::Type("['UP'] extends [Dir.Up] ? 1 : 2"), "2"),
        (Read::Type("Color.Red | Color.Green | Color.Blue"), "Color"),
        (
            Read::Type("Exclude<Color, Color.Red>"),
            "Color.Green | Color.Blue",
        ),
        (Read::Type("Extract<Dir, Dir.Down>"), "Dir.Down"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `return Color;` is `typeof Color`, whose numeric enum object also carries
/// the reverse mapping `[x: number]: string`. Wrong-but-clean: the lane answers
/// the members-only object `{ readonly Red: Color.Red; readonly Green:
/// Color.Green; readonly Blue: Color.Blue; }`.
///
/// What the lane gives:
/// - `eObj`: the checker answers `typeof Color`; the lane measured `{ readonly
///   Red: Color.Red; readonly Green: Color.Green; readonly Blue: Color.Blue;
///   }`.
#[test]
#[ignore = "an enum object value is typeof the enum, with its reverse mapping"]
fn wrong_clean_a_returned_enum_object_is_typeof_the_enum() {
    let matrix = Matrix::new(ENUMS);
    let failures = matrix.returns(&[("eObj", "typeof Color")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
