//! What a captured mutable binding reads, where it is narrowed, and what a
//! call between the guard and the read does to it.
//!
//! The checker narrows a binding in its OWN function between its reads by
//! the guards and assignments on the path, and a call does not undo that
//! narrowing — whatever the called closure assigns. A nested function
//! reads a capture at the narrowing reaching its creation only when the
//! capture is past its last assignment there (a `const`, or a parameter or
//! `let` written neither after the creation nor in any closure); every
//! other capture — a `var` always — reads its DECLARED type in the body and
//! is narrowed by the body's own guards from there. The declared type of an
//! unannotated `let` / `var` is its initializer's widened type (`any` for an
//! auto-typed one), whatever is assigned before or after the creation.
//!
//! Every expected answer is TypeScript 7.0.2's, read off
//! `tsc --declaration --emitDeclarationOnly --ignoreConfig` for each
//! `strictNullChecks` × `noImplicitAny` setting; the four settings agree
//! except where a table says otherwise.

use super::checker_probe_lane_tests::{degradation_in, mismatches_in, ProbeProject};

/// Every `(function, checker print of its return)` pair of `source` whose
/// live return does not match in `project`, or is not complete.
fn failures_in(project: ProbeProject<'_>, source: &str, rows: &[(&str, &str)]) -> Vec<String> {
    let probes: Vec<(String, &str)> = rows
        .iter()
        .map(|(name, checker)| (format!("ReturnType<typeof {name}>"), *checker))
        .collect();
    let probe_rows: Vec<(&str, &str)> = probes
        .iter()
        .map(|(probe, checker)| (probe.as_str(), *checker))
        .collect();
    let mut failures = mismatches_in(project, source, &probe_rows);
    for (name, _) in rows {
        match degradation_in(project, source, name) {
            Ok(None) => {}
            Ok(Some(degradation)) => {
                failures.push(format!("`{name}` is degraded: {degradation:?}"));
            }
            Err(()) => failures.push(format!("`{name}` produced no value")),
        }
    }
    failures
}

