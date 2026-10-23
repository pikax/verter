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
export function dpArrow(cb = () => 7) { return cb; }
export function dpArrowConst(cb = () => 7 as const) { return cb; }
export function dpArrowStr(cb = () => "s") { return cb; }
export function dpArrowNeg(cb = () => -1) { return cb; }
export function dpArrowUnion(cb = () => (cond() ? 1 : 2)) { return cb; }
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
export function wTypeofLet(x: number) { let t = typeof x; return t; }
export function wTypeofMember(o: { p: 1 }) { return { k: typeof o.p }; }
export function wTypeofThis(this: { a: 1 }) { return typeof this.a; }
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
/// "number" | "object" | "string" | "symbol" | "undefined"`, a regular literal
/// union no position widens: a `let`, an object member, a read through
/// `this`.
#[test]
fn a_typeof_expression_is_the_typeof_result_union() {
    const TYPEOF: &str = "\"bigint\" | \"boolean\" | \"function\" | \"number\" | \"object\" | \"string\" | \"symbol\" | \"undefined\"";
    let matrix = Matrix::new(WIDENING);
    let member = format!("{{ k: {TYPEOF}; }}");
    let failures = matrix.returns(&[
        ("wTypeofLit", TYPEOF),
        ("wTypeofLet", TYPEOF),
        ("wTypeofMember", member.as_str()),
        ("wTypeofThis", TYPEOF),
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
export function cTemplateMixed() { return `${true}-${"z"}` as const; }
export function cTemplateBranches(b: boolean) { return `x${b ? 1 : 2}` as const; }
export function cTemplateNumber(n: number) { return `a${n}` as const; }
export function cTemplateLocal() { const s = "q"; return `a${s}` as const; }
export function cTemplateMember() { return { k: `v${2}` } as const; }
export function cTemplateBool(b: boolean) { return `x${b}` as const; }
export function cTemplateUnion(v: "a" | "b") { return `x-${v}` as const; }
export function cTemplateLocalMember() { const n = 1; return { k: `v${n}` } as const; }
export function cTemplateElement(s: string) { return [`p${s}`] as const; }
export function cSpread() { const base = { a: 1 } as const; return { ...base, b: 2 }; }
export function cSpreadConst() { const base = { a: 1 } as const; return { ...base, b: 2 } as const; }
export function cSpreadParam(o: { a: string }) { return { ...o, b: 1 }; }
export function cSpreadOverride(o: { a: string }) { return { ...o, a: 1 }; }
export function cSpreadOptional(o: { a?: string }) { return { ...o }; }
export function cSpreadConstParam(o: { a: string; c?: number }) { return { ...o, b: 2 } as const; }
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

/// A template literal expression under `as const` is the template literal type
/// of its holes' literal types: `\`x${1}\` as const` is `"x1"`, and a
/// conditional hole distributes (TypeScript 7.0.2, all four settings alike).
#[test]
fn a_const_template_infers_as_the_checker_infers_it() {
    let matrix = Matrix::new(AS_CONST);
    let failures = matrix.returns(&[
        ("cTemplate", "\"x1\""),
        ("cTemplateMixed", "\"true-z\""),
        ("cTemplateBranches", "\"x1\" | \"x2\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A const template whose hole reads a value is the template literal type of
/// that value's type, and a template member or element of an `as const`
/// literal is its literal: ``a${n}` as const` over `n: number` is
/// ``a${number}``, over `const s = "q"` it is `"aq"`, a `boolean` or union
/// hole distributes, and `{ k: `v${2}` } as const` is `{ readonly k: "v2";
/// }` (TypeScript 7.0.2, all four settings alike).
#[test]
fn a_const_template_reads_its_value_holes_as_the_checker_reads_them() {
    let matrix = Matrix::new(AS_CONST);
    let failures = matrix.returns(&[
        ("cTemplateNumber", "`a${number}`"),
        ("cTemplateLocal", "\"aq\""),
        ("cTemplateMember", "{ readonly k: \"v2\"; }"),
        ("cTemplateBool", "\"xfalse\" | \"xtrue\""),
        ("cTemplateUnion", "\"x-a\" | \"x-b\""),
        ("cTemplateLocalMember", "{ readonly k: \"v1\"; }"),
        ("cTemplateElement", "readonly [`p${string}`]"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `{ ...base, b: 2 }` over `const base = { a: 1 } as const` is `{ a: 1; b:
/// number; }`: a spread copies its source's properties without their
/// `readonly`, a later property overrides an earlier one, and an optional
/// property stays optional.
#[test]
fn an_object_spread_of_a_const_object_is_its_merged_object() {
    let matrix = Matrix::new(AS_CONST);
    let failures = matrix.returns(&[
        ("cSpread", "{ a: 1; b: number; }"),
        ("cSpreadParam", "{ a: string; b: number; }"),
        ("cSpreadOverride", "{ a: number; }"),
        ("cSpreadOptional", "{ a?: string; }"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `{ ...base, b: 2 } as const` is `{ readonly a: 1; readonly b: 2; }`: a
/// spread in a const context copies its source's properties `readonly`, an
/// optional one staying optional.
#[test]
fn a_const_spread_copies_its_properties_readonly() {
    let matrix = Matrix::new(AS_CONST);
    let failures = matrix.returns(&[
        ("cSpreadConst", "{ readonly a: 1; readonly b: 2; }"),
        (
            "cSpreadConstParam",
            "{ readonly a: string; readonly c?: number; readonly b: 2; }",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Spreads of sources declaring methods and accessors.
const METHOD_SPREADS: &str = r##"
declare const o: { a: 1; m(): void };
declare const q: { get g(): 1; readonly h: 2 };
export function sMethodConst() { return { ...o } as const; }
export function sMethod() { return { ...o }; }
export function sAccessorConst() { return { ...q } as const; }
const s = { get g() { return 1 as const; }, h: 2 as const };
export function sGetterConst() { return { ...s } as const; }
"##;

/// A const-context spread copies a method as a readonly property of its
/// function type, `{ readonly a: 1; readonly m: () => void; }`. Measured
/// on TypeScript 7.0.2, alike under all four settings.
#[test]
fn a_const_spread_copies_a_method_as_a_readonly_property() {
    let matrix = Matrix::new(METHOD_SPREADS);
    let failures =
        matrix.returns(&[("sMethodConst", "{ readonly a: 1; readonly m: () => void; }")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A plain spread keeps a method a method, `{ a: 1; m(): void; }`
/// (measured on TypeScript 7.0.2, alike under all four settings).
///
/// What the lane gives: `{ a: 1; m: () => void; }`, clean.
#[test]
#[ignore = "a spread keeps a copied method a method"]
fn wrong_clean_a_spread_keeps_a_method() {
    let matrix = Matrix::new(METHOD_SPREADS);
    let failures = matrix.returns(&[("sMethod", "{ a: 1; m(): void; }")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A const-context spread copies a getter as a readonly property of its
/// type, `{ readonly g: 1; readonly h: 2; }` (measured on TypeScript 7.0.2,
/// alike under all four settings).
///
/// What the lane gives: `{ readonly g: () => 1; readonly h: 2; }`, clean: a
/// type literal's `get g(): 1` reaches the surface as a method.
#[test]
#[ignore = "a type literal's accessor reaches the surface as an accessor"]
fn wrong_clean_a_const_spread_copies_an_accessor_as_a_readonly_property() {
    let matrix = Matrix::new(METHOD_SPREADS);
    let failures = matrix.returns(&[("sAccessorConst", "{ readonly g: 1; readonly h: 2; }")]);
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
export function eReverse() { return eObj()[5]; }
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

/// `return Color;` is `typeof Color`, whose numeric enum object carries the
/// reverse mapping `readonly [x: number]: string` beside its members, which
/// is no key of it. The checker prints the value by its name, `typeof
/// Color`; the lane prints the object it is, `{ readonly [x: number]:
/// string; readonly Red: Color.Red; readonly Green: Color.Green; readonly
/// Blue: Color.Blue; }` — a difference in print only: every read of the
/// value agrees, as each row here checks.
#[test]
fn a_returned_enum_object_reads_as_typeof_the_enum() {
    let matrix = Matrix::new(ENUMS);
    let mut failures = matrix.types(&[
        ("ReturnType<typeof eObj>[number]", "string"),
        (
            "keyof ReturnType<typeof eObj>",
            "\"Blue\" | \"Green\" | \"Red\"",
        ),
        ("keyof typeof Color", "\"Blue\" | \"Green\" | \"Red\""),
        ("ReturnType<typeof eObj>['Green']", "Color.Green"),
        (
            "[ReturnType<typeof eObj>] extends [typeof Color] ? ([typeof Color] extends [ReturnType<typeof eObj>] ? 1 : 2) : 3",
            "1",
        ),
    ]);
    failures.extend(matrix.returns(&[("eReverse", "string")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A default parameter initialized by an expression-bodied arrow declares the
/// arrow's return widened as a function's own return is: `cb = () => 7` is
/// `() => number`, a `const` assertion keeps `() => 7`, and a union of
/// literals is not a unit type, so `() => cond() ? 1 : 2` keeps `() => 1 | 2`
/// (TypeScript 7.0.2, all four settings alike).
#[test]
fn a_default_parameter_arrow_declares_its_widened_return() {
    let matrix = Matrix::new(WIDENING);
    let failures = matrix.returns(&[
        ("dpArrow", "() => number"),
        ("dpArrowConst", "() => 7"),
        ("dpArrowStr", "() => string"),
        ("dpArrowNeg", "() => number"),
        ("dpArrowUnion", "() => 1 | 2"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A const-context spread of an object literal's getter, whose copy the
/// checker makes a readonly property of the getter's type (`{ readonly g:
/// 1; readonly h: 2; }`, measured on TypeScript 7.0.2, alike under all four
/// settings), degrades rather than copying the accessor's signature.
#[test]
fn a_const_spread_of_an_accessor_is_never_read_as_its_signature() {
    let matrix = Matrix::new(METHOD_SPREADS);
    let rows = [(
        Read::Return("sGetterConst"),
        vec!["{ readonly g: 1; readonly h: 2; }"; 4],
    )];
    let wrong: Vec<String> = matrix
        .verdicts(&rows)
        .into_iter()
        .flatten()
        .filter(|verdict| !verdict.matched && verdict.class != "GAP")
        .map(|verdict| verdict.lane)
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Spreads of a union of one object type and types that spread into an empty
/// object (`undefined`, `null`, `{}`), measured with tsc 7.0.2.
const UNION_SPREADS: &str = r##"
export function spMember(o: { x?: { label: string } }) { return { ...o.x }; }
export function spEmptyArm(v: { a: number } | {}) { return { ...v }; }
export function spNull(v: { a: number; b?: string } | null) { return { ...v, c: 1 }; }
export function spLeft(v?: { a: number }) { return { a: "s" as const, ...v }; }
export function spLeftOpt(v: { a?: number }) { return { a: "s" as const, ...v }; }
"##;

/// An object literal's spread of a union of one object type and types that
/// spread into an empty object copies the object's properties as optional
/// ones (the checker's `tryMergeUnionOfObjectTypeAndEmptyObject`): `{
/// ...o.x }` over `x?: { label: string }` is `{ label?: string | undefined;
/// }`, never `{ label: string } | {}`. Without `strictNullChecks` the
/// nullish arm is absent and the object spreads whole.
#[test]
fn a_spread_of_an_object_or_empty_union_copies_optional_properties() {
    let matrix = Matrix::new(UNION_SPREADS);
    let failures = matrix.nullness(&[
        (
            Read::Return("spMember"),
            "{ label?: string | undefined; }",
            "{ label: string; }",
        ),
        (
            Read::Return("spEmptyArm"),
            "{ a?: number | undefined; }",
            "{ a?: number; }",
        ),
        (
            Read::Return("spNull"),
            "{ a?: number | undefined; b?: string; c: number; }",
            "{ a: number; b?: string; c: number; }",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A spread's optional property over a property the literal already has is
/// the union of both (`{ a: number | "s"; }`).
///
/// What the lane gives:
/// - `spLeftOpt` (a declared optional member): the lane measured `{ a:
///   <opaque OpenSurface>; }` under all four settings.
/// - `spLeft` (the merged partial of `{ a: number } | undefined`): the
///   same `{ a: <opaque OpenSurface>; }` with `strictNullChecks`; without
///   it the object spreads whole and the lane answers `{ a: number; }`.
#[test]
#[ignore = "an optional spread property over an existing one is the union of both"]
fn an_optional_spread_property_over_an_existing_one_is_their_union() {
    let matrix = Matrix::new(UNION_SPREADS);
    let failures = matrix.nullness(&[
        (
            Read::Return("spLeft"),
            "{ a: number | \"s\"; }",
            "{ a: number; }",
        ),
        (
            Read::Return("spLeftOpt"),
            "{ a: number | \"s\"; }",
            "{ a: number | \"s\"; }",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Module and namespace values whose object or array literal initializers
/// hold calls, beside a bare call and a call-free literal.
const CALLS_IN_LITERALS: &str = r##"
type __BenchInit = 0;
declare function g(): 1;
declare function f(): number;
declare const n: number;
export const r = [g()];
export let rLet = [g()];
export const o = { a: f() };
export const oLit = { a: g() };
export const nested = { a: [f()] };
export const deep = [{ a: f(), b: [g(), 2] }];
export const mixed = { a: f(), b: "s", c: n };
export const fresh = [g(), 2];
export let lo = { a: g(), b: 1 };
export const rc = [g(), 2] as const;
export const os = { a: f(), b: 1 } satisfies { a: number; b: number };
export const S = f();
export const plain = { a: n };
namespace N {
    export const r = [g()];
    export const o = { a: f() };
    export const nested = { a: [f()], b: { c: g() } };
    export const S = f();
    export const plain = { a: n };
}
namespace P { export namespace Q { export const o = { a: f() }; } }
namespace M.Inner { export const r = [g()]; export const o = { a: f() }; export const S = f(); }
namespace A.B.C { export const n = { a: [f()], b: { c: g() } }; export const plain = { a: n }; }
namespace D.E {
    export function h(): 2 { return 2; }
    export const v = [h()];
    export function run() { return { b: h() }; }
}
namespace F.A { export function h(): 2 { return 2; } }
namespace F.A.B { export const v = { a: h() }; export const r = [h()]; }
namespace K { export namespace A {
    export function h(): 2 { return 2; }
    export namespace B { export const v = { a: h() }; }
} }
"##;

/// A call that is an array-literal element or an object-literal property
/// value types as its return, in a module value and in a namespace's,
/// nested and dotted namespaces included (a dotted namespace's own members
/// called through its last segment's scope, an enclosing namespace's
/// function called bare from an inner one): a call's result is no fresh
/// literal, so a literal return type stays as it is where an authored
/// literal beside it widens. The answers are complete and read the same
/// warm.
#[test]
fn a_call_inside_a_literal_types_as_its_return() {
    let failures = cold_and_warm(
        &Matrix::new(CALLS_IN_LITERALS),
        &[
            ("typeof r", "1[]"),
            ("typeof rLet", "1[]"),
            ("typeof o", "{ a: number; }"),
            ("typeof oLit", "{ a: 1; }"),
            ("typeof nested", "{ a: number[]; }"),
            ("typeof deep", "{ a: number; b: number[]; }[]"),
            ("typeof mixed", "{ a: number; b: string; c: number; }"),
            ("typeof fresh", "number[]"),
            ("typeof lo", "{ a: 1; b: number; }"),
            ("typeof rc", "readonly [1, 2]"),
            ("typeof os", "{ a: number; b: number; }"),
            ("typeof S", "number"),
            ("typeof plain", "{ a: number; }"),
            ("typeof N.r", "1[]"),
            ("typeof N.o", "{ a: number; }"),
            ("typeof N.nested", "{ a: number[]; b: { c: 1; }; }"),
            ("typeof N.S", "number"),
            ("typeof N.plain", "{ a: number; }"),
            ("typeof P.Q.o", "{ a: number; }"),
            ("typeof M.Inner.r", "1[]"),
            ("typeof M.Inner.o", "{ a: number; }"),
            ("typeof M.Inner.S", "number"),
            ("typeof A.B.C.n", "{ a: number[]; b: { c: 1; }; }"),
            ("typeof A.B.C.plain", "{ a: number; }"),
            ("typeof D.E.v", "2[]"),
            ("ReturnType<typeof D.E.run>", "{ b: 2; }"),
            ("typeof F.A.B.v", "{ a: 2; }"),
            ("typeof F.A.B.r", "2[]"),
            ("typeof K.A.B.v", "{ a: 2; }"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Object literals holding `get` / `set` accessors, alone and paired, in
/// either order, beside a call and beside a call-free value, as module
/// values, nested, under `as const` and as a function's return.
const ACCESSORS_IN_LITERALS: &str = r##"
declare function f(): number;
declare const n: number;
export const getCall = { a: f(), get b() { return 1; } };
export const setCall = { a: f(), set b(v: number) {} };
export const pairCall = { a: f(), get b() { return 1; }, set b(v: number) {} };
export const pairCallReversed = { a: f(), set b(v: number) {}, get b() { return 1; } };
export const getPlain = { a: n, get b() { return 1; } };
export const setPlain = { a: n, set b(v: number) {} };
export const pairPlain = { a: n, get b() { return 1; }, set b(v: number) {} };
export const nestedGet = { o: { get b() { return 1; } } };
export const constPair = { get b() { return 1; }, set b(v: number) {} } as const;
export const annotatedSetter = { get b() { return "a"; }, set b(v: string | number) {} };
export function returnedGetCall() { return { a: f(), get b() { return 1; } }; }
export function returnedPairCall() { return { a: f(), get b() { return 1; }, set b(v: number) {} }; }
export function returnedGetPlain() { return { a: n, get b() { return 1; } }; }
export const getThenPlain = { a: f(), get b() { return 1; }, b: "s" };
export const plainThenGet = { a: f(), b: "s", get b() { return 1; } };
export const plainThenPair = { a: f(), b: "s", get b() { return 1; }, set b(v: number) {} };
export const setThenPlain = { a: f(), set b(v: number) {}, b: "s" };
export const setThenPlainThenGet = { a: f(), set b(v: number) {}, b: "s", get b() { return 1; } };
export const plainThenSetThenGet = { a: f(), b: "s", set b(v: number) {}, get b() { return 1; } };
export const getThenPlainThenGet = { a: f(), get b() { return 1; }, b: "s", get b() { return true; } };
export const pairThenPlain = { a: f(), get b() { return 1; }, set b(v: number) {}, b: "s" };
export const plainThenPlain = { a: f(), b: 1, b: "s" };
export const getThenPlainNoCall = { a: n, get b() { return 1; }, b: "s" };
export const plainThenGetNoCall = { a: n, b: "s", get b() { return 1; } };
export function returnedGetThenPlain() { return { a: f(), get b() { return 1; }, b: "s" }; }
"##;

/// An accessor in an object literal is the property the checker gives it,
/// with or without a call beside it: a getter alone is a `readonly`
/// property of its return, a setter alone a mutable property of its
/// parameter, and a get / set pair, in either order, ONE mutable property
/// (the getter's return, else the setter's annotated parameter) — never the
/// accessor function, never two members. A plain property spelled like an
/// accessor (an error the checker still types) is one property too: the
/// last member's type, `readonly` when the getter it reads through has no
/// setter. The answers are complete and read the same warm.
#[test]
fn an_accessor_in_a_literal_is_the_property_it_reads_as() {
    let matrix = Matrix::new(ACCESSORS_IN_LITERALS);
    let mut failures = cold_and_warm(
        &matrix,
        &[
            ("typeof getCall", "{ a: number; readonly b: number; }"),
            ("typeof setCall", "{ a: number; b: number; }"),
            ("typeof pairCall", "{ a: number; b: number; }"),
            ("typeof pairCallReversed", "{ a: number; b: number; }"),
            ("typeof getPlain", "{ a: number; readonly b: number; }"),
            ("typeof setPlain", "{ a: number; b: number; }"),
            ("typeof pairPlain", "{ a: number; b: number; }"),
            ("typeof nestedGet", "{ o: { readonly b: number; }; }"),
            ("typeof constPair", "{ b: number; }"),
            ("typeof annotatedSetter", "{ b: string | number; }"),
            ("typeof getThenPlain", "{ a: number; readonly b: string; }"),
            ("typeof plainThenGet", "{ a: number; readonly b: number; }"),
            ("typeof plainThenPair", "{ a: number; b: number; }"),
            ("typeof setThenPlain", "{ a: number; b: string; }"),
            ("typeof setThenPlainThenGet", "{ a: number; b: number; }"),
            ("typeof plainThenSetThenGet", "{ a: number; b: number; }"),
            (
                "typeof getThenPlainThenGet",
                "{ a: number; readonly b: boolean; }",
            ),
            ("typeof pairThenPlain", "{ a: number; b: string; }"),
            ("typeof plainThenPlain", "{ a: number; b: string; }"),
            (
                "typeof getThenPlainNoCall",
                "{ a: number; readonly b: string; }",
            ),
            (
                "typeof plainThenGetNoCall",
                "{ a: number; readonly b: number; }",
            ),
        ],
    );
    failures.extend(matrix.returns(&[
        ("returnedGetCall", "{ a: number; readonly b: number; }"),
        ("returnedPairCall", "{ a: number; b: number; }"),
        ("returnedGetPlain", "{ a: number; readonly b: number; }"),
        ("returnedGetThenPlain", "{ a: number; readonly b: string; }"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every `(type probe, checker print)` row answering the same in every
/// setting, read cold and then warm on one host, as the failure list: a
/// degraded, partial or wrong answer fails either read.
fn cold_and_warm(matrix: &Matrix<'_>, rows: &[(&str, &str)]) -> Vec<String> {
    let settings = super::differential_harness_tests::ALL.len();
    let rows: Vec<(Read<'_>, Vec<&str>)> = rows
        .iter()
        .map(|(probe, answer)| (Read::Type(probe), vec![*answer; settings]))
        .collect();
    let host = matrix.host(&rows);
    let mut failures = Vec::new();
    for read in ["cold", "warm"] {
        for ((probe, answers), verdicts) in rows.iter().zip(matrix.verdicts_on(&host, &rows)) {
            for (expected, verdict) in answers.iter().zip(verdicts) {
                if !verdict.matched {
                    failures.push(format!(
                        "{} `{}` ({read}): the checker answers `{expected}`, {}",
                        verdict.class,
                        probe.text(),
                        verdict.lane
                    ));
                }
            }
        }
    }
    failures
}

/// Object and array literals holding block-bodied function values, as
/// module values and as call arguments, beside an expression-bodied one.
/// Explicit method `this` parameters keep contextual inference from masking
/// missing body-return routing; computed returns avoid literal-widening rules.
const BLOCK_BODIES_IN_LITERALS: &str = r##"
declare function keep<T>(value: T): T;
type Returned<T> = T extends (...args: any[]) => infer R ? R : never;
export const property = { run: () => {} };
export const returned = keep({ run: () => { return; } });
export const beside = keep({ run: () => {}, size: 2 });
export const element = keep([() => {}]);
export const nested = { a: 1, b: [function () { return "s"; }] };
export const expression = keep({ run: () => 1 });
export function inBody() { return keep({ run: () => {} }); }
export function methodInBody() { return keep({ run(this: void) { return 1 + 1; } }); }
export function nestedMethodInBody() { return keep({ o: { run(this: void) { return 1 + 1; } } }); }
export function methodInArrayInBody() { return keep([{ run(this: void) { return 1 + 1; } }]); }
export function annotatedMethodInBody() { return keep({ run(): "s" { return "s"; } }); }
export const nestedConst = { nested: ({ run: () => { return 1; } } as const) };
export const nestedSatisfies = { nested: ({ run: () => { return 1; } } satisfies { run: () => number }) };
export const nestedNonNull = { nested: ({ run: () => { return 1; } }!) };
export const nestedAssertion = { nested: ({ run: () => { return 1; } } as { run: () => 1 }) };
"##;

/// A block-bodied function value inside an object or array literal types
/// its return from its body, where the literal is a module value and where
/// it is a call argument; an expression body keeps the literal return its
/// call infers.
#[test]
fn a_block_bodied_function_inside_a_literal_types_its_body_return() {
    let failures = Matrix::new(BLOCK_BODIES_IN_LITERALS).same(&[
        (Read::Type("typeof property"), "{ run: () => void; }"),
        (Read::Type("typeof returned"), "{ run: () => void; }"),
        (
            Read::Type("typeof beside"),
            "{ run: () => void; size: number; }",
        ),
        (Read::Type("typeof element"), "(() => void)[]"),
        (
            Read::Type("typeof nested"),
            "{ a: number; b: (() => string)[]; }",
        ),
        (Read::Type("typeof expression"), "{ run: () => 1; }"),
        (Read::Return("inBody"), "{ run: () => void; }"),
        (
            Read::Type("Returned<Returned<typeof methodInBody>['run']>"),
            "number",
        ),
        (
            Read::Type("Returned<Returned<typeof nestedMethodInBody>['o']['run']>"),
            "number",
        ),
        (
            Read::Type("Returned<Returned<typeof methodInArrayInBody>[number]['run']>"),
            "number",
        ),
        (Read::Return("annotatedMethodInBody"), "{ run(): \"s\"; }"),
        (
            Read::Type("typeof nestedConst"),
            "{ nested: { readonly run: () => number; }; }",
        ),
        (
            Read::Type("typeof nestedSatisfies"),
            "{ nested: { run: () => number; }; }",
        ),
        (
            Read::Type("typeof nestedNonNull"),
            "{ nested: { run: () => number; }; }",
        ),
        (
            Read::Type("typeof nestedAssertion"),
            "{ nested: { run: () => 1; }; }",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
