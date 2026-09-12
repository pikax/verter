//! Completion algebra of a body whose end point is unreachable.
//!
//! Two boundaries meet here, and each discriminates the other:
//!
//! - a loop that is ENTERED and never completes normally ends its region's
//!   normal path, so the body stops contributing the fall-through
//!   `undefined` and a lone literal return widens;
//! - a body that then contributes NO return arm models as `void` or
//!   `never` purely by the function's authored FORM.
//!
//! Every expected value is anchored against `tsc 7.0.2 --strict
//! --ignoreConfig` through `--declaration --emitDeclarationOnly`, which
//! prints the inferred return type directly.
//!
//! The negative controls are the point of the table. The checker decides
//! divergence in its BINDER, on the condition's exact token: `while (1)`
//! and `while ((true))` keep a reachable exit because the token is not the
//! `true` keyword, and the parenthesized form proves the rule does not
//! skip parentheses. A `break` bound to a nested loop or a nested `switch`
//! does not reach the outer loop's exit, while a labeled `break` naming a
//! label that wraps it does. Each of those rows fails against a
//! truthiness-evaluating or break-agnostic classifier.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{LiteralValue, PrimitiveName, TopLevelOwnerId, TypeExpr};

const LOOPS: &str = "/ws/loopcompletion/loops.ts";

/// Each row's authored source, with the checker's answer in the comment.
const LOOPS_SRC: &str = r#"
export function divBare() { while (true) {} }
export function divForever() { for (;;) {} }
export function divDoWhile() { do {} while (true) }
export function divBareReturnAfter(c: boolean) { if (c) return; while (true) {} }

export function divAfterReturn(c: boolean) { if (c) return 1; while (true) {} }
export function divNested(c: boolean) { if (c) return 1; while (true) { while (c) { break } } }
export function divContinueOnly(c: boolean) { if (c) return 1; while (true) { continue } }
export function divBreakInSwitch(c: boolean) { if (c) return 1; while (true) { switch (1) { case 1: break } } }
export function divergeInsideTry(c: boolean) { if (c) return 1; try { while (true) {} } finally { } }

export function divWithBreak(c: boolean) { if (c) return 1; while (true) { break } }
export function divLabeledBreak(c: boolean) { if (c) return 1; L: while (true) { break L } }
export function divInnerLabeledBreakOuter(c: boolean) { if (c) return 1; L: while (true) { while (c) { break L } } }
export function divDoWhileBreak(c: boolean) { if (c) return 1; do { break } while (true) }
export function divForeverBreak(c: boolean) { if (c) return 1; for (;;) { break } }
export function breakFromTryInLoop(c: boolean) { if (c) return 1; while (true) { try { break } finally { } } }
export function divCondTrue(c: boolean) { if (c) return 1; while (1) {} }
export function divParenTrue(c: boolean) { if (c) return 1; while ((true)) {} }

export function ifArmDiverges(c: boolean) { if (c) { while (true) {} } return "a" as const }
export function declThrow() { throw new Error() }

export function nestedArrowDiverge() { return () => { while (true) {} } }
export function nestedArrowThrow() { return () => { throw new Error() } }
export function nestedFnExprDiverge() { return function () { while (true) {} } }
"#;

fn lang(canonical: &str) -> crate::FileLanguage {
    crate::LanguageRegistry::global()
        .classify_static(canonical)
        .static_resolution()
}

fn host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(LOOPS.to_string()),
        input_id: LOOPS.to_string(),
        source: Arc::from(LOOPS_SRC),
        file_language: lang(LOOPS),
        aliases: Vec::new(),
    });
    host
}

/// The published value, its degradation, and the family memo's candidate
/// count — a wrong answer that also WARMS is the defect class these rows
/// exist to catch, so the candidate count is pinned alongside the type.
#[derive(Debug, PartialEq)]
struct Published {
    ty: TypeExpr,
    degraded: bool,
    candidates: usize,
}

fn publish(host: &Arc<VerterHost>, canonical: &str, name: &str) -> Published {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(canonical),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    };
    match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) => {
            let ty = host
                .project_node_to_type_expr_for_test(result.return_type())
                .expect("the flow-return value projects");
            Published {
                ty,
                degraded: result.degradation().is_some(),
                candidates: dispatch
                    .graph()
                    .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            }
        }
        other => panic!("{name}: expected a flow-return value, got {other:?}"),
    }
}