fn assert_rows(project: ProbeProject<'_>, source: &str, rows: &[(&str, &str)]) {
    let failures = failures_in(project, source, rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const OPTIONS_STRICT: &str = r#"{ "strictNullChecks": true, "noImplicitAny": true }"#;
const OPTIONS_NO_IMPLICIT_ANY_OFF: &str = r#"{ "strictNullChecks": true, "noImplicitAny": false }"#;
const OPTIONS_NULL_CHECKS_OFF: &str = r#"{ "strictNullChecks": false, "noImplicitAny": true }"#;
const OPTIONS_BOTH_OFF: &str = r#"{ "strictNullChecks": false, "noImplicitAny": false }"#;

fn with_options(options: &'static str) -> ProbeProject<'static> {
    ProbeProject {
        compiler_options: Some(options),
        ..ProbeProject::default()
    }
}

const GUARDS: &str = "\
export function letOwn(v: string | number) { let x = v; const set = () => { x = 1; }; if (typeof x === \"string\") { return x; } set(); return 0; }
export function letOwnCall(v: string | number) { let x = v; const set = () => { x = 1; }; if (typeof x === \"string\") { set(); return x; } return 0; }
export function letOwnSetAfter(v: string | number) { let x = v; if (typeof x === \"string\") { return x; } const set = () => { x = 1; }; set(); return 0; }
export function paramOwn(x: string | number) { const set = () => { x = 1; }; if (typeof x === \"string\") { return x; } set(); return 0; }
export function paramOwnCall(x: string | number) { const set = () => { x = 1; }; if (typeof x === \"string\") { set(); return x; } return 0; }
export function varOwn(v: string | number) { var x = v; const set = () => { x = 1; }; if (typeof x === \"string\") { return x; } set(); return 0; }
export function varOwnCall(v: string | number) { var x = v; const set = () => { x = 1; }; if (typeof x === \"string\") { set(); return x; } return 0; }
export function letOwnAnn(v: string | number) { let x: string | number | boolean = v; const set = () => { x = 1; }; if (typeof x === \"string\") { set(); return x; } return x; }
export function callAssign(v: string | number) { let x = v; const set = () => { x = 1; }; x = \"s\"; set(); return x; }
export function callOnly(x: string | number) { const set = () => { x = 1; }; set(); return x; }
export function callOnlyNoWrite(x: string | number) { const peek = () => x; peek(); return x; }
export function letIn(v: string | number) { let x = v; const set = () => { x = 1; }; set(); return () => { if (typeof x === \"string\") { return x; } return 0; }; }
export function letInCall(v: string | number) { let x = v; const set = () => { x = 1; }; return () => { if (typeof x === \"string\") { set(); return x; } return 0; }; }
export function letInBefore(v: string | number) { let x = v; const g = () => { if (typeof x === \"string\") { return x; } return 0; }; const set = () => { x = 1; }; set(); return g; }
export function letInUnguarded(v: string | number) { let x = v; const set = () => { x = 1; }; set(); return () => x; }
export function letInNarrowedOuter(v: string | number) { let x = v; const set = () => { x = 1; }; if (typeof x === \"string\") { return () => x; } set(); return undefined; }
export function paramIn(x: string | number) { const set = () => { x = 1; }; set(); return () => { if (typeof x === \"string\") { return x; } return 0; }; }
export function paramInNarrowedOuter(x: string | number) { const set = () => { x = 1; }; if (typeof x === \"string\") { return () => x; } set(); return undefined; }
export function varIn(v: string | number) { var x = v; const set = () => { x = 1; }; set(); return () => { if (typeof x === \"string\") { return x; } return 0; }; }
export function varInNarrowedOuter(v: string | number) { var x = v; const set = () => { x = 1; }; if (typeof x === \"string\") { return () => x; } set(); return undefined; }
export function letInAnn(v: string | number) { let x: string | number | boolean = v; const set = () => { x = 1; }; set(); return () => { if (typeof x === \"string\") { return x; } return x; }; }
export function letInLit() { let x = \"a\"; const set = () => { x = \"b\"; }; set(); return () => { if (x === \"a\") { return x; } return 0; }; }
export function letInSelfWrite(v: string | number) { let x = v; const set = () => { x = 1; }; set(); return () => { if (typeof x === \"string\") { const s = x; x = 2; return s; } return 0; }; }
export function hopLetAfter(v: string | number) { let x = v; const f = () => () => x; x = 1; return f; }
export function hopLetMiddleWrites(v: string | number) { let x = v; return () => { x = 1; return () => x; }; }
export function hopParamMiddleWrites(x: string | number) { return () => { x = 1; return () => x; }; }
export function hopLetPast(v: string | number) { let x = v; x = 1; return () => () => x; }
export function hopLetGuardInner(v: string | number) { let x = v; const s = () => { x = 2; }; return () => () => { if (typeof x === \"string\") { return x; } return 0; }; }
export function letAnnAfter(v: string | number) { let x: string | number | boolean = v; const f = () => x; x = 1; return f; }
export function letUnannAfter(v: string | number) { let x = v; const f = () => x; x = 1; return f; }
export function letLitAfter() { let x = \"a\"; const f = () => x; x = \"b\"; return f; }
export function letUnionLitAfter(c: boolean) { let x = c ? \"a\" : 1; const f = () => x; x = \"b\"; return f; }
";

/// The rows of [`GUARDS`] every setting agrees on.
const GUARD_ROWS: &[(&str, &str)] = &[
    ("letOwn", "string | 0"),
    ("letOwnCall", "string | 0"),
    ("letOwnSetAfter", "string | 0"),
    ("paramOwn", "string | 0"),
    ("paramOwnCall", "string | 0"),
    ("varOwn", "string | 0"),
    ("varOwnCall", "string | 0"),
    ("letOwnAnn", "string | number"),
    ("callAssign", "string"),
    ("callOnly", "string | number"),
    ("callOnlyNoWrite", "string | number"),
    ("letIn", "() => string | 0"),
    ("letInCall", "() => string | 0"),
    ("letInBefore", "() => string | 0"),
    ("letInUnguarded", "() => string | number"),
    ("paramIn", "() => string | 0"),
    ("varIn", "() => string | 0"),
    ("letInAnn", "() => string | number | boolean"),
    ("letInLit", "() => \"a\" | 0"),
    ("letInSelfWrite", "() => string | 0"),
    ("hopLetAfter", "() => () => string | number"),
    ("hopLetMiddleWrites", "() => () => string | number"),
    ("hopParamMiddleWrites", "() => () => string | number"),
    ("hopLetPast", "() => () => number"),
    ("hopLetGuardInner", "() => () => string | 0"),
    ("letAnnAfter", "() => string | number | boolean"),
    ("letUnannAfter", "() => string | number"),
    ("letLitAfter", "() => string"),
    ("letUnionLitAfter", "() => string | number"),
];

/// A guard over a binding some closure writes narrows it in its own
/// function, and a call of that closure between the guard and the read
/// leaves the narrowing in place. Inside a closure the capture reads its
/// declared type — the capture is not past its last assignment — and the
/// closure's own guards narrow it from there, for a `let`, a `var` and a
/// parameter, a closure created before or after the write, and through a
/// function nested two deep.
///
/// Measured on TypeScript 7.0.2, all four settings (`ReturnType<typeof f>`):
///
/// | function | checker |
/// | --- | --- |
/// | `letOwn`, `letOwnCall`, `letOwnSetAfter`, `paramOwn`, `paramOwnCall`, `varOwn`, `varOwnCall` | `string \| 0` |
/// | `letOwnAnn` | `string \| number` |
/// | `callAssign` | `string` |
/// | `callOnly`, `callOnlyNoWrite` | `string \| number` |
/// | `letIn`, `letInCall`, `letInBefore`, `paramIn`, `varIn`, `letInSelfWrite` | `() => string \| 0` |
/// | `letInUnguarded`, `letUnannAfter`, `letUnionLitAfter` | `() => string \| number` |
/// | `letInAnn`, `letAnnAfter` | `() => string \| number \| boolean` |
/// | `letInLit` | `() => "a" \| 0` |
/// | `hopLetAfter`, `hopLetMiddleWrites`, `hopParamMiddleWrites` | `() => () => string \| number` |
/// | `hopLetPast` | `() => () => number` |
/// | `hopLetGuardInner` | `() => () => string \| 0` |
/// | `letLitAfter` | `() => string` |
#[test]
fn a_guard_over_a_binding_a_closure_writes_narrows_as_the_checker_does() {
    for options in [
        OPTIONS_STRICT,
        OPTIONS_NO_IMPLICIT_ANY_OFF,
        OPTIONS_NULL_CHECKS_OFF,
        OPTIONS_BOTH_OFF,
    ] {
        let failures = failures_in(with_options(options), GUARDS, GUARD_ROWS);
        assert!(failures.is_empty(), "{options}:\n{}", failures.join("\n"));
    }
}

/// A capture created under an enclosing guard is outside its extended
/// container when some closure writes it, so the created function reads
/// the declared type, not the guarded one.
///
/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`):
///
/// | function | `strictNullChecks` on | off |
/// | --- | --- | --- |
/// | `letInNarrowedOuter`, `paramInNarrowedOuter`, `varInNarrowedOuter` | `(() => string \| number) \| undefined` | `() => string \| number` |
///
/// `noImplicitAny` does not change either column.
#[test]
fn a_closure_under_a_guard_reads_a_capture_another_closure_writes_at_its_declared_type() {
    let rows_strict: &[(&str, &str)] = &[
        ("letInNarrowedOuter", "(() => string | number) | undefined"),
        (
            "paramInNarrowedOuter",
            "(() => string | number) | undefined",
        ),
        ("varInNarrowedOuter", "(() => string | number) | undefined"),
    ];
    let rows_loose: &[(&str, &str)] = &[
        ("letInNarrowedOuter", "() => string | number"),
        ("paramInNarrowedOuter", "() => string | number"),
        ("varInNarrowedOuter", "() => string | number"),
    ];
    assert_rows(with_options(OPTIONS_STRICT), GUARDS, rows_strict);
    assert_rows(
        with_options(OPTIONS_NO_IMPLICIT_ANY_OFF),
        GUARDS,
        rows_strict,
    );
    assert_rows(with_options(OPTIONS_NULL_CHECKS_OFF), GUARDS, rows_loose);
    assert_rows(with_options(OPTIONS_BOTH_OFF), GUARDS, rows_loose);
}

const UNANNOTATED_VAR: &str = "\
export function varWrittenBefore() { var w = \"a\" as string | number; w = 1; const g = () => w; return g(); }
export function varWrittenBeforeRet() { var w = \"a\" as string | number; w = 1; return () => w; }
export function varLitWrittenBefore() { var w = \"a\"; w = \"b\"; return () => w; }
export function varNumWrittenBefore() { var w = 1; w = 2; return () => w; }
export function varParamWrittenBefore(v: string | number) { var w = v; w = 1; return () => w; }
export function varParamWrittenAfter(v: string | number) { var w = v; const g = () => w; w = 1; return g; }
export function varParamWrittenBoth(v: string | number) { var w = v; w = 1; const g = () => w; w = \"s\"; return g; }
export function varWrittenInClosure(v: string | number) { var w = v; const s = () => { w = 1; }; s(); return () => w; }
export function varObjInit() { var w = { a: 1 }; w = { a: 2 }; return () => w; }
export function varArrInit() { var w = [1]; w = [2]; return () => w; }
export function varConstAssertInit() { var w = \"a\" as const; return () => w; }
export function varGuardedWrittenBefore(v: string | number) { var w = v; w = 1; return () => { if (typeof w === \"string\") { return w; } return w; }; }
export function varRedeclared(v: string | number) { var w = v; w = 1; var w = v; return () => w; }
export function varIifeWrittenBefore(v: string | number) { var w = v; w = 1; return (() => w)(); }
export function varWrittenBeforeObjMethod(v: string | number) { var w = v; w = 1; return { m() { return w; } }; }
export function hopVarBefore(v: string | number) { var x = v; x = 1; return () => () => x; }
export function varNullInit() { var w = null; w = 1; return () => w; }
export function varUndefInit() { var w = undefined; w = 1; return () => w; }
export function letNullAfter() { let x = null; const f = () => x; x = null; return f; }
export function letNoInitAfter() { let x; const f = () => x; x = 1; return f; }
";

/// An unannotated `var` reassigned before a closure is created reads its
/// DECLARED type in the closure — its initializer's widened type, never the
/// assigned one — and an invoked function, which shares the enclosing flow,
/// reads the assignment.
///
/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`), all four settings
/// unless the second table says otherwise:
///
/// | function | checker |
/// | --- | --- |
/// | `varWrittenBefore` | `string \| number` |
/// | `varWrittenBeforeRet`, `varParamWrittenBefore`, `varParamWrittenAfter`, `varParamWrittenBoth`, `varWrittenInClosure`, `varGuardedWrittenBefore`, `varRedeclared` | `() => string \| number` |
/// | `varLitWrittenBefore` | `() => string` |
/// | `varNumWrittenBefore` | `() => number` |
/// | `varObjInit` | `() => { a: number; }` |
/// | `varArrInit` | `() => number[]` |
/// | `varConstAssertInit` | `() => "a"` |
/// | `varIifeWrittenBefore` | `number` |
/// | `varWrittenBeforeObjMethod` | `{ m(): string \| number; }` |
/// | `hopVarBefore` | `() => () => string \| number` |
/// | `letNoInitAfter` | `() => any` |
///
/// | function | `noImplicitAny` on, or `strictNullChecks` off | `strictNullChecks` on, `noImplicitAny` off |
/// | --- | --- | --- |
/// | `varNullInit`, `letNullAfter` | `() => any` | `() => null` |
/// | `varUndefInit` | `() => any` | `() => undefined` |
#[test]
fn an_unannotated_var_capture_reads_its_initializers_widened_type() {
    let rows: &[(&str, &str)] = &[
        ("varWrittenBefore", "string | number"),
        ("varWrittenBeforeRet", "() => string | number"),
        ("varLitWrittenBefore", "() => string"),
        ("varNumWrittenBefore", "() => number"),
        ("varParamWrittenBefore", "() => string | number"),
        ("varParamWrittenAfter", "() => string | number"),
        ("varParamWrittenBoth", "() => string | number"),
        ("varWrittenInClosure", "() => string | number"),
        ("varObjInit", "() => { a: number; }"),
        ("varArrInit", "() => number[]"),
        ("varConstAssertInit", "() => \"a\""),
        ("varGuardedWrittenBefore", "() => string | number"),
        ("varRedeclared", "() => string | number"),
        ("varIifeWrittenBefore", "number"),
        ("varWrittenBeforeObjMethod", "{ m(): string | number; }"),
        ("hopVarBefore", "() => () => string | number"),
        ("letNoInitAfter", "() => any"),
    ];
    let auto_typed: &[(&str, &str)] = &[
        ("varNullInit", "() => any"),
        ("varUndefInit", "() => any"),
        ("letNullAfter", "() => any"),
    ];
    let declared_nullish: &[(&str, &str)] = &[
        ("varNullInit", "() => null"),
        ("varUndefInit", "() => undefined"),
        ("letNullAfter", "() => null"),
    ];
    for options in [
        OPTIONS_STRICT,
        OPTIONS_NO_IMPLICIT_ANY_OFF,
        OPTIONS_NULL_CHECKS_OFF,
        OPTIONS_BOTH_OFF,
    ] {
        let nullish = if options == OPTIONS_NO_IMPLICIT_ANY_OFF {
            declared_nullish
        } else {
            auto_typed
        };
        let mut failures = failures_in(with_options(options), UNANNOTATED_VAR, rows);
        failures.extend(failures_in(with_options(options), UNANNOTATED_VAR, nullish));
        assert!(failures.is_empty(), "{options}:\n{}", failures.join("\n"));
    }
}

const ANNOTATED_VAR: &str = "\
export function annRead(v: string | number) { var w: string | number = v; return w; }
export function annReadLit(v: string | number) { var w: string | number = \"s\"; return w; }
export function annReadWide(v: string) { var w: string | number = v; return w; }
export function annReadBool(v: boolean) { var w: string | number | boolean = v; return w; }
export function annReassigned(v: string | number) { var w: string | number = v; w = 1; return w; }
export function annReassignedStr(v: string | number) { var w: string | number = v; w = \"s\"; return w; }
export function annGuard(v: string | number) { var w: string | number = v; if (typeof w === \"string\") { return w; } return 0; }
export function annClosure(v: string | number) { var w: string | number = v; return () => w; }
export function annClosureAfterWrite(v: string | number) { var w: string | number = v; w = 1; return () => w; }
export function annClosureLit(v: string | number) { var w: string | number = \"s\"; return () => w; }
export function annClosureCall(v: string | number) { var w: string | number = v; const g = () => w; return g(); }
export function annClosureGuard(v: string | number) { var w: string | number = v; if (typeof w === \"string\") { const g = () => w; return g(); } return 0; }
export function annIife(v: string | number) { var w: string | number = v; if (typeof w === \"string\") { return (() => w)(); } return 0; }
export function annIifeWide(v: string) { var w: string | number | boolean = v; return (() => w)(); }
export function annNoInit() { var w: string | number; w = 1; return w; }
export function annUndefinedInit(v: string | undefined) { var w: string | undefined = v; return w; }
";

/// An annotated `var` holds its declared type narrowed by the assignment
/// reaching each read — the declarator's included — and a closure outside
/// its extended container reads the declared type itself.
///
/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`), all four settings
/// unless noted:
///
/// | function | checker |
/// | --- | --- |
/// | `annRead`, `annClosureCall`, `annClosureGuard` | `string \| number` |
/// | `annReadLit`, `annReadWide`, `annReassignedStr`, `annIifeWide` | `string` |
/// | `annReadBool` | `boolean` |
/// | `annReassigned`, `annNoInit` | `number` |
/// | `annGuard`, `annIife` | `string \| 0` |
/// | `annClosure`, `annClosureAfterWrite`, `annClosureLit` | `() => string \| number` |
/// | `annUndefinedInit` | `string \| undefined`; `string` with `strictNullChecks` off |
#[test]
fn an_annotated_var_reads_its_declared_type_narrowed_by_assignment() {
    let rows: &[(&str, &str)] = &[
        ("annRead", "string | number"),
        ("annReadLit", "string"),
        ("annReadWide", "string"),
        ("annReadBool", "boolean"),
        ("annReassigned", "number"),
        ("annReassignedStr", "string"),
        ("annGuard", "string | 0"),
        ("annClosure", "() => string | number"),
        ("annClosureAfterWrite", "() => string | number"),
        ("annClosureLit", "() => string | number"),
        ("annClosureCall", "string | number"),
        ("annClosureGuard", "string | number"),
        ("annIife", "string | 0"),
        ("annIifeWide", "string"),
        ("annNoInit", "number"),
    ];
    for (options, undefined_init) in [
        (OPTIONS_STRICT, "string | undefined"),
        (OPTIONS_NO_IMPLICIT_ANY_OFF, "string | undefined"),
        (OPTIONS_NULL_CHECKS_OFF, "string"),
        (OPTIONS_BOTH_OFF, "string"),
    ] {
        let mut failures = failures_in(with_options(options), ANNOTATED_VAR, rows);
        failures.extend(failures_in(
            with_options(options),
            ANNOTATED_VAR,
            &[("annUndefinedInit", undefined_init)],
        ));
        assert!(failures.is_empty(), "{options}:\n{}", failures.join("\n"));
    }
}

const CALLS: &str = "\
export function bClosureCall() { const a: number[] = []; const f = () => { a.push(1); }; f(); return a; }
export function closureAssignsAnn() { let a: (string | number)[] = []; const f = () => { a = [1]; }; f(); return a; }
export function nestedFuncCall(v: string | number) { let x = v; function g() { x = 1; } if (typeof x === \"string\") { g(); return x; } return 0; }
export function nestedFuncNever(v: string | number) { function fail(): never { throw 0; } if (typeof v === \"string\") { return v; } fail(); }
export function annotatedVoidCallee(v: string | number) { const f: () => void = () => {}; if (typeof v === \"string\") { f(); return v; } return 0; }
export function paramCallee(v: string | number, cb: () => void) { if (typeof v === \"string\") { cb(); return v; } return 0; }
export function paramUnannotated(v: string | number, cb: any) { if (typeof v === \"string\") { cb(); return v; } return 0; }
export function paramDefaultCallee(v: string | number, cb = () => {}) { if (typeof v === \"string\") { cb(); return v; } return 0; }
export function callInClosure(v: string | number) { let x = v; const set = () => { x = 1; }; return () => { if (typeof x === \"string\") { set(); return x; } return 0; }; }
export function nestedDeclInClosure(v: string | number) { function g() {} return () => { if (typeof v === \"string\") { g(); return v; } return 0; }; }
export function discardedCall(v: string | number) { let x = v; const set = () => { x = 1; }; if (typeof x === \"string\") { return (set(), x); } return 0; }
";

/// A statement call through a frame binding whose declared type holds no
/// assertion or `never` signature — a closure in an unannotated or
/// function-typed variable or parameter, a nested function declaration —
/// narrows nothing and does not end the path, inside a closure too; a
/// nested declaration annotated `never` ends it.
///
/// Measured on TypeScript 7.0.2, all four settings (`ReturnType<typeof f>`):
///
/// | function | checker |
/// | --- | --- |
/// | `bClosureCall` | `number[]` |
/// | `closureAssignsAnn` | `(string \| number)[]` |
/// | `nestedFuncCall`, `annotatedVoidCallee`, `paramCallee`, `paramUnannotated`, `paramDefaultCallee`, `discardedCall` | `string \| 0` |
/// | `nestedFuncNever` | `string` |
/// | `callInClosure`, `nestedDeclInClosure` | `() => string \| 0` |
#[test]
fn a_call_through_a_binding_without_a_declared_effect_keeps_the_narrowing() {
    for options in [
        OPTIONS_STRICT,
        OPTIONS_NO_IMPLICIT_ANY_OFF,
        OPTIONS_NULL_CHECKS_OFF,
        OPTIONS_BOTH_OFF,
    ] {
        let failures = failures_in(
            with_options(options),
            CALLS,
            &[
                ("bClosureCall", "number[]"),
                ("closureAssignsAnn", "(string | number)[]"),
                ("nestedFuncCall", "string | 0"),
                ("nestedFuncNever", "string"),
                ("annotatedVoidCallee", "string | 0"),
                ("paramCallee", "string | 0"),
                ("paramUnannotated", "string | 0"),
                ("paramDefaultCallee", "string | 0"),
                ("callInClosure", "() => string | 0"),
                ("nestedDeclInClosure", "() => string | 0"),
                ("discardedCall", "string | 0"),
            ],
        );
        assert!(failures.is_empty(), "{options}:\n{}", failures.join("\n"));
    }
}

const DECLARED_UNION_LOOPS: &str = "\
declare function next(v: number | string | boolean): number | string;
export function chainNoLoop(c: boolean) { let v: number | string | boolean = 1; let w: number | string | boolean = 1; if (c) w = \"s\"; v = w; return v; }
export function chainFor(n: number) { let r: number | string | boolean = 0; let v: number | string | boolean = 1; let w: number | string | boolean = true; for (let i = 0; i < n; i++) { r = v; v = w; w = \"s\"; } return r; }
export function selfFor(n: number) { let v: number | string | boolean = 1; for (let i = 0; i < n; i++) { v = next(v); } return v; }
export function toggleFor(n: number) { let x: string | number | boolean = 0; for (let i = 0; i < n; i++) { x = typeof x === \"number\" ? \"s\" : 1; } return x; }
export function litFor(n: number) { let x: \"a\" | \"b\" | \"c\" = \"a\"; for (let i = 0; i < n; i++) { x = x === \"a\" ? \"b\" : \"c\"; } return x; }
export function loopForI(n: number) { let x: string | number = \"a\"; for (let i = 0; i < n; i++) { x = i; } return x; }
export function copyPrev(n: number) { let prev: string | number = \"a\"; let cur: string | number = 0; for (let i = 0; i < n; i++) { prev = cur; cur = i; } return prev; }
";

/// A variable declared with a union keeps reducing each assignment to the
/// declared constituents the assigned value may be, through a loop's fixed
/// point: the assigned union selects every constituent it overlaps.
///
/// Measured on TypeScript 7.0.2, all four settings (`ReturnType<typeof f>`):
///
/// | function | checker |
/// | --- | --- |
/// | `chainNoLoop`, `selfFor`, `toggleFor`, `loopForI`, `copyPrev` | `string \| number` |
/// | `chainFor` | `string \| number \| true` |
/// | `litFor` | `"a" \| "b" \| "c"` |
#[test]
fn a_declared_union_reduces_every_assignment_through_a_loop() {
    for options in [
        OPTIONS_STRICT,
        OPTIONS_NO_IMPLICIT_ANY_OFF,
        OPTIONS_NULL_CHECKS_OFF,
        OPTIONS_BOTH_OFF,
    ] {
        let failures = failures_in(
            with_options(options),
            DECLARED_UNION_LOOPS,
            &[
                ("chainNoLoop", "string | number"),
                ("chainFor", "string | number | true"),
                ("selfFor", "string | number"),
                ("toggleFor", "string | number"),
                ("litFor", "\"a\" | \"b\" | \"c\""),
                ("loopForI", "string | number"),
                ("copyPrev", "string | number"),
            ],
        );
        assert!(failures.is_empty(), "{options}:\n{}", failures.join("\n"));
    }
}

const AUTO_TYPED: &str = "\
export function capAnnAfterIf(c: boolean) { let x: string | number = \"s\"; if (c) { x = 1; } const f = () => x; return f(); }
export function capAnnArm(c: boolean) { let x: string | number = \"s\"; if (c) { x = 1; const f = () => x; return f(); } return true; }
export function capAnnDef() { let x: string | number = \"s\"; x = 1; const f = () => x; return f(); }
export function capAnnLater() { let x: string | number = \"s\"; const f = () => x; x = 1; return f(); }
export function capAnnBlock(c: boolean) { let x: string | number = \"s\"; { x = 1; } const f = () => x; return f(); }
export function capAnnNestedIf(c: boolean) { let x: string | number = \"s\"; if (c) { if (c) { x = 1; } const f = () => x; return f(); } return true; }
export function capAnnIfBefore(c: boolean) { let x: string | number = \"s\"; if (c) { x = 1; } if (c) { const f = () => x; return f(); } return true; }
export function capParamArm(x: string | number, c: boolean) { if (c) { x = 1; const f = () => x; return f(); } return true; }
export function capLet(c: boolean) { let x; if (c) { x = 1; } const f = () => x; return f(); }
export function capLetArm(c: boolean) { let x; if (c) { x = 1; const f = () => x; return f(); } return \"z\"; }
export function capLetAssigned() { let x; x = \"s\"; const f = () => x; return f(); }
export function capLetDefinite(c: boolean) { let x; if (c) { x = 1; } else { x = \"s\"; } const f = () => x; return f(); }
export function capLetFnExpr() { let x; x = 1; return function () { return x; }(); }
export function capLetNever() { let x; const f = () => x; return f(); }
export function capLetNull() { let x = null; x = 1; const f = () => x; return f(); }
export function capLetNullNever() { let x = null; const f = () => x; return [f()]; }
export function capLetUndef() { let x = undefined; x = 1; const f = () => x; return f(); }
export function capLetLoop(n: number) { let x; for (let i = 0; i < n; i++) { x = i; } const f = () => x; return f(); }
export function capLetSwitch(k: number) { let x; switch (k) { case 1: x = 1; break; } const f = () => x; return f(); }
export function capLetUninitArmExt(c: boolean) { let x; if (c) { x = 1; } if (c) { const f = () => x; return f(); } return \"z\"; }
";

/// A capture is past its last assignment only when every assignment's
/// statement ends before the function is created: an assignment's position
/// extends to the end of the outermost statement holding it below the
/// declaration, so `if (c) { x = 1; const f = () => x; }` reads the
/// declared type in `f`. A capture reads the flow reaching its creation
/// assumed initialized: a path on which an auto-typed `let` (no annotation,
/// no initializer, `noImplicitAny` on) is not yet assigned reads its
/// declared `any`, which absorbs the other paths, unless the variable is
/// never assigned (then `undefined`); outside its extended container it
/// reads `any`. With `noImplicitAny` off the variable is declared `any`.
///
/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`):
///
/// | function | every setting |
/// | --- | --- |
/// | `capAnnAfterIf`, `capAnnLater` | `string \| number` |
/// | `capAnnArm`, `capAnnNestedIf`, `capAnnIfBefore`, `capParamArm` | `string \| number \| true` |
/// | `capAnnDef`, `capAnnBlock` | `number` |
/// | `capLet`, `capLetArm`, `capLetLoop`, `capLetSwitch`, `capLetUninitArmExt` | `any` |
///
/// | function | `noImplicitAny` on, `strictNullChecks` on | `noImplicitAny` on, off | `noImplicitAny` off, `strictNullChecks` on | both off |
/// | --- | --- | --- | --- | --- |
/// | `capLetAssigned` | `string` | `string` | `any` | `any` |
/// | `capLetDefinite` | `string \| number` | `string \| number` | `any` | `any` |
/// | `capLetFnExpr` | `number` | `number` | `any` | `any` |
/// | `capLetNever` | `undefined` | `undefined` | `any` | `any` |
/// | `capLetNull` | `number` | `number` | `null` | `any` |
/// | `capLetNullNever` | `null[]` | `any[]` | `null[]` | `any[]` |
/// | `capLetUndef` | `number` | `number` | `undefined` | `any` |
#[test]
fn a_capture_reads_the_checkers_extended_flow_or_declared_type() {
    let shared: &[(&str, &str)] = &[
        ("capAnnAfterIf", "string | number"),
        ("capAnnLater", "string | number"),
        ("capAnnArm", "string | number | true"),
        ("capAnnNestedIf", "string | number | true"),
        ("capAnnIfBefore", "string | number | true"),
        ("capParamArm", "string | number | true"),
        ("capAnnDef", "number"),
        ("capAnnBlock", "number"),
        ("capLet", "any"),
        ("capLetArm", "any"),
        ("capLetLoop", "any"),
        ("capLetSwitch", "any"),
        ("capLetUninitArmExt", "any"),
    ];
    let implicit_any: &[(&str, [&str; 2])] = &[
        ("capLetAssigned", ["string", "string"]),
        ("capLetDefinite", ["string | number", "string | number"]),
        ("capLetFnExpr", ["number", "number"]),
        ("capLetNever", ["undefined", "undefined"]),
        ("capLetNull", ["number", "number"]),
        ("capLetNullNever", ["null[]", "any[]"]),
        ("capLetUndef", ["number", "number"]),
    ];
    let declared_any: &[(&str, [&str; 2])] = &[
        ("capLetAssigned", ["any", "any"]),
        ("capLetDefinite", ["any", "any"]),
        ("capLetFnExpr", ["any", "any"]),
        ("capLetNever", ["any", "any"]),
        ("capLetNull", ["null", "any"]),
        ("capLetNullNever", ["null[]", "any[]"]),
        ("capLetUndef", ["undefined", "any"]),
    ];
    for (options, varying, column) in [
        (OPTIONS_STRICT, implicit_any, 0),
        (OPTIONS_NULL_CHECKS_OFF, implicit_any, 1),
        (OPTIONS_NO_IMPLICIT_ANY_OFF, declared_any, 0),
        (OPTIONS_BOTH_OFF, declared_any, 1),
    ] {
        let rows: Vec<(&str, &str)> = shared
            .iter()
            .copied()
            .chain(
                varying
                    .iter()
                    .map(|(name, answers)| (*name, answers[column])),
            )
            .collect();
        let failures = failures_in(with_options(options), AUTO_TYPED, &rows);
        assert!(failures.is_empty(), "{options}:\n{}", failures.join("\n"));
    }
}
