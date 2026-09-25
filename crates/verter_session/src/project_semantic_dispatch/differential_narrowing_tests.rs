//! Differential probes of the checker's narrowing: `typeof`,
//! `instanceof`, `in`, equality, discriminant properties, type predicates
//! and assertions, and truthiness. Each row is a function whose one
//! reachable `return` reads the narrowed reference (every other path
//! throws), answered as its body-derived return.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --declaration --emitDeclarationOnly
//! --strict --noErrorTruncation` under each `strictNullChecks` ×
//! `noImplicitAny` setting: `declare const p: ReturnType<typeof f>; const
//! s: never = p;` read off the TS2322 message. A row with one answer answers
//! alike in the four settings; a row with two answers gives the
//! `strictNullChecks` answer then the answer with it off. An ignored test
//! asserts the measured answer for rows the lane does not yet answer as the
//! checker does; "wrong-but-clean" marks a lane answer published complete and
//! undegraded.

use super::differential_harness_tests::{Matrix, Read};

/// `typeof` guards over unions, `unknown`, `any`, members and literal unions.
const TYPEOF: &str = r##"
export function tStr(x: string | number) { if (typeof x === "string") return x; throw 0; }
export function tStrElse(x: string | number) { if (typeof x === "string") throw 0; return x; }
export function tNotStr(x: string | number | boolean) { if (typeof x !== "string") return x; throw 0; }
export function tObj(x: string | { a: 1 } | null) { if (typeof x === "object") return x; throw 0; }
export function tObjElse(x: string | { a: 1 } | null) { if (typeof x === "object") throw 0; return x; }
export function tFn(x: (() => void) | string) { if (typeof x === "function") return x; throw 0; }
export function tUndef(x: string | undefined) { if (typeof x === "undefined") return x; throw 0; }
export function tNotUndef(x: string | undefined) { if (typeof x !== "undefined") return x; throw 0; }
export function tUnkNum(x: unknown) { if (typeof x === "number") return x; throw 0; }
export function tUnkObj(x: unknown) { if (typeof x === "object") return x; throw 0; }
export function tUnkStr(x: unknown) { if (typeof x === "string") return x; throw 0; }
export function tUnkBig(x: unknown) { if (typeof x === "bigint") return x; throw 0; }
export function tUnkSym(x: unknown) { if (typeof x === "symbol") return x; throw 0; }
export function tUnkBool(x: unknown) { if (typeof x === "boolean") return x; throw 0; }
export function tUnkUndef(x: unknown) { if (typeof x === "undefined") return x; throw 0; }
export function tAnyStr(x: any) { if (typeof x === "string") return x; throw 0; }
export function tLitUnion(x: "a" | 1 | true) { if (typeof x === "number") return x; throw 0; }
export function tSwitch(x: string | number | boolean) { switch (typeof x) { case "number": return x; default: throw 0; } }
export function tSwitchFall(x: string | number | boolean) { switch (typeof x) { case "number": case "boolean": return x; default: throw 0; } }
export function tSwitchDefault(x: string | number | boolean) { switch (typeof x) { case "number": throw 0; default: return x; } }
export function tBothSides(x: string | number) { if ("string" === typeof x) return x; throw 0; }
export function tNested(x: string | number | null) { if (x !== null) { if (typeof x === "number") return x; } throw 0; }
export function tAndGuard(x: string | number, y: boolean) { if (typeof x === "string" && y) return x; throw 0; }
export function tOrGuard(x: string | number | boolean) { if (typeof x === "string" || typeof x === "number") return x; throw 0; }
export function tNotOr(x: string | number | boolean) { if (!(typeof x === "string" || typeof x === "number")) return x; throw 0; }
export function tTernary(x: string | number) { return typeof x === "string" ? x : 0; }
export function tTernaryElse(x: string | number) { return typeof x === "string" ? "" : x; }
export function tMember(o: { v: string | number }) { if (typeof o.v === "string") return o.v; throw 0; }
export function tEnumLike(x: 1 | "1" | null | undefined) { if (typeof x === "object") return x; throw 0; }
export function tUndefOfString(x: string) { if (typeof x === "undefined") return x; throw 0; }
export function tObjectOrString(x: { a: 1 } | string) { if (typeof x === "object") return x; throw 0; }
export function tObjectOfNumber(x: number) { if (typeof x === "object") return x; throw 0; }
export function tUndefOfUnion(x: string | number) { if (typeof x === "undefined") return x; throw 0; }
export function tNotUndefElse(x: string) { if (typeof x !== "undefined") throw 0; return x; }
export function tUndefOfNever(x: never) { if (typeof x === "undefined") return x; throw 0; }
"##;

