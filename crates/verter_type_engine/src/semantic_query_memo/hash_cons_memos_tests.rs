use super::*;
use crate::project_semantic_dispatch::cost_receipt::{
    CostIdentity, DemandCostReceipt, LogicalUsage,
};
use crate::semantic_query::{LiteralValue, SemanticNodeData};
use verter_audit::observe::Uncaptured;
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_session_query::facts::fact_read_set::FactReadSetFinalise;
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};

#[test]
fn a_deferred_memo_serve_roots_its_result_in_the_consuming_scope() {
    let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
    let graph = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let context = ProjectionReductionContext::structural_transit();
    let demand = DeferredEvaluationDemand::Shell;
    let dependency = FactVersionRef::FileWholeHash {
        canonical_id: "/dependency.ts".into(),
        hash: [7; 16],
    };
    let (input, result) = {
        let _producer = graph.enter_node_root_scope();
        let input = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "input".into(),
        )));
        let result = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "result".into(),
        )));
        graph.evaluate_deferred_memo_publish(
            input,
            context,
            demand,
            result,
            DemandCostReceipt::new(
                CostIdentity::of_key((input, context, demand)),
                LogicalUsage::default(),
                Vec::new(),
            ),
            (
                Arc::from([dependency.clone()]),
                Arc::from([Arc::from("/dependency.ts")]),
            ),
        );
        (input, result)
    };
    assert!(graph.node_is_live(input) && graph.node_is_live(result));
    {
        let _consumer = graph.enter_node_root_scope();
        let (read, facts) = crate::resolver_core::resolver_context::with_fact_tracer_cell(
            Default::default(),
            |_| {
                let _nested_read = graph.enter_node_root_scope();
                graph
                    .read_deferred_evaluation::<Uncaptured>(input, context, demand, |_| true)
                    .expect("admitted evaluation")
            },
        );
        let FactReadSetFinalise::Ok(facts) = facts.finalise() else {
            panic!("a validated memo proof stays complete");
        };
        assert_eq!(
            facts.as_ref(),
            &[dependency],
            "a consumed memo must deliver its dependency proof"
        );
        assert_eq!(read.value, result);
        graph.clear_hash_cons_memos();
        drop(read);
        assert!(
            graph.node_is_live(result),
            "evicting the memo cannot revoke a consumed result"
        );
        assert!(
            account.snapshot().pinned_bytes > 0,
            "the surviving root owner stays charged"
        );
    }
    assert!(!graph.node_is_live(input) && !graph.node_is_live(result));
    drop(graph);
    assert_eq!(account.snapshot().total_bytes(), 0);
}
