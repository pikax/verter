//! A delivered flow-return result hands out an owning root of its return
//! node, through its own delivery lease, and the shared bounded projection
//! materializes the value from it.

use std::sync::Arc;

use crate::host_flow_return_audit::FlowReturnError;
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host;
use crate::u6_flow_shape_corpus_tests::{module_script, upsert};
use crate::{FileLanguage, VerterHost};
use verter_execution::cancellation::CancellationToken;
use verter_type_engine::semantic_query::{FlowReturnResult, QueryError, ReturnProjectionDemand};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

const FILE: &str = "/flow-return-root/main.ts";
const NUMBER_RETURN: &str = "function makeProps() { return { a: 1 as number } }";
const STRING_RETURN: &str = "function makeProps() { return { b: \"x\" as string } }";
const DEGRADED_RETURN: &str = "import type { AKind, BKind } from \"./absent-nominal-source\";\ntype A = { kind: AKind; a: number }; type B = { kind: BKind; b: number };\nfunction isA(x: A | B): x is A { return true as boolean as never }\nfunction isB(x: A | B): x is B { return true as boolean as never }\nfunction makeProps(x: A | B) { return { v: isA(x) ? (isB(x) ? x : \"ok\") : \"no\" } }";

fn host_with(source: &str) -> Arc<VerterHost> {
    let host = make_audit_host();
    upsert(
        &host,
        FILE,
        &module_script(source),
        FileLanguage::script_ts(),
    );
    host
}

fn identity() -> FlowFunctionReturnIdentity {
    FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from("makeProps"),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

fn deliver(
    host: &VerterHost,
    cancellation: Option<CancellationToken>,
) -> Result<Arc<FlowReturnResult>, FlowReturnError> {
    let demand = ReturnProjectionDemand::whole_return();
    let carrier = match cancellation {
        Some(token) => host.get_flow_return_type_with_audit_cancellable(&identity(), demand, token),
        None => host.get_flow_return_type_with_audit(&identity(), demand),
    };
    carrier.into_parts().0
}

/// The return value materialized from the result's own lease.
fn materialize(host: &VerterHost, result: &FlowReturnResult) -> String {
    let root = result
        .return_root()
        .expect("a delivered result roots its return");
    let bytes = host
        .project_node_to_type_expr_json_bytes(&root)
        .expect("the owned root projects");
    String::from_utf8(bytes).expect("UTF-8 TypeExpr JSON")
}

fn exact_value(source: &str) -> String {
    let host = host_with(source);
    let result = deliver(&host, None).expect("the return resolves");
    assert_eq!(result.degradation(), None, "premise: an exact return");
    materialize(&host, &result)
}

/// A precancelled call followed by an uncancelled retry yields the value a
/// fresh host computes; a different exact return, a degraded return and an
/// undelivered result each fail the comparison.
#[test]
fn a_cancelled_then_retried_return_materializes_like_a_fresh_host() {
    let fresh = exact_value(NUMBER_RETURN);
    assert!(fresh.contains("\"a\""), "{fresh}");

    let host = host_with(NUMBER_RETURN);
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(
        deliver(&host, Some(token)).err(),
        Some(FlowReturnError::Cancelled)
    );
    let retried = deliver(&host, None).expect("the retry resolves");
    assert_eq!(retried.degradation(), None);
    assert_eq!(materialize(&host, &retried), fresh);

    let other = exact_value(STRING_RETURN);
    assert!(other.contains("\"b\""), "{other}");
    assert_ne!(other, fresh, "a different exact return differs");

    let degraded_host = host_with(DEGRADED_RETURN);
    let degraded = deliver(&degraded_host, None).expect("a degraded return is usable");
    assert!(degraded.degradation().is_some(), "premise: degraded");
    assert_ne!(materialize(&degraded_host, &degraded), fresh);
}

/// A result never delivered out of the engine roots nothing, and a root
/// another host's graph minted projects nothing.
#[test]
fn an_undelivered_or_foreign_return_root_projects_nothing() {
    let host = host_with(NUMBER_RETURN);
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let ordinal = crate::for_tests::flow_return_result_for_tests(
        &graph,
        verter_type_engine::semantic_query::SemanticNodeId(0),
    );
    assert!(matches!(
        ordinal.return_root(),
        Err(QueryError::ForeignSemanticOperand)
    ));

    let result = deliver(&host, None).expect("the return resolves");
    let root = result.return_root().expect("rooted");
    let other = host_with(NUMBER_RETURN);
    assert!(other.project_node_to_type_expr_json_bytes(&root).is_none());
}

/// The held result keeps materializing the same value after the caches
/// retire and the host closes, and the last holder's drop drains the node.
#[test]
fn a_held_return_materializes_after_retirement_and_close_then_drains() {
    let host = host_with(NUMBER_RETURN);
    let result = deliver(&host, None).expect("the return resolves");
    let before = materialize(&host, &result);
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let reader = result.return_root().expect("an external reader's root");
    let node = reader.id();

    graph.invalidate_all();
    assert_eq!(
        materialize(&host, &result),
        before,
        "after cache retirement"
    );
    host.close();
    assert_eq!(materialize(&host, &result), before, "after host close");

    drop(result);
    assert!(
        graph.node_is_live(node),
        "the external reader still roots it"
    );
    drop(reader);
    assert!(!graph.node_is_live(node), "the final release drains");
}
