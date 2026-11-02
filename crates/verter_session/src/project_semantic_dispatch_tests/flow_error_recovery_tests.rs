//! The checker's error recovery on the shared flow rail: a member read off a
//! type that proves it absent (TS2339) and an inferred return that requires
//! itself (TS7023) are language-defined answers, each with its diagnostic at
//! the checker's position, in all four `strictNullChecks` × `noImplicitAny`
//! settings. A recovery reads as `any`, leaves the result complete and the
//! rest of the program answering on its own. Whatever the checker's answer
//! is not known for keeps its typed refusal.
//!
//! Oracle comparison: TypeScript 7.0.2.
//! Every row was read through that version's native API over these sources
//! under `noLib`: `getTypeAtPosition` on the probe alias, `typeToString` with
//! `NoTruncation | InTypeAlias`, and `getSemanticDiagnostics` for each code,
//! start and end.

use std::sync::Arc;

use verter_type_engine::semantic_query::{
    FactResult, FlowReturnDegradation, FlowReturnResult, ReturnProjectionDemand,
};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::{OwnedSlice, PrimitiveName, TypeExpr};

use super::differential_harness_tests::ALL;
use super::pinia_options_store_tests::{answer_mismatch, Project, SCENARIO};
use crate::VerterHost;

/// Where a row's diagnostics are read from: a function of the scenario.
#[derive(Clone)]
struct Part(&'static str, FunctionPartIdentity);

fn declaration(symbol: &'static str) -> Part {
    Part(symbol, FunctionPartIdentity::DeclarationBody)
}

fn initializer(symbol: &'static str) -> Part {
    Part(symbol, FunctionPartIdentity::Initializer)
}

fn member(symbol: &'static str, ordinal: u32) -> Part {
    Part(
        symbol,
        FunctionPartIdentity::Member {
            member_path: OwnedSlice::from(vec![ordinal].into_boxed_slice()),
        },
    )
}

/// One expected diagnostic: its code, the text it stands on, the text that
/// finds it, and whether only `noImplicitAny` reports it.
#[derive(Clone, Copy)]
struct Expected {
    code: u32,
    name: &'static str,
    within: &'static str,
    implicit_any_only: bool,
}

/// TS2339 at the member `name`, located by the unique text `within`.
const fn ts2339(name: &'static str, within: &'static str) -> Expected {
    Expected {
        code: 2339,
        name,
        within,
        implicit_any_only: false,
    }
}

/// TS7023 at the function's `name`, located by the unique text `within`.
const fn ts7023(name: &'static str, within: &'static str) -> Expected {
    Expected {
        code: 7023,
        name,
        within,
        implicit_any_only: true,
    }
}

fn result_of(host: &VerterHost, part: &Part) -> Result<Arc<FlowReturnResult>, String> {
    let identity = FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(SCENARIO),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from(part.0),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: part.1.clone(),
        overload_ordinal: 0,
    };
    let (result, _) = host
        .get_flow_return_type_with_audit(&identity, ReturnProjectionDemand::whole_return())
        .into_parts();
    result.map_err(|error| format!("{error:?}"))
}

