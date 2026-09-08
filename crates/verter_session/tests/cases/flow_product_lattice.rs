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

#[test]
fn captured_product_attachment_requires_the_original_pinned_graph() {
    let source = "function products(x) { function nested() { return x; } return nested; }";
    let fixture = flow_graph_fixture_for_tests_nested(source, 1, "nested");
    let mut request = request(0);
    let SemanticQueryKey::FlowReturn(query) = &mut request.query else {
        unreachable!()
    };
    query.function.function_part = fixture.program_key().part.clone();
    query.function.overload_ordinal = fixture.program_key().overload_ordinal;
    let plan = fixture.build_plan(request).unwrap();
    let inputs = fixture.product_inputs();
    let execution =
        FlowProductExecution::new(&inputs, &plan, FlowProductBudget::default()).unwrap();
    let captures = fixture.selected_capture_identities_for_tests(&plan);
    assert_eq!(captures.len(), 1);
    let key = execution
        .selected_sites()
        .find_map(|site| {
            let key = site.key(FlowDomain::ReachingType).unwrap();
            key.binding()
                .is_some_and(|identity| identity == &captures[0])
                .then_some(key)
        })
        .unwrap();
    assert!(matches!(
        inputs.graph().node_kind(key.node()),
        FlowNodeKind::CapturedBinding(_)
    ));
    assert_eq!(
        fixture.product_content_key_for_tests(&execution, key.node(), key.binding_ref()),
        Ok(key.clone())
    );
    let foreign = flow_graph_fixture_for_tests_nested(source, 1, "nested");
    assert_eq!(foreign.product_content_key_for_tests(&execution, key.node(), key.binding_ref()), Err(FlowProductKeyError::GraphMismatch), "identical content in an independently minted graph cannot relabel a raw site into this execution");
}

