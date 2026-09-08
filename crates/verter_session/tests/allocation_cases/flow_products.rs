use std::sync::Arc;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::InputBasisId;
use verter_session::for_tests::*;
use verter_session::semantic_query::{
    CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowInputContext, FlowReturnContext,
    FlowReturnKey, FlowReturnPolicy, PrimitiveKind, ResolvedDeclSlotIdentity,
    ReturnProjectionDemand, SemanticNodeData, SemanticQueryKey,
};

struct Basis(u8);
impl CanonicalEncode for Basis {
    const DOMAIN_TAG: &'static str = "verter.session.product_execution.test_basis.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_u32(1, u32::from(self.0));
    }
}
fn request(basis: u8) -> FlowDemandRequest {
    FlowDemandRequest {
        query: SemanticQueryKey::FlowReturn(Box::new(FlowReturnKey {
            function: FlowFunctionSlotIdentity {
                declaration_slot: ResolvedDeclSlotIdentity::value_slot(
                    Arc::from("/flow_solve_fixture.ts"),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    Arc::from("products"),
                    0,
                    [0; 16],
                    [0; 16],
                ),
                function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
                overload_ordinal: 0,
            },
            normalized_type_args: Arc::from([]),
            context: FlowReturnContext {
                parse_env_hash: [0; 16],
                resolve_env_hash: [0; 16],
                type_env_hash: [0; 16],
                lib_env_hash: [0; 16],
                project_identity: [0; 16],
                type_substitution: CanonicalTypeSubstitution::empty(),
                policy: FlowReturnPolicy {},
            },
            demand: ReturnProjectionDemand::whole_return(),
            input: FlowInputContext::empty(),
            result_contract: flow_return_result_contract_id(),
        })),
        input_basis: InputBasisId::from_canonical(&Basis(basis)),
        resources: FlowResourcePolicy::default(),
        additional_requirements: Arc::from([]),
    }
}

use super::{alloc_bytes, alloc_count, reset_alloc_counter};

