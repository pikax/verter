//! A private semantic graph for ownership tests: the production store, with
//! fixture work run in an unaudited construction scope and its live node
//! count read back.

use super::SemanticGraphStore;

pub struct GraphFixture {
    graph: SemanticGraphStore,
}

impl Default for GraphFixture {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphFixture {
    pub fn new() -> Self {
        Self::with_account(Default::default())
    }

    /// A fixture graph charging `retention_account`.
    pub fn with_account(retention_account: verter_session_query::retention::StoreAccount) -> Self {
        Self {
            graph: SemanticGraphStore::with_account(retention_account),
        }
    }

    /// Run fixture work in an unaudited construction scope. Returned owned
    /// deliveries keep their roots after the scope releases intermediates.
    pub fn run<'a, T>(&'a self, work: impl FnOnce(&'a SemanticGraphStore) -> T) -> T {
        let _scope = self.graph.enter_node_root_scope();
        work(&self.graph)
    }

    pub fn live_nodes(&self) -> usize {
        self.graph.node_count()
    }

    pub(crate) fn into_graph(self) -> SemanticGraphStore {
        self.graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_semantic_dispatch::cost_receipt::{
        CostIdentity, DemandCostReceipt, LogicalUsage,
    };
    use crate::semantic_query::{
        CanonicalTypeSubstitution, IntersectionInputRef, IntersectionPurpose, IntersectionTerm,
        LiteralValue, NodeScopeId, SemanticNodeData, SemanticQueryKey,
    };

    #[test]
    fn signature_tokens_are_rooted_before_the_store_accessor_is_used() {
        let harness = GraphFixture::new();
        let node = harness.run(|graph| {
            let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "constructor attachment".into(),
            )));
            graph.signatures.intern_type_token(node, None).unwrap();
            node
        });
        assert!(harness.graph.node_is_live(node));
        harness.graph.signatures.replace_epoch().unwrap();
        assert_eq!(harness.live_nodes(), 0);
    }

    #[test]
    fn shape_key_roots_preserve_equality_and_hash_identity() {
        use crate::component_meta_caches::ShapeCacheKey;
        use crate::semantic_query::ProjectionMode;
        use std::hash::{Hash, Hasher};

        let harness = GraphFixture::new();
        let key = harness.run(|graph| {
            let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "shape operand".into(),
            )));
            ShapeCacheKey::member_value_node_whole_for_test(
                "/shape.ts".into(),
                node,
                ProjectionMode::Shallow,
            )
        });
        let node = key.member_value_subject_node().unwrap();
        let unrooted = ShapeCacheKey::member_value_node_whole_for_test(
            "/shape.ts".into(),
            node,
            ProjectionMode::Shallow,
        );
        let hash = |key: &ShapeCacheKey| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(key, unrooted);
        assert_eq!(hash(&key), hash(&unrooted));
        assert_eq!(harness.live_nodes(), 1);
        drop(key);
        assert_eq!(harness.live_nodes(), 0, "the key owns its operand");
    }

    /// Every production constructor runs counted reclamation: the node a
    /// closed construction scope alone owned is destroyed with it.
    /// Discriminating against any constructor that leaves graph slots to a
    /// later sweep.
    #[test]
    fn every_production_graph_constructor_destroys_on_last_release() {
        let graphs = [
            SemanticGraphStore::new(),
            SemanticGraphStore::with_account(Default::default()),
            SemanticGraphStore::with_provenance(
                std::sync::Arc::new(Default::default()),
                Default::default(),
            )
            .with_task_registry(Default::default()),
        ];
        for graph in graphs {
            let node = {
                let _scope = graph.enter_node_root_scope();
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    "scoped intermediate".into(),
                )))
            };
            assert!(
                !graph.node_is_live(node),
                "the scope's release is the node's last: it is destroyed"
            );
        }
    }

    #[test]
    fn counted_new_delivery_keeps_a_consumed_value_in_the_outer_builder() {
        let harness = GraphFixture::new();
        harness.run(|graph| {
            let node = {
                let _inner = graph.enter_node_root_scope();
                let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    "inner result".into(),
                )));
                let read = crate::semantic_query::QueryResult::Value(
                    crate::semantic_query::SemanticQueryValue::TypeNode(node),
                );
                let lease = graph
                    .lease_delivery(
                        crate::semantic_query_memo::delivery_lease::DeliveredAllocations::value(
                            &read,
                        ),
                    )
                    .unwrap();
                drop(lease);
                node
            };
            assert!(
                graph.node_is_live(node),
                "the outer builder owns a freshly issued delivery without cloning it"
            );
            let _ = graph.intern_node(SemanticNodeData::Alias(node));
        });
        assert_eq!(harness.live_nodes(), 0);
    }

    #[test]
    fn counted_identity_record_and_prefix_receipt_own_nonempty_operands() {
        let harness = GraphFixture::new();
        let (substitution, recipe, receipt) = harness.run(|graph| {
            let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "operand".into(),
            )));
            let substitution = CanonicalTypeSubstitution::new(graph, vec![(node, node)]);
            let recipe = IntersectionInputRef::from_operands(graph, &[node, node, node]);
            let prepared = graph
                .prepare_rooted_key(SemanticQueryKey::ClassifyTruthinessDomain { subject: node });
            let receipt = DemandCostReceipt::new(
                CostIdentity::of_producer(prepared.as_task_producer()),
                LogicalUsage {
                    work: 1,
                    ..Default::default()
                },
                Vec::new(),
            );
            (substitution, recipe, receipt)
        });
        assert_eq!(harness.live_nodes(), 1);
        drop(substitution);
        drop(recipe);
        assert_eq!(
            harness.live_nodes(),
            1,
            "receipt owns its prepared prefix identity"
        );
        drop(receipt);
        assert_eq!(harness.live_nodes(), 0);
    }

    #[test]
    fn counted_equal_recipes_in_distinct_arenas_do_not_share_nested_owners() {
        let first = GraphFixture::new();
        let second = GraphFixture::new();
        let make = |graph: &SemanticGraphStore| {
            let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "same content".into(),
            )));
            let nested = IntersectionInputRef::from_operands(graph, &[node, node, node]);
            IntersectionInputRef::from_steps(
                graph,
                &[
                    IntersectionTerm::EvaluateSubgroup {
                        input: nested,
                        purpose: IntersectionPurpose::CheckerReduction,
                    },
                    IntersectionTerm::Value(node),
                ],
            )
        };
        let a = first.run(make);
        let b = second.run(make);
        assert_eq!(
            a, b,
            "ownership qualification does not change semantic equality"
        );
        drop(a);
        assert_eq!(
            first.live_nodes(),
            0,
            "the second arena cannot retain the first's nested record roots"
        );
        assert_eq!(second.live_nodes(), 1);
        drop(b);
        assert_eq!(second.live_nodes(), 0);
    }

    #[test]
    fn counted_identity_records_root_in_their_issuing_graph_not_the_newest_dispatcher() {
        use crate::semantic_query::{ContextualTypingKey, FlowNarrowingKey, InferableParamSetId};
        use crate::semantic_query_memo::node_roots::DispatchConstructionScope;
        use std::sync::Arc;

        let issuing = GraphFixture::new();
        let newest = GraphFixture::new();
        let issuing_dispatcher = DispatchConstructionScope::enter(&issuing.graph.arena.core);
        // A second graph's dispatcher opened later is the thread's newest
        // owner, and allocates the same ordinal the issuing graph does.
        let newest_dispatcher = DispatchConstructionScope::enter(&newest.graph.arena.core);
        let node = issuing
            .graph
            .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "issued operand".into(),
            )));
        let coinciding = newest
            .graph
            .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "coinciding operand".into(),
            )));
        assert_eq!(node, coinciding);
        let graph = &issuing.graph;
        let set = || Arc::from([node]);
        let substitution = CanonicalTypeSubstitution::new(graph, vec![(node, node)]);
        let flow = FlowNarrowingKey::new(graph, set());
        let contextual = ContextualTypingKey::new(graph, set());
        let params = InferableParamSetId::new(graph, set());
        let recipe = IntersectionInputRef::from_operands(graph, &[node, node, node]);
        drop(newest_dispatcher);
        drop(issuing_dispatcher);
        assert_eq!(
            newest.live_nodes(),
            0,
            "no identity of the issuing graph retains the newest graph's node"
        );
        assert_eq!(
            issuing.live_nodes(),
            1,
            "the identities own their operand in the graph that issued it"
        );
        drop((substitution, flow, contextual, params));
        assert_eq!(issuing.live_nodes(), 1, "the recipe still owns it");
        drop(recipe);
        assert_eq!(issuing.live_nodes(), 0, "the final release drains it");
    }

    #[test]
    fn a_dispatcher_owner_keeps_each_delivery_once_however_often_it_is_read() {
        use crate::semantic_query::{QueryResult, SemanticQueryValue};
        use crate::semantic_query_memo::node_roots::{DispatchConstructionScope, NodeRootScope};

        let harness = GraphFixture::new();
        let dispatcher = DispatchConstructionScope::enter(&harness.graph.arena.core);
        let node = harness
            .graph
            .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "delivered".into(),
            )));
        let read = QueryResult::Value(SemanticQueryValue::TypeNode(node));
        let lease = harness
            .graph
            .lease_delivery(
                crate::semantic_query_memo::delivery_lease::DeliveredAllocations::value(&read),
            )
            .unwrap();
        for _ in 0..64 {
            drop(lease.clone());
        }
        {
            let _build = harness.graph.enter_node_root_scope();
            drop(lease.clone());
        }
        assert_eq!(
            NodeRootScope::retained_deliveries_for_tests(),
            1,
            "repeated reads of one delivery are kept once, not once per read"
        );
        drop(lease);
        assert_eq!(harness.live_nodes(), 1, "the dispatcher still owns it");
        drop(dispatcher);
        assert_eq!(NodeRootScope::retained_deliveries_for_tests(), 0);
        assert_eq!(harness.live_nodes(), 0);
    }

    #[test]
    fn counted_rooted_intern_rejects_foreign_arena_even_at_equal_ordinals() {
        let a = GraphFixture::new();
        let b = GraphFixture::new();
        let root = a.run(|graph| {
            graph
                .intern_rooted_node(
                    SemanticNodeData::Literal(LiteralValue::String("a".into())),
                    NodeScopeId::Global,
                    &Default::default(),
                )
                .unwrap()
        });
        b.run(|graph| {
            let other =
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::String("b".into())));
            assert_eq!(other, root.id());
            assert!(matches!(
                graph.intern_rooted_node(
                    SemanticNodeData::Alias(root.id()),
                    NodeScopeId::Global,
                    root.roots()
                ),
                Err(crate::semantic_query::QueryError::ForeignSemanticOperand)
            ));
        });
    }

    #[test]
    fn counted_signature_delivery_owns_its_exact_version_after_replacement() {
        let harness = GraphFixture::new();
        let lease = harness.run(|graph| {
            let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "signature token".into(),
            )));
            let store = graph.signature_store();
            let token = store.intern_type_token(node, None).unwrap();
            let set = crate::signature_kernel::test_support::intern_one_call(store);
            let version = crate::signature_kernel::SignatureVersion(Some(
                store.lease_epoch(store.epoch()).unwrap(),
            ));
            let result = crate::semantic_query::QueryResult::Value(
                crate::semantic_query::SemanticQueryValue::SignatureSet(
                    crate::signature_kernel::SignatureSetValue {
                        set,
                        nodes: Default::default(),
                        version,
                    },
                ),
            );
            store.replace_epoch().unwrap();
            let lease = graph
                .lease_delivery(
                    crate::semantic_query_memo::delivery_lease::DeliveredAllocations::value(
                        &result,
                    ),
                )
                .unwrap();
            assert_eq!(
                lease
                    .signature()
                    .unwrap()
                    .read_view()
                    .type_token_node(token)
                    .unwrap(),
                node
            );
            lease
        });
        assert_eq!(harness.live_nodes(), 1);
        drop(lease);
        assert_eq!(harness.live_nodes(), 0);
    }
}