// Numeric addresses are fixture-local conveniences. Production consumers can
// only obtain scoped handles or enter through the pinned content interpreter.
trait FixtureAddresses {
    fn site(&self, node: FlowNodeId) -> Result<SelectedFlowSite, FlowProductKeyError>;
    fn key(
        &self,
        domain: FlowDomain,
        node: FlowNodeId,
    ) -> Result<FlowProductKey, FlowProductKeyError>;
}
impl FixtureAddresses for FlowProductExecution {
    fn site(&self, node: FlowNodeId) -> Result<SelectedFlowSite, FlowProductKeyError> {
        self.selected_sites()
            .find(|site| site.node() == node)
            .ok_or(FlowProductKeyError::UnselectedNode)
    }
    fn key(
        &self,
        domain: FlowDomain,
        node: FlowNodeId,
    ) -> Result<FlowProductKey, FlowProductKeyError> {
        self.key_at_site(domain, &self.site(node)?)
    }
}
struct TestExecution {
    execution: FlowProductExecution,
    plan: FlowDemandPlan,
}
impl std::ops::Deref for TestExecution {
    type Target = FlowProductExecution;
    fn deref(&self) -> &Self::Target {
        &self.execution
    }
}
impl std::ops::DerefMut for TestExecution {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.execution
    }
}
impl TestExecution {
    fn finish(&mut self) -> Result<FlowProductEvidence, FlowProductFailure> {
        self.execution.finish_with_plan(&self.plan)
    }
}
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
) -> (FlowProductInputs, TestExecution) {
    let fixture = flow_graph_fixture_for_tests(source, version);
    let plan = fixture.build_plan(request(0)).unwrap();
    let inputs = fixture.product_inputs();
    let execution = FlowProductExecution::new(&inputs, &plan, budget).unwrap();
    (inputs, TestExecution { execution, plan })
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
        &key.site(),
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
            &key.site(),
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
        &key.site(),
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
    assert_eq!(merge.get(&narrowing), None);
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
        fn literal_provenance(
            &self,
            _: &[LiteralProvenance<'_>],
            _: SemanticNodeId,
        ) -> Result<LiteralProvenanceResult, verter_session::semantic_query::FlowGap> {
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

#[test]
fn executing_a_transfer_does_not_credit_the_unrelated_execution_site() {
    let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
    let (_, number, _) = types();
    let x = binding(&execution, &inputs, "x", FlowDomain::ReachingType);
    let y = binding(&execution, &inputs, "y", FlowDomain::ReachingType);
    let mut state = execution.empty_state();
    execution
        .apply_transfers(
            &mut state,
            &y.site(),
            &[FlowProductTransfer {
                key: x.clone(),
                value: Some(reaching(number)),
            }],
        )
        .unwrap();
    let evidence = execution.finish().unwrap();
    assert!(evidence.executed(&x));
    assert!(!evidence.executed(&y));
}

#[test]
fn execution_caps_cannot_exceed_the_sealed_request_policy() {
    let fixture = flow_graph_fixture_for_tests(SOURCE, 1);
    let mut demand = request(0);
    demand.resources.max_execution_steps = 3;
    demand.resources.max_completion_frontier = 1;
    let plan = fixture.build_plan(demand).unwrap();
    let inputs = fixture.product_inputs();
    let projected = FlowProductBudget::for_demand_plan(&plan);
    assert_eq!(projected.max_execution_steps, 3);
    assert_eq!(projected.max_completion_frontier, 1);
    let mut work = FlowProductExecution::new(&inputs, &plan, FlowProductBudget::default()).unwrap();
    assert!(matches!(
        work.charge_execution_work(4),
        Err(FlowProductFailure::BudgetExceeded(
            FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::ExecutionWork,
                limit: 3,
                observed: 4
            }
        ))
    ));
    let mut frontier =
        FlowProductExecution::new(&inputs, &plan, FlowProductBudget::default()).unwrap();
    assert!(matches!(
        frontier.reserve_completion_frontier(2),
        Err(FlowProductFailure::BudgetExceeded(
            FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::CompletionFrontier,
                limit: 1,
                observed: 2
            }
        ))
    ));
}

#[test]
fn completion_frontiers_share_a_live_limit_and_release_owned_reservations() {
    let (_, mut execution) = execution(
        SOURCE,
        1,
        FlowProductBudget {
            max_completion_frontier: 2,
            ..FlowProductBudget::default()
        },
    );
    let outer = execution.reserve_completion_frontier(1).unwrap();
    let inner = execution.reserve_completion_frontier(1).unwrap();
    drop(outer);
    let sibling = execution.reserve_completion_frontier(1).unwrap();
    drop(inner);
    drop(sibling);
    let all = execution.reserve_completion_frontier(2).unwrap();
    assert!(matches!(
        execution.reserve_completion_frontier(1),
        Err(FlowProductFailure::BudgetExceeded(
            FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::CompletionFrontier,
                limit: 2,
                observed: 3
            }
        ))
    ));
    drop(all);
    assert!(
        execution.finish().is_err(),
        "releasing memory must not erase an exhausted execution"
    );
}

#[test]
fn execution_work_is_shared_across_continuations_and_independent_of_iterations() {
    let (_, mut execution) = execution(
        SOURCE,
        1,
        FlowProductBudget {
            max_execution_steps: 3,
            ..FlowProductBudget::default()
        },
    );
    execution.charge_execution_work(1).unwrap();
    let before = execution.empty_state();
    let _branch = before.clone();
    execution.charge_execution_work(2).unwrap();
    assert!(matches!(
        execution.charge_execution_work(1),
        Err(FlowProductFailure::BudgetExceeded(
            FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::ExecutionWork,
                limit: 3,
                observed: 4
            }
        ))
    ));
    assert!(execution.finish().is_err());
}

#[test]
fn checker_inference_transfers_do_not_certify_runtime_work() {
    for actual_work in [false, true] {
        let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
        let (_, number, _) = types();
        let key = binding(&execution, &inputs, "y", FlowDomain::ReachingType);
        let mut state = execution.empty_state();
        let value = FlowProductValue::ReachingType(ReachingTypeProduct::of(number));
        let outer = execution.begin_checker_inference().unwrap();
        let inner = execution.begin_checker_inference().unwrap();
        drop(outer);
        put(&mut execution, &mut state, &key, value.clone()).unwrap();
        drop(inner);
        assert_eq!(state.get(&key), Some(&value));
        if actual_work {
            assert!(!put(&mut execution, &mut state, &key, value).unwrap());
        }
        let evidence = execution.finish().unwrap();
        assert_eq!(
            evidence.executed(&key),
            actual_work,
            "only successful actual work, including unchanged work, certifies runtime evidence"
        );
    }
}