#[test]
fn sparse_multiway_join_visits_only_materialized_predecessor_cells() {
    let types = SemanticGraphStore::new();
    let number = types.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let algebra = GraphSemanticAlgebra(&types);
    for predecessors in [32, 256] {
        let names = (0..predecessors)
            .map(|i| format!("x{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let fixture = flow_graph_fixture_for_tests(
            &format!("function products({names}) {{ return [{names}]; }}"),
            1,
        );
        let request = request(0);
        let retained = fixture.retained_plan(&request).unwrap();
        let cap = fixture
            .prepare_execution_with_retained(&request, &retained)
            .unwrap();
        let inputs = fixture.product_inputs();
        let mut execution =
            FlowProductExecution::new_for_selection(&inputs, cap, FlowProductBudget::default())
                .unwrap();
        let keys: Vec<_> = execution
            .selected_sites()
            .take(predecessors)
            .map(|site| site.key(FlowDomain::ReachingType).unwrap())
            .collect();
        let mut states: Vec<_> = (0..predecessors).map(|_| execution.empty_state()).collect();
        for (state, key) in states.iter_mut().zip(&keys) {
            execution
                .apply_transfers(
                    state,
                    &key.site(),
                    &[FlowProductTransfer {
                        key: key.clone(),
                        value: Some(FlowProductValue::ReachingType(ReachingTypeProduct::of(
                            number,
                        ))),
                    }],
                )
                .unwrap();
        }
        let incoming: Vec<_> = states.iter().collect();
        reset_alloc_counter();
        let result = execution.join_products(&incoming, &algebra).unwrap();
        let allocated = (alloc_count(), alloc_bytes());
        assert_eq!(result.len(), predecessors);
        assert!(keys.iter().all(|key| result.get(key)
            == Some(&FlowProductValue::ReachingType(ReachingTypeProduct::of(
                number
            )))));
        eprintln!("sparse flow join predecessors={predecessors} materialized={predecessors} visits={} allocations={allocated:?}", execution.join_product_visits_for_tests());
        assert_eq!(
            execution.join_product_visits_for_tests(),
            predecessors,
            "absent predecessor cells must not enter the join work stream"
        );
    }
}

#[test]
fn multiway_reaching_definitions_allocate_for_inputs_once() {
    let graph = SemanticGraphStore::new();
    let algebra = GraphSemanticAlgebra(&graph);
    for predecessors in [32, 256] {
        let names = (0..predecessors)
            .map(|i| format!("x{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let fixture = flow_graph_fixture_for_tests(
            &format!("function products({names}) {{ return [{names}]; }}"),
            1,
        );
        let request = request(0);
        let retained = fixture.retained_plan(&request).unwrap();
        let selection = fixture
            .prepare_execution_with_retained(&request, &retained)
            .unwrap();
        let inputs = fixture.product_inputs();
        let mut execution = FlowProductExecution::new_for_selection(
            &inputs,
            selection,
            FlowProductBudget {
                max_product_width: predecessors as u32,
                ..FlowProductBudget::default()
            },
        )
        .unwrap();
        let sites: Vec<_> = execution.selected_sites().take(predecessors).collect();
        let key = sites[0].key(FlowDomain::ReachingValue).unwrap();
        let mut states: Vec<_> = (0..predecessors).map(|_| execution.empty_state()).collect();
        for (state, site) in states.iter_mut().zip(&sites) {
            execution
                .apply_transfers(
                    state,
                    site,
                    &[FlowProductTransfer {
                        key: key.clone(),
                        value: Some(FlowProductValue::ReachingValue(ReachingValueProduct::at(
                            site,
                        ))),
                    }],
                )
                .unwrap();
        }
        let incoming: Vec<_> = states.iter().collect();
        reset_alloc_counter();
        let joined = execution.join_products(&incoming, &algebra).unwrap();
        let bytes = alloc_bytes();
        let Some(FlowProductValue::ReachingValue(value)) = joined.get(&key) else {
            panic!("joined definitions");
        };
        assert_eq!(value.definitions().len(), predecessors);
        assert!(value
            .definitions()
            .windows(2)
            .all(|pair| pair[0].index() < pair[1].index()));
        eprintln!("reaching definition join predecessors={predecessors} allocated_bytes={bytes}");
        assert!(
            bytes <= predecessors as u64 * 384,
            "join must not allocate growing definition prefixes: {bytes}"
        );
    }
}

#[test]
fn continuation_allocation_depends_on_materialized_products_not_selected_capacity() {
    let types = SemanticGraphStore::new();
    let number = types.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = types.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let algebra = GraphSemanticAlgebra(&types);
    let mut baselines = std::collections::BTreeMap::new();
    for selected_width in [61, 253, 1021, 4093] {
        let names = (0..selected_width)
            .map(|i| format!("x{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let source = format!("function products({names}) {{ return [{names}]; }}");
        let fixture = flow_graph_fixture_for_tests(&source, 1);
        let mut request = request(0);
        request.resources.max_obligations = 0;
        request.resources.slice_budget.max_selected_nodes = 20000;
        let retained = fixture.retained_plan(&request).unwrap();
        let cap = fixture
            .prepare_execution_with_retained(&request, &retained)
            .unwrap();
        let inputs = fixture.product_inputs();
        for materialized in [1, 8, 64] {
            let mut execution = FlowProductExecution::new_for_selection(
                &inputs,
                Arc::clone(&cap),
                FlowProductBudget {
                    max_products: 64,
                    ..FlowProductBudget::default()
                },
            )
            .unwrap();
            let keys: Vec<_> = execution
                .selected_sites()
                .take(materialized)
                .map(|site| site.key(FlowDomain::ReachingType).unwrap())
                .collect();
            assert_eq!(keys.len(), materialized);
            let mut state = execution.empty_state();
            for key in &keys {
                execution
                    .apply_transfers(
                        &mut state,
                        &key.site(),
                        &[FlowProductTransfer {
                            key: key.clone(),
                            value: Some(FlowProductValue::ReachingType(ReachingTypeProduct::of(
                                number,
                            ))),
                        }],
                    )
                    .unwrap();
            }
            reset_alloc_counter();
            let started = std::time::Instant::now();
            let snapshots: [FlowProductStore; 256] = std::array::from_fn(|_| state.clone());
            let clone_ns = started.elapsed().as_nanos();
            let clone_cost = (alloc_count(), alloc_bytes());
            assert_eq!(
                clone_cost,
                (0, 0),
                "branch snapshots must share their persistent directory"
            );
            for snapshot in &snapshots {
                assert!(snapshot.shares_continuation_storage(&state));
            }
            let transfers = [FlowProductTransfer {
                key: keys[0].clone(),
                value: Some(FlowProductValue::ReachingType(ReachingTypeProduct::of(
                    string,
                ))),
            }];
            reset_alloc_counter();
            let mut branches = snapshots;
            let started = std::time::Instant::now();
            for branch in &mut branches {
                execution
                    .apply_transfers(branch, &keys[0].site(), &transfers)
                    .unwrap();
            }
            let write_ns = started.elapsed().as_nanos();
            let write_cost = (alloc_count(), alloc_bytes());
            reset_alloc_counter();
            let started = std::time::Instant::now();
            let count = branches
                .iter()
                .map(|state| state.ordered_entries().count())
                .sum::<usize>();
            let iterate_ns = started.elapsed().as_nanos();
            let iterate_cost = (alloc_count(), alloc_bytes());
            assert_eq!(count, materialized * 256);
            reset_alloc_counter();
            let started = std::time::Instant::now();
            let joined = execution
                .join_products(&[&branches[0], &branches[1]], &algebra)
                .unwrap();
            let join_ns = started.elapsed().as_nanos();
            let join_cost = (alloc_count(), alloc_bytes());
            assert_eq!(joined.len(), materialized);
            let cost = (write_cost, iterate_cost, join_cost);
            if let Some(baseline) = baselines.get(&materialized) {
                assert_eq!(
                    &cost, baseline,
                    "unused selected capacity must not change branch allocation"
                );
            } else {
                baselines.insert(materialized, cost);
            }
            eprintln!("flow-products selected={} active={materialized} snapshots=256 clone={clone_cost:?} write={write_cost:?} iteration={iterate_cost:?} join={join_cost:?} clone_ns={clone_ns} write_ns={write_ns} iterate_ns={iterate_ns} join_ns={join_ns}", execution.selected_subject_count());
        }
    }
}
