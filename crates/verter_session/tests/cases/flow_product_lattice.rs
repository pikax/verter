//! Selected execution, actual continuation joins, and exact product evidence.
use std::sync::Arc;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::InputBasisId;
use verter_semantic::analysis::flow::flow_graph::{FlowNodeId, FlowNodeKind};
use verter_session::for_tests::*;
use verter_session::semantic_query::{
    CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowInputContext, FlowReturnContext,
    FlowReturnKey, FlowReturnPolicy, LiteralValue, PrimitiveKind, ResolvedDeclSlotIdentity,
    ReturnProjectionDemand, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

const SOURCE: &str = "function products(x) { const y = x; return y; }";
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
fn execution(
    source: &str,
    version: u8,
    budget: FlowProductBudget,
) -> (FlowProductInputs, FlowProductExecution) {
    let fixture = flow_graph_fixture_for_tests(source, version);
    let plan = fixture.build_plan(request(0)).unwrap();
    let inputs = fixture.product_inputs();
    let execution = FlowProductExecution::new(&inputs, &plan, budget).unwrap();
    (inputs, execution)
}
fn binding(
    execution: &FlowProductExecution,
    inputs: &FlowProductInputs,
    name: &str,
    domain: FlowDomain,
) -> FlowProductKey {
    inputs
        .graph()
        .nodes()
        .find_map(|node| {
            let key = execution.key(domain, node).ok()?;
            key.binding()
                .is_some_and(|binding| binding.name.as_ref() == name)
                .then_some(key)
        })
        .expect("selected fixture binding")
}
fn return_node(inputs: &FlowProductInputs) -> FlowNodeId {
    inputs
        .graph()
        .nodes()
        .find(|node| matches!(inputs.graph().node_kind(*node), FlowNodeKind::ReturnSite(_)))
        .unwrap()
}
fn put(
    execution: &mut FlowProductExecution,
    store: &mut FlowProductStore,
    key: &FlowProductKey,
    value: FlowProductValue,
) -> Result<bool, FlowProductFailure> {
    execution.apply_transfers(
        store,
        &execution.site(key.node()).unwrap(),
        &[FlowProductTransfer {
            key: key.clone(),
            value: Some(value),
        }],
    )
}
fn types() -> (SemanticGraphStore, SemanticNodeId, SemanticNodeId) {
    let graph = SemanticGraphStore::new();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    (graph, number, string)
}
fn reaching(node: SemanticNodeId) -> FlowProductValue {
    FlowProductValue::ReachingType(ReachingTypeProduct::of(node))
}
fn joined(
    algebra: &dyn FlowSemanticAlgebra,
    a: &FlowProductValue,
    b: &FlowProductValue,
) -> FlowProductValue {
    match join_product(algebra, &FlowProductBudget::default(), a, b) {
        FlowTransferOutcome::Unchanged => a.clone(),
        FlowTransferOutcome::Changed(value) => value,
        other => panic!("valid product join: {other:?}"),
    }
}

#[test]
fn nonbinding_product_keys_preserve_graph_version_demand_and_execution_scope() {
    let fixture = flow_graph_fixture_for_tests(SOURCE, 41);
    let inputs = fixture.product_inputs();
    let plan = fixture.build_plan(request(0)).unwrap();
    let foreign_basis = fixture.build_plan(request(1)).unwrap();
    let (_, number, _) = types();
    let mut first =
        FlowProductExecution::new(&inputs, &plan, FlowProductBudget::default()).unwrap();
    let key = first
        .key(FlowDomain::ReachingType, return_node(&inputs))
        .unwrap();
    let mut store = first.empty_state();
    put(&mut first, &mut store, &key, reaching(number)).unwrap();
    for basis in [&plan, &foreign_basis] {
        let mut other =
            FlowProductExecution::new(&inputs, basis, FlowProductBudget::default()).unwrap();
        let foreign_key = other.key(FlowDomain::ReachingType, key.node()).unwrap();
        assert_ne!(key, foreign_key);
        let definitions = FlowProductValue::ReachingValue(ReachingValueProduct::at(
            &first.site(key.node()).unwrap(),
        ));
        let foreign_definitions = FlowProductValue::ReachingValue(ReachingValueProduct::at(
            &other.site(key.node()).unwrap(),
        ));
        let graph = SemanticGraphStore::new();
        assert!(matches!(
            join_product(
                &GraphSemanticAlgebra(&graph),
                &FlowProductBudget::default(),
                &definitions,
                &foreign_definitions
            ),
            FlowTransferOutcome::Gap(_)
        ));
        assert_eq!(store.get(&foreign_key), None);
        let site = other.site(key.node()).unwrap();
        assert_eq!(
            other.apply_transfers(
                &mut store,
                &site,
                &[FlowProductTransfer {
                    key: foreign_key,
                    value: Some(reaching(number))
                }]
            ),
            Err(FlowProductFailure::ScopeMismatch)
        );
        assert!(other.finish().is_err());
    }
    let later = flow_graph_fixture_for_tests(SOURCE, 42).product_inputs();
    assert!(matches!(
        FlowProductExecution::new(&later, &plan, FlowProductBudget::default()),
        Err(FlowProductKeyError::GraphMismatch)
    ));
    assert_eq!(store.get(&key), Some(&reaching(number)));
}

#[test]
fn unrelated_declarations_do_not_consume_the_product_budget() {
    let (_, number, _) = types();
    let mut sizes = Vec::new();
    for source in [
        "function products() { return 1; }",
        "function products() { const a = 1; const b = 2; const c = 3; return 1; }",
    ] {
        let (inputs, mut execution) = execution(
            source,
            1,
            FlowProductBudget {
                max_products: 1,
                max_iterations: 0,
                ..FlowProductBudget::default()
            },
        );
        sizes.push(execution.selected_subject_count());
        let mut store = execution.empty_state();
        assert!(store.is_empty());
        let key = execution
            .key(FlowDomain::ReachingType, return_node(&inputs))
            .unwrap();
        put(&mut execution, &mut store, &key, reaching(number)).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(execution.finish().unwrap().iterations(), 0);
        for node in inputs.graph().nodes() {
            if matches!(inputs.graph().node_kind(node), FlowNodeKind::Binding(_)) {
                assert_eq!(
                    execution.key(FlowDomain::ReachingType, node),
                    Err(FlowProductKeyError::UnselectedNode)
                );
            }
        }
    }
    assert_eq!(sizes[0], sizes[1]);
}

#[test]
fn writes_replace_reaching_state_and_kill_only_explicit_narrowing() {
    let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
    let (_, number, string) = types();
    let key = binding(&execution, &inputs, "y", FlowDomain::ReachingType);
    let narrow = execution.key(FlowDomain::Narrowing, key.node()).unwrap();
    let fact = FlowNarrowingFact {
        binding: key.binding().unwrap().clone(),
        path: Arc::from([Arc::from("member")]),
        narrowed_to: number,
    };
    let mut state = execution.empty_state();
    put(&mut execution, &mut state, &key, reaching(number)).unwrap();
    put(
        &mut execution,
        &mut state,
        &narrow,
        FlowProductValue::Narrowing(NarrowingProduct::new([fact.clone()])),
    )
    .unwrap();
    assert_eq!(
        state.get(&narrow),
        Some(&FlowProductValue::Narrowing(NarrowingProduct::new([fact])))
    );
    execution
        .apply_transfers(
            &mut state,
            &execution.site(key.node()).unwrap(),
            &[
                FlowProductTransfer {
                    key: key.clone(),
                    value: Some(reaching(string)),
                },
                FlowProductTransfer {
                    key: narrow.clone(),
                    value: None,
                },
            ],
        )
        .unwrap();
    assert_eq!(state.get(&key), Some(&reaching(string)));
    assert_eq!(state.get(&narrow), None);
}

#[test]
fn failed_transfer_bundles_preserve_prior_state_and_never_seal_evidence() {
    let (inputs, mut execution) = execution(
        SOURCE,
        1,
        FlowProductBudget {
            max_product_width: 1,
            ..FlowProductBudget::default()
        },
    );
    let (_, number, string) = types();
    let key = binding(&execution, &inputs, "y", FlowDomain::ReachingType);
    let narrow = execution.key(FlowDomain::Narrowing, key.node()).unwrap();
    let mut store = execution.empty_state();
    put(&mut execution, &mut store, &key, reaching(number)).unwrap();
    let facts = [number, string].map(|narrowed_to| FlowNarrowingFact {
        binding: key.binding().unwrap().clone(),
        path: Arc::from([]),
        narrowed_to,
    });
    let result = execution.apply_transfers(
        &mut store,
        &execution.site(key.node()).unwrap(),
        &[
            FlowProductTransfer {
                key: key.clone(),
                value: Some(reaching(string)),
            },
            FlowProductTransfer {
                key: narrow.clone(),
                value: Some(FlowProductValue::Narrowing(NarrowingProduct::new(facts))),
            },
        ],
    );
    assert!(matches!(
        result,
        Err(FlowProductFailure::BudgetExceeded(
            FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::Width,
                ..
            }
        ))
    ));
    assert_eq!(store.get(&key), Some(&reaching(number)));
    assert_eq!(store.get(&narrow), None);
    assert_eq!(store.len(), 1);
    assert!(execution.finish().is_err());
}

