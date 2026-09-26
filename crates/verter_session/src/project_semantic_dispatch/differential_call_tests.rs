//! Differential probes of the checker's call resolution and inference:
//! overload selection, generic inference from arguments and callbacks,
//! `const` type parameters, and contextual typing. Each row is a function
//! of the fixture, answered as its body-derived return.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --declaration --emitDeclarationOnly
//! --strict --noErrorTruncation` under each `strictNullChecks` ×
//! `noImplicitAny` setting: `declare const p: ReturnType<typeof f>; const
//! s: never = p;` read off the TS2322 message. A row with one answer answers
//! alike in the four settings; a row with two answers gives the
//! `strictNullChecks` answer then the answer with it off. An ignored test
//! asserts the measured answer for rows the lane does not yet answer as the
//! checker does; "wrong-but-clean" marks a lane answer published complete
//! and undegraded.

use super::differential_harness_tests::{Matrix, Read};

/// Overloaded declarations by type, arity, literal and generic parameters.
const OVERLOADS: &str = r##"
declare function ov(a: string): "S";
declare function ov(a: number): "N";
declare function ov(a: string | number): "U";
declare function ovArity(): 0;
declare function ovArity(a: string): 1;
declare function ovArity(a: string, b: string): 2;
declare function ovOpt(a: string, b?: number): "opt";
declare function ovRest(...a: number[]): "rest";
declare function ovLit(a: "x"): "X";
declare function ovLit(a: string): "str";
declare function ovObj(o: { a: string }): "A";
declare function ovObj(o: { b: number }): "B";
declare function ovGen<T extends string>(a: T): [T];
declare function ovGen(a: number): "num";
declare function ovCb(f: (x: string) => void): "cb1";
declare function ovCb(f: (x: string, y: number) => void): "cb2";
interface Ov2 { (a: string): 1; (a: boolean): 2 }
declare const ov2: Ov2;
declare const ovUnion: ((a: string) => "L") | ((a: string) => "R");
export function oStr() { return ov("s"); }
export function oNum() { return ov(1); }
export function oUnion(v: string | number) { return ov(v); }
export function oArity0() { return ovArity(); }
export function oArity1() { return ovArity("a"); }
export function oArity2() { return ovArity("a", "b"); }
export function oOpt() { return ovOpt("a"); }
export function oRest() { return ovRest(1, 2, 3); }
export function oRestNone() { return ovRest(); }
export function oLitExact() { return ovLit("x"); }
export function oLitOther() { return ovLit("y"); }
export function oLitWide(s: string) { return ovLit(s); }
export function oObjA() { return ovObj({ a: "s" }); }
export function oObjB() { return ovObj({ b: 1 }); }
export function oGenStr() { return ovGen("k"); }
export function oGenNum() { return ovGen(2); }
export function oInterface() { return ov2(true); }
export function oUnionCallee() { return ovUnion("a"); }
export function oSpread(t: [string]) { return ovArity(...t); }
"##;

