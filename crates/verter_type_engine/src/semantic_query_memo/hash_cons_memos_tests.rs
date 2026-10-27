use super::*;
use crate::project_semantic_dispatch::cost_receipt::{
    CostDependency, CostIdentity, DemandCostReceipt, LogicalUsage, Nesting,
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
                CostIdentity::of_key((input, context, demand))
                    .with_roots(graph.root_nodes([input]).unwrap()),
                LogicalUsage::default(),
                Vec::new(),
            ),
            (
                verter_session_query::facts::policy::FactPolicy::production()
                    .set_of([dependency.clone()]),
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
            verter_session_query::facts::policy::FactPolicy::production(),
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
            facts,
            verter_session_query::facts::policy::FactPolicy::production().set_of([dependency]),
            "a consumed memo must deliver its dependency proof"
        );
        assert_eq!(read.value, result);
        graph.clear_hash_cons_memos();
        drop(read);
        assert!(
            graph.node_is_live(input),
            "evicting the memo cannot revoke the consumed operand after its receipt drops"
        );
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

#[test]
fn deferred_publication_reuses_the_receipts_operand_owner() {
    for same_result in [false, true] {
        let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
        let graph = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
        let context = ProjectionReductionContext::structural_transit();
        let demand = DeferredEvaluationDemand::Shell;
        let _construction = graph.enter_node_root_scope();
        let input = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "input".into(),
        )));
        let result = if same_result {
            input
        } else {
            graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "result".into(),
            )))
        };
        let receipt = DemandCostReceipt::new(
            CostIdentity::of_key((input, context, demand))
                .with_roots(graph.root_nodes([input]).unwrap()),
            LogicalUsage::default(),
            Vec::new(),
        );
        let before_roots = graph.node_ownership_counts().roots;
        let before_owners = super::super::node_roots::root_owner_constructions_for_tests();
        graph.evaluate_deferred_memo_publish(
            input,
            context,
            demand,
            result,
            receipt,
            (
                verter_session_query::facts::policy::FactPolicy::production().empty_set(),
                Arc::from([]),
            ),
        );
        assert_eq!(
            graph.peek_evaluate_deferred_memo_for_tests(input, context),
            Some(())
        );
        assert_eq!(
            graph.node_ownership_counts().roots - before_roots,
            usize::from(!same_result),
            "publication acquires only a distinct result; the receipt owns the input"
        );
        assert_eq!(
            super::super::node_roots::root_owner_constructions_for_tests() - before_owners,
            0,
            "a short result set allocates no owner"
        );
    }
}

#[test]
fn a_held_evaluation_receipt_does_not_keep_a_distinct_result_after_eviction() {
    let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
    let graph = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let context = ProjectionReductionContext::structural_transit();
    let demand = DeferredEvaluationDemand::Shell;
    let (input, result, prerequisite) = {
        let _construction = graph.enter_node_root_scope();
        let input = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "input".into(),
        )));
        let result = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "result".into(),
        )));
        let receipt = DemandCostReceipt::new(
            CostIdentity::of_key((input, context, demand))
                .with_roots(graph.root_nodes([input]).unwrap()),
            LogicalUsage::default(),
            Vec::new(),
        );
        graph.evaluate_deferred_memo_publish(
            input,
            context,
            demand,
            result,
            Arc::clone(&receipt),
            (
                verter_session_query::facts::policy::FactPolicy::production().empty_set(),
                Arc::from([]),
            ),
        );
        (input, result, receipt)
    };
    assert!(graph.node_is_live(input) && graph.node_is_live(result));
    graph.clear_hash_cons_memos();
    assert!(
        graph.node_is_live(input),
        "the prerequisite still owns its operand"
    );
    assert!(
        !graph.node_is_live(result),
        "a prerequisite must not acquire the evaluated result"
    );
    drop(prerequisite);
    assert!(!graph.node_is_live(input));
    drop(graph);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

#[test]
fn result_owner_reuse_refuses_a_stale_result_without_acquiring_the_input_again() {
    let graph = SemanticGraphStore::default();
    let _construction = graph.enter_node_root_scope();
    let input = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "input".into(),
    )));
    let operand = graph.root_nodes([input]).unwrap();
    let stale = {
        let _nested = graph.enter_node_root_scope();
        graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "stale".into(),
        )))
    };
    assert!(!graph.node_is_live(stale));
    let before = graph.node_ownership_counts().roots;
    assert!(matches!(
        operand.retain_evaluation_result(&graph, input, stale),
        Err(crate::semantic_query::QueryError::StaleSemanticOperand)
    ));
    assert_eq!(graph.node_ownership_counts().roots, before);
    assert!(graph.node_is_live(input));
    let other_graph = SemanticGraphStore::default();
    assert!(matches!(
        operand.retain_evaluation_result(&other_graph, input, input),
        Err(crate::semantic_query::QueryError::ForeignSemanticOperand)
    ));
}

#[test]
fn retained_topology_includes_an_unrelated_prerequisite_operand() {
    for prepared_identity in [false, true] {
        for substitution in [false, true] {
            let graph = SemanticGraphStore::default();
            let context = ProjectionReductionContext::structural_transit();
            let demand = DeferredEvaluationDemand::Shell;
            let (input, result, prerequisite) = {
                let _construction = graph.enter_node_root_scope();
                let nodes: Vec<_> = ["input", "result", "prerequisite"]
                    .into_iter()
                    .map(|name| {
                        graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                            name.into(),
                        )))
                    })
                    .collect();
                let prerequisite = DemandCostReceipt::new(
                    if prepared_identity {
                        let prepared = graph.prepare_rooted_key(
                            crate::semantic_query::SemanticQueryKey::KeyOf {
                                base: nodes[2],
                                context,
                            },
                        );
                        CostIdentity::of_producer(prepared.as_task_producer())
                    } else {
                        CostIdentity::of_key(nodes[2])
                            .with_roots(graph.root_nodes([nodes[2]]).unwrap())
                    },
                    LogicalUsage::default(),
                    Vec::new(),
                );
                let receipt = DemandCostReceipt::new(
                    CostIdentity::of_key((nodes[0], context, demand))
                        .with_roots(graph.root_nodes([nodes[0]]).unwrap()),
                    LogicalUsage::default(),
                    vec![CostDependency {
                        receipt: prerequisite,
                        nesting: Nesting::InPlace,
                    }],
                );
                if substitution {
                    graph.substitute_memo_publish_priced(
                        nodes[0], nodes[0], nodes[0], nodes[1], receipt,
                    );
                } else {
                    graph.evaluate_deferred_memo_publish(
                        nodes[0],
                        context,
                        demand,
                        nodes[1],
                        receipt,
                        (
                            verter_session_query::facts::policy::FactPolicy::production()
                                .empty_set(),
                            Arc::from([]),
                        ),
                    );
                }
                (nodes[0], nodes[1], nodes[2])
            };
            assert!(graph.node_is_live(prerequisite));
            let retained = graph.hash_cons_retained_nodes_for_tests();
            assert!(retained.contains(&input) && retained.contains(&result));
            assert!(
                retained.contains(&prerequisite),
                "the memo owns the prerequisite through its receipt"
            );
            graph.clear_hash_cons_memos();
            assert!(!graph.node_is_live(prerequisite));
        }
    }
}