#[test]
fn successful_unchanged_transfers_prove_only_their_exact_domain_and_binding() {
    let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
    let (_, number, _) = types();
    let key = binding(&execution, &inputs, "y", FlowDomain::ReachingType);
    let other = binding(&execution, &inputs, "x", FlowDomain::ReachingType);
    let declared = execution.key(FlowDomain::DeclaredType, key.node()).unwrap();
    let mut state = execution.empty_state();
    assert!(put(&mut execution, &mut state, &key, reaching(number)).unwrap());
    assert!(!put(&mut execution, &mut state, &key, reaching(number)).unwrap());
    let evidence = execution.finish().unwrap();
    assert!(evidence.executed(&key));
    assert!(!evidence.executed(&other));
    assert!(!evidence.executed(&declared));
    assert_eq!(
        put(&mut execution, &mut state, &key, reaching(number)),
        Err(FlowProductFailure::Sealed)
    );
}

#[test]
fn continuation_joins_use_actual_predecessors_and_preserve_domain_rules() {
    let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
    let (graph, number, string) = types();
    let algebra = GraphSemanticAlgebra(&graph);
    let key = binding(&execution, &inputs, "y", FlowDomain::ReachingType);
    let assignment = execution
        .key(FlowDomain::DefiniteAssignment, key.node())
        .unwrap();
    let narrowing = execution.key(FlowDomain::Narrowing, key.node()).unwrap();
    let mut left = execution.empty_state();
    let mut right = execution.empty_state();
    put(&mut execution, &mut left, &key, reaching(number)).unwrap();
    put(&mut execution, &mut right, &key, reaching(string)).unwrap();
    put(
        &mut execution,
        &mut left,
        &assignment,
        FlowProductValue::DefiniteAssignment(DefiniteAssignmentProduct::assigned()),
    )
    .unwrap();
    put(
        &mut execution,
        &mut left,
        &narrowing,
        FlowProductValue::Narrowing(NarrowingProduct::new([FlowNarrowingFact {
            binding: key.binding().unwrap().clone(),
            path: Arc::from([]),
            narrowed_to: number,
        }])),
    )
    .unwrap();
    let merge = execution.join_products(&[&left, &right], &algebra).unwrap();
    assert_eq!(
        merge.get(&key),
        Some(&joined(&algebra, &reaching(number), &reaching(string)))
    );
    assert_eq!(
        merge.get(&assignment),
        Some(&FlowProductValue::DefiniteAssignment(
            DefiniteAssignmentProduct::default().with_state(DefiniteAssignment::MaybeAssigned)
        ))
    );
    assert_eq!(
        merge.get(&narrowing),
        Some(&FlowProductValue::Narrowing(NarrowingProduct::default()))
    );
    let surviving = execution.join_products(&[&right], &algebra).unwrap();
    assert_eq!(surviving.get(&key), right.get(&key));
    let mut later_right = execution.empty_state();
    let mut later_left = execution.empty_state();
    put(&mut execution, &mut later_right, &key, reaching(string)).unwrap();
    put(&mut execution, &mut later_left, &key, reaching(number)).unwrap();
    let repeated = execution
        .join_products(&[&later_left, &later_right], &algebra)
        .unwrap();
    assert_eq!(repeated.get(&key), merge.get(&key));
    assert_eq!(execution.finish().unwrap().iterations(), 0);
}

