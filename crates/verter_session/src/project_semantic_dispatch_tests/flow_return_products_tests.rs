use std::sync::Arc;
use verter_session_query::flow::binding::FlowBindingRef;
use verter_session_query::flow::flow_graph::FlowNodeKind;
use verter_session_query::flow::hashing::compute_flow_slice_hash;
use verter_session_query::flow::peeker::ReturnPathPeeker;
use verter_session_query::flow::peeker::SliceDemand;
use verter_session_query::flow::policy::FlowReturnPolicy;
use verter_session_query::flow::policy::NullabilityPolicy;
use verter_type_engine::project_semantic_dispatch::flow_products::DefiniteAssignmentProduct;
use verter_type_engine::project_semantic_dispatch::flow_products::FlowProductBudget;
use verter_type_engine::project_semantic_dispatch::flow_products::FlowProductExecution;
use verter_type_engine::project_semantic_dispatch::flow_products::FlowProductFailure;
use verter_type_engine::project_semantic_dispatch::flow_products::FlowProductInputs;
use verter_type_engine::project_semantic_dispatch::flow_products::FlowProductValue;
use verter_type_engine::project_semantic_dispatch::flow_products::GraphSemanticAlgebra;
use verter_type_engine::project_semantic_dispatch::flow_products::NarrowingProduct;
use verter_type_engine::project_semantic_dispatch::flow_products::ReachingTypeProduct;
use verter_type_engine::project_semantic_dispatch::flow_return_products::*;
use verter_type_engine::project_semantic_dispatch::flow_solve::build_flow_demand_plan;
use verter_type_engine::project_semantic_dispatch::flow_solve::FlowDemandRequest;
use verter_type_engine::project_semantic_dispatch::flow_solve::FlowDomain;
use verter_type_engine::project_semantic_dispatch::flow_solve::FlowResourcePolicy;
use verter_type_engine::semantic_query::CanonicalTypeSubstitution;
use verter_type_engine::semantic_query::FlowFunctionSlotIdentity;
use verter_type_engine::semantic_query::FlowInputContext;
use verter_type_engine::semantic_query::FlowReturnContext;
use verter_type_engine::semantic_query::FlowReturnKey;
use verter_type_engine::semantic_query::PrimitiveKind;
use verter_type_engine::semantic_query::ResolvedDeclSlotIdentity;
use verter_type_engine::semantic_query::ReturnProjectionDemand;
use verter_type_engine::semantic_query::SemanticNodeData;
use verter_type_engine::semantic_query::SemanticQueryKey;

use verter_type_engine::project_semantic_dispatch::flow_return_products::WRITE_KEY_COMPARISONS;

fn fixture(
    source: &str,
    nested: bool,
) -> (
    FlowFrameProducts,
    verter_session_query::flow::bundle::BoundFlowGraph,
    verter_type_engine::project_semantic_dispatch::flow_solve::FlowDemandPlan,
) {
    fixture_with_resources(source, nested, FlowResourcePolicy::default())
}

fn fixture_with_resources(
    source: &str,
    nested: bool,
    resources: FlowResourcePolicy,
) -> (
    FlowFrameProducts,
    verter_session_query::flow::bundle::BoundFlowGraph,
    verter_type_engine::project_semantic_dispatch::flow_solve::FlowDemandPlan,
) {
    let (state, _) =
        crate::resolver_core::ShallowFileState::service_backed_with_provenance_for_test(
            "/ws/product-continuations.ts",
            source,
        );
    let memo = state.decl_bodies();
    let index = memo.function_program_index().value;
    let entry = index
        .matches_named("products")
        .find(|candidate| candidate.entry().lexical_parent().is_some() == nested)
        .unwrap()
        .entry();

    let bound = crate::host_source_demand::flow_bound_graph_for_tests(memo, entry);
    let bundle = bound.bundle();
    let request = FlowDemandRequest {
        ancestry: verter_type_engine::project_semantic_dispatch::flow_solve::FlowInputAncestry::default(),
        query: SemanticQueryKey::FlowReturn(Box::new(FlowReturnKey {
            function: FlowFunctionSlotIdentity {
                declaration_slot: ResolvedDeclSlotIdentity::value_slot(
                    Arc::clone(&bound.key().canonical_id),
                    entry.key().declaration.owner,
                    Arc::clone(&entry.key().declaration.name),
                    0,
                    [0; 16],
                    [0; 16],
                ),
                function_part: entry.key().part.clone(),
                overload_ordinal: entry.key().overload_ordinal,
            },
            normalized_type_args: Arc::from([]),
            context: FlowReturnContext {
                parse_env_hash: bound.key().parse_env_hash,
                resolve_env_hash: [0; 16],
                type_env_hash: [0; 16],
                lib_env_hash: [0; 16],
                project_identity: [0; 16],
                result_evaluation: verter_type_engine::semantic_query::CONTEXT_FREE_EVALUATION,
                type_substitution: CanonicalTypeSubstitution::empty(),
                policy: FlowReturnPolicy {
                    nullability: NullabilityPolicy::Strict,
                    no_implicit_any: true,
                    use_unknown_in_catch_variables: true,
                    no_implicit_this: true,
                },
            },
            demand: ReturnProjectionDemand::whole_return(),
            input: FlowInputContext::empty(),
            result_contract: verter_type_engine::project_semantic_dispatch::flow_solve::flow_return_result_contract_id(),
        })),
        input_basis: verter_identity::identity::InputBasisId::from_canonical(
            &verter_type_engine::project_semantic_dispatch::dispatch_txn::flow_obligation_state::FlowEvaluationProvenance::new(
                1, 1, 1, 0,
            ),
        ),
        resources,
        additional_requirements: Arc::from([]),
    };
    let selection = ReturnPathPeeker::new(bundle.graph())
        .plan(
            &SliceDemand::for_return_projection(bundle.skeleton(), &[]),
            &request.resources.slice_budget,
        )
        .unwrap();
    let retained = verter_type_engine::cache_runtime::flow_slice_node::PlannedFlowSlice::for_test(
        compute_flow_slice_hash(&selection, bundle.graph(), bundle.skeleton()),
        selection,
    );
    let plan = build_flow_demand_plan(request, &bound, &retained).unwrap();
    let execution = FlowProductExecution::new(
        &FlowProductInputs::for_bound_graph(&bound),
        &plan,
        FlowProductBudget::for_demand_plan(&plan),
    )
    .unwrap();
    let products = FlowFrameProducts::new(execution, &bound).unwrap();
    (products, bound, plan)
}

