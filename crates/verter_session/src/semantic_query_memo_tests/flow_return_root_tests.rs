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
    identity_of("makeProps")
}

fn identity_of(symbol: &str) -> FlowFunctionReturnIdentity {
    FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from(symbol),
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
    deliver_of(host, &identity(), cancellation)
}

fn deliver_of(
    host: &VerterHost,
    function: &FlowFunctionReturnIdentity,
    cancellation: Option<CancellationToken>,
) -> Result<Arc<FlowReturnResult>, FlowReturnError> {
    let demand = ReturnProjectionDemand::whole_return();
    let carrier = match cancellation {
        Some(token) => host.get_flow_return_type_with_audit_cancellable(function, demand, token),
        None => host.get_flow_return_type_with_audit(function, demand),
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

    // The repeated delivery is the warm one: it hands back the node the
    // retry computed, and materializes the same value.
    let repeated = deliver(&host, None).expect("the repeat resolves");
    assert_eq!(repeated.degradation(), None);
    assert_eq!(repeated.return_type(), retried.return_type());
    assert_eq!(materialize(&host, &repeated), fresh);

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

/// Materializing a delivered return reads the node the lease already owns:
/// with the memos retired it publishes no memo entry and interns no node (no
/// second evaluation), and it leaves the graph's ownership exactly as it
/// found it (no accounting of its own). A re-delivery, by contrast,
/// evaluates and publishes.
#[test]
fn materializing_a_return_evaluates_nothing_and_keeps_no_ownership() {
    let host = host_with(NUMBER_RETURN);
    let result = deliver(&host, None).expect("the return resolves");
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    graph.invalidate_all();
    assert_eq!(graph.memo_entry_count(), 0, "premise: no memo survives");
    let allocated = graph.node_slot_count();
    let owned = graph.node_ownership_counts();

    for _ in 0..2 {
        let value = materialize(&host, &result);
        assert!(value.contains("\"a\""), "{value}");
        assert_eq!(graph.memo_entry_count(), 0, "nothing was evaluated");
        assert_eq!(graph.node_slot_count(), allocated, "no node was interned");
        assert_eq!(
            graph.node_ownership_counts(),
            owned,
            "ownership is unchanged"
        );
    }

    let again = deliver(&host, None).expect("the re-delivery resolves");
    assert!(graph.memo_entry_count() > 0, "a re-delivery evaluates");
    assert_eq!(materialize(&host, &again), materialize(&host, &result));
}

/// The held result keeps materializing the same value after the caches
/// retire and the host closes, the last holder's drop drains its node, and
/// an unrelated reader is left live and readable.
#[test]
fn a_held_return_materializes_after_retirement_and_close_then_drains() {
    let host = host_with(&format!(
        "{NUMBER_RETURN}\nfunction makeOther() {{ return {{ c: true as boolean }} }}"
    ));
    let result = deliver(&host, None).expect("the return resolves");
    let before = materialize(&host, &result);
    let unrelated =
        deliver_of(&host, &identity_of("makeOther"), None).expect("the unrelated return resolves");
    let unrelated_value = materialize(&host, &unrelated);
    assert!(unrelated_value.contains("\"c\""), "{unrelated_value}");
    assert_ne!(unrelated_value, before);
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let reader = result.return_root().expect("an external reader's root");
    let node = reader.id();
    let unrelated_node = unrelated.return_type();
    assert_ne!(unrelated_node, node);

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
    assert!(
        graph.node_is_live(unrelated_node),
        "the unrelated reader survives"
    );
    assert_eq!(materialize(&host, &unrelated), unrelated_value);
}