#[test]
fn hoisted_declaration_aliases_share_runtime_state_but_keep_exact_evidence() {
    let (inputs, mut execution) = execution(
        "function products(x) { var x = 1; return x; }",
        1,
        FlowProductBudget::default(),
    );
    let (_, number, _) = types();
    let keys: Vec<_> = inputs
        .graph()
        .nodes()
        .filter_map(|node| execution.key(FlowDomain::ReachingType, node).ok())
        .filter(|key| {
            key.binding()
                .is_some_and(|binding| binding.name.as_ref() == "x")
        })
        .collect();
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0].binding(), keys[1].binding());
    let mut state = execution.empty_state();
    put(&mut execution, &mut state, &keys[1], reaching(number)).unwrap();
    assert_eq!(state.get(&keys[0]), state.get(&keys[1]));
    assert_eq!(state.len(), 1);
    let evidence = execution.finish().unwrap();
    assert!(!evidence.executed(&keys[0]));
    assert!(evidence.executed(&keys[1]));
}

#[test]
fn iteration_and_product_caps_are_exact_without_counting_acyclic_work() {
    let (inputs, mut exact) = execution(
        SOURCE,
        1,
        FlowProductBudget {
            max_iterations: 1,
            max_products: 1,
            ..FlowProductBudget::default()
        },
    );
    let (_, number, _) = types();
    let key = binding(&exact, &inputs, "y", FlowDomain::ReachingType);
    let mut state = exact.empty_state();
    exact.note_iteration().unwrap();
    put(&mut exact, &mut state, &key, reaching(number)).unwrap();
    assert_eq!(exact.finish().unwrap().iterations(), 1);
    let (_, mut capped) = execution(
        SOURCE,
        1,
        FlowProductBudget {
            max_iterations: 1,
            ..FlowProductBudget::default()
        },
    );
    capped.note_iteration().unwrap();
    assert!(matches!(
        capped.note_iteration(),
        Err(FlowProductFailure::BudgetExceeded(
            FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::Iterations,
                observed: 2,
                ..
            }
        ))
    ));
    assert!(capped.finish().is_err());
    let (inputs, mut crowded) = execution(
        SOURCE,
        1,
        FlowProductBudget {
            max_products: 1,
            ..FlowProductBudget::default()
        },
    );
    let key = binding(&crowded, &inputs, "y", FlowDomain::ReachingType);
    let other = binding(&crowded, &inputs, "x", FlowDomain::ReachingType);
    let mut state = crowded.empty_state();
    put(&mut crowded, &mut state, &key, reaching(number)).unwrap();
    assert!(matches!(
        put(&mut crowded, &mut state, &other, reaching(number)),
        Err(FlowProductFailure::BudgetExceeded(
            FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::Products,
                ..
            }
        ))
    ));
    assert_eq!(state.len(), 1);
}