#[test]
fn declared_authority_survives_snapshot_restore_and_cannot_be_erased() {
    for erase in [
        None,
        Some(FlowProductValue::DeclaredType(
            DeclaredTypeProduct::default(),
        )),
    ] {
        let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
        let (_, number, string) = types();
        let key = binding(&execution, &inputs, "y", FlowDomain::DeclaredType);
        let mut before = execution.empty_state();
        let mut branch = before.clone();
        let authored = FlowProductValue::DeclaredType(DeclaredTypeProduct::of(number));
        put(&mut execution, &mut branch, &key, authored.clone()).unwrap();
        assert_eq!(
            before.get(&key),
            Some(&authored),
            "source authority outlives a branch snapshot"
        );
        assert!(execution
            .apply_transfers(
                &mut branch,
                &key.site(),
                &[FlowProductTransfer {
                    key: key.clone(),
                    value: erase
                }]
            )
            .is_err());
        assert_eq!(branch.get(&key), Some(&authored));
        assert!(put(
            &mut execution,
            &mut before,
            &key,
            FlowProductValue::DeclaredType(DeclaredTypeProduct::of(string))
        )
        .is_err());
        assert!(execution.finish().is_err());
    }
}

#[test]
fn declared_types_belong_to_source_declarations_even_when_runtime_bindings_alias() {
    let (inputs, mut execution) = execution(
        "function products(x: number) { var x: number = 1; var x; return x; }",
        1,
        FlowProductBudget::default(),
    );
    let (_, number, string) = types();
    let keys: Vec<_> = inputs
        .graph()
        .nodes()
        .filter_map(|node| execution.key(FlowDomain::DeclaredType, node).ok())
        .filter(|key| key.binding().is_some())
        .collect();
    assert_eq!(keys.len(), 3);
    let mut state = execution.empty_state();
    let before = state.clone();
    let first = FlowProductValue::DeclaredType(DeclaredTypeProduct::of(number));
    let second = FlowProductValue::DeclaredType(DeclaredTypeProduct::of(string));
    assert_eq!(before.declared_type(&keys[2]), None);
    put(&mut execution, &mut state, &keys[1], second.clone()).unwrap();
    assert_eq!(before.declared_type(&keys[0]), Some(string));
    assert_eq!(before.declared_type(&keys[2]), Some(string));
    put(&mut execution, &mut state, &keys[0], first.clone()).unwrap();
    assert_eq!(state.get(&keys[0]), Some(&first));
    assert_eq!(state.get(&keys[1]), Some(&second));
    assert_eq!(
        state.get(&keys[2]),
        None,
        "fallback does not fabricate authored facts"
    );
    assert_eq!(before.declared_type(&keys[0]), Some(number));
    assert_eq!(
        before.declared_type(&keys[1]),
        Some(string),
        "exact declaration wins"
    );
    assert_eq!(
        before.declared_type(&keys[2]),
        Some(number),
        "fallback follows source order, not publication order"
    );
}

#[test]
fn alias_normalization_precedes_narrowing_width_accounting() {
    let (inputs, mut execution) = execution(
        "function products(x) { var x = 1; return x; }",
        1,
        FlowProductBudget {
            max_product_width: 1,
            ..FlowProductBudget::default()
        },
    );
    let (_, number, _) = types();
    let keys: Vec<_> = inputs
        .graph()
        .nodes()
        .filter_map(|node| execution.key(FlowDomain::Narrowing, node).ok())
        .filter(|key| key.binding().is_some())
        .collect();
    let facts = keys.iter().map(|key| FlowNarrowingFact {
        binding: key.binding().unwrap().clone(),
        path: Arc::from([]),
        narrowed_to: number,
    });
    let mut state = execution.empty_state();
    put(
        &mut execution,
        &mut state,
        &keys[0],
        FlowProductValue::Narrowing(NarrowingProduct::new(facts)),
    )
    .unwrap();
    let Some(FlowProductValue::Narrowing(product)) = state.get(&keys[0]) else {
        panic!("narrowing")
    };
    assert_eq!(product.facts().len(), 1);
}

