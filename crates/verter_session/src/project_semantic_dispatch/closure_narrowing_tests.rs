//! Narrowing inside a nested function body. A nested function applies its
//! OWN guards to the bindings it captures, and a capture whose narrowing
//! at the function's creation still holds — a `const`, or a parameter or
//! `let` past its last assignment (no write after the creation, none in
//! any closure) — enters the body at that narrowed type: the checker
//! extends a function expression's, arrow's or object-literal or
//! class-expression method's control-flow container to the enclosing one
//! for such a reference. A callable in a class PROPERTY initializer stops
//! at the property, so no enclosing narrowing reaches it. An immediately
//! invoked function is no control-flow container at all: its captures read
//! the flow reaching the call. Every other capture reads its declared type.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `declare const v: <probe>; export const s: null = v;` read
//! off the TS2322 message (`tsc --noEmit --strict --ignoreConfig`).

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
export function arrowCase(n: number | undefined) { return () => { if (n) { return n; } return 'none'; }; }
export function objCase(n: number | undefined) { return { m() { if (n) { return n; } return 'none'; } }; }
export function clsCase(n: number | undefined) { return class { m() { if (n) { return n; } return 'none'; } }; }
export function guardedMember(o: { s: string } | undefined) { return () => { if (o) { return o.s; } return ''; }; }
export function outerNarrow(n: number | undefined) { if (n === undefined) { throw 0; } return () => n; }
export function outerNarrowObj(n: number | undefined) { if (n === undefined) { throw 0; } return { m() { return n; } }; }
export function methodCase(n: number | undefined) { if (n === undefined) { throw 0; } return class { m() { return n; } }; }
export function letAfter(n: number | undefined) { let x = n; if (x === undefined) { throw 0; } return () => x; }
export function constAnn(n: number | undefined) { const c: number | undefined = n; if (c === undefined) { throw 0; } return () => c; }
export function letReassigned(n: number | undefined) { let x = n; if (x === undefined) { throw 0; } const f = () => x; x = undefined; return f; }
export function letInClosure(n: number | undefined) { let x = n; if (x === undefined) { throw 0; } const set = () => { x = undefined; }; set(); return () => x; }
export function propInit(n: number | undefined) { if (n === undefined) { throw 0; } return class { f = () => n; }; }
";