#[test]
fn product_domains_refuse_conflicts_mismatches_and_unproven_algebra() {
    let (graph, number, string) = types();
    let algebra = GraphSemanticAlgebra(&graph);
    let budget = FlowProductBudget::default();
    let a = FlowProductValue::DeclaredType(DeclaredTypeProduct::of(number));
    let b = FlowProductValue::DeclaredType(DeclaredTypeProduct::of(string));
    assert!(matches!(
        join_product(&algebra, &budget, &a, &b),
        FlowTransferOutcome::Gap(_)
    ));
    assert!(matches!(
        join_product(&algebra, &budget, &a, &reaching(number)),
        FlowTransferOutcome::Gap(_)
    ));
    let (inputs, execution) = execution(SOURCE, 1, budget);
    assert_eq!(
        execution.key(FlowDomain::Coverage, return_node(&inputs)),
        Err(FlowProductKeyError::DomainCarriesNoProduct)
    );
    let nodes: Vec<_> = inputs.graph().nodes().take(2).collect();
    let a = FlowProductValue::ReachingValue(ReachingValueProduct::at(
        &execution.site(nodes[0]).unwrap(),
    ));
    let b = FlowProductValue::ReachingValue(ReachingValueProduct::at(
        &execution.site(nodes[1]).unwrap(),
    ));
    assert_eq!(joined(&algebra, &a, &b), joined(&algebra, &b, &a));
    struct Incomplete;
    impl FlowSemanticAlgebra for Incomplete {
        fn union(&self, _: &[SemanticNodeId]) -> FlowAlgebraComposite {
            FlowAlgebraComposite {
                node: SemanticNodeId(0),
                incomplete: true,
            }
        }
        fn literal_arms(
            &self,
            _: SemanticNodeId,
        ) -> Result<Vec<(SemanticNodeId, LiteralValue)>, verter_session::semantic_query::FlowGap>
        {
            unreachable!()
        }
    }
    assert!(matches!(
        join_product(&Incomplete, &budget, &reaching(number), &reaching(string)),
        FlowTransferOutcome::Gap(_)
    ));
}

