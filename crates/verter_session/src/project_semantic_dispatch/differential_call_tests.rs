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
declare const ovUnionOpt: ((a: string) => "L") | ((a: string, b?: number) => "R");
declare const ovUnionObj: ((a: { x: 1 }) => "L") | ((a: { y: 2 }) => "R");
declare const ovUnion3: (() => 1) | (() => 2) | (() => 3);
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
export function oUnionOpt() { return ovUnionOpt("a"); }
export function oUnionObj() { return ovUnionObj({ x: 1, y: 2 }); }
export function oUnion3() { return ovUnion3(); }
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
/// signature (`getUnionSignatures`), so the call returns `"L" | "R"`; an
/// optional extra parameter, object parameters (intersected) and a
/// parameterless union alike.
#[test]
fn a_call_through_a_union_of_signatures_unions_the_results() {
    let matrix = Matrix::new(OVERLOADS);
    let failures = matrix.returns(&[
        ("oUnionCallee", "\"L\" | \"R\""),
        ("oUnionOpt", "\"L\" | \"R\""),
        ("oUnionObj", "\"L\" | \"R\""),
        ("oUnion3", "1 | 2 | 3"),
    ]);
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
/// }` is `("a" | "b")[]`: the instantiation reduces `keyof` over the object
/// literal type wherever it sits in the declared return.
#[test]
fn a_keyof_of_an_inferred_object_prints_its_keys() {
    let matrix = Matrix::new(GENERIC_INFERENCE);
    let failures = matrix.returns(&[("gKeys", "(\"a\" | \"b\")[]")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Declared returns whose operators the call's instantiation closes.
const INSTANTIATED_OPERATORS: &str = r##"
declare function keys<T>(o: T): (keyof T)[];
declare function kbox<T>(o: T): { k: keyof T };
declare function pickArr<T, K extends keyof T>(o: T, k: K): T[K][];
declare function pickBox<T, K extends keyof T>(o: T, k: K): { v: T[K] };
declare function keyU<T>(o: T): keyof T | undefined;
interface I { a: string; b: number }
export function n0(o: I) { return keys(o); }
export function n1(o: { a: string; b: number }) { return kbox(o); }
export function n2(o: { a: string; b: number }) { return pickArr(o, "a"); }
export function n3(o: { a: string; b: number }) { return pickBox(o, "b"); }
export function n4(o: { a: string; b: number }) { return keyU(o); }
export function n5() { return keys({ x: 1, y: "s" }); }
export function n6(o: { 0: string; b: number }) { return keys(o); }
type A = { a: string };
class C { x = 1 }
export function n7(o: A) { return keys(o); }
export function n8(o: C) { return keys(o); }
"##;

/// A `keyof` or an indexed access the call's instantiation closes is
/// reduced inside the declared return as at its top (`getIndexType`,
/// `getIndexedAccessType` over non-generic operands): an array element, an
/// object member, a numeric key. A `keyof` over an interface, a class or an
/// alias keeps its name. TypeScript 7.0.2, all four settings alike but for the `undefined`
/// arm `strictNullChecks` drops.
#[test]
fn an_operator_the_instantiation_closes_is_reduced_in_the_declared_return() {
    let matrix = Matrix::new(INSTANTIATED_OPERATORS);
    let mut failures = matrix.returns(&[
        ("n0", "(keyof I)[]"),
        ("n1", "{ k: \"a\" | \"b\"; }"),
        ("n2", "string[]"),
        ("n3", "{ v: number; }"),
        ("n5", "(\"x\" | \"y\")[]"),
        ("n6", "(\"b\" | 0)[]"),
        ("n7", "(keyof A)[]"),
        ("n8", "(keyof C)[]"),
    ]);
    failures.extend(matrix.nullness(&[(
        Read::Return("n4"),
        "\"a\" | \"b\" | undefined",
        "\"a\" | \"b\"",
    )]));
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
declare function gd<T>(x: T | undefined): T;
declare function gn<T>(x: T | undefined | null): T;
export function pDef(x: string | undefined) { return gd(x); }
export function pDefN(x: string | undefined) { return gn(x); }
export function pDefNN(x: string | null | undefined) { return gn(x); }
declare function gsu<T>(x: T | string): T;
declare function gbo<T>(x: T | { a: 1 }): T;
export function pDefLit(x: "a" | 1) { return gsu(x); }
export function pDefObj(x: number | { a: 1 }) { return gbo(x); }
export function pDefThree(x: string | number | undefined) { return gd(x); }
export function wUnionLet() { let x = f(h(1)); return x; }
declare function hn<T extends number>(x: T): T | undefined;
export function wUnionArray() { return g(h(1)); }
export function wUnionMember() { return { a: f(h(1)) }; }
export function wUnionConstrained() { let x = f(hn(1)); return x; }
export function wUnionNested() { let x = f(f(h(1))); return x; }
let mUL = f(h(1));
const mUC = f(h(1));
export function wModuleUnionLet() { return mUL; }
export function wModuleUnionConst() { return mUC; }
export function wModuleUnionConstLet() { let x = mUC; return x; }
const mTop = f(f(1));
let mLet = f(f(1));
const mArr = g(f(1));
const mDirect = f(1);
const mN = n(1);
const mReg = f(one);
export const mExp = f(1);
export function wModule() { return mTop; }
export function wModuleLet() { return mLet; }
export function wModuleArray() { return mArr; }
export function wModuleDirect() { return mDirect; }
export function wModuleArgument() { return f(mTop); }
export function wModuleConstrained() { return mN; }
export function wModuleRegular() { return mReg; }
export function wModuleExported() { return mExp; }
export function wModuleTypeof() { const x: typeof mExp = mExp; return x; }
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
        ("pDef", "string"),
        ("pDefN", "string"),
        ("pDefNN", "string"),
        ("pDefLit", "1"),
        ("pDefObj", "number"),
        ("pDefThree", "string | number"),
        ("wCallbackReturn", "number"),
        ("wCallbackLiteralReturn", "1"),
        ("wModuleArray", "number[]"),
    ]);
    failures.extend(matrix.nullness(&[(Read::Return("wIntoUnion"), "1 | undefined", "number")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A union a call returns keeps its fresh literal members through an
/// enclosing call, for a mutable binding, a member or an array element to
/// widen: `let x = f(h(1))` over `h<T>(x: T): T | undefined` is `number |
/// undefined`, `g(h(1))` over `g<T>(x: T): T[]` is `(number | undefined)[]`;
/// a constrained parameter's literal stays regular (`1 | undefined`). A
/// module `let` of such a call widens it, and a module `const` of one reads
/// as that fresh union, which the return keeps (not a unit type) and a
/// mutable binding widens.
/// Measured on TypeScript 7.0.2 under all four settings.
#[test]
fn a_fresh_union_constituent_of_a_nested_call_widens() {
    let matrix = Matrix::new(LITERAL_INFERENCE);
    let failures = matrix.nullness(&[
        (Read::Return("wUnionLet"), "number | undefined", "number"),
        (
            Read::Return("wUnionArray"),
            "(number | undefined)[]",
            "number[]",
        ),
        (
            Read::Return("wUnionMember"),
            "{ a: number | undefined; }",
            "{ a: number; }",
        ),
        (Read::Return("wUnionConstrained"), "1 | undefined", "1"),
        (Read::Return("wUnionNested"), "number | undefined", "number"),
        (
            Read::Return("wModuleUnionLet"),
            "number | undefined",
            "number",
        ),
        (Read::Return("wModuleUnionConst"), "1 | undefined", "number"),
        (
            Read::Return("wModuleUnionConstLet"),
            "number | undefined",
            "number",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A module-level declaration initialized by a call that returns a fresh
/// literal declares that fresh literal: a `let` widens it (`let mLet =
/// f(f(1))` is `number`), and a read of a `const` is a fresh literal source
/// that the return widens (`return mDirect` over `const mDirect = f(1)` is
/// `number`, as `f(mTop)` over `const mTop = f(f(1))` is), while its type
/// stays the literal (`typeof mExp` is `1`). A constrained parameter or a
/// regular argument infers a regular literal no read widens. Measured on
/// TypeScript 7.0.2 under all four settings.
#[test]
fn a_module_declaration_of_a_fresh_call_result_widens() {
    let matrix = Matrix::new(LITERAL_INFERENCE);
    let failures = matrix.returns(&[
        ("wModule", "number"),
        ("wModuleLet", "number"),
        ("wModuleDirect", "number"),
        ("wModuleArgument", "number"),
        ("wModuleConstrained", "1"),
        ("wModuleRegular", "1"),
        ("wModuleExported", "number"),
        ("wModuleTypeof", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Generic methods of a generic interface, instantiated by the receiver.
const METHOD_BINDERS: &str = r##"
interface Bx<T> {
  pick<S extends T>(x: S): S;
  pickU<S>(x: S): S;
  guard<S extends T>(p: (v: T) => v is S): S;
}
declare const bx: Bx<string | number>;
interface Plain { pick<S extends string>(x: S): S; }
declare const pl: Plain;
export function mPick() { return bx.pick("a"); }
export function mPickUnconstrained() { return bx.pickU("a"); }
export function mGuard() { return bx.guard((v: string | number): v is string => true); }
export function mPlain() { return pl.pick("a"); }
"##;

/// A method's type parameter constrained by the interface's own parameter
/// infers from its arguments once the receiver instantiates the interface:
/// `bx.pick("a")` over `pick<S extends T>(x: S): S` on `Bx<string | number>`
/// is `"a"` (the constraint makes the literal context), and an annotated
/// guard callback infers `S` from its predicate (`string`). Measured on
/// TypeScript 7.0.2 under all four settings.
#[test]
fn a_constrained_method_type_parameter_infers_on_an_instantiated_interface() {
    let matrix = Matrix::new(METHOD_BINDERS);
    let failures = matrix.returns(&[
        ("mPick", "\"a\""),
        ("mPickUnconstrained", "string"),
        ("mGuard", "string"),
        ("mPlain", "\"a\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Arguments whose types a declaration names, against naked type
/// parameters.
const NAMED_ARGUMENTS: &str = r##"
declare function id<T>(o: T): T;
declare function wrap<T>(o: T): { v: T };
declare function arr<T>(o: T): T[];
interface I { a: string; b: number }
type A = { a: string };
type U = "a" | "b";
type S = string;
class C { x = 1 }
interface G<X> { g: X }
export function b1(o: I) { return id(o); }
export function b2(o: I) { return wrap(o); }
export function b3(o: A) { return id(o); }
export function b4(o: U) { return id(o); }
export function b5(o: S) { return id(o); }
export function b6(o: C) { return arr(o); }
export function b7(o: G<number>) { return id(o); }
export function b8(o: I | undefined) { return id(o); }
"##;

/// An argument whose type a declaration names infers that declaration:
/// `id(o)` over `o: I` is `I`, `wrap(o)` is `{ v: I; }`, and an alias of an
/// object or a union type is kept by name too; an alias of an intrinsic
/// type is that type (`type S = string` makes `id(o)` a `string`).
/// TypeScript 7.0.2, all four settings alike but for the `undefined` arm
/// `strictNullChecks` drops.
#[test]
fn a_declared_argument_type_infers_by_its_name() {
    let matrix = Matrix::new(NAMED_ARGUMENTS);
    let mut failures = matrix.returns(&[
        ("b1", "I"),
        ("b2", "{ v: I; }"),
        ("b3", "A"),
        ("b4", "U"),
        ("b5", "string"),
        ("b6", "C[]"),
        ("b7", "G<number>"),
    ]);
    failures.extend(matrix.nullness(&[(Read::Return("b8"), "I | undefined", "I")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Calls whose argument meets a union parameter holding one type variable.
const NAKED_UNION_INFERENCE: &str = r##"
declare function g<T>(a: T | string, b: T): T;
export function u1(s: string) { return g(s, 1); }
export function u2(s: "a" | "b") { return g(s, 1); }
export function u5(s: "a" | "b") { return g(s, s); }
export function u6(s: string) { return g(s, s); }
declare function h<T>(a: T | string): T;
export function h1(s: "a" | "b") { return h(s); }
export function h2(s: string | number) { return h(s); }
export function h3(s: string) { return h(s); }
declare class Box<U> { put<T>(x: T, y: U | string): T; put(x: unknown, y: unknown): "second"; }
export function outer4<U>(b: Box<U>) { const v = "a" as "a" | "b"; return b.put(1, v); }
declare class Box2<U> { put<T>(x: T, y: U | string): T; }
export function outer5<U>(b: Box2<U>) { const v = "a" as "a" | "b"; return b.put(1, v); }
"##;

/// An argument every member of which a fixed member of the union parameter
/// matches (`string` against `T | string`, `"a" | "b"` by its base) infers
/// the parameter's type variable only below a direct inference, so `b: T`
/// decides it (`number`), and alone it still does (`h1`, `h3`); the
/// unmatched members infer it directly (`h2`). A type parameter of an
/// enclosing declaration is no inference target of the call: `U | string`
/// takes `"a" | "b"` through its `string` member.
/// Measured on TypeScript 7.0.2, alike under all four settings.
#[test]
fn an_argument_a_fixed_union_member_matches_infers_below_a_direct_inference() {
    let matrix = Matrix::new(NAKED_UNION_INFERENCE);
    let failures = matrix.returns(&[
        ("u1", "number"),
        ("u2", "number"),
        ("u5", "\"a\" | \"b\""),
        ("u6", "string"),
        ("h1", "\"a\" | \"b\""),
        ("h2", "number"),
        ("h3", "string"),
        ("outer4", "number"),
        ("outer5", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Declared returns whose union holds an operator the instantiation closes.
const NULLABLE_INSTANTIATED_OPERATORS: &str = r##"
declare function g<T>(x: T): keyof { a: 1 } | T;
export function r1() { return g<undefined>(undefined); }
export function r2() { return g<null>(null); }
declare function h<T>(x: T): { v: { a: 1 }["a"] | T };
export function r3() { return h<null>(null); }
"##;

/// A union rebuilt around a reduced operator keeps the settings' `null` /
/// `undefined` algebra: `keyof { a: 1 } | T` at `T := undefined` is `"a" |
/// undefined` with `strictNullChecks` and `"a"` without it. Measured on
/// TypeScript 7.0.2.
#[test]
fn a_union_rebuilt_around_a_reduced_operator_keeps_the_nullability_algebra() {
    let matrix = Matrix::new(NULLABLE_INSTANTIATED_OPERATORS);
    let failures = matrix.nullness(&[
        (Read::Return("r1"), "\"a\" | undefined", "\"a\""),
        (Read::Return("r2"), "\"a\" | null", "\"a\""),
        (Read::Return("r3"), "{ v: 1 | null; }", "{ v: 1; }"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Overloads whose first candidate relates through a method parameter, and
/// arguments inferred against a union parameter's fixed members.
const OVERLOADS_AND_UNION_MEMBERS: &str = r##"
declare function pick(v: { m(x: string | number): void }): "wide";
declare function pick(v: {}): "fallback";
declare function pickP(v: { m: (x: string | number) => void }): "wide";
declare function pickP(v: {}): "fallback";
declare const a: { m(x: string): void };
export function oMethod() { return pick(a); }
export function oProperty() { return pickP(a); }
declare function inferObject<T>(x: T | { a: unknown }): T;
declare const anyObject: { a: any } | 1;
declare const unknownObject: { a: unknown } | 1;
export function uAny() { return inferObject(anyObject); }
export function uUnknown() { return inferObject(unknownObject); }
declare function inferBoolean<T>(x: T | boolean): T;
declare function inferTrue<T>(x: T | true): T;
declare const trueOrOne: true | 1;
declare const booleanOrOne: boolean | 1;
export function uTrue() { return inferBoolean(trueOrOne); }
export function uBoolean() { return inferBoolean(booleanOrOne); }
export function uFalse() { return inferTrue(booleanOrOne); }
"##;

/// A method member keeps its bivariant parameters in every overload pass,
/// the subtype pass included, so `{ m(x: string): void }` selects the
/// method candidate (`"wide"`) while a function-typed property stays
/// contravariant (`"fallback"`). Measured on TypeScript 7.0.2, alike under
/// all four settings.
#[test]
fn a_method_parameter_stays_bivariant_in_the_overload_subtype_pass() {
    let matrix = Matrix::new(OVERLOADS_AND_UNION_MEMBERS);
    let failures = matrix.returns(&[("oMethod", "\"wide\""), ("oProperty", "\"fallback\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An argument member matches a fixed member of the union parameter only by
/// the checker's identity: `{ a: any }` is not `{ a: unknown }` (`uAny` is
/// `1 | { a: any; }`), and `boolean` on either side is its two literals
/// (`true | 1` against `T | boolean` infers `1`, `boolean | 1` against `T |
/// true` infers `1 | false`). Measured on TypeScript 7.0.2, alike under all
/// four settings.
#[test]
fn a_union_argument_matches_a_fixed_member_by_the_checkers_identity() {
    let matrix = Matrix::new(OVERLOADS_AND_UNION_MEMBERS);
    let failures = matrix.returns(&[
        ("uAny", "1 | { a: any; }"),
        ("uUnknown", "1"),
        ("uTrue", "1"),
        ("uBoolean", "1"),
        ("uFalse", "1 | false"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Calls through a rigid type parameter and through local overloads.
const RIGID_AND_LOCAL_OVERLOADS: &str = r##"
declare class Box2<U> { put<T>(x: T, y: U | string): T; }
export function rigidArgument<U>(b: Box2<U>, u: U) { return b.put(1, u); }
export function localOverloads<U>(u: U) { function k(x: string): 1; function k(x: number): 2; function k(x: any): any { return x; } return k("s"); }
export function localGenericOverloads<U>(u: U) { function k<T>(x: T, y: U | string): "first"; function k(x: unknown, y: unknown): "second"; function k(x: any, y: any): any { return x; } const v = "a" as "a" | "b"; return k(1, v); }
"##;

/// A rigid type parameter argument relates to a union parameter holding it
/// through that identical member, so `b.put(1, u)` over `put<T>(x: T, y: U
/// | string): T` infers `T` from `1`: `number`. Measured on TypeScript
/// 7.0.2, alike under all four settings.
#[test]
fn a_rigid_type_parameter_relates_to_a_union_holding_it() {
    let matrix = Matrix::new(RIGID_AND_LOCAL_OVERLOADS);
    let failures = matrix.returns(&[("rigidArgument", "number")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The rows of local overloaded functions, with the checker's answers:
/// the call resolves over the overload signatures, never the
/// implementation's (`1` and `"first"`). Measured on TypeScript 7.0.2, alike
/// under all four settings.
const LOCAL_OVERLOAD_ROWS: [(&str, &str); 2] = [
    ("localOverloads", "1"),
    ("localGenericOverloads", "\"first\""),
];

/// Each local-overload row that does not read the checker's answer, only
/// those the lane publishes clean when `clean_only`.
fn local_overload_misses(clean_only: bool) -> Vec<String> {
    let matrix = Matrix::new(RIGID_AND_LOCAL_OVERLOADS);
    let rows: Vec<(Read<'_>, Vec<&str>)> = LOCAL_OVERLOAD_ROWS
        .iter()
        .map(|(function, answer)| (Read::Return(function), vec![*answer; 4]))
        .collect();
    LOCAL_OVERLOAD_ROWS
        .iter()
        .zip(matrix.verdicts(&rows))
        .flat_map(|((function, _), verdicts)| {
            verdicts
                .into_iter()
                .filter(|verdict| !verdict.matched && (!clean_only || verdict.class != "GAP"))
                .map(move |verdict| format!("`{function}`: {}", verdict.lane))
        })
        .collect()
}

/// A local function with overload signatures is never called through its
/// implementation's signature (whose `any` return the checker never
/// exposes): the call degrades rather than publishing `any`.
#[test]
fn a_local_overloaded_function_is_never_called_through_its_implementation() {
    let wrong = local_overload_misses(true);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// What the lane gives: each row degrades (the overloaded local function
/// has no modelled value).
#[test]
#[ignore = "a local overloaded function resolves the call over its overload signatures"]
fn a_local_overloaded_function_resolves_over_its_overloads() {
    let failures = local_overload_misses(false);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Member calls chained on call results, each receiver its own: every call
/// of a chain starts where the chain does, and each reads the value its own
/// callee's object evaluated to, on every loop pass.
const RECEIVER_CHAINS: &str = r##"
interface A { m(this: A): B }
interface B { m(this: B): C }
interface C { m(this: C): D }
interface D { tag: string | number }
declare const a: A;
declare function cond(): boolean;
export function looped() { let x: string | number = 0; while (cond()) { x = a.m().m().m().tag; } return x; }
export function straight() { return a.m().m().m(); }
"##;

/// TypeScript 7.0.2, under every setting, with no diagnostics:
/// `string | number` looped, `D` straight.
#[test]
fn each_call_of_a_receiver_chain_reads_its_own_receiver() {
    let matrix = Matrix::new(RECEIVER_CHAINS);
    let failures = matrix.returns(&[("looped", "string | number"), ("straight", "D")]);
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}

/// Calls whose value depends on the receiver they are called through: a
/// member's polymorphic `this` (the reference the member is read through
/// binds it), a type parameter a `this` parameter infers, and overloads a
/// `this` parameter selects between.
const RECEIVER_SENSITIVE_CALLS: &str = r##"
interface Box<V> { v: V }
interface S { self(): this; wrap(): Box<this>; pair(o: this): this; n: number }
interface T extends S { t: string }
declare const t: T;
interface G { id<X>(this: X): X; next(): H }
interface H { tag: "h" }
declare const g: G;
class K { self(): this { return this; } k = 1 }
declare function mkT(): T;
declare const u: T | K;
interface O { pick(this: O & { a: 1 }): "a"; pick(this: O): "o" }
declare const o: O;
declare const oa: O & { a: 1 };
export function selfOnce() { return t.self(); }
export function selfTwice() { return t.self().self(); }
export function idOnce() { return g.id(); }
export function idThenNext() { return g.id().next().tag; }
export function onParam(p: T) { return p.self(); }
export function onLocal() { const l = t; return l.self(); }
export function onClass(k: K) { return k.self(); }
export function onResult() { return mkT().self(); }
export function onUnion() { return u.self(); }
export function wrapped() { return t.wrap().v; }
export function paired() { return t.pair(t); }
export function spreadArg() { const a: [T] = [t]; return t.pair(...a); }
export function detached() { const fn = t.self; return fn(); }
export function detachedOnResult() { const fn = mkT().self; return fn(); }
export function parenthesized() { return (t.self)(); }
export function overloadPlain() { return o.pick(); }
export function overloadA() { return oa.pick(); }
export const constSelf = t.self();
export const constId = g.id();
"##;

/// TypeScript 7.0.2, under every setting, with no diagnostics: `T` for
/// every call of `self`, `pair` and `wrap().v` through a `T` (a detached
/// `self` read off one too), `G` and `"h"` through `g`, `K` through `k`,
/// `K | T` through `u`, `"o"` and `"a"` for the overloads, and `T` and `G`
/// for the two module constants.
#[test]
fn a_call_reads_its_receiver_where_its_value_depends_on_it() {
    let matrix = Matrix::new(RECEIVER_SENSITIVE_CALLS);
    let mut failures = matrix.returns(&[
        ("selfOnce", "T"),
        ("selfTwice", "T"),
        ("idOnce", "G"),
        ("idThenNext", "\"h\""),
        ("onParam", "T"),
        ("onLocal", "T"),
        ("onClass", "K"),
        ("onResult", "T"),
        ("onUnion", "K | T"),
        ("wrapped", "T"),
        ("paired", "T"),
        ("spreadArg", "T"),
        ("detached", "T"),
        ("detachedOnResult", "T"),
        ("parenthesized", "T"),
        ("overloadPlain", "\"o\""),
        ("overloadA", "\"a\""),
    ]);
    failures.extend(matrix.same(&[
        (Read::Type("typeof constSelf"), "T"),
        (Read::Type("typeof constId"), "G"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Calls with no receiver: the checker's this-argument is `void`.
const RECEIVERLESS_CALLS: &str = r##"
function d<X>(this: X): X { return this; }
function q<X>(this: X, x: number): X { return this; }
function vo(this: void) { return 1 as const; }
export function direct() { return d(); }
export function inferVoid() { return q(1); }
export function voidThis() { return vo(); }
export function bareGeneric(f: <X>(this: X) => X) { return f(); }
"##;

/// TypeScript 7.0.2, under every setting, with no diagnostics: `void` for
/// each `this` type parameter a bare call infers, and `1` for the
/// `this: void` function.
#[test]
fn a_call_without_a_receiver_passes_void_as_its_this() {
    let matrix = Matrix::new(RECEIVERLESS_CALLS);
    let failures = matrix.returns(&[
        ("direct", "void"),
        ("inferVoid", "void"),
        ("voidThis", "1"),
        ("bareGeneric", "void"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A `super` call of a base member returning its polymorphic `this`: the
/// member's `this` is the calling class's own, which a call through a
/// `B` binds to `B`.
const SUPER_THIS_CALL: &str = r##"
declare class A { self(): this; a: number }
class B extends A { m() { return super.self(); } b = 1 }
export function viaSuper(b: B) { return b.m(); }
"##;

/// TypeScript 7.0.2, under every setting: `B`.
#[test]
fn a_super_call_binds_the_calling_class_this() {
    let matrix = Matrix::new(SUPER_THIS_CALL);
    let failures = matrix.returns(&[("viaSuper", "B")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A method whose body returns `this`, called through a parameter, a
/// module constant and a constructed value.
const BODY_THIS_CALLS: &str = r##"
class K2 { self() { return this; } k = 1 }
declare const k2: K2;
export function bodyThis(k: K2) { return k.self(); }
export function bodyThisConst() { return k2.self(); }
export function bodyThisNew() { return new K2().self(); }
"##;

/// TypeScript 7.0.2, under every setting: `K2` for each call.
#[test]
fn a_method_returning_this_reads_its_receiver() {
    let matrix = Matrix::new(BODY_THIS_CALLS);
    let failures = matrix.returns(&[
        ("bodyThis", "K2"),
        ("bodyThisConst", "K2"),
        ("bodyThisNew", "K2"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Calls of an element (`t["m"]()`, `t[0]()`, `t[k]()` with a literal-typed
/// `k`): the element's object is the call's receiver.
const ELEMENT_CALLS: &str = r##"
interface S { self(): this; n: number; 0(): this; pick<X>(this: X): X }
interface T extends S { t: string }
declare const t: T;
declare const k: "self";
declare const z: 0;
declare const tup: [() => string, (x: number) => number];
declare function mkT(): T;
export function elementCall() { return t["self"](); }
export function templateElement() { return t[`self`](); }
export function elementKeyCall() { return t[k](); }
export function elementNumericCall() { return t[0](); }
export function elementZeroCall() { return t[z](); }
export function elementGenericThis() { return t["pick"](); }
export function tupleCall() { return tup[1](2); }
export function elementOnResult() { return mkT()["self"](); }
export function elementChain() { return t["self"]()["self"](); }
export function paramElement(p: T) { return p["self"](); }
"##;

/// TypeScript 7.0.2, under every setting, with no diagnostics: `T` for
/// each element call through a `T`, `number` for the tuple's.
#[test]
fn an_element_call_reads_its_object_as_its_receiver() {
    let matrix = Matrix::new(ELEMENT_CALLS);
    let failures = matrix.returns(&[
        ("elementCall", "T"),
        ("templateElement", "T"),
        ("elementKeyCall", "T"),
        ("elementNumericCall", "T"),
        ("elementZeroCall", "T"),
        ("elementGenericThis", "T"),
        ("tupleCall", "number"),
        ("elementOnResult", "T"),
        ("elementChain", "T"),
        ("paramElement", "T"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Calls of a call's value: the callee is what the inner call evaluates
/// to, its arguments included.
const CALLS_OF_CALLS: &str = r##"
declare function mk(): (x: number) => string;
declare function g2<T>(x: T): () => T;
export function callOfCall() { return g2(1)(); }
export function callOfCallResult() { return mk()(1); }
export function callOfCallTwice() { return g2(g2("a"))()(); }
"##;

/// TypeScript 7.0.2, under every setting: `number`, `string`, `string`.
#[test]
fn a_call_of_a_call_value_reads_the_inner_call() {
    let matrix = Matrix::new(CALLS_OF_CALLS);
    let failures = matrix.returns(&[
        ("callOfCall", "number"),
        ("callOfCallResult", "string"),
        ("callOfCallTwice", "string"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The ambient `Function` surface the `call` / `apply` / `bind` rows read:
/// `CallableFunction`'s generic signatures under `strictBindCallApply`,
/// `Function`'s `any`-returning ones without it.
const FUNCTION_LIB: &str = r##"interface Object { toString(): string; }
interface Function { apply(this: Function, thisArg: any, argArray?: any): any; call(this: Function, thisArg: any, ...argArray: any[]): any; bind(this: Function, thisArg: any, ...argArray: any[]): any; readonly length: number; }
interface CallableFunction extends Function {
  apply<T, R>(this: (this: T) => R, thisArg: T): R;
  apply<T, A extends any[], R>(this: (this: T, ...args: A) => R, thisArg: T, args: A): R;
  call<T, A extends any[], R>(this: (this: T, ...args: A) => R, thisArg: T, ...args: A): R;
  bind<T>(this: T, thisArg: ThisParameterType<T>): OmitThisParameter<T>;
  bind<T, A extends any[], B extends any[], R>(this: (this: T, ...args: [...A, ...B]) => R, thisArg: T, ...args: A): (...args: B) => R;
}
interface NewableFunction extends Function {}
interface IArguments { [index: number]: any; length: number; }
interface Array<T> { length: number; [n: number]: T; }
interface String {} interface Number {} interface Boolean {} interface RegExp {}
type ThisParameterType<T> = T extends (this: infer U, ...args: never) => any ? U : unknown;
type OmitThisParameter<T> = unknown extends ThisParameterType<T> ? T : T extends (...args: infer A) => infer R ? (...args: A) => R : T;
"##;

/// `Function.prototype`'s `call` / `apply` / `bind` over function values,
/// module functions, call results, local arrows and methods read off
/// objects.
const FUNCTION_CALL_APPLY_BIND: &str = r##"
declare const f: (x: number) => string;
declare function mk(): (x: number) => string;
declare function add(a: number, b: number): number;
interface M { m(this: M, x: number): string; n(x: number): boolean }
declare const obj: M;
interface Ctx { c: 1 }
interface M2 { m(this: Ctx, x: number): string }
declare const obj2: M2;
declare const ctx: Ctx;
declare function id<T>(x: T): T;
declare function ov(x: string): string;
declare function ov(x: number): number;
export function called() { return f.call(null, 1); }
export function calledOnResult() { return mk().call(null, 1); }
export function appliedOnResult() { return mk().apply(null, [1]); }
export function applied() { return add.apply(undefined, [1, 2]); }
export function bound() { return f.bind(null); }
export function boundCalled() { return f.bind(null)(1); }
export function boundPartial() { return add.bind(undefined, 1); }
export function boundPartialCalled() { return add.bind(undefined, 1)(2); }
export function methodCall() { return obj.m.call(obj, 1); }
export function methodApply() { return obj.n.apply(obj, [1]); }
export function methodBind() { return obj.m.bind(obj); }
export function methodCall2() { return obj2.m.call(ctx, 1); }
export function methodBind2() { return obj2.m.bind(ctx); }
export function localArrow() { const g = (x: number) => x > 0; return g.call(undefined, 1); }
export function nestedDecl() { function h(a: number) { return a > 0; } return h.call(undefined, 1); }
export function idCall() { return id.call(null, 1); }
export function idApply() { return id.apply(null, [1]); }
export function ovCall() { return ov.call(null, 1); }
export function ovApply() { return ov.apply(null, [1]); }
export function idBind() { return id.bind(null); }
export function ovBind() { return ov.bind(null); }
"##;

/// TypeScript 7.0.2, under every setting (`strictBindCallApply` on, as
/// `strict` sets it), with no diagnostics: `string` for `call` and
/// `apply` over `(x: number) => string` values and methods, `number` for
/// `add.apply`, `boolean` for the `n` method and the local arrow.
#[test]
fn function_call_and_apply_read_the_function_they_are_read_off() {
    let matrix = Matrix::new(FUNCTION_CALL_APPLY_BIND).lib(FUNCTION_LIB);
    let failures = matrix.returns(&[
        ("called", "string"),
        ("calledOnResult", "string"),
        ("appliedOnResult", "string"),
        ("applied", "number"),
        ("methodCall", "string"),
        ("methodApply", "boolean"),
        ("methodCall2", "string"),
        ("localArrow", "boolean"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// TypeScript 7.0.2 with `strictBindCallApply` off, under every
/// `strictNullChecks` × `noImplicitAny` setting: `any` for every
/// `call` / `apply` / `bind` form (`Function`'s signatures), and for a
/// call of a bound value.
#[test]
fn function_call_apply_and_bind_are_any_without_strict_bind_call_apply() {
    let matrix = Matrix::new(FUNCTION_CALL_APPLY_BIND)
        .lib(FUNCTION_LIB)
        .settings(&super::differential_harness_tests::BIND_CALL_APPLY_OFF);
    let failures = matrix.returns(&[
        ("called", "any"),
        ("calledOnResult", "any"),
        ("appliedOnResult", "any"),
        ("applied", "any"),
        ("bound", "any"),
        ("boundCalled", "any"),
        ("boundPartial", "any"),
        ("boundPartialCalled", "any"),
        ("methodCall", "any"),
        ("methodApply", "any"),
        ("methodBind", "any"),
        ("methodCall2", "any"),
        ("methodBind2", "any"),
        ("localArrow", "any"),
        ("nestedDecl", "any"),
        ("idCall", "any"),
        ("ovCall", "any"),
        ("idBind", "any"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// TypeScript 7.0.2, under every setting (`strictBindCallApply` on), with
/// no diagnostics, through `CallableFunction`'s generic signatures: `bind`
/// with no bound argument gives `(x: number) => string` over `f` and both
/// methods (their `this` omitted, `OmitThisParameter`) and its call
/// `string`; `call` / `apply` over the generic `id` are `unknown` (its
/// base signature) and over the overloaded `ov` `number` (its last
/// signature); `bind` over them is their own type; a nested function
/// declaration's `call` is `boolean`.
#[test]
fn function_bind_and_generic_calls_read_the_function_they_are_read_off() {
    let matrix = Matrix::new(FUNCTION_CALL_APPLY_BIND).lib(FUNCTION_LIB);
    let failures = matrix.returns(&[
        ("bound", "(x: number) => string"),
        ("boundCalled", "string"),
        ("methodBind", "(x: number) => string"),
        ("methodBind2", "(x: number) => string"),
        ("idCall", "unknown"),
        ("idApply", "unknown"),
        ("ovCall", "number"),
        ("ovApply", "number"),
        ("idBind", "<T>(x: T) => T"),
        ("ovBind", "{ (x: string): string; (x: number): number; }"),
        ("nestedDecl", "boolean"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// TypeScript 7.0.2, under every setting (`strictBindCallApply` on):
/// `add.bind(undefined, 1)` is `(b: number) => number` (`bind`'s second
/// overload splits `add`'s parameters as `[...A, ...B]` at the bound
/// arguments' count) and its call `number`.
#[test]
#[ignore = "bind with bound arguments infers no [...A, ...B] parameter split, and degrades as an unrepresentable callee"]
fn a_partially_applied_bind_reads_its_remaining_parameters() {
    let matrix = Matrix::new(FUNCTION_CALL_APPLY_BIND).lib(FUNCTION_LIB);
    let failures = matrix.returns(&[
        ("boundPartial", "(b: number) => number"),
        ("boundPartialCalled", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A member a derived class or interface declares replaces the base's
/// member of the same name.
const OVERRIDDEN_MEMBERS: &str = r##"
declare class C0 { m(): string | number }
declare class C1 extends C0 { m(): number }
interface B0 { m(): string | number }
interface D0 extends B0 { m(): number }
declare const c1: C1;
declare const d0: D0;
export function classDerivedCall() { return c1.m(); }
export function interfaceDerivedCall() { return d0.m(); }
"##;

/// TypeScript 7.0.2, under every setting, with no diagnostics: `number`
/// for both calls, `() => number` for `C1["m"]` and `D0["m"]`.
#[test]
#[ignore = "a derived member intersects the base member of the same name instead of replacing it"]
fn a_derived_member_replaces_the_base_member() {
    let matrix = Matrix::new(OVERRIDDEN_MEMBERS);
    let mut failures = matrix.returns(&[
        ("classDerivedCall", "number"),
        ("interfaceDerivedCall", "number"),
    ]);
    failures.extend(matrix.same(&[
        (Read::Type("C1[\"m\"]"), "() => number"),
        (Read::Type("D0[\"m\"]"), "() => number"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
