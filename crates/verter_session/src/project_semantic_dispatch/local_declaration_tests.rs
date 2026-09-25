//! A function declared inside a function body is read as the value it
//! declares, called through its own signature, and read before its
//! declaration (a function declaration is hoisted): the body's frame gives
//! the declaration's captures, a bare call to the function itself in one of
//! its own `return` statements does not contribute to its return type, and
//! a call cycle through return positions makes the signature's return
//! `any`.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `export const a: null = null! as <probe>;` read off the
//! TS2322 message (`tsc --noEmit --strict --ignoreConfig`), under all four
//! `strictNullChecks` × `noImplicitAny` settings; every answer below is the
//! same under all four (`noImplicitAny` adds TS7023 on the cyclic ones, it
//! does not change the type).

use std::sync::Arc;

use super::checker_probe_lane_tests::mismatches;
use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;

const CANONICAL: &str = "/ws/local-declarations.ts";

const FIXTURE: &str = "\
export function called() { function g() { return 1; } return g(); }
export function hoisted() { return g(); function g() { return 'a' as string; } }
export function readValue() { function g(x: number) { return x > 0 ? 'p' : 0; } return g; }
export function readHoisted() { return g; function g(x: string) { return x; } }
export function callsSibling() { function g() { return h(); } function h() { return true; } return g(); }
export function viaArrow() { function h() { return 1; } const k = () => h(); return k(); }
export function shadowsOwnName(v: number) { return (function helper() { function helper() { return v; } return helper(); })(); }
export function generic() { function g<T>(x: T) { return [x]; } return g(1); }
export function throughAlias() { function g() { return 1; } const a = g; return a(); }
export function hoistedIntoConst() { const r = g(); function g() { return [1, 'a'] as const; } return r; }
export function annotatedRecursion() { function rec(n: number): number { return n <= 0 ? 0 : rec(n - 1); } return rec(3); }
export function annotatedRecursionValue() { function rec(n: number): number { return n <= 0 ? 0 : rec(n - 1); } return rec; }
export function bareSelfReturn() { function rec(n: number) { if (n <= 0) return 0; return rec(n - 1); } return rec(3); }
export function bareSelfOnly() { function g(n: number) { return g(n - 1); } return g(3); }
export function parenthesizedSelfReturn() { function g(n: number) { if (n <= 0) return 1; return (g(n - 1)); } return g(3); }
export async function awaitedSelfReturn() { async function g(n: number) { if (n <= 0) return 1; return await g(n - 1); } return g(3); }
export function asyncSelfReturn() { async function g(n: number) { if (n <= 0) return 1; return g(n - 1); } return g(3); }
export function asyncSelfReturnValue() { async function g(n: number) { if (n <= 0) return 1; return await g(n - 1); } return g; }
export function asyncSelfOnly() { async function g(n: number) { return await g(n - 1); } return g(3); }
export function selfInExpression() { function g(n: number) { if (n <= 0) return 1; return g(n - 1) + 1; } return g(3); }
export function mutualCycle() { function a(n: number) { if (n <= 0) return 'x'; return b(n); } function b(n: number) { return a(n - 1); } return a(3); }
export function capturesConst() { const v = 1; function g() { return v; } return g(); }
export function capturesLet() { let v = 1; function g() { return v; } return g(); }
export function capturesParam(v: number) { function g() { return v; } return g(); }
export function capturesNarrowed(p: string | number) { if (typeof p === 'string') { function g() { return p; } return g(); } return 0; }
export function capturesReassignedLet() { let v = 1; function g() { return v; } v = 2; return g(); }
";

