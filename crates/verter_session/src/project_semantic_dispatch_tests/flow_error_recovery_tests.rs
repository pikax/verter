//! The checker's error recovery on the shared flow rail: a member read off a
//! type that proves it absent (TS2339) and an inferred return that requires
//! itself (TS7023) are language-defined answers, each with its diagnostic at
//! the checker's position, in all four `strictNullChecks` × `noImplicitAny`
//! settings. A recovery reads as `any`, leaves the result complete and the
//! rest of the program answering on its own. Whatever the checker's answer
//! is not known for keeps its typed refusal.
//!
//! Oracle comparison: TypeScript 7.0.2, whose `tsc` differs from later releases
//! on what a marked `this` reports.
//! Every row was read through that version over these sources under `noLib`,
//! by its native API (`getTypeAtPosition` on the probe alias, `typeToString`
//! with `NoTruncation | InTypeAlias`, `getSemanticDiagnostics` for each code,
//! start and end) or by `tsc`, assigning each probe to a type it does not fit.

use std::sync::Arc;

use verter_type_engine::semantic_query::{
    FactResult, FlowReturnDegradation, FlowReturnResult, ReturnProjectionDemand,
};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::{OwnedSlice, PrimitiveName, TypeExpr};

use super::differential_harness_tests::ALL;
use super::pinia_options_store_tests::{
    answer_mismatch, named_answer, wire_answer, Project, SCENARIO,
};
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

