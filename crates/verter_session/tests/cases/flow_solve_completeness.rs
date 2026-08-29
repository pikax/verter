//! Completeness-proof discipline of the private flow-solve layer: a flow
//! result is COMPLETE only when every planned obligation of the demand
//! discharged under the exact basis the demand was planned against, with
//! validated evidence and deterministic convergence. Undeclared
//! domain/fact-family requirements become typed gaps — never silently
//! dropped — and no partial, gapped, failed, stale, or non-converged replay
//! is a warm candidate.

use std::sync::Arc;

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};
use verter_semantic::analysis::flow::flow_graph::FlowEdgeClass;
use verter_session::for_tests::{
    finalize_flow_solve, flow_graph_fixture_for_tests, flow_operation_contract,
    flow_result_contract_id, flow_return_result_for_tests, CompleteFlowResult, DischargeEvidence,
    FlowConvergenceEvidence, FlowConvergencePolicy, FlowDemandPlan, FlowDemandRequest, FlowDomain,
    FlowFactFamily, FlowFailure, FlowFailureClass, FlowGraphFixtureForTests, FlowObligationId,
    FlowPartialReason, FlowRequirement, FlowRequirementKind, FlowSolveOutcome, ObligationRuntime,
    ObligationState, SemanticGraphStore,
};
use verter_session::semantic_query::{
    CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowInputContext, FlowReturnContext,
    FlowReturnKey, FlowReturnPolicy, FlowReturnResult, PrimitiveKind, ResolvedDeclSlotIdentity,
    ReturnProjectionDemand, SemanticNodeData, SemanticQueryKey, SemanticQueryKeyTag,
};

/// The fixture body: one parameter, one local, one object-literal return
/// with a call entry, so the demand plan exercises binding-slot, return-site,
/// edge, and call-site expansion.
const FIXTURE_SOURCE: &str = r#"
function solve_me(x) {
  const y = x;
  return { value: y, other: side_effect(y) };
}
"#;

/// Content identity for test-minted basis ids.
struct TestBasis(u64);

impl CanonicalEncode for TestBasis {
    const DOMAIN_TAG: &'static str = "verter.session.flow_solve_completeness.test_basis.v1";

    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_u64(1, self.0);
    }
}

fn test_input_basis(tag: u64) -> InputBasisId {
    InputBasisId::from_canonical(&TestBasis(tag))
}

fn foreign_result_contract(tag: u64) -> ResultContractId {
    ResultContractId::from_canonical(&TestBasis(tag))
}

fn flow_return_query(env_tag: u8) -> SemanticQueryKey {
    SemanticQueryKey::FlowReturn(Box::new(FlowReturnKey {
        function: FlowFunctionSlotIdentity {
            declaration_slot: ResolvedDeclSlotIdentity::type_slot_unscoped(
                Arc::from("/flow_solve_fixture.ts"),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from("solve_me"),
            ),
            function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        },
        normalized_type_args: Arc::from([]),
        context: FlowReturnContext {
            parse_env_hash: [env_tag; 16],
            resolve_env_hash: [env_tag; 16],
            type_env_hash: [env_tag; 16],
            lib_env_hash: [env_tag; 16],
            project_identity: [env_tag; 16],
            type_substitution: CanonicalTypeSubstitution::empty(),
            policy: FlowReturnPolicy {},
        },
        demand: ReturnProjectionDemand::whole_return(),
        input: FlowInputContext::empty(),
    }))
}

fn registered_result_contract() -> ResultContractId {
    flow_result_contract_id(
        flow_operation_contract(SemanticQueryKeyTag::FlowReturn)
            .expect("FlowReturn is a registered flow operation"),
    )
}

fn base_request(fixture: &FlowGraphFixtureForTests) -> FlowDemandRequest {
    fixture.demand_request(
        flow_return_query(0),
        test_input_basis(1),
        registered_result_contract(),
    )
}

fn planned() -> (
    verter_session::for_tests::FlowGraphFixtureForTests,
    FlowDemandPlan,
) {
    let fixture = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 7);
    let plan = fixture
        .build_plan(base_request(&fixture))
        .expect("the fixture demand plans within budget");
    (fixture, plan)
}

fn evidence_for(plan: &FlowDemandPlan) -> DischargeEvidence {
    DischargeEvidence {
        input_basis: plan.basis.input_basis.clone(),
        result_contract: plan.basis.result_contract.clone(),
        dependencies: Arc::from([]),
        suboperations: Arc::from([]),
    }
}

fn converged(plan: &FlowDemandPlan) -> FlowConvergenceEvidence {
    FlowConvergenceEvidence {
        policy: plan.convergence,
        iterations: 1,
        stable: true,
    }
}