#[test]
fn all_fresh_identity_join_does_not_expand_membership_or_exceed_width() {
    let graph = SemanticGraphStore::new();
    let a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(1.0)));
    let b = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(2.0)));
    let algebra = GraphSemanticAlgebra(&graph);
    let union = algebra.union(&[a, b]).node;
    let value = FlowProductValue::ReachingType(
        ReachingTypeProduct::of(union).with_widening(Some(WideningMembership::All)),
    );
    let bottom = FlowProductValue::ReachingType(ReachingTypeProduct::default());
    let outcome = join_product(
        &algebra,
        &FlowProductBudget {
            max_product_width: 1,
            ..FlowProductBudget::default()
        },
        &value,
        &bottom,
    );
    assert_eq!(outcome, FlowTransferOutcome::Unchanged);
}

#[test]
fn a_foreign_selected_site_cannot_be_reattached_to_an_execution() {
    let (_, first) = execution(SOURCE, 1, FlowProductBudget::default());
    let site = first.selected_sites().next().unwrap();
    for version in [1, 2] {
        let (_, other) = execution(SOURCE, version, FlowProductBudget::default());
        assert_eq!(
            other.key_at_site(FlowDomain::ReachingValue, &site),
            Err(FlowProductKeyError::GraphMismatch)
        );
    }
}

#[test]
fn execution_evidence_requires_the_exact_attached_plan_and_cold_completion_seals() {
    let fixture = flow_graph_fixture_for_tests(SOURCE, 1);
    let mut request = request(0);
    request.resources.max_obligations = 0;
    let retained = fixture.retained_plan(&request).unwrap();
    let cap = fixture
        .prepare_execution_with_retained(&request, &retained)
        .unwrap();
    assert!(fixture
        .build_plan_from_execution(Arc::clone(&cap), Arc::from([]))
        .is_err());
    let inputs = fixture.product_inputs();
    let mut cold = FlowProductExecution::new_for_selection(
        &inputs,
        Arc::clone(&cap),
        FlowProductBudget::for_execution_selection(&cap),
    )
    .unwrap();
    let key = cold
        .selected_sites()
        .next()
        .unwrap()
        .key(FlowDomain::ReachingType)
        .unwrap();
    let (_, number, _) = types();
    let mut store = cold.empty_state();
    put(&mut cold, &mut store, &key, reaching(number)).unwrap();
    assert_eq!(cold.finish().unwrap().iterations(), 0);
    assert_eq!(
        put(&mut cold, &mut store, &key, reaching(number)),
        Err(FlowProductFailure::Sealed)
    );

    request.resources.max_obligations = 1024;
    let first = fixture.build_plan(request.clone()).unwrap();
    let independent = fixture.build_plan(request).unwrap();
    let mut execution =
        FlowProductExecution::new(&inputs, &first, FlowProductBudget::default()).unwrap();
    assert_eq!(
        execution.finish_with_plan(&independent).unwrap_err(),
        FlowProductFailure::ScopeMismatch
    );
    assert!(
        execution.finish_with_plan(&first).is_err(),
        "failed evidence attachment cannot be retried as success"
    );
}

#[test]
fn snapshots_share_runtime_storage_and_writes_preserve_other_continuations() {
    let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
    let (_, number, string) = types();
    let key = binding(&execution, &inputs, "x", FlowDomain::ReachingType);
    let mut state = execution.empty_state();
    put(&mut execution, &mut state, &key, reaching(number)).unwrap();
    let mut branch = state.clone();
    assert!(state.shares_continuation_storage(&branch));
    put(&mut execution, &mut branch, &key, reaching(string)).unwrap();
    assert_eq!(state.get(&key), Some(&reaching(number)));
    assert_eq!(branch.get(&key), Some(&reaching(string)));
    assert_eq!(state.ordered_entries().count(), 1);
}