/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<typeof …>`
/// of `called` is `number`, `hoisted` `string`, `readValue` `(x: number) =>
/// "p" | 0`, `readHoisted` `(x: string) => string`, `callsSibling`
/// `boolean`, `viaArrow` `number`, `shadowsOwnName` (a body declaration of
/// a function expression's own name, capturing the outer parameter)
/// `number`, `generic` `number[]`, `throughAlias` `number`, `annotatedRecursion` `number`, `annotatedRecursionValue` `(n:
/// number) => number`; `hoistedIntoConst` is `readonly [1, "a"]`: its `[0]`
/// is `1`, its `[1]` is `"a"`, it extends `readonly [1, 'a']` (`1`) and not
/// `[1, 'a']` (`0`).
#[test]
fn a_local_function_declaration_is_its_value_and_its_call_is_its_return() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof called>", "number"),
            ("ReturnType<typeof hoisted>", "string"),
            ("ReturnType<typeof readValue>", "(x: number) => \"p\" | 0"),
            ("ReturnType<typeof readHoisted>", "(x: string) => string"),
            ("ReturnType<typeof callsSibling>", "boolean"),
            ("ReturnType<typeof viaArrow>", "number"),
            ("ReturnType<typeof shadowsOwnName>", "number"),
            ("ReturnType<typeof generic>", "number[]"),
            ("ReturnType<typeof throughAlias>", "number"),
            ("ReturnType<typeof hoistedIntoConst>[0]", "1"),
            ("ReturnType<typeof hoistedIntoConst>[1]", "\"a\""),
            (
                "ReturnType<typeof hoistedIntoConst> extends readonly [1, 'a'] ? 1 : 0",
                "1",
            ),
            (
                "ReturnType<typeof hoistedIntoConst> extends [1, 'a'] ? 1 : 0",
                "0",
            ),
            ("ReturnType<typeof annotatedRecursion>", "number"),
            (
                "ReturnType<typeof annotatedRecursionValue>",
                "(n: number) => number",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The checker skips a `return` whose expression is a bare call of the
/// function itself (parentheses peeled, and `await` peeled in an async
/// function) when it joins the return expressions — with nothing else to
/// join the return is `never` — and every other path to the function's own
/// return while that return is being resolved makes the return `any`.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<typeof …>`
/// of `bareSelfReturn` is `number` (the literal `0` widens, the skipped
/// self-call leaves no other contributor), `bareSelfOnly` `never` (`[never]`
/// in a tuple), `parenthesizedSelfReturn` `number`, `awaitedSelfReturn` and
/// `asyncSelfReturn` `Promise<number>`, `asyncSelfReturnValue` `(n: number)
/// => Promise<number>`, `asyncSelfOnly` `Promise<never>`, and `mutualCycle`
/// `any` (`0 extends 1 & T` holds).
#[test]
fn a_local_function_reaching_its_own_return_follows_the_checker() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof bareSelfReturn>", "number"),
            ("ReturnType<typeof bareSelfOnly>", "never"),
            ("ReturnType<typeof parenthesizedSelfReturn>", "number"),
            ("ReturnType<typeof awaitedSelfReturn>", "Promise<number>"),
            ("ReturnType<typeof asyncSelfReturn>", "Promise<number>"),
            (
                "ReturnType<typeof asyncSelfReturnValue>",
                "(n: number) => Promise<number>",
            ),
            ("ReturnType<typeof asyncSelfOnly>", "Promise<never>"),
            ("ReturnType<typeof mutualCycle>", "any"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A call of the function itself inside a larger return expression reads
/// the return that is being resolved: `any`, and `any + 1` is `any`.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<typeof
/// selfInExpression>` is `any` (`0 extends 1 & T` holds). The lane reads
/// the `+` over an `any` operand as an unmodelled position.
#[test]
#[ignore = "the checker's `+` over an `any` operand is `any`"]
fn a_local_function_calling_itself_inside_an_expression_returns_any() {
    let failures = mismatches(FIXTURE, &[("ReturnType<typeof selfInExpression>", "any")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A function declaration reads its captures at their declared type:
/// narrowing does not extend into a function declaration, and a literal
/// read through a capture widens in the declaration's return.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<typeof …>`
/// of `capturesConst`, `capturesLet`, `capturesParam` and
/// `capturesReassignedLet` is `number`, and `capturesNarrowed` is `string |
/// number` (`p` inside `g` is `string | number`, not the narrowed
/// `string`).
#[test]
fn a_local_function_reads_its_captures_at_their_declared_type() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof capturesConst>", "number"),
            ("ReturnType<typeof capturesLet>", "number"),
            ("ReturnType<typeof capturesParam>", "number"),
            ("ReturnType<typeof capturesNarrowed>", "string | number"),
            ("ReturnType<typeof capturesReassignedLet>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every enclosing function above evaluates clean and warm-admits exactly
/// one candidate.
#[test]
fn a_local_function_declaration_evaluates_clean() {
    let host = make_host();
    for name in [
        "called",
        "hoisted",
        "readValue",
        "readHoisted",
        "callsSibling",
        "viaArrow",
        "shadowsOwnName",
        "generic",
        "throughAlias",
        "hoistedIntoConst",
        "annotatedRecursion",
        "annotatedRecursionValue",
        "bareSelfReturn",
        "bareSelfOnly",
        "parenthesizedSelfReturn",
        "awaitedSelfReturn",
        "asyncSelfReturn",
        "asyncSelfReturnValue",
        "asyncSelfOnly",
        "mutualCycle",
        "capturesConst",
        "capturesLet",
        "capturesParam",
        "capturesNarrowed",
    ] {
        assert_clean(&host, name);
    }
}

/// A function declaration capturing a `let` the body reassigns after the
/// declaration reads the `let`'s declared type (`number`, measured above).
#[test]
fn a_local_function_capturing_a_reassigned_let_evaluates_clean() {
    assert_clean(&make_host(), "capturesReassignedLet");
}

/// A class declared inside a function body is read as its constructor,
/// instantiated, and read through its members, as a top-level class is.
///
/// Measured on TypeScript 7.0.2 (all four settings): over `f3` the
/// return's `v` and its `m`'s return are `number`; `ReturnType<typeof f4>`,
/// `…f7` and `…r8` are `number`, `…f11` is `string`; `f9`'s `m` returns
/// the instance (`1`); and `InstanceType<ReturnType<typeof r9>>['t']` is
/// `unknown`. The lane answers with the missing-member or unmodelled
/// marker, or an unreduced conditional.
#[test]
#[ignore = "a class declared in a function body is read as its constructor and instances"]
fn a_local_class_declaration_is_its_value() {
    let source = "\
export function f3() { class L { v = 1; m() { return this.v; } } return new L(); }
export function f4() { class L { v = 1; } return new L().v; }
export function f7() { class L { static s = 1; } return L.s; }
export function f9() { class L { m() { return this; } } return new L().m(); }
export function f11() { class L<T> { constructor(public t: T) {} } return new L('s').t; }
export function r8() { class L { static make() { return new L(); } v = 1; } return L.make().v; }
export function r9() { class L<T> { constructor(public t: T) {} } return L; }
";
    let failures = mismatches(
        source,
        &[
            ("ReturnType<typeof f3>['v']", "number"),
            ("ReturnType<ReturnType<typeof f3>['m']>", "number"),
            ("ReturnType<typeof f4>", "number"),
            ("ReturnType<typeof f7>", "number"),
            (
                "ReturnType<ReturnType<typeof f9>['m']> extends ReturnType<typeof f9> ? 1 : 0",
                "1",
            ),
            ("ReturnType<typeof f11>", "string"),
            ("ReturnType<typeof r8>", "number"),
            ("InstanceType<ReturnType<typeof r9>>['t']", "unknown"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[track_caller]
fn assert_clean(host: &Arc<VerterHost>, name: &str) {
    let (degradation, candidates) = evaluate(host, name);
    assert_eq!(degradation, None, "{name} must evaluate clean");
    assert_eq!(
        candidates, 1,
        "{name} must warm-admit exactly one candidate"
    );
}

fn make_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: Arc::from(FIXTURE),
        file_language: crate::LanguageRegistry::global()
            .classify_static(CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn evaluate(host: &Arc<VerterHost>, name: &str) -> (Option<FlowReturnDegradation>, usize) {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(CANONICAL),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(CANONICAL),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    };
    let QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(result),
        ..
    }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
    else {
        panic!("{name} must produce a value");
    };
    let degradation = result.degradation();
    let candidates = dispatch
        .graph()
        .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
    (degradation, candidates)
}