fn clean_value() -> FlowReturnResult {
    let graph = SemanticGraphStore::new();
    let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    flow_return_result_for_tests(&graph, node)
}

fn discharge_all(
    runtime: &mut ObligationRuntime,
    plan: &FlowDemandPlan,
    order: &[FlowObligationId],
) {
    for id in order {
        runtime
            .start_flow_obligation(*id)
            .expect("a planned pending obligation starts");
        runtime
            .discharge_flow_obligation(*id, evidence_for(plan))
            .expect("a running obligation discharges under the installed basis");
    }
}

fn completed(runtime: &ObligationRuntime, plan: &FlowDemandPlan) -> FlowSolveOutcome {
    finalize_flow_solve(runtime, plan, clean_value(), &converged(plan))
}

#[test]
fn complete_result_requires_every_planned_obligation() {
    let (fixture, plan) = planned();

    // Positive control: install, start + discharge every planned obligation
    // in work order, finalize — the sole construction path of a complete,
    // warm-admissible result.
    let mut runtime = ObligationRuntime::default();
    runtime
        .install_flow_demand(&plan)
        .expect("the plan installs on a fresh runtime");
    discharge_all(&mut runtime, &plan, &plan.work_order);
    let outcome = completed(&runtime, &plan);
    assert!(
        matches!(outcome, FlowSolveOutcome::Complete(_)),
        "a fully discharged plan must complete: {outcome:?}"
    );
    assert!(outcome.warm_candidate().is_some());

    // A planned obligation the runtime never installed cannot complete: the
    // installed obligation set must equal the plan's exact spec set.
    let mut wider_request = base_request(&fixture);
    wider_request.additional_requirements = Arc::from(vec![FlowRequirement {
        operation: SemanticQueryKeyTag::FlowNarrowingAt,
        requirement: FlowRequirementKind::Domain(FlowDomain::Narrowing),
    }]);
    let wider_plan = fixture
        .build_plan(wider_request)
        .expect("the widened demand plans within budget");
    let outcome = completed(&runtime, &wider_plan);
    assert!(
        matches!(
            outcome,
            FlowSolveOutcome::Partial(ref partial)
                if partial.reason == FlowPartialReason::ObligationSetMismatch
        ),
        "a runtime missing one planned record must not complete: {outcome:?}"
    );

    // A planned obligation left Pending cannot complete either.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    let mut order = plan.work_order.to_vec();
    let held = order.pop().expect("the plan has obligations");
    discharge_all(&mut runtime, &plan, &order);
    let outcome = completed(&runtime, &plan);
    assert!(
        matches!(
            outcome,
            FlowSolveOutcome::Partial(ref partial)
                if partial.reason == FlowPartialReason::IncompleteObligations
        ),
        "a pending obligation must block completion: {outcome:?}"
    );
    assert!(matches!(
        runtime
            .flow_obligations()
            .iter()
            .find(|record| record.spec.id == held)
            .map(|record| &record.state),
        Some(ObligationState::Pending)
    ));
}

#[test]
fn unregistered_flow_requirement_becomes_a_gap() {
    let fixture = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 7);
    let mut request = base_request(&fixture);
    request.additional_requirements = Arc::from(vec![
        // No operation declares a coverage domain.
        FlowRequirement {
            operation: SemanticQueryKeyTag::FlowReturn,
            requirement: FlowRequirementKind::Domain(FlowDomain::Coverage),
        },
        // The relation suboperation consumes relation facts only.
        FlowRequirement {
            operation: SemanticQueryKeyTag::Relate,
            requirement: FlowRequirementKind::FactFamily(FlowFactFamily::GraphEdge(
                FlowEdgeClass::PathWrite,
            )),
        },
    ]);
    let plan = fixture.build_plan(request).expect("the demand plans");
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");

    let gaps: Vec<_> = runtime
        .flow_obligations()
        .iter()
        .filter(|record| matches!(record.state, ObligationState::Gap(_)))
        .collect();
    assert_eq!(gaps.len(), 2, "exactly the two undeclared requirements gap");
    for record in &gaps {
        assert!(
            matches!(
                record.state,
                ObligationState::Gap(verter_session::semantic_query::FlowGap::UnmodeledExpression)
            ),
            "the gap carrier is the typed unmodeled gap: {:?}",
            record.state
        );
    }
    // The record retains the offending requirement — the private typed
    // reason survives behind the shared public gap carrier.
    assert!(gaps.iter().any(|record| record.spec.requirement
        == FlowRequirement {
            operation: SemanticQueryKeyTag::FlowReturn,
            requirement: FlowRequirementKind::Domain(FlowDomain::Coverage),
        }));
    assert!(gaps.iter().any(|record| record.spec.requirement
        == FlowRequirement {
            operation: SemanticQueryKeyTag::Relate,
            requirement: FlowRequirementKind::FactFamily(FlowFactFamily::GraphEdge(
                FlowEdgeClass::PathWrite
            )),
        }));
}