#[test]
fn continuation_rewind_and_join_preserve_only_actual_reaching_definitions() {
    let (mut products, bound, plan) = fixture(
        "function products(flag: boolean) { let x = 0; if (flag) { x = 1; x = 2; } else { x = 3; } return x; }",
        false,
    );
    let bundle = bound.bundle();
    let binding = bundle
        .graph()
        .nodes()
        .find_map(|node| {
            let FlowNodeKind::Binding(binding) = bundle.graph().node_kind(node) else {
                return None;
            };
            (bundle.bindings().identity(binding)?.name.as_ref() == "x").then_some(binding)
        })
        .unwrap();
    let subject = FlowBindingRef::Local(binding);
    let initializer = bundle
        .graph()
        .expr_site_node(bundle.skeleton().binding(binding).initializer.unwrap());
    let writes: Vec<_> = bundle
        .skeleton()
        .writes
        .iter()
        .map(|write| {
            bundle
                .graph()
                .expr_site_node(write.value.expect("assignment RHS"))
        })
        .collect();
    assert_eq!(writes.len(), 3);
    let graph = verter_type_engine::semantic_query_memo::SemanticGraphStore::new();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    products.bind_at(
        &subject,
        DefiniteAssignmentProduct::assigned(),
        ReachingTypeProduct::of(number),
        Some(initializer),
    );
    let entry = products.clone();
    let entry_writes = products.observe_writes();
    products.bind_at(
        &subject,
        DefiniteAssignmentProduct::assigned(),
        ReachingTypeProduct::of(number),
        Some(writes[0]),
    );
    products.bind_at(
        &subject,
        DefiniteAssignmentProduct::assigned(),
        ReachingTypeProduct::of(number),
        Some(writes[1]),
    );
    let consequent = products.clone();
    products.restore_reaching_from(&subject, &entry, None, false);
    let definitions = |products: &FlowFrameProducts| {
        let Some(FlowProductValue::ReachingValue(value)) =
            products.get(FlowDomain::ReachingValue, &subject)
        else {
            panic!("an established binding retains definition provenance");
        };
        value.definitions().to_vec()
    };
    assert_eq!(
        definitions(&products),
        vec![initializer],
        "arm rewind restores definition provenance with its type"
    );
    products.bind_at(
        &subject,
        DefiniteAssignmentProduct::assigned(),
        ReachingTypeProduct::of(number),
        Some(writes[2]),
    );
    let joined = FlowFrameProducts::join(
        &[&consequent, &products],
        &entry_writes,
        &GraphSemanticAlgebra(&graph),
    );
    let mut expected = vec![writes[1], writes[2]];
    expected.sort_unstable_by_key(|node| node.index());
    assert_eq!(definitions(&joined), expected, "the stale overwritten arm and entry definition are absent from the actual predecessor join");
    assert!(joined.finish(Some(&plan)).unwrap().is_some());
}
#[test]
fn unchanged_write_receipts_skip_unrelated_history_and_follow_actual_continuations() {
    for count in [32, 512] {
        let declarations = (0..count)
            .map(|i| format!("let x{i}=0;"))
            .collect::<String>();
        let members = (0..count)
            .map(|i| format!("x{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let source = format!("function products(){{{declarations}return {{{members}}};}}");
        let (mut products, bound, _) = fixture_with_resources(
            &source,
            false,
            FlowResourcePolicy {
                max_obligations: 30_000,
                ..Default::default()
            },
        );
        let graph = verter_type_engine::semantic_query_memo::SemanticGraphStore::new();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let bindings: Vec<_> = bound
            .bundle()
            .graph()
            .nodes()
            .filter_map(|node| match bound.bundle().graph().node_kind(node) {
                FlowNodeKind::Binding(binding) => Some(binding),
                _ => None,
            })
            .collect();
        assert_eq!(bindings.len(), count);
        for binding in &bindings {
            products.bind_at(
                &FlowBindingRef::Local(*binding),
                DefiniteAssignmentProduct::assigned(),
                ReachingTypeProduct::of(number),
                Some(
                    bound.bundle().graph().expr_site_node(
                        bound
                            .bundle()
                            .skeleton()
                            .binding(*binding)
                            .initializer
                            .unwrap(),
                    ),
                ),
            );
            assert!(
                products.execution.borrow().failure.is_none(),
                "input binding {binding:?}: {:?}",
                products.execution.borrow().failure
            );
        }
        let before = products.clone();
        let observation = products.observe_writes();
        assert!(
            before.writes.ptr_eq(&products.writes),
            "snapshots share receipt storage"
        );
        assert!(before
            .writes_by_sequence
            .ptr_eq(&products.writes_by_sequence));
        let binding = bindings[count / 2];
        let subject = FlowBindingRef::Local(binding);
        let definition = bound.bundle().graph().expr_site_node(
            bound
                .bundle()
                .skeleton()
                .binding(binding)
                .initializer
                .unwrap(),
        );
        products.bind_at(
            &subject,
            DefiniteAssignmentProduct::assigned(),
            ReachingTypeProduct::of(number),
            Some(definition),
        );
        assert!(
            products.execution.borrow().failure.is_none(),
            "repeated write: {:?}",
            products.execution.borrow().failure
        );
        let write_subject = products.write_subject(&subject).unwrap();
        assert!(
            products.writes.get(&write_subject).unwrap().sequence
                > before.writes.get(&write_subject).unwrap().sequence,
            "a successful unchanged write advances its receipt"
        );
        for domain in [
            FlowDomain::ReachingValue,
            FlowDomain::ReachingType,
            FlowDomain::DefiniteAssignment,
        ] {
            assert_eq!(
                products.get(domain, &subject),
                before.get(domain, &subject),
                "the repeated site succeeds without changing products"
            );
        }
        WRITE_KEY_COMPARISONS.with(|work| work.set(0));
        assert_eq!(
            products.writes_since(&observation),
            std::slice::from_ref(&subject)
        );
        let comparisons = WRITE_KEY_COMPARISONS.with(|work| work.get());
        assert!(
            comparisons <= 128,
            "one write after {count} prior bindings compared {comparisons} keys"
        );
        assert_eq!(products.writes.len(), count);
        assert_eq!(
            products.writes_by_sequence.len(),
            count,
            "replacement removes the earlier sequence entry"
        );
        let surviving =
            FlowFrameProducts::join(&[&before], &observation, &GraphSemanticAlgebra(&graph));
        assert!(
            surviving.writes_since(&observation).is_empty(),
            "a terminated arm contributes no receipt"
        );
        WRITE_KEY_COMPARISONS.with(|work| work.set(0));
        let continuing = FlowFrameProducts::join(
            &[&before, &products],
            &observation,
            &GraphSemanticAlgebra(&graph),
        );
        let merge_comparisons = WRITE_KEY_COMPARISONS.with(|work| work.get());
        assert!(merge_comparisons <= 256, "one changed receipt after {count} prior bindings compared {merge_comparisons} keys during merge");
        assert_eq!(
            continuing.writes_since(&observation),
            std::slice::from_ref(&subject),
            "an untouched predecessor cannot hide an executed write"
        );
        let after_other_arm = before.observe_writes();
        assert!(
            products.writes_since(&after_other_arm).is_empty(),
            "entry observations use the shared execution clock, including work on other snapshots"
        );
        // bounded-loop: fixed repeated-write fixture verifies receipt replacement.
        for _ in 0..16 {
            products.bind_at(
                &subject,
                DefiniteAssignmentProduct::assigned(),
                ReachingTypeProduct::of(number),
                Some(definition),
            );
        }
        assert_eq!(
            products.writes_since(&after_other_arm),
            std::slice::from_ref(&subject)
        );
        assert_eq!(products.writes.len(), count);
        assert_eq!(products.writes_by_sequence.len(), count);
        products.restore_reaching_from(&subject, &before, None, true);
        assert!(
            products.writes_since(&observation).is_empty(),
            "rewind restores matching control receipts"
        );
    }
}

#[test]
fn clause_write_observations_reject_foreign_executions() {
    let source = "function products(x:number){return x;}";
    let (products, _, _) = fixture(source, false);
    let (foreign, _, _) = fixture(source, false);
    let observation = foreign.observe_writes();
    assert!(products.writes_since(&observation).is_empty());
    assert_eq!(
        products.execution.borrow().failure,
        Some(FlowProductFailure::ScopeMismatch)
    );
    let (other, _, _) = fixture(source, false);
    let graph = verter_type_engine::semantic_query_memo::SemanticGraphStore::new();
    let joined = FlowFrameProducts::join(&[&other], &observation, &GraphSemanticAlgebra(&graph));
    assert_eq!(
        joined.execution.borrow().failure,
        Some(FlowProductFailure::ScopeMismatch)
    );
}

#[test]
fn clause_write_replay_moves_every_runtime_domain_and_kills_old_guards() {
    let (mut entering, bound, _) =
        fixture("function products(x:string|number){x=1;return x;}", false);
    let binding = bound
        .bundle()
        .graph()
        .nodes()
        .find_map(|node| match bound.bundle().graph().node_kind(node) {
            FlowNodeKind::Binding(binding)
                if bound.bundle().skeleton().binding(binding).kind
                    == verter_session_query::flow::skeleton::SkeletonBindingKind::Param =>
            {
                Some(binding)
            }
            _ => None,
        })
        .unwrap();
    let subject = FlowBindingRef::Local(binding);
    let graph = verter_type_engine::semantic_query_memo::SemanticGraphStore::new();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    entering.bind(
        &subject,
        DefiniteAssignmentProduct::default(),
        ReachingTypeProduct::of(string),
    );
    entering.set_declared_type(&subject, Some(string));
    entering.set_narrowing(
        &subject,
        NarrowingProduct::new([
            verter_type_engine::project_semantic_dispatch::flow_products::FlowNarrowingFact {
                binding: entering.identity(&subject).unwrap(),
                path: Arc::from([]),
                narrowed_to: string,
                fresh_literal: None,
            },
        ]),
    );
    let observation = entering.observe_writes();
    let mut end = entering.clone();
    let definition = bound
        .bundle()
        .graph()
        .expr_site_node(bound.bundle().skeleton().writes[0].value.unwrap());
    end.bind_at(
        &subject,
        DefiniteAssignmentProduct::assigned(),
        ReachingTypeProduct::of(number),
        Some(definition),
    );
    assert_eq!(
        end.writes_since(&observation),
        std::slice::from_ref(&subject)
    );
    entering.apply_executed_write_from(&subject, &end);
    for domain in [
        FlowDomain::ReachingValue,
        FlowDomain::ReachingType,
        FlowDomain::DefiniteAssignment,
    ] {
        assert_eq!(entering.get(domain, &subject), end.get(domain, &subject));
    }
    assert!(entering.narrowing(&subject).is_none());
    assert_eq!(
        entering.declared_type(&subject),
        Some(string),
        "source authority is independent of continuation writes"
    );
}

#[test]
fn declared_capture_input_records_its_hub_and_preserves_unassigned_state() {
    let (mut products, bound, plan) = fixture(
        "function products() { const read = () => x; let x: 'a' | 'b' = 'a'; return read; }",
        true,
    );
    let (hub, identity) = bound
        .bundle()
        .graph()
        .nodes()
        .find_map(|node| {
            let FlowNodeKind::CapturedBinding(binding) = bound.bundle().graph().node_kind(node)
            else {
                return None;
            };
            Some((
                node,
                bound.bundle().graph().captured_binding(binding).clone(),
            ))
        })
        .expect("real selected captured hub");
    let graph = verter_type_engine::semantic_query_memo::SemanticGraphStore::new();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let unassigned = DefiniteAssignmentProduct::default();
    let subject = FlowBindingRef::Captured(identity);
    products.import_capture_input(
        &subject,
        unassigned,
        Some(ReachingTypeProduct::of(string)),
        Some(string),
    );
    assert_eq!(
        products.assignment(&subject),
        unassigned,
        "importing declared input does not execute the later initializer"
    );
    let Some(FlowProductValue::ReachingValue(definitions)) =
        products.get(FlowDomain::ReachingValue, &subject)
    else {
        panic!("declared capture input needs exact definition provenance");
    };
    assert_eq!(
        definitions.definitions(),
        &[hub],
        "only the child's actual input hub defines the imported value"
    );
    let key = products.key(FlowDomain::ReachingValue, &subject).unwrap();
    let evidence = products.finish(Some(&plan)).unwrap().unwrap();
    assert!(evidence.executed(&key));
}