/// Every failure of `source` to answer each `(probe, checker)` row, to leave
/// `part` undegraded, and to report exactly `expected` from it, in all four
/// settings.
fn mismatches(
    source: &str,
    rows: &[(&str, &str)],
    part: &Part,
    expected: &[Expected],
) -> Vec<String> {
    let project = Project::module(source);
    let mut failures = project.mismatches(rows);
    let probes: Vec<&str> = rows.iter().map(|(probe, _)| *probe).collect();
    let text = format!("type __BenchInit = 0;\n{}\n", project.scenario);
    for setting in ALL {
        let no_implicit_any = !setting.options.contains("noImplicitAny");
        let host = project.host(setting.options, &probes);
        let result = match result_of(&host, part) {
            Ok(result) => result,
            Err(error) => {
                failures.push(format!("{}: `{}` is {error}", setting.label, part.0));
                continue;
            }
        };
        if result.degradation().is_some() {
            failures.push(format!(
                "{}: `{}` is degraded: {:?}",
                setting.label,
                part.0,
                result.degradation()
            ));
        }
        let mut wanted: Vec<(u32, u32, u32)> = expected
            .iter()
            .filter(|diagnostic| no_implicit_any || !diagnostic.implicit_any_only)
            .map(|diagnostic| {
                let within = text
                    .find(diagnostic.within)
                    .expect("the row's text is in the source");
                let start = within
                    + diagnostic
                        .within
                        .find(diagnostic.name)
                        .expect("the name is in the row's text");
                (
                    diagnostic.code,
                    u32::try_from(start).expect("an offset"),
                    u32::try_from(start + diagnostic.name.len()).expect("an offset"),
                )
            })
            .collect();
        wanted.sort_by_key(|&(_, start, _)| start);
        let found: Vec<(u32, u32, u32)> = result
            .checker_diagnostics()
            .iter()
            .map(|found| (found.code().code(), found.start(), found.end()))
            .collect();
        if found != wanted {
            failures.push(format!(
                "{}: `{}` reports {found:?}, the checker {wanted:?}",
                setting.label, part.0
            ));
        }
    }
    failures
}

#[track_caller]
fn assert_row(source: &str, rows: &[(&str, &str)], part: &Part, expected: &[Expected]) {
    let failures = mismatches(source, rows, part, expected);
    assert!(failures.is_empty(), "{source}\n{}", failures.join("\n"));
}

/// A member read off a local, a parameter, a primitive, a union no arm of
/// which has it, and through an optional link is TS2339 at the member's name
/// and `any`; a chain and a call read on through the error type and report
/// nothing further.
#[test]
fn an_absent_member_is_the_checkers_error_type() {
    assert_row(
        "export function f() { const o = { a: 1 }; return o.bb; }",
        &[("ReturnType<typeof f>", "any")],
        &declaration("f"),
        &[ts2339("bb", "o.bb")],
    );
    assert_row(
        "interface P { a: number }\nexport function f(p: P) { return p.bb; }",
        &[("ReturnType<typeof f>", "any")],
        &declaration("f"),
        &[ts2339("bb", ".bb")],
    );
    assert_row(
        "export function f(s: number) { return s.nothing; }",
        &[("ReturnType<typeof f>", "any")],
        &declaration("f"),
        &[ts2339("nothing", ".nothing")],
    );
    assert_row(
        "export function f(o: { a: 1 } | { a: 2 }) { return o.bb; }",
        &[("ReturnType<typeof f>", "any")],
        &declaration("f"),
        &[ts2339("bb", ".bb")],
    );
    assert_row(
        "export function f(o: { a: 1 }) { return o?.bb; }",
        &[("ReturnType<typeof f>", "any")],
        &declaration("f"),
        &[ts2339("bb", ".bb")],
    );
    assert_row(
        "export function f() { const o = { a: 1 }; return o.bb.c.d; }",
        &[("ReturnType<typeof f>", "any")],
        &declaration("f"),
        &[ts2339("bb", ".bb")],
    );
    assert_row(
        "export function f() { const o = { a: 1 }; return o.bb(1); }",
        &[("ReturnType<typeof f>", "any")],
        &declaration("f"),
        &[ts2339("bb", ".bb")],
    );
}

/// The error type stands for the one position: the siblings the literal
/// composes around it answer as they would.
#[test]
fn an_absent_member_leaves_its_siblings_answering() {
    assert_row(
        "export function f() { const o = { a: 1 }; return { x: o.bb, y: o.a }; }",
        &[("ReturnType<typeof f>", "{ x: any; y: number; }")],
        &declaration("f"),
        &[ts2339("bb", ".bb")],
    );
}