#[test]
fn discharge_order_does_not_change_the_completed_result() {
    let (_fixture, plan) = planned();
    let canonical: Vec<FlowObligationId> = plan.work_order.to_vec();
    assert!(
        canonical.len() >= 4,
        "the fixture must expand to enough obligations to permute"
    );
    let reversed: Vec<FlowObligationId> = canonical.iter().rev().copied().collect();
    let rotated: Vec<FlowObligationId> = canonical[3..]
        .iter()
        .chain(canonical[..3].iter())
        .copied()
        .collect();
    // A fixed shuffle: even positions ascending, then odd positions descending.
    let shuffled: Vec<FlowObligationId> = canonical
        .iter()
        .step_by(2)
        .chain(canonical.iter().skip(1).step_by(2).rev())
        .copied()
        .collect();
    assert_ne!(canonical, reversed);
    assert_ne!(canonical, rotated);
    assert_ne!(canonical, shuffled);
    assert_ne!(reversed, rotated);

    let mut results: Vec<CompleteFlowResult> = Vec::new();
    for order in [&canonical, &reversed, &rotated, &shuffled] {
        let mut runtime = ObligationRuntime::default();
        runtime.install_flow_demand(&plan).expect("install");
        discharge_all(&mut runtime, &plan, order);
        match completed(&runtime, &plan) {
            FlowSolveOutcome::Complete(complete) => results.push(complete),
            other => panic!("every discharge order must complete: {other:?}"),
        }
    }
    for result in &results[1..] {
        assert_eq!(
            &results[0], result,
            "the completeness proof is a function of the plan, not the discharge order"
        );
    }
}

#[test]
fn mismatched_basis_or_nonconvergence_cannot_complete() {
    let (fixture, plan) = planned();
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    discharge_all(&mut runtime, &plan, &plan.work_order);

    // Every basis axis the finalize-time comparison covers: a plan rebuilt
    // from a request differing in any one of them must not complete against
    // this runtime.
    let stale_plans: Vec<(&str, FlowDemandPlan)> = {
        let other_fixture = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 8);
        let mut legs: Vec<(&str, FlowDemandRequest)> = Vec::new();

        let graph_hash = other_fixture.demand_request(
            flow_return_query(0),
            test_input_basis(1),
            registered_result_contract(),
        );
        legs.push(("graph hash", graph_hash));

        let mut demand = base_request(&fixture);
        let SemanticQueryKey::FlowReturn(key) = &mut demand.query else {
            unreachable!()
        };
        key.demand.point.projection.call_signatures = true;
        legs.push(("query demand", demand));

        let mut input = base_request(&fixture);
        let SemanticQueryKey::FlowReturn(key) = &mut input.query else {
            unreachable!()
        };
        key.input = FlowInputContext {
            contextual_parameters: Arc::from(vec![clean_value().return_type()]),
        };
        legs.push(("query input", input));

        let mut profile = base_request(&fixture);
        let SemanticQueryKey::FlowReturn(key) = &mut profile.query else {
            unreachable!()
        };
        key.context.type_env_hash = [9; 16];
        legs.push(("query profile", profile));

        let mut input_basis = base_request(&fixture);
        input_basis.input_basis = test_input_basis(2);
        legs.push(("input basis", input_basis));

        let mut result_contract = base_request(&fixture);
        result_contract.result_contract = foreign_result_contract(3);
        legs.push(("result contract", result_contract));

        legs.into_iter()
            .map(|(name, request)| (name, fixture.build_plan(request).expect("the demand plans")))
            .collect()
    };
    for (name, stale_plan) in &stale_plans {
        let outcome = completed(&runtime, stale_plan);
        assert!(
            matches!(outcome, FlowSolveOutcome::Partial(_)),
            "a stale {name} must not complete: {outcome:?}"
        );
        assert!(outcome.warm_candidate().is_none());
    }

    // A result contract foreign to the operation's registered contract must
    // not complete even when installed and finalized consistently.
    let mut foreign_request = base_request(&fixture);
    foreign_request.result_contract = foreign_result_contract(3);
    let foreign_plan = fixture
        .build_plan(foreign_request)
        .expect("the demand plans");
    let mut foreign_runtime = ObligationRuntime::default();
    foreign_runtime
        .install_flow_demand(&foreign_plan)
        .expect("install");
    discharge_all(
        &mut foreign_runtime,
        &foreign_plan,
        &foreign_plan.work_order,
    );
    let outcome = completed(&foreign_runtime, &foreign_plan);
    assert!(
        matches!(
            outcome,
            FlowSolveOutcome::Partial(ref partial)
                if partial.reason == FlowPartialReason::ResultContractMismatch
        ),
        "a foreign result contract must not complete: {outcome:?}"
    );

    // Convergence mismatches: unstable evidence, an over-budget iteration
    // count, and a foreign policy are each non-converged.
    for (name, evidence) in [
        (
            "unstable",
            FlowConvergenceEvidence {
                policy: plan.convergence,
                iterations: 1,
                stable: false,
            },
        ),
        (
            "over-budget",
            FlowConvergenceEvidence {
                policy: plan.convergence,
                iterations: plan.convergence.max_iterations + 1,
                stable: true,
            },
        ),
        (
            "foreign policy",
            FlowConvergenceEvidence {
                policy: FlowConvergencePolicy {
                    max_iterations: plan.convergence.max_iterations + 1,
                },
                iterations: 1,
                stable: true,
            },
        ),
    ] {
        let outcome = finalize_flow_solve(&runtime, &plan, clean_value(), &evidence);
        assert!(
            matches!(
                outcome,
                FlowSolveOutcome::Partial(ref partial)
                    if partial.reason == FlowPartialReason::NonConverged
            ),
            "{name} convergence evidence must not complete: {outcome:?}"
        );
    }
}

