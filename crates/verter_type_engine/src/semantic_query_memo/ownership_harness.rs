//! Controlled graph ownership fixture. Ordinary constructors always retain
//! canonical-sweep reclamation, including builds with test support enabled.

use super::{arena::NodeArena, SemanticGraphStore};

pub struct CountedOwnershipHarness {
    graph: SemanticGraphStore,
}

impl Default for CountedOwnershipHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl CountedOwnershipHarness {
    pub fn new() -> Self {
        Self::with_account(Default::default())
    }

    /// Configure fixture retention admission without changing any production
    /// constructor's reclamation mode.
    pub fn with_account(retention_account: verter_session_query::retention::StoreAccount) -> Self {
        Self {
            graph: SemanticGraphStore {
                arena: NodeArena::counted_for_tests(),
                retention_account,
                ..Default::default()
            },
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
    fn ordinary_graph_constructor_variants_keep_legacy_reclamation() {
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
            assert!(!graph.arena.counted());
            let node = {
                let _scope = graph.enter_node_root_scope();
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    "legacy constructor".into(),
                )))
            };
            assert!(
                graph.node_is_live(node),
                "dropping the construction scope must not reclaim a production slot"
            );
        }
    }

    #[test]
    fn counted_new_delivery_keeps_a_consumed_value_in_the_outer_builder() {
        let harness = CountedOwnershipHarness::new();
        harness.run(|graph| {
            let node = {
                let _inner = graph.enter_node_root_scope();
                let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    "inner result".into(),
                )));
                let read = crate::semantic_query::QueryResult::Value(
                    crate::semantic_query::SemanticQueryValue::TypeNode(node),
                );
                let lease = graph.lease_delivery(&read, &[]).unwrap();
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
        let harness = CountedOwnershipHarness::new();
        let (substitution, recipe, receipt) = harness.run(|graph| {
            let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "operand".into(),
            )));
            let substitution = CanonicalTypeSubstitution::new(vec![(node, node)]);
            let recipe = IntersectionInputRef::from_operands(&[node, node, node]);
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
        let first = CountedOwnershipHarness::new();
        let second = CountedOwnershipHarness::new();
        let make = |graph: &SemanticGraphStore| {
            let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "same content".into(),
            )));
            let nested = IntersectionInputRef::from_operands(&[node, node, node]);
            IntersectionInputRef::from_steps(&[
                IntersectionTerm::EvaluateSubgroup {
                    input: nested,
                    purpose: IntersectionPurpose::CheckerReduction,
                },
                IntersectionTerm::Value(node),
            ])
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
    fn counted_rooted_intern_rejects_foreign_arena_even_at_equal_ordinals() {
        let a = CountedOwnershipHarness::new();
        let b = CountedOwnershipHarness::new();
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
        let harness = CountedOwnershipHarness::new();
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
                        nodes: std::sync::Arc::from([]),
                        version,
                    },
                ),
            );
            store.replace_epoch().unwrap();
            let lease = graph.lease_delivery(&result, &[]).unwrap();
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