fn primitive(name: PrimitiveName) -> TypeExpr {
    TypeExpr::Primitive(name)
}

/// `1 | undefined` — the checker's answer whenever the body's end point
/// stays reachable alongside a single literal return.
fn one_or_undefined() -> TypeExpr {
    TypeExpr::Union(Arc::from(vec![
        TypeExpr::Literal(LiteralValue::Number(1.0)),
        primitive(PrimitiveName::Undefined),
    ]))
}

fn assert_clean(host: &Arc<VerterHost>, canonical: &str, name: &str, expected: TypeExpr) {
    assert_eq!(
        publish(host, canonical, name),
        Published {
            ty: expected,
            degraded: false,
            candidates: 1,
        },
        "{name} must publish the checker's answer, clean and warm-admitted"
    );
}

/// The RETURN type of the callable a function returns — the nested
/// function's own join, which is where a non-declaration form is seeded.
fn nested_return(host: &Arc<VerterHost>, name: &str) -> TypeExpr {
    let published = publish(host, LOOPS, name);
    assert!(
        !published.degraded && published.candidates == 1,
        "{name} must publish clean and warm"
    );
    match published.ty {
        TypeExpr::Function(ref function) => function
            .return_type
            .as_deref()
            .cloned()
            .unwrap_or_else(|| panic!("{name} returns a callable with an inferred return")),
        other => panic!("{name} must publish a callable, got {other:?}"),
    }
}

/// A loop whose exit edge is unreachable ends the body's normal path, so a
/// lone literal return is the function's SOLE contributor and widens.
///
/// Before this rule the fall-through `undefined` was still joined and the
/// widening was suppressed, publishing `1 | undefined` — warm-admitted,
/// with no degradation to mark it.
#[test]
fn a_divergent_loop_ends_the_bodys_normal_path() {
    let host = host();
    for name in [
        "divAfterReturn",
        "divNested",
        "divContinueOnly",
        "divBreakInSwitch",
        "divergeInsideTry",
    ] {
        assert_clean(&host, LOOPS, name, primitive(PrimitiveName::Number));
    }
}

/// The exit edge stays reachable whenever a `break` can target the loop,
/// and whenever the condition is not the bare `true` keyword.
///
/// `divNested` / `divBreakInSwitch` above and `divInnerLabeledBreakOuter`
/// here are the same shape up to which construct the `break` binds to, so
/// a classifier that merely looks for a `break` anywhere inside the loop
/// fails one pair or the other. `divCondTrue` and `divParenTrue` fail any
/// classifier that evaluates truthiness or skips parentheses instead of
/// matching the checker's binder token.
#[test]
fn a_reachable_loop_exit_still_contributes_the_implicit_undefined() {
    let host = host();
    for name in [
        "divWithBreak",
        "divLabeledBreak",
        "divInnerLabeledBreakOuter",
        "divDoWhileBreak",
        "divForeverBreak",
        "breakFromTryInLoop",
        "divCondTrue",
        "divParenTrue",
    ] {
        assert_clean(&host, LOOPS, name, one_or_undefined());
    }
}

/// A body that contributes no return arm and never completes normally
/// models as `void` or `never` purely by the function's authored form —
/// the checker's `mayReturnNever` rule. A function DECLARATION is `void`,
/// including the throw-only body, which published `never` before this
/// rule existed.
#[test]
pub(crate) fn an_empty_completion_seeds_by_the_functions_authored_form() {
    let host = host();
    for name in [
        "divBare",
        "divForever",
        "divDoWhile",
        "divBareReturnAfter",
        "declThrow",
    ] {
        assert_clean(&host, LOOPS, name, primitive(PrimitiveName::Void));
    }
    for name in [
        "nestedArrowDiverge",
        "nestedArrowThrow",
        "nestedFnExprDiverge",
    ] {
        assert_eq!(
            nested_return(&host, name),
            primitive(PrimitiveName::Never),
            "{name} returns a callable whose own empty completion is never"
        );
    }
}

/// Divergence is a property of the ARM it appears in, not of the body: an
/// `if` arm that diverges leaves the function's other path reaching the
/// trailing return, which stays the sole contributor.
#[test]
fn a_divergent_branch_arm_does_not_terminate_the_enclosing_body() {
    let host = host();
    assert_clean(
        &host,
        LOOPS,
        "ifArmDiverges",
        TypeExpr::Literal(LiteralValue::String("a".into())),
    );
}
