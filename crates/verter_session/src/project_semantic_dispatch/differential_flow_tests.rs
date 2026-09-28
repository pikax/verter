//! Differential probes of the checker's control flow: loops, evolving
//! (auto-typed) arrays and variables, assignments, logical operators and
//! their compound writes, closures and captures, `try` / `catch` /
//! `finally`, and unreachable code. Each row is a function of the fixture,
//! answered as its body-derived return.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --declaration --emitDeclarationOnly
//! --strict --noErrorTruncation` under each `strictNullChecks` ×
//! `noImplicitAny` setting: `declare const p: ReturnType<typeof f>; const
//! s: never = p;` read off the TS2322 message (the emitted `.d.ts` prints
//! the same return for every row). A row with one answer answers alike in
//! the four settings; a row with two answers gives the `strictNullChecks`
//! answer then the answer with it off; a row with four answers lists
//! `strict`, `strictNullChecks` off, `noImplicitAny` off and both off.
//! An ignored test asserts the measured answer for rows the lane does not
//! yet answer as the checker does; "wrong-but-clean" marks a lane answer
//! published complete and undegraded.

use super::differential_harness_tests::{Matrix, Read};

/// `while`, `do`, `for`, `for…of`, `for…in`, labeled and infinite loops.
const LOOPS: &str = r##"
declare function cond(): boolean;
declare const items: string[];
export function lWhile() { let x: string | number = 1; while (cond()) { x = "s"; } return x; }
export function lWhileBreak() { let x: string | number | boolean = 1; while (cond()) { x = "s"; if (cond()) break; x = true; } return x; }
export function lDoWhile() { let x: string | number = 1; do { x = "s"; } while (cond()); return x; }
export function lFor() { let x: string | number = 1; for (let i = 0; i < 3; i++) { x = "a"; } return x; }
export function lForCounter() { let i = 0; for (; i < 3; i++) {} return i; }
export function lForOf() { let last: string | undefined; for (const it of items) { last = it; } return last; }
export function lForOfElem() { for (const it of items) { return it; } throw 0; }
export function lForIn(o: { a: 1; b: 2 }) { for (const k in o) { return k; } throw 0; }
export function lContinue() { let x: string | number = 1; for (const it of items) { if (cond()) continue; x = it; } return x; }
export function lNested() { let x: 1 | 2 | 3 = 1; while (cond()) { while (cond()) { x = 2; } x = 3; } return x; }
export function lNarrowInLoop(x: string | null) { while (x === null) { x = "s"; } return x; }
export function lInfinite() { while (true) { if (cond()) return 1; } }
export function lInfiniteBreak() { let x: string | number = 1; while (true) { x = "s"; break; } return x; }
export function lLabeled() { let x: string | number | boolean = 1; outer: for (const a of items) { for (const b of items) { x = "s"; break outer; } x = true; } return x; }
export function lWidenCounter() { let n = 0; while (cond()) n++; return n; }
export function lLetInLoop() { let r: number[] = []; for (let i = 0; i < 3; i++) { const v = i * 2; r = [v]; } return r; }
export function lForOfTuple(t: [string, number]) { for (const v of t) { return v; } throw 0; }
export function lForOfString(s: "ab") { for (const c of s) { return c; } throw 0; }
export function lLoopNull() { let x: string | null = null; for (const it of items) { if (x === null) x = it; } return x; }
"##;