/// A nested body narrows a captured binding with its own guards.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof arrowCase>`,
/// `ReturnType<typeof objCase>['m']` and `InstanceType<ReturnType<typeof
/// clsCase>>['m']` are `() => number | "none"`, and `ReturnType<typeof
/// guardedMember>` is `() => string`.
#[test]
fn a_nested_body_narrows_a_captured_binding_by_its_own_guards() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof arrowCase>", "() => number | \"none\""),
            ("ReturnType<typeof objCase>['m']", "() => number | \"none\""),
            (
                "InstanceType<ReturnType<typeof clsCase>>['m']",
                "() => number | \"none\"",
            ),
            ("ReturnType<typeof guardedMember>", "() => string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A capture past its last assignment enters the body at its narrowed
/// type where the function is created; one written after the creation, or
/// in any closure, enters at its declared type, and a class property
/// initializer's callable sees no enclosing narrowing.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof outerNarrow>`,
/// `ReturnType<typeof outerNarrowObj>['m']`, `InstanceType<ReturnType<
/// typeof methodCase>>['m']`, `ReturnType<typeof letAfter>` and
/// `ReturnType<typeof constAnn>` are `() => number`; `ReturnType<typeof
/// letReassigned>`, `ReturnType<typeof letInClosure>` and
/// `InstanceType<ReturnType<typeof propInit>>['f']` are `() => number |
/// undefined`.
#[test]
fn a_capture_past_its_last_assignment_keeps_its_narrowing() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof outerNarrow>", "() => number"),
            ("ReturnType<typeof outerNarrowObj>['m']", "() => number"),
            (
                "InstanceType<ReturnType<typeof methodCase>>['m']",
                "() => number",
            ),
            ("ReturnType<typeof letAfter>", "() => number"),
            ("ReturnType<typeof constAnn>", "() => number"),
            (
                "ReturnType<typeof letReassigned>",
                "() => number | undefined",
            ),
            (
                "ReturnType<typeof letInClosure>",
                "() => number | undefined",
            ),
            (
                "InstanceType<ReturnType<typeof propInit>>['f']",
                "() => number | undefined",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const INVOKED: &str = "\
export function iifeWrittenLater(x: string | number) { if (typeof x === \"string\") { return (() => x)(); } x = 0; return 0; }
export function iifeTernary(x: string | number) { const r = typeof x === \"string\" ? (() => x)() : 0; x = 0; return r; }
export function iifeFunction(x: string | number) { if (typeof x === \"string\") { return (function () { return x; })(); } x = 0; return 0; }
export function iifeVar(v: string | number) { var w = v; if (typeof w === \"string\") { return (() => w)(); } return 0; }
export function iifeLetWrittenLater(v: string | number) { let w = v; if (typeof w === \"string\") { const r = (() => w)(); w = 0; return r; } return 0; }
export function iifeWritesItself(x: string | number) { if (typeof x === \"string\") { return (() => { const b = x; x = 0; return b; })(); } return 0; }
export function iifeLetWritesItself(v: string | number) { let w = v; if (typeof w === \"string\") { return (() => { const b = w; w = 0; return b; })(); } return 0; }
export function iifeBeforeSameStatementWrite(x: string | number) { if (typeof x === \"string\") { return { a: (() => x)(), b: (x = 0) }; } return undefined; }
export function asyncLater(x: string | number) { if (typeof x === \"string\") { const p = (async () => x)(); x = 0; return p; } return undefined; }
export function asyncStored(x: string | number) { if (typeof x === \"string\") { const g = async () => x; x = 0; return g(); } return undefined; }
export function paramWrittenLater(x: string | number) { if (typeof x === \"string\") { const g = () => x; x = 0; return g(); } return 0; }
export function varUnwritten(v: string | number) { var w = v; if (typeof w === \"string\") { const g = () => w; return g(); } return 0; }
export function varWrittenBefore() { var w = \"a\" as string | number; w = 1; const g = () => w; return g(); }
";

const FILE: &str = "/ws/closures/invoked.ts";

/// The degradation `name`'s own flow return carries.
fn degradation_of(
    source: &str,
    name: &str,
) -> Option<crate::semantic_query::FlowReturnDegradation> {
    use crate::semantic_query::{
        SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
    };
    let host = std::sync::Arc::new(crate::VerterHost::new_standalone(
        crate::types::HostConfig::default(),
    ));
    let _ = host.upsert(crate::types::UpsertRequest {
        canonical_id: Some(FILE.to_string()),
        input_id: FILE.to_string(),
        source: std::sync::Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(FILE)
            .static_resolution(),
        aliases: Vec::new(),
    });
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = super::ProjectSemanticDispatch::new(&host_ctx);
    let key = crate::semantic_query::FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            std::sync::Arc::from(FILE),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            std::sync::Arc::from(name),
            verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(FILE),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    };
    let super::QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(result),
        ..
    }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
    else {
        panic!("{name} must produce a value");
    };
    result.degradation()
}

/// An immediately invoked function — async ones included — is no
/// control-flow container of its own, so its captures read the narrowing
/// reaching the call whatever the enclosing body assigns afterwards or the
/// function itself assigns.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof iifeWrittenLater>`,
/// `ReturnType<typeof iifeTernary>`, `ReturnType<typeof iifeFunction>`,
/// `ReturnType<typeof iifeVar>`, `ReturnType<typeof iifeLetWrittenLater>`,
/// `ReturnType<typeof iifeWritesItself>` and `ReturnType<typeof
/// iifeLetWritesItself>` are `string | 0`, `ReturnType<typeof
/// iifeBeforeSameStatementWrite>` is `{ a: string; b: number; } |
/// undefined` (the write later in the same statement runs after the call),
/// and `ReturnType<typeof asyncLater>` is `Promise<string> | undefined`.
#[test]
fn an_invoked_function_reads_the_narrowing_reaching_its_call() {
    let failures = mismatches(
        INVOKED,
        &[
            ("ReturnType<typeof iifeWrittenLater>", "string | 0"),
            ("ReturnType<typeof iifeTernary>", "string | 0"),
            ("ReturnType<typeof iifeFunction>", "string | 0"),
            ("ReturnType<typeof iifeVar>", "string | 0"),
            ("ReturnType<typeof iifeLetWrittenLater>", "string | 0"),
            ("ReturnType<typeof iifeWritesItself>", "string | 0"),
            ("ReturnType<typeof iifeLetWritesItself>", "string | 0"),
            (
                "ReturnType<typeof iifeBeforeSameStatementWrite>",
                "{ a: string; b: number; } | undefined",
            ),
            (
                "ReturnType<typeof asyncLater>",
                "Promise<string> | undefined",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for name in [
        "iifeWrittenLater",
        "iifeTernary",
        "iifeFunction",
        "iifeVar",
        "iifeLetWrittenLater",
        "iifeWritesItself",
        "iifeLetWritesItself",
        "iifeBeforeSameStatementWrite",
        "asyncLater",
    ] {
        assert_eq!(degradation_of(INVOKED, name), None, "{name} is complete");
    }
}

/// A capture outside its extended container reads its declared type in
/// the body: a parameter's authority, and an unannotated `var`'s
/// declarator type while no write retypes it before the creation. An
/// unannotated `var` reassigned before the creation reaches the closure at
/// the assigned type, which is not what the checker reads, so it takes the
/// typed closure-capture gap.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof paramWrittenLater>`,
/// `ReturnType<typeof varUnwritten>` and `ReturnType<typeof
/// varWrittenBefore>` are `string | number`, and `ReturnType<typeof
/// asyncStored>` is `Promise<string | number> | undefined`.
#[test]
fn a_capture_outside_its_container_reads_its_declared_type() {
    let failures = mismatches(
        INVOKED,
        &[
            ("ReturnType<typeof paramWrittenLater>", "string | number"),
            ("ReturnType<typeof varUnwritten>", "string | number"),
            (
                "ReturnType<typeof asyncStored>",
                "Promise<string | number> | undefined",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for name in ["paramWrittenLater", "varUnwritten", "asyncStored"] {
        assert_eq!(degradation_of(INVOKED, name), None, "{name} is complete");
    }
    assert_eq!(
        degradation_of(INVOKED, "varWrittenBefore"),
        Some(crate::semantic_query::FlowReturnDegradation::FlowGap(
            crate::semantic_query::FlowGap::ClosureCapture
        )),
        "a `var` retyped before the creation takes the typed gap"
    );
}

const INVOKED_ASYNC_JOIN: &str = "export function asyncOrUndefined(x: string) { if (x) { return (async () => 1)(); } return undefined; }
export function asyncOrLiteral(x: string) { if (x) { return (async () => 1)(); } return 2; }
";

/// An invoked async function's result is a lib `Promise` — an object that
/// is never below a primitive or a literal — so joining it with one is
/// decided, not left behind the relation gap.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof asyncOrUndefined>` is
/// `Promise<number> | undefined` and `ReturnType<typeof asyncOrLiteral>`
/// is `2 | Promise<number>`.
#[test]
fn an_invoked_async_result_joins_a_primitive_return_decidedly() {
    let failures = mismatches(
        INVOKED_ASYNC_JOIN,
        &[
            (
                "ReturnType<typeof asyncOrUndefined>",
                "Promise<number> | undefined",
            ),
            ("ReturnType<typeof asyncOrLiteral>", "2 | Promise<number>"),
        ],
    );
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
    for name in ["asyncOrUndefined", "asyncOrLiteral"] {
        assert_eq!(
            degradation_of(INVOKED_ASYNC_JOIN, name),
            None,
            "{name} is complete"
        );
    }
}