#[test]
fn partial_replay_has_no_warm_candidate() {
    let (fixture, plan) = planned();

    // Fully discharged control: warm.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    discharge_all(&mut runtime, &plan, &plan.work_order);
    assert!(completed(&runtime, &plan).warm_candidate().is_some());

    // A gapped obligation: never warm.
    let mut gapped_request = base_request(&fixture);
    gapped_request.additional_requirements = Arc::from(vec![FlowRequirement {
        operation: SemanticQueryKeyTag::FlowReturn,
        requirement: FlowRequirementKind::Domain(FlowDomain::Coverage),
    }]);
    let gapped_plan = fixture.build_plan(gapped_request).expect("plans");
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&gapped_plan).expect("install");
    let pending: Vec<FlowObligationId> = runtime
        .flow_obligations()
        .iter()
        .filter(|record| matches!(record.state, ObligationState::Pending))
        .map(|record| record.spec.id)
        .collect();
    discharge_all(&mut runtime, &gapped_plan, &pending);
    let outcome = completed(&runtime, &gapped_plan);
    assert!(matches!(outcome, FlowSolveOutcome::Partial(_)));
    assert!(outcome.warm_candidate().is_none());

    // A failed obligation — internal failure and cancellation alike: never
    // warm.
    for class in [FlowFailureClass::Internal, FlowFailureClass::Cancelled] {
        let mut runtime = ObligationRuntime::default();
        runtime.install_flow_demand(&plan).expect("install");
        let mut order = plan.work_order.to_vec();
        let failed = order.pop().expect("the plan has obligations");
        discharge_all(&mut runtime, &plan, &order);
        runtime.start_flow_obligation(failed).expect("start");
        runtime
            .fail_flow_obligation(failed, FlowFailure { class })
            .expect("a running obligation fails");
        let outcome = completed(&runtime, &plan);
        assert!(matches!(outcome, FlowSolveOutcome::Partial(_)));
        assert!(outcome.warm_candidate().is_none());
    }

    // A stale basis: never warm.
    let mut stale_request = base_request(&fixture);
    stale_request.input_basis = test_input_basis(2);
    let stale_plan = fixture.build_plan(stale_request).expect("plans");
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    discharge_all(&mut runtime, &plan, &plan.work_order);
    let outcome = completed(&runtime, &stale_plan);
    assert!(matches!(outcome, FlowSolveOutcome::Partial(_)));
    assert!(outcome.warm_candidate().is_none());

    // A partial replay (some obligations never discharged): never warm.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    discharge_all(
        &mut runtime,
        &plan,
        &plan.work_order[..plan.work_order.len() / 2],
    );
    let outcome = completed(&runtime, &plan);
    assert!(matches!(outcome, FlowSolveOutcome::Partial(_)));
    assert!(outcome.warm_candidate().is_none());
}

#[test]
fn unused_flow_runtime_reserves_no_obligation_storage() {
    let runtime = ObligationRuntime::default();
    assert!(runtime.flow_basis().is_none());
    assert!(runtime.flow_obligations().is_empty());
    assert_eq!(
        runtime.flow_obligation_storage_capacity(),
        0,
        "a runtime that never served a flow demand reserves no obligation storage"
    );
}