/// Reads with a member to read report nothing: a declared member, an index
/// signature and a read off `any`.
#[test]
fn a_present_member_reports_nothing() {
    assert_row(
        "export function f() { const o = { a: 1 }; return o.a; }",
        &[("ReturnType<typeof f>", "number")],
        &declaration("f"),
        &[],
    );
    assert_row(
        "export function f(o: { [k: string]: number }) { return o.bb; }",
        &[("ReturnType<typeof f>", "number")],
        &declaration("f"),
        &[],
    );
    assert_row(
        "export function f(o: any) { return o.bb; }",
        &[("ReturnType<typeof f>", "any")],
        &declaration("f"),
        &[],
    );
}

/// An object literal's own `this` prints the literal in the message, which
/// needs the return being inferred when the read is part of it: the method is
/// also TS7023 under `noImplicitAny`, and its whole return is `any`. A read
/// the return does not depend on reports TS2339 alone, and a declared return
/// is never inferred.
#[test]
fn a_method_reading_its_own_literals_absent_member_is_circular() {
    assert_row(
        "export const a = { n: 1, read() { return this.absent; }, keep() { return this.n; } };",
        &[
            ("ReturnType<typeof a['read']>", "any"),
            ("ReturnType<typeof a['keep']>", "number"),
            ("typeof a", "{ n: number; read(): any; keep(): number; }"),
        ],
        &member("a", 1),
        &[ts7023("read", "read()"), ts2339("absent", ".absent")],
    );
    assert_row(
        "export const o = { n: 1, read(c: boolean) { if (c) return 1; return this.absent; } };",
        &[("ReturnType<typeof o['read']>", "any")],
        &member("o", 1),
        &[ts7023("read", "read(c"), ts2339("absent", ".absent")],
    );
    assert_row(
        "export const o = { n: 1, read() { this.absent; return 1; } };",
        &[("ReturnType<typeof o['read']>", "number")],
        &member("o", 1),
        &[ts2339("absent", ".absent")],
    );
    assert_row(
        "export const o = { m(): number { return this.absent; } };",
        &[("ReturnType<typeof o['m']>", "number")],
        &member("o", 0),
        &[ts2339("absent", ".absent")],
    );
}

/// A call inferring from such a literal infers the recovery, and the literal's
/// other members answer.
#[test]
fn a_generic_call_infers_through_the_recovery() {
    assert_row(
        "declare function mk<T>(o: T): T;\n\
         export const v = mk({ n: 1, r() { return this.n; }, s() { return this.absent; } });",
        &[("typeof v", "{ n: number; r(): number; s(): any; }")],
        &initializer("v"),
        &[ts7023("s", "s()"), ts2339("absent", ".absent")],
    );
}

/// Functions requiring each other's returns are each `any`, TS7023 on the one
/// the demand reads under `noImplicitAny`; with a concrete return of their
/// own, too. A function outside the cycle reads the error type and reports
/// nothing of its own.
#[test]
fn returns_requiring_each_other_are_the_circular_return() {
    let pair = "export function a() { return b(); }\nexport function b() { return a(); }";
    assert_row(
        pair,
        &[
            ("ReturnType<typeof a>", "any"),
            ("ReturnType<typeof b>", "any"),
        ],
        &declaration("a"),
        &[ts7023("a", "function a()")],
    );
    assert_row(
        "export function a(n: number) { if (n) return b(n); return 0; }\n\
         export function b(n: number) { return a(n); }",
        &[
            ("ReturnType<typeof a>", "any"),
            ("ReturnType<typeof b>", "any"),
        ],
        &declaration("a"),
        &[ts7023("a", "function a(n")],
    );
}

/// A call of the function composed into a return (a member value, a
/// conditional arm) requires the return it builds, and a function a variable
/// holds reads the variable: both circular.
#[test]
fn a_composed_or_variable_self_call_is_circular() {
    assert_row(
        "export function f() { return { v: f() }; }",
        &[("ReturnType<typeof f>", "any")],
        &declaration("f"),
        &[ts7023("f", "f() {")],
    );
    assert_row(
        "export const g = () => g();",
        &[("ReturnType<typeof g>", "any")],
        &initializer("g"),
        &[ts7023("g", "const g")],
    );
    assert_row(
        "export const g = () => { return g(); };",
        &[("ReturnType<typeof g>", "any")],
        &initializer("g"),
        &[ts7023("g", "const g")],
    );
}