/// A literal-typed receiver reads its primitive wrapper, so a member that
/// wrapper lacks is TS2339 at its name and `any`.
#[test]
fn an_absent_member_of_a_literal_is_the_checkers_error_type() {
    for (source, within) in [
        ("export function f() { const s = 'x'; return s.zz; }", ".zz"),
        ("export function f(t: 'x' | 'y') { return t.zz; }", ".zz"),
        ("export function f(t: true) { return t.zz; }", ".zz"),
        ("export function f(t: 1) { return t.zz; }", ".zz"),
    ] {
        assert_row(
            source,
            &[("ReturnType<typeof f>", "any")],
            &declaration("f"),
            &[ts2339("zz", within)],
        );
    }
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

/// A written access a closed type proves absent is the checker's TS2339 and its
/// error type, read through whatever names the type and in whatever position
/// the access stands; a conditional over the error type is the error type.
#[test]
fn a_written_access_the_object_proves_absent_is_the_error_type() {
    let project = Project::module(
        "type Named = { a: 1 } & { c: 2 };\n\
         type Applied<T> = { own: T } & Named;\n\
         type Open<T> = (T & { a: 1 })['b'];\n",
    );
    let failures = project.mismatches(&[
        ("{ a: 1 }['b']", "any"),
        ("Named['b']", "any"),
        ("Applied<1>['b']", "any"),
        ("Pick<{ a: 1 }, 'a'>['b']", "any"),
        ("[{ a: 1 }['b'], { a: 1 }['a']]", "[any, 1]"),
        ("ReturnType<{ a: 1 }['b']>", "any"),
        ("{ a: 1 }['b'] extends string ? 1 : 2", "any"),
        ("{ a: 1 }['a' | 'b']", "any"),
        ("{ a: 1 }['b']['c']", "any"),
        ("Named['b']['c']", "any"),
        // Both are the error type, which the checker prints as the name.
        ("Undeclared['b']", "any"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let host = project.host(ALL[0].options, &["{ a: 1 }['b']", "Applied<1>['b']"]);
    for probe in 0..2 {
        assert_eq!(
            named_answer(&host, probe).checker_recovery_code(),
            Some(2339)
        );
    }
}

/// The checker answers a generic access where it instantiates, and with no
/// access to report on: an absent key is `unknown`, with TS2536 at the generic
/// declaration and no TS2339. An access over no generic object is settled where
/// it is written, so its instantiation holds that access's error type.
#[test]
fn an_instantiated_access_of_an_absent_key_is_unknown() {
    let project = Project::module(
        "type Named = { a: 1 };\n\
         type Open<T> = (T & { a: 1 })['b'];\n\
         type Bounded<T extends { a: 1 }> = T['b'];\n\
         type Branch<T> = T extends { a: 1 } ? T['b'] : never;\n\
         type ByKey<K extends string> = { a: 1 }[K];\n\
         type KeyUnion<K extends 'a' | 'b'> = { a: 1 }[K];\n\
         type Mapped<K extends string> = { [P in K]: { a: 1 }[P] };\n\
         type Two<T, U> = (T & U)['b'];\n\
         type OverConditional<T> = (T extends { a: 1 } ? { c: 1 } : { d: 1 })['nope'];\n\
         type Bare<T> = T['b'];\n\
         type Member<T> = { x: T['b'] };\n\
         type Own<T> = { own: T }['nope'];\n",
    );
    let unknown = [
        "Open<{ c: 2 }>",
        "Bounded<{ a: 1 }>",
        "Branch<{ a: 1 }>",
        "ByKey<'b'>",
        "KeyUnion<'a' | 'b'>",
        "Mapped<'b'>['b']",
        "Two<{ a: 1 }, { c: 1 }>",
        "OverConditional<{ a: 1 }>",
        "Bare<Named>",
        "Member<Named>['x']",
    ];
    let mut rows: Vec<(&str, &str)> = unknown.iter().map(|probe| (*probe, "unknown")).collect();
    rows.extend([
        ("Open<{ b: 2 }>", "2"),
        ("ByKey<'a'>", "1"),
        ("Own<1>", "any"),
        // A written access of the instantiation's `unknown` is its own TS2339.
        ("Bare<Named>['z']", "any"),
    ]);
    let failures = project.mismatches(&rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let host = project.host(ALL[0].options, &unknown);
    for (ordinal, probe) in unknown.iter().enumerate() {
        assert_eq!(
            named_answer(&host, ordinal).checker_recovery_code(),
            None,
            "{probe}"
        );
    }
    let host = project.host(ALL[0].options, &["Own<1>"]);
    assert_eq!(named_answer(&host, 0).checker_recovery_code(), Some(2339));
}

/// An access absence is not proven for keeps its answer or its cause: an index
/// signature or a present member reads its value, an unresolved import proves
/// nothing, and neither does an application whose genericness depends on what
/// it names.
#[test]
fn an_access_absence_is_not_proven_for_is_no_recovery() {
    let project = Project::module(
        "import { Missing } from './nowhere';\n\
         type Foo<T> = { own: T };\n\
         type Id<T> = T;\n\
         type ThroughFoo<T> = Foo<T>['nope'];\n\
         type ThroughId<T> = Id<T>['nope'];\n\
         export type Unresolved = Missing['b'];\n\
         export type Heritage = Missing & { a: 1 };\n",
    );
    let failures = project.mismatches(&[("Record<string, 1>['b']", "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let host = project.host(ALL[0].options, &["ThroughFoo<1>", "ThroughId<{ a: 1 }>"]);
    for probe in 0..2 {
        let node = named_answer(&host, probe);
        assert_eq!(node.checker_recovery_code(), None);
        assert_ne!(
            wire_answer(&host, &node),
            TypeExpr::Primitive(PrimitiveName::Any)
        );
        assert_ne!(
            wire_answer(&host, &node),
            TypeExpr::Primitive(PrimitiveName::Unknown)
        );
    }
    let host = project.host(ALL[0].options, &["Unresolved", "Heritage['b']"]);
    for probe in 0..2 {
        let node = named_answer(&host, probe);
        assert_eq!(node.checker_recovery_code(), None);
        assert_ne!(
            wire_answer(&host, &node),
            TypeExpr::Primitive(PrimitiveName::Any)
        );
    }
}

/// What a literal's functions report on their own is no report once the call
/// gives the literal a `this`: a valid marked store reports nothing. A read the
/// marked `this` still lacks stays reported, and a return it decides is circular
/// under `noImplicitAny` when that `this` holds the type the literal infers,
/// which the message prints.
#[test]
fn a_contextual_this_takes_the_literals_own_reports() {
    let marked = "declare function m<A>(o: { a?: A & ThisType<A & { n: 1 }> }): A;\n";
    let valid = Project::module(&format!(
        "{marked}export const v = m({{ a: {{ r() {{ return this.n; }} }} }});"
    ));
    let host = valid.host(ALL[0].options, &["typeof v"]);
    let result = result_of(&host, &initializer("v")).expect("the store has a value");
    assert!(
        result.checker_diagnostics().is_empty(),
        "{:?}",
        result.checker_diagnostics()
    );
    assert_row(
        &format!("{marked}export const v = m({{ a: {{ r() {{ return this.nope; }} }} }});"),
        &[("typeof v", "{ r(): any }")],
        &initializer("v"),
        &[ts7023("r", "r() {"), ts2339("nope", "this.nope")],
    );
    assert_row(
        "declare function m<A>(o: { a?: A & ThisType<{ n: 1 }> }): A;\n\
         export const v = m({ a: { r() { return this.nope; } } });",
        &[("typeof v", "{ r(): any }")],
        &initializer("v"),
        &[ts2339("nope", "this.nope")],
    );
}

/// A namespace member the export table excludes is no absent member: reading it
/// from a function keeps its typed cause, while a name the module does not
/// export is the checker's TS2339.
#[test]
fn an_excluded_namespace_member_is_not_proven_absent() {
    let files = [
        ("a.ts", "export const x = 1;"),
        ("b.ts", "export const x = 's';"),
        ("index.ts", "export * from './a'; export * from './b';"),
    ];
    let project = Project::module_beside(
        "import * as ns from './index';\n\
         export function excluded() { return ns.x; }\n\
         export function absent() { return ns.zz; }",
        &files,
    );
    let host = project.host(ALL[0].options, &[]);
    let result = result_of(&host, &declaration("excluded")).expect("the value degrades");
    assert!(result.degradation().is_some());
    assert!(result.checker_diagnostics().is_empty());
    let result = result_of(&host, &declaration("absent")).expect("the read recovers");
    assert_eq!(result.degradation(), None);
    assert_eq!(result.checker_diagnostics().len(), 1);
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

/// A nested declaration reports its own circular return on the enclosing flow.
#[test]
fn a_local_declarations_circular_return_reports_at_its_name() {
    assert_row(
        "export function outer() { function local() { return 1 + local(); } return local(); }",
        &[("ReturnType<typeof outer>", "any")],
        &declaration("outer"),
        &[ts7023("local", "local() {")],
    );
    assert_row(
        "export function outer() { function local(): number { return 1 + local(); } return local(); }",
        &[("ReturnType<typeof outer>", "number")],
        &declaration("outer"),
        &[],
    );
    assert_row(
        "export function outer() { function local() { return local(); } return local(); }",
        &[("ReturnType<typeof outer>", "never")],
        &declaration("outer"),
        &[],
    );
}