#[test]
fn predecessor_joins_follow_domain_order_and_a_failure_permanently_seals_evidence() {
    struct Refusing(std::cell::Cell<usize>);
    impl FlowSemanticAlgebra for Refusing {
        fn union(&self, _: &[SemanticNodeId]) -> FlowAlgebraComposite {
            self.0.set(self.0.get() + 1);
            FlowAlgebraComposite {
                node: SemanticNodeId(0),
                incomplete: true,
            }
        }
        fn literal_provenance(
            &self,
            _: &[LiteralProvenance<'_>],
            _: SemanticNodeId,
        ) -> Result<LiteralProvenanceResult, verter_session::semantic_query::FlowGap> {
            unreachable!()
        }
    }
    let algebra = Refusing(std::cell::Cell::new(0));
    let (inputs, mut execution) = execution(
        SOURCE,
        1,
        FlowProductBudget {
            max_product_width: 1,
            ..FlowProductBudget::default()
        },
    );
    let (_, number, string) = types();
    let early = binding(&execution, &inputs, "x", FlowDomain::ReachingType);
    let late = binding(&execution, &inputs, "y", FlowDomain::ReachingValue);
    assert!(early.node().index() < late.node().index());
    let mut left = execution.empty_state();
    let mut right = execution.empty_state();
    put(&mut execution, &mut left, &early, reaching(number)).unwrap();
    put(&mut execution, &mut right, &early, reaching(string)).unwrap();
    put(
        &mut execution,
        &mut left,
        &late,
        FlowProductValue::ReachingValue(ReachingValueProduct::at(&early.site())),
    )
    .unwrap();
    put(
        &mut execution,
        &mut right,
        &late,
        FlowProductValue::ReachingValue(ReachingValueProduct::at(&late.site())),
    )
    .unwrap();
    assert!(matches!(
        execution.join_products(&[&left, &right], &algebra),
        Err(FlowProductFailure::BudgetExceeded(
            FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::Width,
                observed: 2,
                ..
            }
        ))
    ));
    assert_eq!(
        algebra.0.get(),
        0,
        "later domain must not run before reaching-value width failure"
    );
    assert_eq!(left.get(&early), Some(&reaching(number)));
    assert!(execution.finish().is_err());
}

#[test]
fn declarations_have_an_execution_lifetime_budget_and_publish_bundles_atomically() {
    let (inputs, mut execution) = execution(
        SOURCE,
        1,
        FlowProductBudget {
            max_products: 1,
            max_declared_products: 1,
            ..FlowProductBudget::default()
        },
    );
    let (_, number, _) = types();
    let x = binding(&execution, &inputs, "x", FlowDomain::DeclaredType);
    let y = binding(&execution, &inputs, "y", FlowDomain::DeclaredType);
    let mut old = execution.empty_state();
    let mut branch = old.clone();
    let reaching_y = y.for_domain(FlowDomain::ReachingType).unwrap();
    put(&mut execution, &mut old, &reaching_y, reaching(number)).unwrap();
    let declared = FlowProductValue::DeclaredType(DeclaredTypeProduct::of(number));
    assert!(matches!(
        execution.apply_transfers(
            &mut branch,
            &x.site(),
            &[
                FlowProductTransfer {
                    key: x.clone(),
                    value: Some(declared.clone())
                },
                FlowProductTransfer {
                    key: y.clone(),
                    value: Some(declared)
                },
            ]
        ),
        Err(FlowProductFailure::BudgetExceeded(
            FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::DeclaredProducts,
                limit: 1,
                observed: 2
            }
        ))
    ));
    assert_eq!(old.get(&x), None);
    assert_eq!(old.get(&y), None);
    assert_eq!(old.get(&reaching_y), Some(&reaching(number)));
    assert!(branch.is_empty());
    assert!(execution.finish().is_err());
}

#[test]
fn source_authority_cannot_be_replaced_through_an_old_healthy_snapshot() {
    let (inputs, mut execution) = execution(
        SOURCE,
        1,
        FlowProductBudget {
            max_products: 1,
            max_declared_products: 1,
            ..FlowProductBudget::default()
        },
    );
    let (_, number, string) = types();
    let declared = binding(&execution, &inputs, "x", FlowDomain::DeclaredType);
    let runtime = declared.for_domain(FlowDomain::ReachingType).unwrap();
    let mut old = execution.empty_state();
    let mut branch = old.clone();
    put(&mut execution, &mut old, &runtime, reaching(number)).unwrap();
    let authored = FlowProductValue::DeclaredType(DeclaredTypeProduct::of(number));
    put(&mut execution, &mut branch, &declared, authored.clone()).unwrap();
    assert_eq!(
        old.len(),
        2,
        "one runtime cell plus one separately bounded source fact"
    );
    assert_eq!(old.get(&declared), Some(&authored));
    assert!(put(
        &mut execution,
        &mut old,
        &declared,
        FlowProductValue::DeclaredType(DeclaredTypeProduct::of(string))
    )
    .is_err());
    assert_eq!(old.get(&declared), Some(&authored));
    assert!(execution.finish().is_err());
}