/// A loop's exit joins the entry value with every back edge and `break`:
/// assignments in the body reach the read after the loop, `continue` and
/// labeled `break` carry their values, a `do` body always runs once, an
/// infinite loop is left only by `break` or `return`, and `for…of` / `for…in`
/// bind the element and key types.
#[test]
fn loops_join_their_back_edges_as_the_checker_joins_them() {
    let matrix = Matrix::new(LOOPS);
    let mut failures = matrix.returns(&[
        ("lWhile", "string | number"),
        ("lWhileBreak", "string | number | true"),
        ("lDoWhile", "string"),
        ("lFor", "string | number"),
        ("lForCounter", "number"),
        ("lForOfElem", "string"),
        ("lForIn", "string"),
        ("lContinue", "string | number"),
        ("lNested", "1 | 3"),
        ("lNarrowInLoop", "string"),
        ("lInfinite", "number"),
        ("lInfiniteBreak", "string"),
        ("lLabeled", "string | number | true"),
        ("lWidenCounter", "number"),
        ("lLetInLoop", "number[]"),
        ("lForOfTuple", "string | number"),
        ("lForOfString", "string"),
    ]);
    failures.extend(matrix.nullness(&[
        (Read::Return("lForOf"), "string | undefined", "string"),
        (Read::Return("lLoopNull"), "string | null", "string"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Empty-array and `null` / `undefined` initializers that evolve with later
/// writes.
const EVOLVING: &str = r##"
declare function cond(): boolean;
export function evPush() { const a = []; a.push(1); return a; }
export function evPushTwo() { const a = []; a.push(1); a.push("s"); return a; }
export function evPushCond() { const a = []; if (cond()) a.push(1); return a; }
export function evLetEmpty() { let a = []; a = [1]; return a; }
export function evIndexWrite() { const a = []; a[0] = true; return a; }
export function evLoopPush() { const a = []; for (let i = 0; i < 3; i++) a.push(i); return a; }
export function evUnshift() { const a = []; a.unshift("x"); return a; }
export function evNeverRead() { const a = []; return a; }
export function evNullInit() { let x = null; x = 1; return x; }
export function evUndefInit() { let x; x = "s"; x = 1; return x; }
export function evUndefCond() { let x; if (cond()) x = "s"; return x; }
export function evElemRead() { const a = []; a.push(1); return a[0]; }
"##;

/// Under `noImplicitAny` an empty array literal evolves with `push`, `unshift`
/// and index writes (and reads `any[]` when nothing is written), and a `let`
/// initialised with nothing or `null` takes its later writes; without
/// `noImplicitAny` the array is `never[]` (`any[]` without `strictNullChecks`)
/// and the variable is declared `any` or `null`.
#[test]
fn auto_typed_arrays_and_variables_evolve_as_the_checker_evolves_them() {
    let matrix = Matrix::new(EVOLVING);
    let failures = matrix.four(&[
        (
            Read::Return("evPush"),
            "number[]",
            "number[]",
            "never[]",
            "any[]",
        ),
        (
            Read::Return("evPushTwo"),
            "(string | number)[]",
            "(string | number)[]",
            "never[]",
            "any[]",
        ),
        (
            Read::Return("evPushCond"),
            "number[]",
            "number[]",
            "never[]",
            "any[]",
        ),
        (
            Read::Return("evLetEmpty"),
            "number[]",
            "number[]",
            "never[]",
            "any[]",
        ),
        (
            Read::Return("evIndexWrite"),
            "boolean[]",
            "boolean[]",
            "never[]",
            "any[]",
        ),
        (
            Read::Return("evLoopPush"),
            "number[]",
            "number[]",
            "never[]",
            "any[]",
        ),
        (
            Read::Return("evUnshift"),
            "string[]",
            "string[]",
            "never[]",
            "any[]",
        ),
        (
            Read::Return("evNeverRead"),
            "any[]",
            "any[]",
            "never[]",
            "any[]",
        ),
        (
            Read::Return("evNullInit"),
            "number",
            "number",
            "null",
            "any",
        ),
        (
            Read::Return("evUndefInit"),
            "number",
            "number",
            "any",
            "any",
        ),
        (
            Read::Return("evUndefCond"),
            "string | undefined",
            "string",
            "any",
            "any",
        ),
        (
            Read::Return("evElemRead"),
            "number",
            "number",
            "never",
            "any",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Declared unions narrowed by initializers and later assignments.
const ASSIGNMENTS: &str = r##"
declare function cond(): boolean;
export function aReassign() { let x: string | number = "s"; x = 1; return x; }
export function aDeclaredRead() { let x: string | number = "s"; return x; }
export function aCondReassign() { let x: string | number = "s"; if (cond()) x = 1; return x; }
export function aTernaryAssign() { let x: string | number | boolean = true; x = cond() ? "a" : 1; return x; }
export function aLiteralDeclared() { let x: "a" | "b" = "a"; return x; }
export function aLiteralReassign() { let x: "a" | "b" = "a"; x = "b"; return x; }
export function aConstWiden() { const x = "lit"; return x; }
export function aLetWiden() { let x = "lit"; return x; }
export function aObjAssign() { let o: { a: string | number } = { a: "s" }; o = { a: 1 }; return o; }
export function aMemberAssign(o: { a: string | number }) { o.a = 1; return o.a; }
export function aMemberAssignCond(o: { a: string | number }) { if (cond()) o.a = 1; else o.a = "s"; return o.a; }
export function aDestructure(t: [string, number]) { let a: string | number, b: string | number; [a, b] = t; return b; }
export function aObjDestructure(o: { p: number }) { let p: string | number = "s"; ({ p } = o); return p; }
export function aParamReassign(x: string | number) { x = 1; return x; }
export function aNullReassign(x: string | null) { if (x === null) x = "d"; return x; }
export function aAnyAssign() { let x: any = 1; x = "s"; return x; }
export function aUnknownAssign() { let x: unknown = 1; x = "s"; return x; }
export function aDeclaredUnionNarrowedByInit() { let x: string | number | undefined = 1; return x; }
export function aSwapInArms(c: boolean) { let x: string | number = "s"; if (c) { x = 1; } else { x = 2; } return x; }
"##;

/// An assignment narrows a declared union to the assigned type
/// (`getAssignmentReducedType`), from an initializer, a later write, a
/// conditional write, a destructuring write, a member write and a parameter
/// write; `any` and `unknown` declarations keep their declared type; an
/// unannotated `let` or `const` of a literal widens.
#[test]
fn assignments_narrow_declared_types_as_the_checker_narrows_them() {
    let matrix = Matrix::new(ASSIGNMENTS);
    let failures = matrix.returns(&[
        ("aReassign", "number"),
        ("aDeclaredRead", "string"),
        ("aCondReassign", "string | number"),
        ("aTernaryAssign", "string | number"),
        ("aLiteralDeclared", "\"a\""),
        ("aLiteralReassign", "\"b\""),
        ("aConstWiden", "string"),
        ("aLetWiden", "string"),
        ("aObjAssign", "{ a: string | number; }"),
        ("aMemberAssign", "number"),
        ("aMemberAssignCond", "string | number"),
        ("aDestructure", "number"),
        ("aObjDestructure", "number"),
        ("aParamReassign", "number"),
        ("aNullReassign", "string"),
        ("aAnyAssign", "any"),
        ("aUnknownAssign", "unknown"),
        ("aDeclaredUnionNarrowedByInit", "number"),
        ("aSwapInArms", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `&&`, `||`, `??`, their compound assignments, `!`, optional chains and
/// non-null assertions.
const LOGICAL: &str = r##"
declare function cond(): boolean;
declare function maybe(): string | undefined;
export function lgAndAssign() { let x: string | number = "s"; x &&= 1; return x; }
export function lgOrAssign(v: string | undefined) { let x = v; x ||= "d"; return x; }
export function lgNullishAssign(v: string | null) { let x = v; x ??= "d"; return x; }
export function lgOrValue(v: string | undefined) { return v || 0; }
export function lgAndValue(v: string | undefined) { return v && 0; }
export function lgAndEmpty(v: string | undefined) { return v && ""; }
export function lgAndOne(v: string | undefined) { return v && 1; }
export function lgAndFalse(v: string | undefined) { return v && false; }
export function lgAndLet(v: string | undefined) { let r = v && 0; return r; }
export function lgAndNumber(v: number) { let r = v && 0; return r; }
export function lgAndString(v: string) { let r = v && ""; return r; }
export function lgAndBoolean(v: boolean) { let r = v && false; return r; }
export function lgAndMaybeNumber(v: number | undefined) { let r = v && 0; return r; }
export function lgAndOtherKind(v: string) { let r = v && 0; return r; }
export function lgAndLiteralLeft(v: 0 | 1) { let r = v && 0; return r; }
export function lgAndChainEmpty(v: number, w: string) { let r = v && w && ""; return r; }
export function lgAndChainZero(v: number) { let r = v && 0 && 0; return r; }
export function lgAndMember(v: number) { let r = { k: v && 0 }; return r; }
export function lgShortAnd(v: 0) { let r = v && 0; return r; }
export function lgShortOr(v: 1) { let r = v || 1; return r; }
export function lgShortCoalesce(v: 0) { let r = v ?? 0; return r; }
export function lgOrTwin(v: 0 | 1) { let r = v || 1; return r; }
export function lgOrBoolean(v: boolean) { let r = v || true; return r; }
export function lgCoalesceTwin(v: 1 | null) { let r = v ?? 1; return r; }
export function lgCoalesceOther(v: 1 | null) { let r = v ?? 2; return r; }
export function lgNullishValue(v: string | null | undefined) { return v ?? 0; }
export function lgChain(a: string | undefined, b: number | undefined) { return a ?? b ?? true; }
export function lgNotValue(v: string) { return !v; }
export function lgAndNarrow(v: string | undefined) { return v !== undefined && v; }
export function lgOrNarrow(v: string | undefined) { return v === undefined || v; }
export function lgAssignInCond() { let x: string | undefined; if ((x = maybe())) return x; throw 0; }
export function lgCommaValue() { let x = 0; return (x++, "s"); }
export function lgCommaAssign() { let x: string | number = 0; return (x = "s", x); }
export function lgCommaTwo() { let x = 0; let y: string | boolean = true; return (x++, y = "t", y); }
export function lgCommaLet() { let x: string | number = "a"; const v = (x = 1, 2); return [v, x] as const; }
export function lgOptionalMember(o: { a?: { b: number } }) { return o.a?.b; }
export function lgOptionalCall(f?: () => string) { return f?.(); }
export function lgNonNull(v: string | undefined) { return v!; }
"##;

/// `||`, `??` and `&&` type by the truthy / nullish part of their left operand,
/// their compound assignments narrow the written reference, an assignment
/// inside a condition narrows by its value, and an optional chain or a non-null
/// assertion adds or removes `undefined`.
#[test]
fn logical_operators_type_as_the_checker_types_them() {
    let matrix = Matrix::new(LOGICAL);
    let mut failures = matrix.returns(&[
        ("lgAndAssign", "string | number"),
        ("lgOrAssign", "string"),
        ("lgNullishAssign", "string"),
        ("lgOrValue", "string | 0"),
        ("lgNullishValue", "string | 0"),
        ("lgChain", "string | number | true"),
        ("lgNotValue", "boolean"),
        ("lgOrNarrow", "string | true"),
        ("lgAssignInCond", "string"),
        ("lgNonNull", "string"),
    ]);
    failures.extend(matrix.nullness(&[
        (Read::Return("lgAndNarrow"), "string | false", "string"),
        (
            Read::Return("lgOptionalMember"),
            "number | undefined",
            "number",
        ),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A logical expression's literal right operand meets its regular twin when
/// the left's kept part holds it — the definitely falsy part of the left (of
/// the right's base type without `strictNullChecks`) for `&&`, its truthy part
/// for `||`, its non-nullable part for `??` — and the union keeps the regular
/// one (`removeRedundantLiteralTypes`): `v && 0` over `v: number` is a `0` that
/// does not widen, even in a mutable binding, while over `v: string` it is
/// `number | ""` there. A right operand the result does not hold at all adds
/// nothing fresh either.
#[test]
fn a_logical_expression_keeps_a_redundant_literal_as_the_checker_reduces_it() {
    let matrix = Matrix::new(LOGICAL);
    let failures = matrix.nullness(&[
        (Read::Return("lgAndValue"), "\"\" | 0 | undefined", "0"),
        (Read::Return("lgAndEmpty"), "\"\" | undefined", "\"\""),
        (Read::Return("lgAndOne"), "\"\" | 1 | undefined", "0 | 1"),
        (
            Read::Return("lgAndFalse"),
            "\"\" | false | undefined",
            "false",
        ),
        (Read::Return("lgAndLet"), "number | \"\" | undefined", "0"),
        (Read::Return("lgAndNumber"), "0", "0"),
        (Read::Return("lgAndString"), "\"\"", "\"\""),
        (Read::Return("lgAndBoolean"), "false", "false"),
        (Read::Return("lgAndMaybeNumber"), "0 | undefined", "0"),
        (Read::Return("lgAndOtherKind"), "number | \"\"", "0"),
        (Read::Return("lgAndLiteralLeft"), "0", "0"),
        (Read::Return("lgAndChainEmpty"), "\"\" | 0", "\"\""),
        (Read::Return("lgAndChainZero"), "0", "0"),
        (Read::Return("lgAndMember"), "{ k: 0; }", "{ k: 0; }"),
        (Read::Return("lgShortAnd"), "0", "0"),
        (Read::Return("lgShortOr"), "1", "1"),
        (Read::Return("lgShortCoalesce"), "0", "0"),
        (Read::Return("lgOrTwin"), "1", "1"),
        (Read::Return("lgOrBoolean"), "true", "true"),
        (Read::Return("lgCoalesceTwin"), "1", "1"),
        (Read::Return("lgCoalesceOther"), "number", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `(x++, "s")` is `string`: the comma expression applies the increment and
/// takes its last operand's type; every discarded write applies in order
/// and the last operand reads what they wrote.
#[test]
fn a_comma_expression_is_its_last_operand_after_its_writes() {
    let matrix = Matrix::new(LOGICAL);
    let failures = matrix.returns(&[
        ("lgCommaValue", "string"),
        ("lgCommaAssign", "string"),
        ("lgCommaTwo", "string"),
        ("lgCommaLet", "readonly [2, number]"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `f?.()` over `f?: () => string` is `string | undefined` (`string` without
/// `strictNullChecks`).
#[test]
fn an_optional_call_adds_undefined_to_the_callee_result() {
    let matrix = Matrix::new(LOGICAL);
    let failures = matrix.nullness(&[(
        Read::Return("lgOptionalCall"),
        "string | undefined",
        "string",
    )]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Optional chains that hold a call, measured with tsc 7.0.2 under the four
/// settings (`noImplicitAny` never changes the answer).
const OPTIONAL_CALLS: &str = r##"
export interface S { self(): this; v: number }
export function ocCallee(g?: () => string) { return g?.(); }
export function ocMember(o: { m?: () => string }) { return o.m?.(); }
export function ocReceiver(o?: { m(): string }) { return o?.m(); }
export function ocBoth(o?: { m?: () => string }) { return o?.m?.(); }
export function ocThen(g?: () => string) { return g?.().length; }
export function ocDefinite(g: () => string) { return g?.(); }
export function ocNullReceiverThen(o: { m(): string } | null) { return o?.m().length; }
export function ocNullCallee(g: (() => string) | null) { return g?.(); }
export function ocDeep(o?: { a: { m(): string } }) { return o?.a.m(); }
export function ocObjThen(o?: { m(): { v: number } }) { return o?.m().v; }
export function ocCallMethod(f?: (a: number) => string) { return f?.call(undefined, 1); }
export function ocBind(f?: (a: number) => string) { return f?.bind(undefined, 1); }
export function ocThisParam(o?: { v: number; m<T>(this: T): T }) { return o?.m(); }
export function pThisParam(o: { v: number; m<T>(this: T): T }) { return o.m(); }
export function ocThisThen(o?: S) { return o?.self().v; }
export function ocThis(o: S | undefined) { return o?.self(); }
export function pThis(o: S) { return o.self(); }
export function ocNarrowed(o: { m?: () => string }) { if (o.m) { return o.m?.(); } throw 0; }
"##;

/// An optional chain holding a call short-circuits to `undefined` on every
/// `?.` edge whose base has nullish arms: the chain is the call's value (and
/// the links read off it) with `undefined` beside it exactly when a strip
/// removed arms, and `undefined` is a strict-null fact. A callee that is
/// never nullish (`ocDefinite`) adds nothing, and a member called off the
/// stripped receiver reads that receiver (`f?.call(…)`, an interface's
/// `self(): this` in `o?.self()`). The library
/// supplies `String` for the `.length` reads and `CallableFunction` for
/// `call`.
#[test]
fn an_optional_chain_call_adds_undefined_on_its_short_circuit_edge() {
    let matrix =
        Matrix::new(OPTIONAL_CALLS).lib(super::differential_global_library_tests::GLOBALS_LIB);
    let failures = matrix.nullness(&[
        (Read::Return("ocCallee"), "string | undefined", "string"),
        (Read::Return("ocMember"), "string | undefined", "string"),
        (Read::Return("ocReceiver"), "string | undefined", "string"),
        (Read::Return("ocBoth"), "string | undefined", "string"),
        (Read::Return("ocThen"), "number | undefined", "number"),
        (Read::Return("ocDefinite"), "string", "string"),
        (
            Read::Return("ocNullReceiverThen"),
            "number | undefined",
            "number",
        ),
        (Read::Return("ocNullCallee"), "string | undefined", "string"),
        (Read::Return("ocDeep"), "string | undefined", "string"),
        (Read::Return("ocObjThen"), "number | undefined", "number"),
        (Read::Return("ocCallMethod"), "string | undefined", "string"),
        (Read::Return("pThis"), "S", "S"),
        (Read::Return("ocThis"), "S | undefined", "S"),
        (Read::Return("ocThisThen"), "number | undefined", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A guarded root's optional call reads the narrowed member, and a call
/// infers its type parameters from a `this` parameter.
///
/// What the lane gives:
/// - `ocNarrowed`: the checker answers `string`; the lane measured
///   `<opaque UnmodeledPosition>` degraded by FlowGap(UnmodeledExpression)
///   (the write-effect rail every optional chain root takes refuses a root
///   an in-frame guard changes).
/// - `pThisParam` (no optional chain) and `ocThisParam`: the checker infers
///   `T` from the receiver (`{ v: number; m<T>(this: T): T; }`, with
///   `| undefined` under the chain); the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnrepresentableCallee.
/// - `ocBind`: the checker answers `(() => string) | undefined`; the lane
///   measured `<opaque UnmodeledPosition> | undefined` degraded by
///   UnrepresentableCallee (`bind` has no modeled call, chain or not).
#[test]
#[ignore = "a guarded optional-chain root reads its narrowing; a this parameter infers"]
fn an_optional_chain_call_reads_a_guarded_root_and_infers_from_this() {
    let matrix =
        Matrix::new(OPTIONAL_CALLS).lib(super::differential_global_library_tests::GLOBALS_LIB);
    let failures = matrix.nullness(&[
        (Read::Return("ocNarrowed"), "string", "string"),
        (
            Read::Return("pThisParam"),
            "{ v: number; m<T>(this: T): T; }",
            "{ v: number; m<T>(this: T): T; }",
        ),
        (
            Read::Return("ocThisParam"),
            "{ v: number; m<T>(this: T): T; } | undefined",
            "{ v: number; m<T>(this: T): T; }",
        ),
        (
            Read::Return("ocBind"),
            "(() => string) | undefined",
            "() => string",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Arrows and function expressions that capture narrowed parameters and locals.
const CLOSURES: &str = r##"
declare function cond(): boolean;
export function cConstCapture(x: string | null) { if (x !== null) { return () => x; } throw 0; }
export function cLetCapture(x: string | null) { if (x !== null) { const f = () => x; return f; } throw 0; }
export function cParamReassigned(x: string | null) { if (x !== null) { const f = () => x; x = null; return f; } throw 0; }
export function cConstLocal() { const v: string | number = "s"; return () => v; }
export function cLetLocal() { let v: string | number = "s"; return () => v; }
export function cLetLocalNeverReassigned(x: string | null) { let v = x; if (v !== null) { return () => v; } throw 0; }
export function cIIFE() { return (() => 1)(); }
declare function each(f: () => void): void;
export function cCallbackResult() { let seen = false; each(() => { seen = true; }); return seen; }
export function cClosureWrites() { let x: string | number = "s"; const f = () => { x = 1; }; f(); return x; }
export function cNested(x: string | undefined) { if (x) { return () => () => x; } throw 0; }
export function cNestedWrite(x: string | undefined) { if (x) { return () => { x = undefined; return () => x; }; } throw 0; }
export function cNestedLet(v: string | undefined) { let x = v; if (x) { return () => () => x; } throw 0; }
export function cNestedLetLater(v: string | undefined) { let x = v; if (x) { const f = () => () => x; x = undefined; return f; } throw 0; }
export function cNestedThree(x: string | undefined) { if (x) { return () => () => () => x; } throw 0; }
export function cArrowReturn() { const f = (a: number) => a > 0 ? "p" : "n"; return f(1); }
export function cFnExprReturn() { const f = function (a: number) { return a; }; return f; }
"##;

/// A closure over a `const`, or over a parameter or `let` never reassigned
/// after it, keeps the narrowing at its creation; a reassigned capture reads
/// its declared type; a closure's writes do not reach the outer read; an
/// immediately invoked arrow and a called local arrow return their bodies'
/// types.
#[test]
fn closures_capture_as_the_checker_captures() {
    let matrix = Matrix::new(CLOSURES);
    let mut failures = matrix.returns(&[
        ("cConstCapture", "() => string"),
        ("cLetCapture", "() => string"),
        ("cConstLocal", "() => string"),
        ("cLetLocal", "() => string"),
        ("cLetLocalNeverReassigned", "() => string"),
        ("cIIFE", "number"),
        ("cCallbackResult", "boolean"),
        ("cClosureWrites", "string"),
        ("cArrowReturn", "\"n\" | \"p\""),
        ("cFnExprReturn", "(a: number) => number"),
    ]);
    failures.extend(matrix.nullness(&[(
        Read::Return("cParamReassigned"),
        "() => string | null",
        "() => string",
    )]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `if (x) return () => () => x;` over `x: string | undefined` is `() => () =>
/// string`: the narrowing of a reference past its last assignment reaches
/// closures at any depth (a `let` too), and a write anywhere after it (in
/// the frame or a closure) keeps the declared type.
#[test]
fn a_nested_closure_keeps_its_outer_parameter_narrowing() {
    let matrix = Matrix::new(CLOSURES);
    let mut failures = matrix.returns(&[
        ("cNested", "() => () => string"),
        ("cNestedLet", "() => () => string"),
        ("cNestedThree", "() => () => () => string"),
    ]);
    failures.extend(matrix.nullness(&[
        (
            Read::Return("cNestedWrite"),
            "() => () => string | undefined",
            "() => () => string",
        ),
        (
            Read::Return("cNestedLetLater"),
            "() => () => string | undefined",
            "() => () => string",
        ),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Writes in `try`, `catch` and `finally` blocks, and catch variables.
const TRY_CATCH: &str = r##"
declare function cond(): boolean;
declare function risky(): number;
export function tryTry() { let x: string | number = "s"; try { x = 1; } catch { } return x; }
export function tryCatch() { let x: string | number | boolean = "s"; try { x = 1; risky(); } catch { x = true; } return x; }
export function tryFinally() { let x: string | number = "s"; try { x = 1; } finally { } return x; }
export function tryFinallyWrite() { let x: string | number | boolean = "s"; try { x = 1; } finally { x = true; } return x; }
export function tryReturnInTry() { try { return 1; } catch { return "e"; } }
export function tryReturnFinally() { try { return 1; } finally { cond(); } }
export function tryCatchVar() { try { risky(); } catch (e) { return e; } throw 0; }
export function tryCatchVarTyped() { try { risky(); } catch (e: unknown) { return e; } throw 0; }
export function tryCatchNarrow() { try { risky(); } catch (e) { if (typeof e === "string") return e; } throw 0; }
export function tryThrowOnly() { try { throw 1; } catch { return "c"; } }
export function tryNested() { let x: 1 | 2 | 3 = 1; try { try { x = 2; risky(); } finally { x = 3; } } catch { } return x; }
"##;

/// A `finally` block sees and overrides its `try`'s writes, returns in `try`
/// and `catch` both contribute, and a catch variable is `unknown` under
/// `useUnknownInCatchVariables` (part of `strict`) and narrows by `typeof`.
#[test]
fn try_statements_join_as_the_checker_joins_them() {
    let matrix = Matrix::new(TRY_CATCH);
    let failures = matrix.returns(&[
        ("tryFinally", "number"),
        ("tryFinallyWrite", "boolean"),
        ("tryReturnInTry", "\"e\" | 1"),
        ("tryReturnFinally", "number"),
        ("tryCatchVar", "unknown"),
        ("tryCatchVarTyped", "unknown"),
        ("tryCatchNarrow", "string"),
        ("tryThrowOnly", "string"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// After `try { x = 1; … } catch { … }` the read joins the value before the
/// `try` (the throw may come first), each write in the `try` block, and the
/// `catch` block's writes; a `finally` block's write replaces them all.
///
/// What the lane gives:
/// - `tryTry`: the checker answers `string | number`; the lane measured `number
///   | string` degraded by ConditionalVarDefinition.
/// - `tryCatch`: the checker answers `number | true`; the lane measured `true |
///   number` degraded by ConditionalVarDefinition.
/// - `tryNested`: the checker answers `1 | 3`; the lane measured `3 | 2 | 1`
///   degraded by ConditionalVarDefinition.
#[test]
#[ignore = "the read after a try statement joins the values every point of the try block may have left"]
fn a_try_catch_join_holds_every_write_that_may_have_run() {
    let matrix = Matrix::new(TRY_CATCH);
    let failures = matrix.returns(&[
        ("tryTry", "string | number"),
        ("tryCatch", "number | true"),
        ("tryNested", "1 | 3"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Code after `return`, `throw` and calls to `never`-returning functions.
const UNREACHABLE: &str = r##"
declare function cond(): boolean;
declare function fail(): never;
export function uAfterReturn() { return 1; return "s"; }
export function uAfterThrow() { throw 0; return "s"; }
export function uNeverCall(x: string | number) { if (typeof x === "string") fail(); return x; }
export function uNeverCallLocal(x: string | number) { if (typeof x === "number") { fail(); } return x; }
export function uOnlyThrow() { throw new Error2(); }
export function uWhileTrueNoBreak() { while (true) {} }
export function uIfTrueReturn() { if (true) return 1; return "s"; }
export function uIfFalseReturn() { if (false) return 1; return "s"; }
export function uAllThrow(c: boolean) { if (c) throw 1; else throw 2; }
export function uVoid() { }
export function uVoidReturn() { return; }
export function uMixedVoid(c: boolean) { if (c) return 1; return; }
class Error2 {}
"##;

/// A `return` after `return` or `throw` still contributes to the inferred
/// return (the checker infers over every `return` statement), a call to a
/// `never`-returning function ends its path, a body with no reachable `return`
/// is `void`, and `if (true)` / `if (false)` are not constant-folded.
#[test]
fn unreachable_code_contributes_as_the_checker_says() {
    let matrix = Matrix::new(UNREACHABLE);
    let mut failures = matrix.returns(&[
        ("uAfterReturn", "\"s\" | 1"),
        ("uAfterThrow", "string"),
        ("uNeverCall", "number"),
        ("uNeverCallLocal", "string"),
        ("uOnlyThrow", "void"),
        ("uWhileTrueNoBreak", "void"),
        ("uIfTrueReturn", "\"s\" | 1"),
        ("uIfFalseReturn", "\"s\" | 1"),
        ("uAllThrow", "void"),
        ("uVoid", "void"),
        ("uVoidReturn", "void"),
    ]);
    failures.extend(matrix.nullness(&[(Read::Return("uMixedVoid"), "1 | undefined", "number")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Code past a path only the checker proves dead: an exhaustive `switch`
/// whose clauses all exit, and a call to a `never`-returning function.
const PAST_DEAD_PATH: &str = r##"
declare function fail(): never;
export function sEnd(x: "a" | "b") { switch (x) { case "a": return 1; case "b": return 2; } return "end"; }
export function sSubject(x: "a" | "b") { switch (x) { case "a": return 1; case "b": return 2; } return x; }
export function sOther(x: "a" | "b", y: string) { switch (x) { case "a": return 1; case "b": return 2; } return y; }
export function sConst(x: "a" | "b") { switch (x) { case "a": return 1; case "b": return 2; } const z = "zz"; return z; }
export function sBranch(x: "a" | "b", c: boolean) { switch (x) { case "a": return 1; case "b": return 2; } if (c) return 3; return 4; }
export function sDefault(x: "a" | "b") { switch (x) { case "a": return 1; default: return 2; } return "end"; }
export function sLet(x: "a" | "b") { switch (x) { case "a": return 1; case "b": return 2; } let w = 5; w = 6; return w; }
export function sNested(x: "a" | "b") { { switch (x) { case "a": return 1; case "b": return 2; } } return "end"; }
export function sSubjectMember(o: { k: "a" } | { k: "b" }) { switch (o.k) { case "a": return 1; case "b": return 2; } return o; }
export function sThenSwitch(x: "a" | "b", y: "c" | "d") { switch (x) { case "a": return 1; case "b": return 2; } switch (y) { case "c": return 3; case "d": return 4; } return "end"; }
export function sLoopAfter(x: "a" | "b") { switch (x) { case "a": return 1; case "b": return 2; } for (;;) { return "loop"; } }
export function sAnnot(x: "a" | "b") { switch (x) { case "a": return 1; case "b": return 2; } const w: string | number = "a"; return w; }
export function sNarrowed(x: "a" | "b", y: string | number) { if (typeof y === "string") { switch (x) { case "a": return 1; case "b": return 2; } return y; } return true; }
export function sAssign(x: "a" | "b", y: string | number) { switch (x) { case "a": return 1; case "b": return 2; } y = "q"; return y; }
export function sGuardAfter(x: "a" | "b", y: string | number) { switch (x) { case "a": return 1; case "b": return 2; } if (typeof y === "string") return y; return false; }
export function sTwice(x: "a" | "b") { switch (x) { case "a": return 1; case "b": return 2; } switch (x) { case "a": return 3; } return x; }
export function nCall(y: string) { fail(); return y; }
export function nCallConst(y: string) { fail(); const z = "zz"; return z; }
export function nCallAssign(y: string | number) { fail(); y = "q"; return y; }
export function dWhile(x: string | number) { if (typeof x === "string") { while (false) { return x; } } return 2; }
export function dFor(x: string | number) { if (typeof x === "string") { for (; false; ) { return x; } } return true; }
declare const o: { fail(): never };
export function nNarrowed(y: string | number) { if (typeof y === "string") { fail(); return y; } return true; }
export function nMethodNarrowed(y: string | number) { if (typeof y === "string") { o.fail(); return y; } return true; }
export function nLocal(y: string | number) { let w = y; fail(); return w; }
export function nNarrowLet(y: string | number) { let w: string | number = 1; fail(); return w; }
export function nStop(x: string | number, stop: { fail(): never }) { if (typeof x === "string") { stop.fail(); return x; } return 1; }
"##;

/// The checker aggregates every `return` in the body, the ones past a path
/// it proves dead included. Past an exhaustive `switch` a reference reads
/// the no-matching-case edge (the discriminant `never`, every other
/// reference as it reached the `switch`); past a `never`-returning call it
/// reads its declared type; and a write on either dead path leaves the
/// written reference its declared type.
#[test]
fn returns_past_a_path_the_checker_proves_dead_contribute() {
    let matrix = Matrix::new(PAST_DEAD_PATH);
    let failures = matrix.returns(&[
        ("sEnd", "\"end\" | 1 | 2"),
        ("sSubject", "1 | 2"),
        ("sOther", "string | 1 | 2"),
        ("sConst", "\"zz\" | 1 | 2"),
        ("sBranch", "1 | 2 | 3 | 4"),
        ("sDefault", "\"end\" | 1 | 2"),
        ("sLet", "number"),
        ("sNested", "\"end\" | 1 | 2"),
        ("sSubjectMember", "1 | 2"),
        ("sThenSwitch", "\"end\" | 1 | 2 | 3 | 4"),
        ("sLoopAfter", "\"loop\" | 1 | 2"),
        ("sAnnot", "string | number"),
        ("sNarrowed", "string | 1 | 2 | true"),
        ("sAssign", "string | number"),
        ("sGuardAfter", "string | 1 | 2 | false"),
        ("sTwice", "1 | 2 | 3"),
        ("nCall", "string"),
        ("nCallConst", "string"),
        ("nCallAssign", "string | number"),
        ("dWhile", "string | number"),
        ("dFor", "string | number | true"),
        ("nNarrowed", "string | number | true"),
        ("nLocal", "string | number"),
        ("nNarrowLet", "string | number"),
        ("nMethodNarrowed", "string | number | true"),
        ("nStop", "string | number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