/// A `return` of the function's own bare call is skipped: alone it is `never`,
/// beside another return that return's answer, in neither case a diagnostic.
/// A declared return is never inferred.
#[test]
fn a_bare_self_return_is_not_circular() {
    assert_row(
        "export function h() { return h(); }",
        &[("ReturnType<typeof h>", "never")],
        &declaration("h"),
        &[],
    );
    assert_row(
        "export function f(n: number) { if (n) return f(n - 1); return 0; }",
        &[("ReturnType<typeof f>", "number")],
        &declaration("f"),
        &[],
    );
    let project = Project::module("export function f(): number { return f(); }");
    assert!(project
        .mismatches(&[("ReturnType<typeof f>", "number")])
        .is_empty());
}

/// A recovery is complete, and the next demand answers it with its
/// diagnostics.
#[test]
fn a_recovery_is_complete_with_its_diagnostics() {
    let project = Project::module("export function f() { const o = { a: 1 }; return o.bb; }");
    let host = project.host(ALL[0].options, &["ReturnType<typeof f>"]);
    let part = declaration("f");
    let first = result_of(&host, &part).expect("the recovery has a value");
    let second = result_of(&host, &part).expect("the recovery has a value");
    assert!(matches!(first.fact(), FactResult::Complete(_)));
    assert_eq!(first.return_type(), second.return_type());
    assert_eq!(second.checker_diagnostics().len(), 1);
}

/// What this rail cannot prove keeps its typed refusal, never a recovered
/// `any`: an arm declaring the member may be the one a narrowing selects.
#[test]
fn a_union_with_an_arm_declaring_the_member_proves_nothing() {
    let project =
        Project::module("export function f(o: { a: 1 } | { a: 2; bb: 3 }) { return o.bb; }");
    let host = project.host(ALL[0].options, &["ReturnType<typeof f>"]);
    let result = result_of(&host, &declaration("f")).expect("the value degrades");
    assert_eq!(
        result.degradation(),
        Some(FlowReturnDegradation::UnresolvedValue)
    );
    assert!(result.checker_diagnostics().is_empty());
}

/// What only looks like a recovery keeps its typed refusal: a name nothing
/// declares is no member of anything, an unmodelled position holds no error
/// type, and neither reads as `any`.
#[test]
fn a_position_the_checker_recovers_nothing_for_keeps_its_cause() {
    let project = Project::module("export function f() { return missingDependency.bb; }");
    let host = project.host(ALL[0].options, &["ReturnType<typeof f>"]);
    let result = result_of(&host, &declaration("f")).expect("the value degrades");
    assert_eq!(
        result.degradation(),
        Some(FlowReturnDegradation::UnresolvedValue)
    );
    assert!(result.checker_diagnostics().is_empty());
    assert_ne!(
        host.project_node_to_type_expr_for_test(result.return_type())
            .expect("the value projects"),
        TypeExpr::Primitive(PrimitiveName::Any)
    );
}

/// A cancelled request answers no recovery, and the retry computes it.
#[test]
fn a_cancelled_request_keeps_no_recovery() {
    let project = Project::module("export function f() { const o = { a: 1 }; return o.bb; }");
    let host = project.host(ALL[0].options, &["ReturnType<typeof f>"]);
    let cancellation = verter_execution::cancellation::CancellationToken::new();
    cancellation.cancel();
    let identity = FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(SCENARIO),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from("f"),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let (cancelled, _) = host
        .get_flow_return_type_with_audit_cancellable(
            &identity,
            ReturnProjectionDemand::whole_return(),
            cancellation,
        )
        .into_parts();
    assert!(cancelled.is_err(), "a cancelled request has no answer");
    let answered = result_of(&host, &declaration("f")).expect("the retry computes it");
    assert_eq!(answered.checker_diagnostics().len(), 1);
    let printed = host
        .project_node_to_type_expr_for_test(answered.return_type())
        .expect("the value projects");
    assert_eq!(answer_mismatch(&printed, "any"), None);
}