#[test]
fn successful_unchanged_transfer_retains_shared_storage_and_still_proves_work() {
    let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
    let (_, number, _) = types();
    let key = binding(&execution, &inputs, "x", FlowDomain::ReachingType);
    let mut state = execution.empty_state();
    put(&mut execution, &mut state, &key, reaching(number)).unwrap();
    let mut branch = state.clone();
    assert!(!put(&mut execution, &mut branch, &key, reaching(number)).unwrap());
    assert!(branch.shares_continuation_storage(&state));
    assert!(execution.finish().unwrap().executed(&key));
}

#[test]
fn actual_multiway_type_join_constructs_one_canonical_union_and_one_provenance_batch() {
    struct Counting<'a> {
        graph: GraphSemanticAlgebra<'a>,
        unions: std::cell::Cell<usize>,
        inputs: std::cell::RefCell<Vec<usize>>,
    }
    impl FlowSemanticAlgebra for Counting<'_> {
        fn union(&self, members: &[SemanticNodeId]) -> FlowAlgebraComposite {
            self.unions.set(self.unions.get() + 1);
            self.graph.union(members)
        }
        fn literal_provenance(
            &self,
            inputs: &[LiteralProvenance<'_>],
            result: SemanticNodeId,
        ) -> Result<LiteralProvenanceResult, verter_session::semantic_query::FlowGap> {
            self.inputs.borrow_mut().push(inputs.len());
            self.graph.literal_provenance(inputs, result)
        }
    }
    let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
    let graph = SemanticGraphStore::new();
    let values = [1.0, 2.0, 3.0]
        .map(|n| graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(n))));
    let algebra = Counting {
        graph: GraphSemanticAlgebra(&graph),
        unions: std::cell::Cell::new(0),
        inputs: std::cell::RefCell::new(Vec::new()),
    };
    let key = binding(&execution, &inputs, "x", FlowDomain::ReachingType);
    let mut states: [FlowProductStore; 3] = std::array::from_fn(|_| execution.empty_state());
    for (i, state) in states.iter_mut().enumerate() {
        put(
            &mut execution,
            state,
            &key,
            FlowProductValue::ReachingType(
                ReachingTypeProduct::of(values[i])
                    .with_widening((i != 2).then_some(WideningMembership::All)),
            ),
        )
        .unwrap();
    }
    let result = execution
        .join_products(&[&states[0], &states[1], &states[2]], &algebra)
        .unwrap();
    assert_eq!(algebra.unions.get(), 1);
    assert_eq!(algebra.inputs.borrow().as_slice(), &[3]);
    let Some(FlowProductValue::ReachingType(product)) = result.get(&key) else {
        panic!("reaching type")
    };
    assert_eq!(product.contributors(), &values);
    assert_eq!(
        product.widening(),
        Some(&WideningMembership::Partial(Arc::from(&values[..2])))
    );
}

#[test]
fn actual_multiway_join_cannot_reset_the_canonical_work_budget_for_each_predecessor() {
    let (inputs, mut execution) = execution(SOURCE, 1, FlowProductBudget::default());
    let graph = SemanticGraphStore::new();
    let values: Vec<_> = (0..64)
        .map(|n| graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(n as f64))))
        .collect();
    let algebra = GraphSemanticAlgebra(&graph);
    let union = algebra.union(&values).node;
    let key = binding(&execution, &inputs, "x", FlowDomain::ReachingType);
    let mut states: Vec<_> = (0..128).map(|_| execution.empty_state()).collect();
    for (i, state) in states.iter_mut().enumerate() {
        let widening = if i % 2 == 0 {
            WideningMembership::All
        } else {
            WideningMembership::Partial(Arc::from(&values[..32]))
        };
        put(
            &mut execution,
            state,
            &key,
            FlowProductValue::ReachingType(
                ReachingTypeProduct::of(union).with_widening(Some(widening)),
            ),
        )
        .unwrap();
    }
    let predecessors: Vec<_> = states.iter().collect();
    assert!(matches!(
        execution.join_products(&predecessors, &algebra),
        Err(FlowProductFailure::Gap(_))
    ));
    assert!(execution.finish().is_err());
}