/// A call takes the first overload its arguments fit, by argument type, arity,
/// optional and rest parameters, a literal-typed parameter, an object argument
/// and a generic overload; a spread tuple argument counts its elements.
#[test]
fn overloads_resolve_as_the_checker_resolves_them() {
    let matrix = Matrix::new(OVERLOADS);
    let failures = matrix.returns(&[
        ("oStr", "\"S\""),
        ("oNum", "\"N\""),
        ("oUnion", "\"U\""),
        ("oArity0", "0"),
        ("oArity1", "1"),
        ("oArity2", "2"),
        ("oOpt", "\"opt\""),
        ("oRest", "\"rest\""),
        ("oRestNone", "\"rest\""),
        ("oLitExact", "\"X\""),
        ("oLitOther", "\"str\""),
        ("oLitWide", "\"str\""),
        ("oObjA", "\"A\""),
        ("oObjB", "\"B\""),
        ("oGenStr", "[\"k\"]"),
        ("oGenNum", "\"num\""),
        ("oInterface", "2"),
        ("oSpread", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A callee typed `((a: string) => "L") | ((a: string) => "R")` has the union
/// signature (`getUnionSignatures`), so the call returns `"L" | "R"`.
///
/// What the lane gives:
/// - `oUnionCallee`: the checker answers `"L" | "R"`; the lane measured
///   `<opaque UnmodeledPosition>` degraded by UnrepresentableCallee.
#[test]
#[ignore = "a call through a union of compatible signatures returns the union of their results"]
fn a_call_through_a_union_of_signatures_unions_the_results() {
    let matrix = Matrix::new(OVERLOADS);
    let failures = matrix.returns(&[("oUnionCallee", "\"L\" | \"R\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Generic declarations inferring from values, arrays, objects, callbacks and
/// defaults.
const GENERIC_INFERENCE: &str = r##"
declare function id<T>(x: T): T;
declare function pair<A, B>(a: A, b: B): [A, B];
declare function first<T>(xs: T[]): T;
declare function wrap<T>(x: T): { v: T };
declare function map<T, U>(xs: T[], f: (x: T) => U): U[];
declare function pick<T, K extends keyof T>(o: T, k: K): T[K];
declare function keys<T>(o: T): (keyof T)[];
declare function def<T = string>(): T;
declare function constrained<T extends { n: number }>(x: T): T;
declare function arr<T>(...xs: T[]): T[];
declare function union<T>(a: T, b: T): T;
declare function tuple<T extends unknown[]>(...xs: T): T;
declare function fnArg<R>(f: () => R): R;
declare function fnParam<P>(f: (p: P) => void): P;
declare function promiseLike<T>(x: { then(cb: (v: T) => void): void }): T;
declare function literal<T extends string>(x: T): T;
declare function objLit<T>(x: { a: T; b: T }): T;
declare function nested<T>(x: { deep: { v: T[] } }): T;
declare function readonlyArr<T>(xs: readonly T[]): T;
declare function partialInfer<T, U = number>(x: T): [T, U];
export function gId() { return id("s"); }
export function gIdConst() { const v = id("s"); return v; }
export function gIdNum() { return id(1); }
export function gIdObj() { return id({ a: 1 }); }
export function gPair() { return pair(1, "s"); }
export function gFirst() { return first([1, 2]); }
export function gFirstMixed() { return first([1, "s"]); }
declare function firstRo<T>(xs: readonly T[]): T;
declare const gTup: [1, "s"];
declare const gTupRest: [1, ...string[]];
export function gFirstThree() { return first([1, "s", true]); }
export function gFirstTuple() { return first(gTup); }
export function gFirstRest() { return first(gTupRest); }
export function gFirstObjects() { return first([{ a: 1 }, { b: "x" }]); }
export function gFirstReadonly() { return firstRo([1, 2]); }
export function gFnExpr() { return fnArg(function () { return 42; }); }
export function gWrap() { return wrap(true); }
export function gMap(xs: number[]) { return map(xs, (x) => "" + x); }
export function gMapObj(xs: number[]) { return map(xs, (x) => ({ x })); }
export function gMapLit(xs: number[]) { return map(xs, (x) => 1); }
export function gMapId(xs: number[]) { return map(xs, (x) => x); }
declare function pipe2<A, B, C>(a: A, f: (a: A) => B, g: (b: B) => C): C;
export function gPipe() { return pipe2(1, (a) => a + "", (b) => b > ""); }
declare function apply<T>(cb: (v: string) => T): T;
declare function applyN<T extends number>(cb: (v: string) => T): T;
export function gApplyLit() { return apply((v) => 1); }
export function gApplyLitN() { return applyN((v) => 1); }
export function gApplyBranch(b: boolean) { return apply((v) => b ? 1 : 2); }
export function gPick(o: { a: string; b: number }) { return pick(o, "b"); }
export function gKeys(o: { a: string; b: number }) { return keys(o); }
export function gDefault() { return def(); }
export function gExplicit() { return def<number>(); }
export function gConstrained() { return constrained({ n: 1, m: "x" }); }
export function gArr() { return arr(1, 2, 3); }
export function gUnion() { return union(1, 2); }
export function gTuple() { return tuple(1, "a"); }
export function gFnArg() { return fnArg(() => 42); }
export function gFnArgObj() { return fnArg(() => ({ k: "v" })); }
export function gFnParam() { return fnParam((p: string) => {}); }
export function gThen(x: { then(cb: (v: number) => void): void }) { return promiseLike(x); }
export function gLiteral() { return literal("lit"); }
export function gObjLit() { return objLit({ a: 1, b: 2 }); }
export function gNested() { return nested({ deep: { v: ["x"] } }); }
export function gReadonly(xs: readonly boolean[]) { return readonlyArr(xs); }
export function gPartial() { return partialInfer("s"); }
export function gIdUnionArg(v: string | number) { return id(v); }
export function gIdNull() { return id(null); }
export function gIdUndefined() { return id(undefined); }
declare function nbox<T>(x: T): { v: T };
export function gBoxNull() { return nbox(null); }
declare function ntwo<T>(x: T, y: T): T;
export function gTwoNull() { return ntwo(null, 1); }
"##;

/// A generic call infers its type arguments from its arguments (widening
/// literals for an unconstrained parameter, keeping them for one constrained to
/// a primitive or with several candidates of one literal kind), from nested
/// positions, from a callback's declared parameter, from a method's callback,
/// and falls back to the default or the explicit arguments.
#[test]
fn generic_calls_infer_as_the_checker_infers() {
    let matrix = Matrix::new(GENERIC_INFERENCE);
    let failures = matrix.returns(&[
        ("gId", "string"),
        ("gIdConst", "string"),
        ("gIdNum", "number"),
        ("gIdObj", "{ a: number; }"),
        ("gPair", "[number, string]"),
        ("gFirst", "number"),
        ("gWrap", "{ v: boolean; }"),
        ("gPick", "number"),
        ("gDefault", "string"),
        ("gExplicit", "number"),
        ("gConstrained", "{ n: number; m: string; }"),
        ("gArr", "number[]"),
        ("gUnion", "1 | 2"),
        ("gTuple", "[number, string]"),
        ("gFnArgObj", "{ k: string; }"),
        ("gFnParam", "string"),
        ("gThen", "number"),
        ("gLiteral", "\"lit\""),
        ("gObjLit", "number"),
        ("gNested", "string"),
        ("gReadonly", "boolean"),
        ("gPartial", "[string, number]"),
        ("gIdUnionArg", "string | number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An array literal or a tuple infers an array's element from ONE candidate,
/// the union of its element types (`inferFromIndexTypes`): `first([1, "s"])`
/// with `first<T>(xs: T[]): T` is `string | number`, and a declared tuple keeps
/// its literal elements (TypeScript 7.0.2, all four settings alike).
#[test]
fn array_element_candidates_infer_as_the_checker_infers_them() {
    let matrix = Matrix::new(GENERIC_INFERENCE);
    let failures = matrix.returns(&[
        ("gFirstMixed", "string | number"),
        ("gFirstThree", "string | number | boolean"),
        ("gFirstTuple", "\"s\" | 1"),
        ("gFirstRest", "string | 1"),
        (
            "gFirstObjects",
            "{ a: number; b?: undefined; } | { a?: undefined; b: string; }",
        ),
        ("gFirstReadonly", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `map(xs, (x) => "" + x)` with `map<T, U>(xs: T[], f: (x: T) => U): U[]`
/// fixes `T` from `xs`, then types the callback under `(x: number) => U`
/// and infers `U` from its return, a single literal return widening unless
/// the contextual return is a literal context for it, through an uninferred
/// parameter's constraint (TypeScript 7.0.2, all four settings alike).
#[test]
fn a_callback_return_infers_as_the_checker_infers_it() {
    let matrix = Matrix::new(GENERIC_INFERENCE);
    let failures = matrix.returns(&[
        ("gMap", "string[]"),
        ("gMapObj", "{ x: number; }[]"),
        ("gMapLit", "number[]"),
        ("gMapId", "number[]"),
        ("gPipe", "boolean"),
        ("gApplyLit", "number"),
        ("gApplyLitN", "1"),
        ("gApplyBranch", "1 | 2"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `fnArg(function () { return 42; })` and `fnArg(() => 42)` with `fnArg<R>(f:
/// () => R): R` infer the widened `number` from the function's return: a
/// function value whose parameters are all annotated is typed in the call's
/// first pass, and its lone fresh literal return widens under a contextual
/// return type that is no literal context for it (TypeScript 7.0.2, all four
/// settings alike).
#[test]
fn a_function_value_return_infers_its_type_parameter_widened() {
    let matrix = Matrix::new(GENERIC_INFERENCE);
    let failures = matrix.returns(&[("gFnExpr", "number"), ("gFnArg", "number")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `keys(o)` with `keys<T>(o: T): (keyof T)[]` and `o: { a: string; b: number
/// }` is `("a" | "b")[]`. Wrong-but-clean (unreduced): the lane keeps `keyof`
/// over the object literal type.
///
/// What the lane gives:
/// - `gKeys`: the checker answers `("a" | "b")[]`; the lane measured `(keyof {
///   a: string; b: number; })[]`.
#[test]
#[ignore = "keyof an inferred object argument reduces to its key union"]
fn wrong_clean_a_keyof_of_an_inferred_object_prints_its_keys() {
    let matrix = Matrix::new(GENERIC_INFERENCE);
    let failures = matrix.returns(&[("gKeys", "(\"a\" | \"b\")[]")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Without `strictNullChecks` an inference of `null` or `undefined` widens to
/// `any` (`getWidenedType` over the covariant inference): `id(null)` and
/// `id(undefined)` are `any`, and `nbox(null)` is `{ v: any; }`.
#[test]
fn a_nullish_argument_infers_as_the_checker_widens_it() {
    let matrix = Matrix::new(GENERIC_INFERENCE);
    let failures = matrix.nullness(&[
        (Read::Return("gIdNull"), "null", "any"),
        (Read::Return("gIdUndefined"), "undefined", "any"),
        (Read::Return("gBoxNull"), "{ v: null; }", "{ v: any; }"),
        (Read::Return("gTwoNull"), "1 | null", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `const` type parameters, `as const` arguments, contextually typed callbacks
/// and literals.
const CONST_AND_CONTEXT: &str = r##"
declare function c1<const T>(x: T): T;
declare function c2<const T extends readonly unknown[]>(x: T): T;
declare function c3<const T extends { k: string }>(x: T): T;
declare function nonConst<T>(x: T): T;
declare function cb(f: (x: number, y: string) => void): void;
declare function takesHandler(h: { on(e: "a" | "b"): void }): void;
type Handler = (ev: { kind: "click"; x: number }) => void;
declare function reg(h: Handler): void;
declare function withCtx<T>(f: (x: T) => T, v: T): T;
export function kConstStr() { return c1("a"); }
export function kConstObj() { return c1({ a: 1, b: ["x"] }); }
export function kConstArr() { return c1([1, 2]); }
export function kConstReadonlyArr() { return c2([1, "a"]); }
export function kConstConstrained() { return c3({ k: "v", n: 1 }); }
export function kNonConstObj() { return nonConst({ a: 1 }); }
export function kNonConstArr() { return nonConst([1, 2]); }
export function kAsConst() { return nonConst([1, 2] as const); }
declare function arrOf<T>(x: readonly T[]): T;
export function kAsConstObj() { return nonConst({ a: 1 } as const); }
export function kAsConstNested() { return nonConst(["a", ["b"]] as const); }
export function kAsConstElement() { return arrOf([1, 2] as const); }
export function kAsConstScalar() { return nonConst("s" as const); }
export function kCtxParam() { let seen: unknown; cb((x, y) => { seen = x; }); return seen; }
export function kCtxArrow() { const f: (a: string) => number = (a) => 1; return f; }
export function kCtxReturnLiteral() { const f: () => "a" | "b" = () => "a"; return f(); }
export function kCtxObjMethod() { const o: { m(x: number): void } = { m(x) {} }; return o; }
export function kCtxHandler() { let got: unknown; reg((ev) => { got = ev.kind; }); return got; }
export function kCtxTuple() { const t: [number, string] = [1, "a"]; return t; }
export function kCtxWithInfer() { return withCtx((x) => x, 3); }
export function kCtxStr() { return withCtx((x) => x, "s"); }
export function kSatisfies() { const v = { a: 1 } satisfies { a: number }; return v; }
export function kSatisfiesLiteral() { const v = "x" satisfies string; return v; }
export function kAnnotatedArr() { const a: readonly ("x" | "y")[] = ["x"]; return a; }
export function kReturnCtx(): () => "q" { return () => "q"; }
export function kReturnWide(): () => number { return () => 1; }
export function kReturnParam(): (x: number) => number { return (x) => x; }
export function kIIFECtx() { return ((x: number) => x * 2)(3); }
"##;

/// A `const` type parameter infers readonly literal types, a non-const one
/// widens; a contextual type types callback parameters, an arrow's return, an
/// object method and a tuple literal; `satisfies` checks without changing the
/// literal's widening.
#[test]
fn const_type_parameters_and_contextual_types_apply_as_the_checker_applies_them() {
    let matrix = Matrix::new(CONST_AND_CONTEXT);
    let failures = matrix.returns(&[
        ("kConstStr", "\"a\""),
        (
            "kConstObj",
            "{ readonly a: 1; readonly b: readonly [\"x\"]; }",
        ),
        ("kConstArr", "readonly [1, 2]"),
        ("kConstReadonlyArr", "readonly [1, \"a\"]"),
        ("kConstConstrained", "{ readonly k: \"v\"; readonly n: 1; }"),
        ("kNonConstObj", "{ a: number; }"),
        ("kNonConstArr", "number[]"),
        ("kCtxParam", "unknown"),
        ("kCtxArrow", "(a: string) => number"),
        ("kCtxReturnLiteral", "\"a\" | \"b\""),
        ("kCtxObjMethod", "{ m(x: number): void; }"),
        ("kCtxHandler", "unknown"),
        ("kCtxTuple", "[number, string]"),
        ("kSatisfies", "{ a: number; }"),
        ("kSatisfiesLiteral", "string"),
        ("kAnnotatedArr", "readonly (\"x\" | \"y\")[]"),
        ("kIIFECtx", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An `as const` argument infers the type its const context spells:
/// `nonConst([1, 2] as const)` is `readonly [1, 2]`, an object `{ readonly a:
/// 1; }`, and an array parameter's element `1 | 2` (TypeScript 7.0.2, all
/// four settings alike).
#[test]
fn an_as_const_argument_infers_as_the_checker_reads_it() {
    let matrix = Matrix::new(CONST_AND_CONTEXT);
    let failures = matrix.returns(&[
        ("kAsConst", "readonly [1, 2]"),
        ("kAsConstObj", "{ readonly a: 1; }"),
        ("kAsConstNested", "readonly [\"a\", readonly [\"b\"]]"),
        ("kAsConstElement", "1 | 2"),
        ("kAsConstScalar", "\"s\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `withCtx((x) => x, 3)` with `withCtx<T>(f: (x: T) => T, v: T): T` defers the
/// context-sensitive arrow, infers `T` from `3`, fixes it widened to `number`
/// where the arrow reads it, and returns `number` (TypeScript 7.0.2, all four
/// settings alike).
#[test]
fn a_context_sensitive_argument_is_typed_as_the_checker_types_it() {
    let matrix = Matrix::new(CONST_AND_CONTEXT);
    let failures = matrix.returns(&[("kCtxWithInfer", "number"), ("kCtxStr", "string")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A returned arrow is typed under the declared return type: `function
/// kReturnCtx(): () => "q" { return () => "q"; }` returns `() => "q"`, its
/// literal return kept by the literal context, while `(): () => number`
/// widens `() => 1` to `() => number` and `(): (x: number) => number` types
/// `(x) => x`'s parameter (TypeScript 7.0.2, all four settings alike).
#[test]
fn a_returned_arrow_takes_the_declared_return_as_context() {
    let matrix = Matrix::new(CONST_AND_CONTEXT);
    let failures = matrix.returns(&[
        ("kReturnCtx", "() => \"q\""),
        ("kReturnWide", "() => number"),
        ("kReturnParam", "(x: number) => number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Generic calls over literal arguments, directly and nested, under an
/// unconstrained, a primitive-constrained and a literal-constrained type
/// parameter, a `const` one, an `as const` argument, a contextual type (a
/// callback's return inferring a type parameter, a contextually typed arrow)
/// and a `const` or `let` binding of the result.
const LITERAL_INFERENCE: &str = r##"
declare function f<T>(x: T): T;
declare function g<T>(x: T): T[];
declare function k<T>(x: T): { v: T };
declare function h<T>(x: T): T | undefined;
declare function n<T extends number>(x: T): T;
declare function sn<T extends string | number>(x: T): T;
declare function lit<T extends 1 | 2>(x: T): T;
declare function c<const T>(x: T): T;
declare const one: 1;
export function wDirect() { return f(1); }
export function wNested() { return f(f(1)); }
export function wNestedThree() { return f(f(f("s"))); }
export function wIntoArray() { return g(f(1)); }
export function wArrayInto() { return f(g(1)); }
export function wIntoObject() { return k(f(1)); }
export function wIntoUnion() { return f(h(1)); }
export function wBoolean() { return f(f(true)); }
export function wConstBinding() { const x = f(f(1)); return x; }
export function wLetBinding() { let x = f(f(1)); return x; }
export function wObjectMember() { const o = { a: f(f(1)) }; return o; }
export function wArrayElement() { return [f(f(1))]; }
export function wConditional(b: boolean) { return b ? f(f(1)) : f(2); }
export function wNumber() { return n(n(1)); }
export function wStringOrNumber() { return sn(sn("a")); }
export function wLiteral() { return lit(lit(1)); }
export function wNumberInto() { return f(n(1)); }
export function wIntoNumber() { return n(f(1)); }
export function wConst() { return c(c(1)); }
export function wConstInto() { return f(c(1)); }
export function wIntoConst() { return c(f(1)); }
export function wAsConst() { return f(f(1 as const)); }
export function wAsConstLet() { let x = f(f(1 as const)); return x; }
export function wRegular() { return f(f(one)); }
export function wContextual() { const x: 1 = f(f(1)); return x; }
declare function run<R>(cb: () => R): R;
declare function runN<R extends number>(cb: () => R): R;
export function wCallbackReturn() { return run(() => f(f(1))); }
export function wCallbackLiteralReturn() { return runN(() => f(f(1))); }
export function wContextualReturn() { const r: () => 1 = () => f(f(1)); return r; }
export function wImmediate() { return (() => f(f(1)))(); }
export function wUnionLet() { let x = f(h(1)); return x; }
const mTop = f(f(1));
let mLet = f(f(1));
const mArr = g(f(1));
const mDirect = f(1);
export function wModule() { return mTop; }
export function wModuleLet() { return mLet; }
export function wModuleArray() { return mArr; }
export function wModuleDirect() { return mDirect; }
export function wModuleArgument() { return f(mTop); }
"##;

/// An unconstrained type parameter inferred from a fresh literal argument is
/// that FRESH literal (it sits at the top level of the return, so the
/// candidate is not widened), and so is the call's result: a nested call
/// hands the fresh literal on as its own argument, and the return, a mutable
/// binding or member widens it (`f(f(1))` is `number`). A primitive or
/// literal constraint, a `const` type parameter, an `as const` argument and a
/// regular literal argument infer the REGULAR literal, which no position
/// widens; a union of fresh literals is not a unit type, so the return keeps
/// it (`b ? f(f(1)) : f(2)` is `1 | 2`). A function value's lone fresh literal
/// return widens unless its parameter's contextual return type is a literal
/// context (`run(() => f(f(1)))` over `run<R>(cb: () => R): R` is `number`,
/// `runN(() => f(f(1)))` over `R extends number` is `1`). Measured on TypeScript 7.0.2 under
/// all four settings.
#[test]
fn a_generic_call_keeps_or_widens_a_literal_as_the_checker_infers_it() {
    let matrix = Matrix::new(LITERAL_INFERENCE);
    let mut failures = matrix.returns(&[
        ("wDirect", "number"),
        ("wNested", "number"),
        ("wNestedThree", "string"),
        ("wIntoArray", "number[]"),
        ("wArrayInto", "number[]"),
        ("wIntoObject", "{ v: number; }"),
        ("wBoolean", "boolean"),
        ("wConstBinding", "number"),
        ("wLetBinding", "number"),
        ("wObjectMember", "{ a: number; }"),
        ("wArrayElement", "number[]"),
        ("wConditional", "1 | 2"),
        ("wNumber", "1"),
        ("wStringOrNumber", "\"a\""),
        ("wLiteral", "1"),
        ("wNumberInto", "1"),
        ("wIntoNumber", "1"),
        ("wConst", "1"),
        ("wConstInto", "1"),
        ("wIntoConst", "1"),
        ("wAsConst", "1"),
        ("wAsConstLet", "1"),
        ("wRegular", "1"),
        ("wContextual", "1"),
        ("wContextualReturn", "() => 1"),
        ("wImmediate", "number"),
        ("wCallbackReturn", "number"),
        ("wCallbackLiteralReturn", "1"),
        ("wModuleArray", "number[]"),
    ]);
    failures.extend(matrix.nullness(&[(Read::Return("wIntoUnion"), "1 | undefined", "number")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A union a call returns keeps its fresh literal for a mutable binding to
/// widen: `let x = f(h(1))` over `h<T>(x: T): T | undefined` is `number |
/// undefined`. Measured on TypeScript 7.0.2 under all four settings.
///
/// What the lane gives:
/// - `wUnionLet`: the checker answers `number | undefined` (`number` without
///   `strictNullChecks`); the lane measured `undefined | 1` with
///   `strictNullChecks`.
#[test]
#[ignore = "a call's fresh union constituent widens in a mutable binding"]
fn wrong_clean_a_fresh_union_constituent_of_a_nested_call_widens() {
    let matrix = Matrix::new(LITERAL_INFERENCE);
    let failures = matrix.nullness(&[(Read::Return("wUnionLet"), "number | undefined", "number")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A module-level declaration initialized by a call that returns a fresh
/// literal declares that fresh literal: a `let` widens it (`let mLet =
/// f(f(1))` is `number`), and a read of a `const` is a fresh literal source
/// that the return widens (`return mDirect` over `const mDirect = f(1)` is
/// `number`, as `f(mTop)` over `const mTop = f(f(1))` is). Measured on
/// TypeScript 7.0.2 under all four settings.
///
/// What the lane gives:
/// - `wModule`, `wModuleLet`, `wModuleDirect`, `wModuleArgument`: the checker
///   answers `number`; the lane measured `1`.
#[test]
#[ignore = "a module declaration initialized by a fresh-literal call declares the fresh literal"]
fn wrong_clean_a_module_declaration_of_a_fresh_call_result_widens() {
    let matrix = Matrix::new(LITERAL_INFERENCE);
    let failures = matrix.returns(&[
        ("wModule", "number"),
        ("wModuleLet", "number"),
        ("wModuleDirect", "number"),
        ("wModuleArgument", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