/// A `typeof` guard keeps the union members of the tested kind in its true
/// branch and the rest in its false branch, in either operand order, negated,
/// combined with `&&` / `||`, over a member reference, and from `unknown` or
/// `any` to the tested primitive (`object | null` for `"object"`).
#[test]
fn typeof_guards_narrow_as_the_checker_narrows() {
    let matrix = Matrix::new(TYPEOF);
    let mut failures = matrix.returns(&[
        ("tStr", "string"),
        ("tStrElse", "number"),
        ("tNotStr", "number | boolean"),
        ("tObjElse", "string"),
        ("tFn", "() => void"),
        ("tNotUndef", "string"),
        ("tUnkNum", "number"),
        ("tUnkStr", "string"),
        ("tUnkBig", "bigint"),
        ("tUnkSym", "symbol"),
        ("tUnkBool", "boolean"),
        ("tUnkUndef", "undefined"),
        ("tAnyStr", "string"),
        ("tLitUnion", "1"),
        ("tBothSides", "string"),
        ("tNested", "number"),
        ("tAndGuard", "string"),
        ("tOrGuard", "string | number"),
        ("tNotOr", "boolean"),
        ("tTernary", "string | 0"),
        ("tTernaryElse", "number | \"\""),
        ("tMember", "string"),
    ]);
    failures.extend(matrix.nullness(&[
        (Read::Return("tObj"), "{ a: 1; } | null", "{ a: 1; }"),
        (Read::Return("tUnkObj"), "object | null", "object"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `switch (typeof x)` narrows `x` in each `case` clause to the tested kinds
/// (several for fall-through labels) and in `default` to the kinds no clause
/// names.
///
/// What the lane gives:
/// - `tSwitch`: the checker answers `number`; the lane measured `number |
///   boolean | string` degraded by FlowGap(GuardNarrowing).
/// - `tSwitchFall`: the checker answers `number | boolean`; the lane measured
///   `number | boolean | string` degraded by FlowGap(GuardNarrowing).
/// - `tSwitchDefault`: the checker answers `string | boolean`; the lane
///   measured `number | boolean | string` degraded by FlowGap(GuardNarrowing).
#[test]
#[ignore = "a switch over typeof x narrows x in each clause"]
fn a_switch_on_typeof_narrows_each_clause() {
    let matrix = Matrix::new(TYPEOF);
    let failures = matrix.returns(&[
        ("tSwitch", "number"),
        ("tSwitchFall", "number | boolean"),
        ("tSwitchDefault", "string | boolean"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Without `strictNullChecks` `undefined` and `null` are subtypes of every
/// type, so the true branch of `typeof x === "undefined"` reads `undefined` and
/// of `typeof x === "object"` reads `null` out of an arm of another kind (the
/// checker's `narrowTypeByTypeFacts`), unless an arm of the tested kind
/// survives; with it such an arm is `never`.
#[test]
fn a_typeof_guard_implies_a_nullish_type_as_the_checker_narrows_it() {
    let matrix = Matrix::new(TYPEOF);
    let mut failures = matrix.returns(&[("tUndef", "undefined"), ("tEnumLike", "null")]);
    failures.extend(matrix.nullness(&[
        (Read::Return("tUndefOfString"), "never", "undefined"),
        (Read::Return("tObjectOrString"), "{ a: 1; }", "{ a: 1; }"),
        (Read::Return("tObjectOfNumber"), "never", "null"),
        (Read::Return("tUndefOfUnion"), "never", "undefined"),
        (Read::Return("tNotUndefElse"), "never", "undefined"),
        (Read::Return("tUndefOfNever"), "never", "never"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Classes, a subclass, and a constructor-typed value.
const INSTANCEOF: &str = r##"
class A { a = 1 }
class B { b = 2 }
class Base { base = 0 }
class Derived extends Base { derived = 1 }
class Other { other = 2 }
declare const Ctor: new () => { z: 1 };
interface HasZ { z: 1 }
export function iA(x: A | B) { if (x instanceof A) return x; throw 0; }
export function iAElse(x: A | B) { if (x instanceof A) throw 0; return x; }
export function iUnk(x: unknown) { if (x instanceof A) return x; throw 0; }
export function iDerived(x: Base) { if (x instanceof Derived) return x; throw 0; }
export function iDerivedElse(x: Base) { if (x instanceof Derived) throw 0; return x; }
export function iSub(x: Derived | Other) { if (x instanceof Base) return x; throw 0; }
export function iSubElse(x: Derived | Other) { if (x instanceof Base) throw 0; return x; }
export function iCtorVal(x: unknown) { if (x instanceof Ctor) return x; throw 0; }
export function iStrOrA(x: string | A) { if (x instanceof A) return x; throw 0; }
export function iStrOrAElse(x: string | A) { if (x instanceof A) throw 0; return x; }
export function iNot(x: A | B) { if (!(x instanceof A)) return x; throw 0; }
export function iObj(x: { a: number }) { if (x instanceof B) return x; throw 0; }
export function iInterface(x: HasZ | A) { if (x instanceof A) return x; throw 0; }
export function iAny(x: any) { if (x instanceof A) return x; throw 0; }
export function iTernary(x: A | B) { return x instanceof B ? x : null; }
export function iMember(o: { v: A | B }) { if (o.v instanceof A) return o.v; throw 0; }
"##;

/// An `instanceof` guard narrows to the union members that are the class (or
/// its subclasses) and to the class itself from `unknown`, `any` or an
/// unrelated object type (as an intersection); the false branch keeps the
/// members that are not.
#[test]
fn instanceof_guards_narrow_as_the_checker_narrows() {
    let matrix = Matrix::new(INSTANCEOF);
    let mut failures = matrix.returns(&[
        ("iA", "A"),
        ("iAElse", "B"),
        ("iUnk", "A"),
        ("iDerived", "Derived"),
        ("iDerivedElse", "Base"),
        ("iSub", "Derived"),
        ("iSubElse", "Other"),
        ("iStrOrA", "A"),
        ("iStrOrAElse", "string"),
        ("iNot", "B"),
        ("iObj", "{ a: number; } & B"),
        ("iAny", "A"),
        ("iMember", "A"),
    ]);
    failures.extend(matrix.nullness(&[(Read::Return("iTernary"), "B | null", "B")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `x instanceof Ctor` with `declare const Ctor: new () => { z: 1 }` narrows
/// `unknown` to the construct signature's return type.
///
/// What the lane gives:
/// - `iCtorVal`: the checker answers `{ z: 1; }`; the lane measured `unknown`
///   degraded by FlowGap(GuardNarrowing).
#[test]
#[ignore = "instanceof narrows to the instance type of a constructor-typed value"]
fn instanceof_narrows_to_a_constructor_values_instance_type() {
    let matrix = Matrix::new(INSTANCEOF);
    let failures = matrix.returns(&[("iCtorVal", "{ z: 1; }")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `x instanceof A` over `HasZ | A` keeps only `A`: the unrelated interface arm
/// is filtered out.
///
/// What the lane gives:
/// - `iInterface`: the checker answers `A`; the lane measured `A | HasZ`
///   degraded by FlowGap(GuardNarrowing); measured `HasZ | A` degraded by
///   FlowGap(GuardNarrowing).
#[test]
#[ignore = "instanceof drops a union arm unrelated to the class"]
fn instanceof_filters_an_unrelated_interface_arm() {
    let matrix = Matrix::new(INSTANCEOF);
    let failures = matrix.returns(&[("iInterface", "A")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Interfaces with required, shared and optional keys.
const IN_OPERATOR: &str = r##"
interface HA { a: 1 }
interface HB { b: 2 }
interface HAB { a: 3; b: 4 }
interface OptA { a?: 5; c: 6 }
export function nA(x: HA | HB) { if ("a" in x) return x; throw 0; }
export function nAElse(x: HA | HB) { if ("a" in x) throw 0; return x; }
export function nBoth(x: HA | HAB) { if ("b" in x) return x; throw 0; }
export function nBothElse(x: HA | HAB) { if ("b" in x) throw 0; return x; }
export function nOpt(x: OptA | HB) { if ("a" in x) return x; throw 0; }
export function nOptElse(x: OptA | HB) { if ("a" in x) throw 0; return x; }
export function nObject(x: object) { if ("a" in x) return x; throw 0; }
export function nObjectProp(x: object) { if ("a" in x) return x.a; throw 0; }
export function nUnknownObj(x: unknown) { if (typeof x === "object" && x !== null && "k" in x) return x; throw 0; }
export function nNot(x: HA | HB) { if (!("a" in x)) return x; throw 0; }
export function nTernary(x: HA | HB) { return "b" in x ? x : null; }
export function nMissing(x: HA | HB) { if ("z" in x) return x; throw 0; }
export function nRecord(x: Record<string, number> | HA) { if ("q" in x) return x; throw 0; }
export function nSwitchLike(x: HA | HB) { if ("a" in x) { return x.a; } return x.b; }
"##;

/// An `in` guard keeps the members that declare the key (required or optional)
/// in its true branch and those that do not declare it, or declare it
/// optionally, in its false branch.
#[test]
fn in_guards_narrow_as_the_checker_narrows() {
    let matrix = Matrix::new(IN_OPERATOR);
    let mut failures = matrix.returns(&[
        ("nA", "HA"),
        ("nAElse", "HB"),
        ("nBoth", "HAB"),
        ("nBothElse", "HA"),
        ("nOpt", "OptA"),
        ("nOptElse", "HB | OptA"),
        ("nNot", "HB"),
        ("nMissing", "(HA | HB) & Record<\"z\", unknown>"),
        ("nSwitchLike", "1 | 2"),
    ]);
    failures.extend(matrix.nullness(&[(Read::Return("nTernary"), "HB | null", "HB")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `"a" in x` over `object` (or an `unknown` narrowed to `object`) narrows to
/// `object & Record<"a", unknown>`, so `x.a` reads `unknown`.
///
/// What the lane gives:
/// - `nObject`: the checker answers `object & Record<"a", unknown>`; the lane
///   measured `object` degraded by FlowGap(GuardNarrowing).
/// - `nObjectProp`: the checker answers `unknown`; the lane measured `<opaque
///   Miss>` degraded by FlowGap(GuardNarrowing).
/// - `nUnknownObj`: the checker answers `object & Record<"k", unknown>`; the
///   lane measured `object` degraded by FlowGap(GuardNarrowing).
#[test]
#[ignore = "an in guard over a type without the key intersects Record<key, unknown>"]
fn an_in_guard_intersects_a_record_of_the_key() {
    let matrix = Matrix::new(IN_OPERATOR);
    let failures = matrix.returns(&[
        ("nObject", "object & Record<\"a\", unknown>"),
        ("nObjectProp", "unknown"),
        ("nUnknownObj", "object & Record<\"k\", unknown>"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `"q" in x` over `Record<string, number> | HA` keeps only the record: `HA`
/// neither declares `q` nor has an index signature.
///
/// What the lane gives:
/// - `nRecord`: the checker answers `Record<string, number>`; the lane measured
///   `Record<string, number> | HA` degraded by FlowGap(GuardNarrowing).
#[test]
#[ignore = "an in guard drops an arm that neither declares the key nor has an index signature"]
fn an_in_guard_drops_an_arm_without_the_key_or_an_index() {
    let matrix = Matrix::new(IN_OPERATOR);
    let failures = matrix.returns(&[("nRecord", "Record<string, number>")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Literal unions, nullable unions, booleans and `unknown`.
const EQUALITY: &str = r##"
type ABC = "a" | "b" | "c";
export function eEq(x: ABC) { if (x === "a") return x; throw 0; }
export function eNe(x: ABC) { if (x !== "a") return x; throw 0; }
export function eLooseNull(x: string | null | undefined) { if (x == null) return x; throw 0; }
export function eLooseNotNull(x: string | null | undefined) { if (x != null) return x; throw 0; }
export function eStrictNull(x: string | null | undefined) { if (x === null) return x; throw 0; }
export function eStrictUndef(x: string | null | undefined) { if (x === undefined) return x; throw 0; }
export function eNotUndef(x: string | undefined) { if (x !== undefined) return x; throw 0; }
export function eNumLit(x: number) { if (x === 1) return x; throw 0; }
export function eStrLit(x: string) { if (x === "k") return x; throw 0; }
export function eTwoVars(x: "a" | "b", y: "b" | "c") { if (x === y) return x; throw 0; }
export function eTwoVarsY(x: "a" | "b", y: "b" | "c") { if (x === y) return y; throw 0; }
export function eSwitch(x: ABC) { switch (x) { case "a": case "b": return x; default: throw 0; } }
export function eSwitchDefault(x: ABC) { switch (x) { case "a": throw 0; default: return x; } }
export function eSwitchFallthrough(x: ABC) { switch (x) { case "a": case "c": return x; } throw 0; }
export function eUnknownLit(x: unknown) { if (x === "lit") return x; throw 0; }
export function eUnknownNull(x: unknown) { if (x === null) return x; throw 0; }
export function eAnyEq(x: any) { if (x === 1) return x; throw 0; }
export function eBoolTrue(x: boolean) { if (x === true) return x; throw 0; }
export function eBoolNotTrue(x: boolean) { if (x !== true) return x; throw 0; }
export function eLooseEqLit(x: 1 | "1" | 2) { if (x == 1) return x; throw 0; }
export function eYoda(x: ABC) { if ("b" === x) return x; throw 0; }
export function eMember(o: { k: ABC }) { if (o.k === "c") return o.k; throw 0; }
export function eSwitchTrue(x: string | number) { switch (true) { case typeof x === "string": return x; default: throw 0; } }
export function eNullOrUndefUnion(x: number | null) { if (x === undefined) return x; throw 0; }
"##;

/// `===` / `!==` / `==` / `!=` against a literal, `null` or `undefined` narrow
/// the compared reference (either operand order, over a member, between two
/// references, in `switch` clauses); a literal comparison of a non-union
/// primitive keeps it.
#[test]
fn equality_guards_narrow_as_the_checker_narrows() {
    let matrix = Matrix::new(EQUALITY);
    let mut failures = matrix.returns(&[
        ("eEq", "\"a\""),
        ("eNe", "\"b\" | \"c\""),
        ("eLooseNotNull", "string"),
        ("eNotUndef", "string"),
        ("eNumLit", "number"),
        ("eStrLit", "string"),
        ("eTwoVars", "\"b\""),
        ("eTwoVarsY", "\"b\""),
        ("eSwitch", "\"a\" | \"b\""),
        ("eSwitchDefault", "\"b\" | \"c\""),
        ("eSwitchFallthrough", "\"a\" | \"c\""),
        ("eUnknownLit", "string"),
        ("eAnyEq", "any"),
        ("eBoolTrue", "true"),
        ("eBoolNotTrue", "false"),
        ("eLooseEqLit", "1"),
        ("eYoda", "\"b\""),
        ("eMember", "\"c\""),
    ]);
    failures.extend(matrix.nullness(&[
        (Read::Return("eLooseNull"), "null | undefined", "string"),
        (Read::Return("eStrictNull"), "null", "string"),
        (Read::Return("eStrictUndef"), "undefined", "string"),
        (Read::Return("eUnknownNull"), "null", "unknown"),
        (Read::Return("eNullOrUndefUnion"), "never", "number"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `switch (true) { case typeof x === "string": … }` narrows `x` in the clause
/// by the clause's condition.
///
/// What the lane gives:
/// - `eSwitchTrue`: the checker answers `string`; the lane measured `number |
///   string` degraded by FlowGap(GuardNarrowing).
#[test]
#[ignore = "switch (true) narrows by each case condition"]
fn a_switch_on_true_narrows_by_each_clause_condition() {
    let matrix = Matrix::new(EQUALITY);
    let failures = matrix.returns(&[("eSwitchTrue", "string")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Discriminated unions by string, boolean, number and optional tags.
const DISCRIMINANTS: &str = r##"
interface Circle { kind: "circle"; r: number }
interface Square { kind: "square"; s: number }
interface Tri { kind: "tri"; t: number }
type Shape = Circle | Square | Tri;
type Result = { ok: true; value: string } | { ok: false; error: Error2 };
interface Error2 { msg: string }
type NumTag = { tag: 1; one: string } | { tag: 2; two: number };
type Action = { type: "add"; payload: number } | { type: "name"; payload: string };
type Opt = { k: "a"; a: 1 } | { k?: undefined; none: 0 };
export function dEq(s: Shape) { if (s.kind === "circle") return s; throw 0; }
export function dNe(s: Shape) { if (s.kind !== "circle") return s; throw 0; }
export function dMember(s: Shape) { if (s.kind === "square") return s.s; throw 0; }
export function dSwitch(s: Shape) { switch (s.kind) { case "tri": return s; default: throw 0; } }
export function dSwitchTwo(s: Shape) { switch (s.kind) { case "tri": case "square": return s; default: throw 0; } }
export function dSwitchDefault(s: Shape) { switch (s.kind) { case "tri": throw 0; default: return s; } }
export function dBoolNot(r: Result) { if (!r.ok) return r; throw 0; }
export function dBoolEq(r: Result) { if (r.ok === false) return r; throw 0; }
export function dNum(n: NumTag) { if (n.tag === 2) return n.two; throw 0; }
export function dDestructured(a: Action) { const { type, payload } = a; if (type === "add") return payload; throw 0; }
export function dDestructuredElse(a: Action) { const { type, payload } = a; if (type === "add") throw 0; return payload; }
export function dParamDestructured({ type, payload }: Action) { if (type === "name") return payload; throw 0; }
export function dOptional(o: Opt) { if (o.k === undefined) return o; throw 0; }
export function dOptionalElse(o: Opt) { if (o.k === "a") return o; throw 0; }
export function dOptChain(s: Shape | undefined) { if (s?.kind === "circle") return s; throw 0; }
export function dOptChainNe(s: Shape | undefined) { if (s?.kind !== "circle") return s; throw 0; }
export function dElemAccess(s: Shape) { if (s["kind"] === "tri") return s; throw 0; }
export function dTernary(s: Shape) { return s.kind === "circle" ? s.r : s.kind === "square" ? s.s : s.t; }
export function dExhaust(s: Shape) { switch (s.kind) { case "circle": return 1; case "square": return 2; case "tri": return 3; } }
export function dExhaustNever(s: Shape) { switch (s.kind) { case "circle": case "square": case "tri": throw 0; default: return s; } }
export function dIn(s: Shape) { if ("r" in s) return s; throw 0; }
export function dTypeofMember(x: { v: string } | { v: number }) { if (typeof x.v === "string") return x; throw 0; }
export function dTruthyMember(x: { v: string; a: 1 } | { v?: undefined; b: 2 }) { if (x.v) return x; throw 0; }
"##;

/// A comparison of a discriminant property narrows the union it is read from:
/// in `if`, `switch` and conditional expressions, by boolean and numeric tags,
/// through destructured `const` bindings and an element access, and to `never`
/// past an exhaustive switch.
#[test]
fn discriminant_guards_narrow_as_the_checker_narrows() {
    let matrix = Matrix::new(DISCRIMINANTS);
    let mut failures = matrix.returns(&[
        ("dEq", "Circle"),
        ("dNe", "Square | Tri"),
        ("dMember", "number"),
        ("dSwitch", "Tri"),
        ("dSwitchTwo", "Square | Tri"),
        ("dSwitchDefault", "Circle | Square"),
        ("dBoolEq", "{ ok: false; error: Error2; }"),
        ("dNum", "number"),
        ("dDestructured", "number"),
        ("dDestructuredElse", "string"),
        ("dElemAccess", "Tri"),
        ("dTernary", "number"),
        ("dExhaust", "1 | 2 | 3"),
        ("dIn", "Circle"),
        ("dTypeofMember", "{ v: string; } | { v: number; }"),
    ]);
    failures.extend(matrix.nullness(&[
        (
            Read::Return("dBoolNot"),
            "{ ok: false; error: Error2; }",
            "Result",
        ),
        (
            Read::Return("dOptional"),
            "{ k?: undefined; none: 0; }",
            "Opt",
        ),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `({ type, payload }: Action)` narrows `payload` when `type` is compared, as
/// a destructured `const` does.
///
/// What the lane gives:
/// - `dParamDestructured`: the checker answers `string`; the lane measured
///   `<opaque UnmodeledPosition>` degraded by UnmodeledPosition.
#[test]
#[ignore = "a discriminant destructured in a parameter list narrows its sibling binding"]
fn a_destructured_parameter_discriminant_narrows_its_sibling() {
    let matrix = Matrix::new(DISCRIMINANTS);
    let failures = matrix.returns(&[("dParamDestructured", "string")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `s?.kind === "circle"` narrows `s` to `Circle` (the `undefined` arm cannot
/// produce the literal), and `!==` keeps `undefined` beside the other members.
///
/// What the lane gives:
/// - `dOptChain`: the checker answers `Circle`; the lane measured `Shape |
///   undefined` degraded by FlowGap(GuardNarrowing); measured `Shape` degraded
///   by FlowGap(GuardNarrowing).
/// - `dOptChainNe`: the checker answers `Square | Tri | undefined` (strict),
///   `Square | Tri` (strictNullChecks off), `Square | Tri | undefined`
///   (noImplicitAny off), `Square | Tri` (both off); the lane measured `Shape |
///   undefined` degraded by FlowGap(GuardNarrowing); measured `Shape` degraded
///   by FlowGap(GuardNarrowing).
#[test]
#[ignore = "an optional-chain discriminant comparison narrows the chain's root"]
fn an_optional_chain_discriminant_narrows_the_chain_root() {
    let matrix = Matrix::new(DISCRIMINANTS);
    let mut failures = matrix.returns(&[("dOptChain", "Circle")]);
    failures.extend(matrix.nullness(&[(
        Read::Return("dOptChainNe"),
        "Square | Tri | undefined",
        "Square | Tri",
    )]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The `default` clause of a `switch` whose cases name every discriminant value
/// reads the union as `never`.
///
/// What the lane gives:
/// - `dExhaustNever`: the checker answers `never`; the lane measured `void`
///   degraded by FlowGap(GuardNarrowing).
#[test]
#[ignore = "the default clause of an exhaustive discriminant switch sees never"]
fn an_exhaustive_switch_default_is_never() {
    let matrix = Matrix::new(DISCRIMINANTS);
    let failures = matrix.returns(&[("dExhaustNever", "never")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Without `strictNullChecks` a union whose discriminant is `undefined` (or
/// optional) in one member is not narrowed by comparing or testing that
/// property: `o.k === "a"` and `if (x.v)` keep the whole union.
/// Wrong-but-clean: the lane narrows.
///
/// What the lane gives:
/// - `dOptionalElse`: the checker answers `{ k: "a"; a: 1; }` (strict), `Opt`
///   (strictNullChecks off), `{ k: "a"; a: 1; }` (noImplicitAny off), `Opt`
///   (both off); the lane measured `{ k: "a"; a: 1; }` (strictNullChecks off,
///   both off).
/// - `dTruthyMember`: the checker answers `{ v: string; a: 1; }` (strict), `{
///   v: string; a: 1; } | { v?: undefined; b: 2; }` (strictNullChecks off), `{
///   v: string; a: 1; }` (noImplicitAny off), `{ v: string; a: 1; } | { v?:
///   undefined; b: 2; }` (both off); the lane measured `{ v: string; a: 1; }`
///   (strictNullChecks off, both off).
#[test]
#[ignore = "a discriminant with an undefined member does not narrow without strictNullChecks"]
fn wrong_clean_an_undefined_discriminant_does_not_narrow_without_strict_null_checks() {
    let matrix = Matrix::new(DISCRIMINANTS);
    let failures = matrix.nullness(&[
        (Read::Return("dOptionalElse"), "{ k: \"a\"; a: 1; }", "Opt"),
        (
            Read::Return("dTruthyMember"),
            "{ v: string; a: 1; }",
            "{ v: string; a: 1; } | { v?: undefined; b: 2; }",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Declared, `this`-based, generic and inferred type predicates, and assertion
/// functions.
const PREDICATES: &str = r##"
interface Fish { swim(): void; fins: number }
interface Bird { fly(): void; wings: number }
declare function isFish(p: Fish | Bird): p is Fish;
declare function isString(x: unknown): x is string;
declare function assertString(x: unknown): asserts x is string;
declare function assertTruthy(x: unknown): asserts x;
declare function isDefined<T>(x: T | undefined | null): x is T;
declare function isKey<K extends string>(k: K, x: string): x is K;
class Box2 { v: string | number = 0; isStr(): this is { v: string } { return typeof this.v === "string"; } }
const isNum = (x: unknown) => typeof x === "number";
function isFishInferred(p: Fish | Bird) { return "swim" in p; }
export function pFish(p: Fish | Bird) { if (isFish(p)) return p; throw 0; }
export function pFishElse(p: Fish | Bird) { if (isFish(p)) throw 0; return p; }
export function pUnkStr(x: unknown) { if (isString(x)) return x; throw 0; }
export function pUnionStr(x: string | number) { if (isString(x)) return x; throw 0; }
export function pUnionStrElse(x: string | number) { if (isString(x)) throw 0; return x; }
export function pNarrowLiteral(x: "a" | 1) { if (isString(x)) return x; throw 0; }
export function pAssert(x: unknown) { assertString(x); return x; }
export function pAssertUnion(x: string | number) { assertString(x); return x; }
export function pAssertTruthy(x: string | undefined) { assertTruthy(x); return x; }
export function pGenericDefined(x: string | undefined) { if (isDefined(x)) return x; throw 0; }
export function pGenericKey(x: string) { if (isKey("k", x)) return x; throw 0; }
export function pThis(b: Box2) { if (b.isStr()) return b; throw 0; }
export function pThisMember(b: Box2) { if (b.isStr()) return b.v; throw 0; }
export function pInferredArrow(x: string | number) { if (isNum(x)) return x; throw 0; }
export function pInferredFn(p: Fish | Bird) { if (isFishInferred(p)) return p; throw 0; }
export function pNot(p: Fish | Bird) { if (!isFish(p)) return p; throw 0; }
export function pAnd(x: unknown, y: unknown) { if (isString(x) && isString(y)) return y; throw 0; }
export function pOr(x: string | number | boolean) { if (isString(x) || typeof x === "number") return x; throw 0; }
export function pTernary(p: Fish | Bird) { return isFish(p) ? p.fins : p.wings; }
export function pMemberArg(o: { p: Fish | Bird }) { if (isFish(o.p)) return o.p; throw 0; }
"##;

/// A call to a type predicate narrows its argument (a member reference too) in
/// the true branch and filters it out in the false branch; an assertion narrows
/// what follows it; a `this is` predicate narrows the receiver; an inferred
/// predicate of a function or arrow narrows like a declared one.
#[test]
fn predicates_and_assertions_narrow_as_the_checker_narrows() {
    let matrix = Matrix::new(PREDICATES);
    let failures = matrix.returns(&[
        ("pFish", "Fish"),
        ("pFishElse", "Bird"),
        ("pUnkStr", "string"),
        ("pUnionStr", "string"),
        ("pUnionStrElse", "number"),
        ("pNarrowLiteral", "\"a\""),
        ("pAssert", "string"),
        ("pAssertUnion", "string"),
        ("pAssertTruthy", "string"),
        ("pGenericKey", "\"k\""),
        ("pThis", "Box2 & { v: string; }"),
        ("pThisMember", "string"),
        ("pInferredArrow", "number"),
        ("pInferredFn", "Fish"),
        ("pNot", "Bird"),
        ("pAnd", "string"),
        ("pOr", "string | number"),
        ("pTernary", "number"),
        ("pMemberArg", "Fish"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `isDefined<T>(x: T | undefined | null): x is T` called with `string |
/// undefined` infers `T = string` and narrows the argument to `string`.
/// Wrong-but-clean: the lane keeps `string | undefined`.
///
/// What the lane gives:
/// - `pGenericDefined`: the checker answers `string`; the lane measured `string
///   | undefined` (strict, noImplicitAny off).
#[test]
#[ignore = "a generic type predicate narrows by its inferred type argument"]
fn wrong_clean_a_generic_predicate_narrows_by_its_inferred_argument() {
    let matrix = Matrix::new(PREDICATES);
    let failures = matrix.returns(&[("pGenericDefined", "string")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Nullable, literal, boolean and `unknown` references.
const TRUTHINESS: &str = r##"
export function tr1(x: string | undefined) { if (x) return x; throw 0; }
export function tr1Else(x: string | undefined) { if (x) throw 0; return x; }
export function tr2(x: number | null) { if (x) return x; throw 0; }
export function tr3(x: "" | "a" | 0 | 1 | null) { if (x) return x; throw 0; }
export function tr3Else(x: "" | "a" | 0 | 1 | null) { if (x) throw 0; return x; }
export function tr4(x: boolean) { if (x) return x; throw 0; }
export function tr4Else(x: boolean) { if (x) throw 0; return x; }
export function tr4Mixed(x: boolean | "a") { if (x) throw 0; return x; }
export function tr4Not(x: boolean) { if (!x) return x; throw 0; }
export function tr4Number(x: boolean | number) { if (x) throw 0; return x; }
export function tr5(x: { a: 1 } | undefined) { if (x) return x; throw 0; }
export function tr5Else(x: { a: 1 } | undefined) { if (x) throw 0; return x; }
export function tr6(x: string | undefined) { if (!x) return x; throw 0; }
export function tr7(x: string | undefined) { if (!!x) return x; throw 0; }
export function tr9(x: string | null) { return x || "d"; }
export function tr10(x: string | null) { return x ?? "d"; }
export function tr11(x: unknown) { if (x) return x; throw 0; }
export function tr12(x: unknown) { if (!x) return x; throw 0; }
export function tr13(x: 0 | 1n | 0n | 2) { if (x) return x; throw 0; }
export function tr14(o: { v?: string }) { if (o.v) return o.v; throw 0; }
export function tr15(o?: { v: string }) { if (o) return o.v; throw 0; }
export function tr16(x: string | number | undefined) { if (x && typeof x === "string") return x; throw 0; }
export function tr17(x: true | undefined) { if (x) return x; throw 0; }
export function tr18(x: any) { if (x) return x; throw 0; }
export function tr19(x: string | undefined) { while (x) { return x; } throw 0; }
export function tr20(x: string | undefined) { if (x !== undefined && x) return x; throw 0; }
"##;

/// A truthiness test removes `null`, `undefined` and falsy literals in its true
/// branch and keeps the possibly falsy members in its false branch; `unknown`
/// narrows to `{}` under `strictNullChecks`; `||`, `??`, `!!` and loops test
/// the same way.
#[test]
fn truthiness_narrows_as_the_checker_narrows() {
    let matrix = Matrix::new(TRUTHINESS);
    let mut failures = matrix.returns(&[
        ("tr1", "string"),
        ("tr2", "number"),
        ("tr3", "\"a\" | 1"),
        ("tr4", "true"),
        ("tr5", "{ a: 1; }"),
        ("tr7", "string"),
        ("tr9", "string"),
        ("tr10", "string"),
        ("tr12", "unknown"),
        ("tr13", "2 | 1n"),
        ("tr14", "string"),
        ("tr15", "string"),
        ("tr16", "string"),
        ("tr17", "true"),
        ("tr18", "any"),
        ("tr19", "string"),
        ("tr20", "string"),
    ]);
    failures.extend(matrix.nullness(&[
        (Read::Return("tr1Else"), "string | undefined", "string"),
        (
            Read::Return("tr3Else"),
            "\"\" | 0 | null",
            "\"\" | \"a\" | 0 | 1",
        ),
        (Read::Return("tr5Else"), "undefined", "{ a: 1; }"),
        (Read::Return("tr6"), "string | undefined", "string"),
        (Read::Return("tr11"), "{}", "unknown"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Without `strictNullChecks` `true` carries the checker's `Falsy` fact too,
/// so the false branch of a truthiness test over `boolean` keeps `boolean`;
/// with it the branch reads `false`.
#[test]
fn a_falsy_boolean_narrows_as_the_checker_narrows_it() {
    let matrix = Matrix::new(TRUTHINESS);
    let failures = matrix.nullness(&[
        (Read::Return("tr4Else"), "false", "boolean"),
        (Read::Return("tr4Mixed"), "false", "\"a\" | boolean"),
        (Read::Return("tr4Not"), "false", "boolean"),
        (
            Read::Return("tr4Number"),
            "number | false",
            "number | boolean",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