#[test]
fn widening_joins_preserve_pinned_values_and_only_surviving_literal_arms() {
    let graph = SemanticGraphStore::new();
    let algebra = GraphSemanticAlgebra(&graph);
    let literal = |n| graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(n)));
    let [one, two, three] = [1.0, 2.0, 3.0].map(literal);
    let left = algebra.union(&[one, two]).node;
    let right = algebra.union(&[two, three]).node;
    let fresh = FlowProductValue::ReachingType(
        ReachingTypeProduct::of(left).with_widening(Some(WideningMembership::All)),
    );
    let pinned = reaching(right);
    assert_eq!(joined(&algebra, &fresh, &fresh), fresh);
    for result in [
        joined(&algebra, &fresh, &pinned),
        joined(&algebra, &pinned, &fresh),
    ] {
        let FlowProductValue::ReachingType(result) = result else {
            unreachable!()
        };
        assert_eq!(
            result.widening(),
            Some(&WideningMembership::Partial(Arc::from([one])))
        );
    }
    let fresh_zero = FlowProductValue::ReachingType(
        ReachingTypeProduct::of(literal(0.0)).with_widening(Some(WideningMembership::All)),
    );
    let FlowProductValue::ReachingType(zero) =
        joined(&algebra, &fresh_zero, &reaching(literal(-0.0)))
    else {
        unreachable!()
    };
    assert_eq!(zero.widening(), None);
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let FlowProductValue::ReachingType(absorbed) = joined(&algebra, &fresh, &reaching(number))
    else {
        unreachable!()
    };
    assert_eq!(absorbed.united(), Some(number));
    assert_eq!(absorbed.widening(), None);
    let partial = FlowProductValue::ReachingType(
        ReachingTypeProduct::of(left)
            .with_widening(Some(WideningMembership::Partial(Arc::from([one])))),
    );
    let FlowProductValue::ReachingType(partial) = joined(&algebra, &partial, &reaching(three))
    else {
        unreachable!()
    };
    assert_eq!(
        partial.widening(),
        Some(&WideningMembership::Partial(Arc::from([one])))
    );
}

#[test]
fn narrowing_facts_follow_runtime_aliases_without_rewriting_declaration_evidence() {
    let (inputs, mut execution) = execution(
        "function products(x) { var x = 1; return x; }",
        1,
        FlowProductBudget::default(),
    );
    let (_, number, _) = types();
    let keys: Vec<_> = inputs
        .graph()
        .nodes()
        .filter_map(|node| execution.key(FlowDomain::Narrowing, node).ok())
        .filter(|key| key.binding().is_some())
        .collect();
    let mut state = execution.empty_state();
    let fact = FlowNarrowingFact {
        binding: keys[0].binding().unwrap().clone(),
        path: Arc::from([]),
        narrowed_to: number,
    };
    put(
        &mut execution,
        &mut state,
        &keys[1],
        FlowProductValue::Narrowing(NarrowingProduct::new([fact.clone()])),
    )
    .unwrap();
    assert_eq!(
        state.get(&keys[0]),
        Some(&FlowProductValue::Narrowing(NarrowingProduct::new([fact])))
    );
    let evidence = execution.finish().unwrap();
    assert!(!evidence.executed(&keys[0]));
    assert!(evidence.executed(&keys[1]));
}
