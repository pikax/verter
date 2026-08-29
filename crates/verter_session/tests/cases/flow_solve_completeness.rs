//! Completeness-proof discipline of the private flow-solve layer: a flow
//! result is COMPLETE only when every planned obligation of the demand
//! discharged under the exact basis the demand was planned against, with
//! per-spec validated evidence and runtime-observed deterministic
//! convergence, sealed into ONE completion artifact by the obligation
//! runtime itself. Undeclared domain/fact-family requirements become typed
//! gaps — never silently dropped — and no partial, gapped, failed, stale,
//! or non-converged replay is a warm candidate.
//!
//! The finalizer accepts ONLY the runtime-sealed artifact: no
//! caller-supplied value, no caller-authored convergence evidence, and no
//! caller-assembled discharge evidence can reach it.

use std::sync::Arc;

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};
use verter_semantic::analysis::flow::flow_graph::FlowEdgeClass;
use verter_session::for_tests::{
    degraded_flow_return_result_for_tests, finalize_flow_solve, flow_graph_fixture_for_tests,
    flow_operation_contract, flow_result_contract_id, flow_return_result_for_tests,
    CompleteFlowResult, FlowDemandPlan, FlowDemandPlanError, FlowDemandRequest, FlowDomain,
    FlowFactFamily, FlowFailure, FlowFailureClass, FlowFinalizerKind, FlowGraphFixtureForTests,
    FlowObligationBasis, FlowObligationId, FlowObligationSpec, FlowOperationContract,
    FlowOperationRole, FlowOperationStatus, FlowPartialReason, FlowRequirement,
    FlowRequirementKind, FlowResourcePolicy, FlowResultContractDescriptor, FlowSealError,
    FlowSolveOutcome, FlowSuboperationEvidence, FlowTransitionError, ObligationRuntime,
    ObligationState, SealedFlowCompletion, SemanticGraphStore,
};
use verter_session::semantic_query::demand::{ProjectionPath, SurfaceFacet, SurfaceFacetSet};
use verter_session::semantic_query::{
    CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowGap, FlowInputContext,
    FlowReturnContext, FlowReturnKey, FlowReturnPolicy, FlowReturnResult, PathSegment,
    PrimitiveKind, PropertyKey, ResolvedDeclSlotIdentity, ReturnProjectionDemand, SemanticNodeData,
    SemanticQueryKey, SemanticQueryKeyTag,
};

/// The fixture body: one parameter, one local, one object-literal return
/// with a call entry, so the demand plan exercises binding-slot, return-site,
/// edge, and call-site expansion — including TWO binding obligations of the
/// same family with distinct provenance.
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

/// A demand request carries NO graph axis and NO subject axis: the bound
/// graph pins the body identity and the query payload carries the demand.
fn base_request() -> FlowDemandRequest {
    FlowDemandRequest {
        query: flow_return_query(0),
        input_basis: test_input_basis(1),
        result_contract: registered_result_contract(),
        resources: FlowResourcePolicy::default(),
        additional_requirements: Arc::from([]),
    }
}

fn planned() -> (FlowGraphFixtureForTests, FlowDemandPlan) {
    let fixture = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 7);
    let plan = fixture
        .build_plan(base_request())
        .expect("the fixture demand plans within budget");
    (fixture, plan)
}

fn spec(plan: &FlowDemandPlan, id: FlowObligationId) -> &FlowObligationSpec {
    plan.obligation_specs()
        .iter()
        .find(|spec| spec.id == id)
        .expect("every planned id has a spec")
}

/// The suboperation evidence a faithful solve presents for `spec`: exactly
/// the declared suboperations under the installed result contract.
fn expected_suboperations(
    plan: &FlowDemandPlan,
    spec: &FlowObligationSpec,
) -> Arc<[FlowSuboperationEvidence]> {
    spec.expected_suboperations
        .iter()
        .map(|operation| FlowSuboperationEvidence {
            operation: *operation,
            result_contract: plan.basis.result_contract.clone(),
        })
        .collect()
}

fn discharge_one(runtime: &mut ObligationRuntime, plan: &FlowDemandPlan, id: FlowObligationId) {
    let obligation = spec(plan, id);
    runtime
        .start_flow_obligation(id)
        .expect("a planned pending obligation starts");
    runtime
        .discharge_flow_obligation(
            id,
            obligation.expected_dependencies.clone(),
            expected_suboperations(plan, obligation),
        )
        .expect("a running obligation discharges with its spec-declared evidence");
}

fn discharge_all(
    runtime: &mut ObligationRuntime,
    plan: &FlowDemandPlan,
    order: &[FlowObligationId],
) {
    for id in order {
        discharge_one(runtime, plan, *id);
    }
}

/// The runtime observes the fixed point: one changing iteration, then one
/// stable one. Convergence is runtime-OBSERVED state, never caller evidence.
fn observe_convergence(runtime: &mut ObligationRuntime) {
    runtime
        .observe_flow_iteration(true)
        .expect("a changing fixed-point iteration is observed");
    runtime
        .observe_flow_iteration(false)
        .expect("a stable fixed-point iteration closes convergence");
}

/// A clean value payload minted over a fresh graph store (deliberately not
/// derived from any real solve — the pipeline must not care, only the
/// sealed path may carry it).
fn solve_value() -> FlowReturnResult {
    let graph = SemanticGraphStore::new();
    let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    flow_return_result_for_tests(&graph, node)
}

/// The sole completion path: install, discharge every planned obligation in
/// work order, let the runtime observe convergence, seal, finalize.
fn drive_to_completion(plan: &FlowDemandPlan) -> (ObligationRuntime, SealedFlowCompletion) {
    let mut runtime = ObligationRuntime::default();
    runtime
        .install_flow_demand(plan)
        .expect("the plan installs on a fresh runtime");
    discharge_all(&mut runtime, plan, &plan.work_order);
    observe_convergence(&mut runtime);
    let sealed = runtime
        .seal_flow_completion(solve_value())
        .expect("a fully discharged, runtime-converged solve seals");
    (runtime, sealed)
}

#[test]
fn complete_result_requires_every_planned_obligation() {
    let (fixture, plan) = planned();

    // Positive control: the sealed path is the sole construction of a
    // complete, warm-admissible result.
    let (runtime, sealed) = drive_to_completion(&plan);
    let sealed_value = sealed.value().clone();
    let outcome = finalize_flow_solve(&runtime, &plan, &sealed);
    let FlowSolveOutcome::Complete(complete) = &outcome else {
        panic!("a fully discharged plan must complete: {outcome:?}")
    };
    assert!(outcome.warm_candidate().is_some());
    // The completed value IS the value the runtime sealed — no
    // substitution is possible at finalization.
    assert_eq!(complete.value(), &sealed_value);

    // A planned obligation the runtime never installed cannot complete: the
    // sealed proofs must equal the plan's exact spec set.
    let mut wider_request = base_request();
    wider_request.additional_requirements = Arc::from(vec![FlowRequirement {
        operation: SemanticQueryKeyTag::FlowNarrowingAt,
        requirement: FlowRequirementKind::Domain(FlowDomain::Narrowing),
    }]);
    let wider_plan = fixture
        .build_plan(wider_request)
        .expect("the widened demand plans within budget");
    let outcome = finalize_flow_solve(&runtime, &wider_plan, &sealed);
    assert!(
        matches!(
            outcome,
            FlowSolveOutcome::Partial(ref partial)
                if partial.reason == FlowPartialReason::ObligationSetMismatch
        ),
        "a runtime missing one planned record must not complete: {outcome:?}"
    );

    // A planned obligation left Pending can never seal, so it can never
    // complete.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    let mut order = plan.work_order.to_vec();
    let held = order.pop().expect("the plan has obligations");
    discharge_all(&mut runtime, &plan, &order);
    observe_convergence(&mut runtime);
    assert!(
        matches!(
            runtime.seal_flow_completion(solve_value()),
            Err(FlowSealError::UndischargedObligations)
        ),
        "a pending obligation must block sealing"
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
    let mut request = base_request();
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
                ObligationState::Gap(FlowGap::UnmodeledExpression)
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
        observe_convergence(&mut runtime);
        let sealed = runtime
            .seal_flow_completion(solve_value())
            .expect("every discharge order seals");
        match finalize_flow_solve(&runtime, &plan, &sealed) {
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
fn plan_binds_the_store_minted_graph_identity() {
    // The SAME request planned over two store-minted bound graphs yields
    // two distinct bases: the plan's body identity is the bound graph's
    // key, and the request carries no graph axis that could disagree.
    let fixture_a = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 7);
    let fixture_b = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 8);
    let plan_a = fixture_a.build_plan(base_request()).expect("plans");
    let plan_b = fixture_b.build_plan(base_request()).expect("plans");
    assert_ne!(
        plan_a.basis, plan_b.basis,
        "distinct bound graphs must yield distinct bases"
    );
    // Replanning over the same bound graph is deterministic.
    let plan_a2 = fixture_a.build_plan(base_request()).expect("plans");
    assert_eq!(plan_a.basis, plan_a2.basis);
    assert_eq!(plan_a.obligation_specs(), plan_a2.obligation_specs());

    // A solve sealed under one bound graph can never complete against a
    // plan minted over the other, even with identical source.
    let (runtime, sealed) = drive_to_completion(&plan_a);
    let outcome = finalize_flow_solve(&runtime, &plan_b, &sealed);
    assert!(
        matches!(
            outcome,
            FlowSolveOutcome::Partial(ref partial)
                if partial.reason == FlowPartialReason::StaleBasis
        ),
        "a foreign bound graph's plan must not complete: {outcome:?}"
    );
    assert!(outcome.warm_candidate().is_none());
}

#[test]
fn query_demand_drives_the_subject_exhaustively() {
    let fixture = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 7);

    // The canonical whole-return point derives the empty projection path.
    let plan = fixture.build_plan(base_request()).expect("plans");
    assert!(plan.subject.projection_path.is_empty());

    // A named member-path demand derives its subject from the QUERY
    // payload — there is no caller-supplied subject to disagree with it.
    let mut named = base_request();
    let SemanticQueryKey::FlowReturn(key) = &mut named.query else {
        unreachable!()
    };
    key.demand.point.projection.path = ProjectionPath::from_segments([PathSegment::Member(
        PropertyKey::String(Arc::from("value")),
    )]);
    let plan = fixture
        .build_plan(named)
        .expect("a named member path plans");
    assert_eq!(
        plan.subject
            .projection_path
            .iter()
            .map(|segment| segment.as_ref())
            .collect::<Vec<_>>(),
        vec!["value"],
        "the derived subject is the query's demand path"
    );

    // Every other demand shape is a TYPED planning error, never a silent
    // default subject: a widened signature axis, a display/member facet
    // axis, and a non-authored-key path segment are each unrepresentable.
    let mut signatures = base_request();
    let SemanticQueryKey::FlowReturn(key) = &mut signatures.query else {
        unreachable!()
    };
    key.demand.point.projection.call_signatures = true;
    assert!(matches!(
        fixture.build_plan(signatures),
        Err(FlowDemandPlanError::UnrepresentableDemand)
    ));

    let mut faceted = base_request();
    let SemanticQueryKey::FlowReturn(key) = &mut faceted.query else {
        unreachable!()
    };
    key.demand.point.projection.facets = SurfaceFacetSet::single(SurfaceFacet::Members);
    assert!(matches!(
        fixture.build_plan(faceted),
        Err(FlowDemandPlanError::UnrepresentableDemand)
    ));

    let mut indexed = base_request();
    let SemanticQueryKey::FlowReturn(key) = &mut indexed.query else {
        unreachable!()
    };
    key.demand.point.projection.path =
        ProjectionPath::from_segments([PathSegment::Member(PropertyKey::Number(
            verter_type_expr::CanonicalIndexInt::from_canonical_i64(0)
                .expect("zero is a canonical index"),
        ))]);
    assert!(
        matches!(
            fixture.build_plan(indexed),
            Err(FlowDemandPlanError::UnrepresentableDemand)
        ),
        "a numeric member segment has no authored key text"
    );
}

#[test]
fn same_kind_obligations_keep_distinct_provenance() {
    let (_fixture, plan) = planned();
    let bindings: Vec<&FlowObligationSpec> = plan
        .obligation_specs()
        .iter()
        .filter(|spec| matches!(spec.basis, FlowObligationBasis::Binding { .. }))
        .collect();
    assert!(
        bindings.len() >= 2,
        "the fixture must plan at least two binding obligations: {}",
        bindings.len()
    );
    // Same family, same origin, same declared evidence — yet distinct
    // semantic subjects. The identity, not the plan-local ordinal, is
    // what separates them.
    assert_eq!(bindings[0].requirement, bindings[1].requirement);
    assert_eq!(bindings[0].origin, bindings[1].origin);
    assert_ne!(
        bindings[0].basis, bindings[1].basis,
        "two binding obligations of one family carry distinct provenance"
    );
    let (
        FlowObligationBasis::Binding { slot: first, .. },
        FlowObligationBasis::Binding { slot: second, .. },
    ) = (&bindings[0].basis, &bindings[1].basis)
    else {
        unreachable!()
    };
    assert_ne!(first.binding, second.binding);
    assert_ne!(first.identity, second.identity);

    // Each requires its OWN discharge: discharging the first leaves the
    // second pending, and a second discharge of the same obligation is an
    // illegal transition.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    discharge_one(&mut runtime, &plan, bindings[0].id);
    assert_eq!(
        runtime.discharge_flow_obligation(
            bindings[0].id,
            bindings[0].expected_dependencies.clone(),
            expected_suboperations(&plan, bindings[0]),
        ),
        Err(FlowTransitionError::IllegalTransition),
        "an already-discharged obligation cannot discharge again"
    );
    observe_convergence(&mut runtime);
    assert!(
        matches!(
            runtime.seal_flow_completion(solve_value()),
            Err(FlowSealError::UndischargedObligations)
        ),
        "the second binding obligation's own evidence was never presented"
    );
}

#[test]
fn evidence_must_match_the_specific_spec() {
    let (_fixture, plan) = planned();
    let call_spec = plan
        .obligation_specs()
        .iter()
        .find(|spec| !spec.expected_suboperations.is_empty())
        .expect("the fixture plans a call-site obligation with required suboperations")
        .id;
    let edge_spec = plan
        .obligation_specs()
        .iter()
        .find(|spec| !spec.expected_dependencies.is_empty())
        .expect("the fixture plans an edge obligation with required dependencies")
        .id;

    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");

    // Empty evidence cannot discharge a spec that declares required
    // suboperations — the check is against THIS spec, not a global set.
    runtime.start_flow_obligation(call_spec).expect("start");
    assert_eq!(
        runtime.discharge_flow_obligation(call_spec, Arc::from([]), Arc::from([])),
        Err(FlowTransitionError::NonSuboperationEvidence),
        "empty evidence must not discharge a spec declaring a suboperation"
    );
    // A wrong suboperation tag is refused, as is a foreign result contract.
    let valid = expected_suboperations(&plan, spec(&plan, call_spec));
    let mut wrong_tag = valid.to_vec();
    wrong_tag[0].operation = SemanticQueryKeyTag::Relate;
    assert_eq!(
        runtime.discharge_flow_obligation(call_spec, Arc::from([]), Arc::from(wrong_tag)),
        Err(FlowTransitionError::NonSuboperationEvidence)
    );
    let mut foreign = valid.to_vec();
    foreign[0].result_contract = foreign_result_contract(4);
    assert_eq!(
        runtime.discharge_flow_obligation(call_spec, Arc::from([]), Arc::from(foreign)),
        Err(FlowTransitionError::NonSuboperationEvidence)
    );

    // The same holds for declared dependencies: empty or foreign.
    runtime.start_flow_obligation(edge_spec).expect("start");
    assert_eq!(
        runtime.discharge_flow_obligation(edge_spec, Arc::from([]), Arc::from([])),
        Err(FlowTransitionError::UnplannedDependency),
        "empty evidence must not discharge a spec declaring a dependency"
    );
    assert_eq!(
        runtime.discharge_flow_obligation(
            edge_spec,
            Arc::from(vec![FlowObligationId(u32::MAX)]),
            Arc::from([])
        ),
        Err(FlowTransitionError::UnplannedDependency),
        "a foreign dependency id must not discharge the spec"
    );

    // A refused discharge leaves the obligation Running; the spec-exact
    // evidence still lands.
    assert!(matches!(
        runtime
            .flow_obligations()
            .iter()
            .find(|record| record.spec.id == call_spec)
            .map(|record| &record.state),
        Some(ObligationState::Running)
    ));
    let obligation = spec(&plan, call_spec);
    runtime
        .discharge_flow_obligation(
            call_spec,
            obligation.expected_dependencies.clone(),
            expected_suboperations(&plan, obligation),
        )
        .expect("the still-running call-site obligation discharges");
    let obligation = spec(&plan, edge_spec);
    runtime
        .discharge_flow_obligation(
            edge_spec,
            obligation.expected_dependencies.clone(),
            expected_suboperations(&plan, obligation),
        )
        .expect("the still-running edge obligation discharges");
}

#[test]
fn convergence_must_be_runtime_observed() {
    let (_fixture, plan) = planned();

    // Fully discharged, but the runtime never observed a fixed point.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    discharge_all(&mut runtime, &plan, &plan.work_order);
    assert!(
        matches!(
            runtime.seal_flow_completion(solve_value()),
            Err(FlowSealError::NonConverged)
        ),
        "no convergence observation cannot seal"
    );

    // A changing iteration alone is not convergence.
    runtime
        .observe_flow_iteration(true)
        .expect("a changing iteration is observed");
    assert!(
        matches!(
            runtime.seal_flow_completion(solve_value()),
            Err(FlowSealError::NonConverged)
        ),
        "a still-changing fixed point cannot seal"
    );

    // The stable iteration closes convergence; observing past it is an
    // illegal transition (the solve kept running past its fixed point).
    runtime
        .observe_flow_iteration(false)
        .expect("the stable iteration closes convergence");
    assert_eq!(
        runtime.observe_flow_iteration(true),
        Err(FlowTransitionError::IllegalTransition),
        "no iteration exists past the observed fixed point"
    );
    assert!(runtime.seal_flow_completion(solve_value()).is_ok());

    // The iteration budget is enforced at observation time: the
    // (max + 1)-th changing iteration is refused, and a solve that ran
    // into it can never stabilize, so it can never seal.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    discharge_all(&mut runtime, &plan, &plan.work_order);
    for _ in 0..plan.convergence.max_iterations {
        runtime.observe_flow_iteration(true).expect("within budget");
    }
    assert_eq!(
        runtime.observe_flow_iteration(true),
        Err(FlowTransitionError::ConvergenceBudget),
        "the first over-budget iteration is refused"
    );
    assert!(matches!(
        runtime.seal_flow_completion(solve_value()),
        Err(FlowSealError::NonConverged)
    ));

    // Observing with no demand installed is a typed error, not a default.
    let mut idle = ObligationRuntime::default();
    assert_eq!(
        idle.observe_flow_iteration(false),
        Err(FlowTransitionError::NoDemandInstalled)
    );
}

#[test]
fn foreign_value_and_partial_solve_cannot_seal() {
    let (_fixture, plan) = planned();

    // A value minted over a foreign graph store with NO solve work behind
    // it: nothing discharged, nothing observed — the runtime refuses to
    // seal it.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    assert!(
        matches!(
            runtime.seal_flow_completion(solve_value()),
            Err(FlowSealError::UndischargedObligations)
        ),
        "a foreign value with no discharge evidence cannot seal"
    );

    // A degraded value can never seal, even over a fully discharged,
    // converged solve.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    discharge_all(&mut runtime, &plan, &plan.work_order);
    observe_convergence(&mut runtime);
    let degraded = {
        let graph = SemanticGraphStore::new();
        let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        degraded_flow_return_result_for_tests(&graph, node)
    };
    assert!(matches!(
        runtime.seal_flow_completion(degraded),
        Err(FlowSealError::DegradedValue)
    ));

    // A runtime that never served a demand has nothing to seal.
    let idle = ObligationRuntime::default();
    assert!(matches!(
        idle.seal_flow_completion(solve_value()),
        Err(FlowSealError::NoDemandInstalled)
    ));
}

#[test]
fn partial_replay_never_seals() {
    let (fixture, plan) = planned();

    // Fully discharged control: seals, completes, warms.
    let (runtime, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, &plan, &sealed);
    assert!(outcome.warm_candidate().is_some());

    // A gapped obligation: never sealed, never warm.
    let mut gapped_request = base_request();
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
    observe_convergence(&mut runtime);
    assert!(matches!(
        runtime.seal_flow_completion(solve_value()),
        Err(FlowSealError::UndischargedObligations)
    ));

    // A failed obligation — internal failure and cancellation alike:
    // never sealed, never warm.
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
        observe_convergence(&mut runtime);
        assert!(matches!(
            runtime.seal_flow_completion(solve_value()),
            Err(FlowSealError::UndischargedObligations)
        ));
    }

    // A partial replay (some obligations never discharged): never sealed.
    let mut runtime = ObligationRuntime::default();
    runtime.install_flow_demand(&plan).expect("install");
    discharge_all(
        &mut runtime,
        &plan,
        &plan.work_order[..plan.work_order.len() / 2],
    );
    observe_convergence(&mut runtime);
    assert!(matches!(
        runtime.seal_flow_completion(solve_value()),
        Err(FlowSealError::UndischargedObligations)
    ));

    // A stale basis still reaches the finalizer as a typed partial, and a
    // partial is never a warm candidate.
    let mut stale_request = base_request();
    stale_request.input_basis = test_input_basis(2);
    let stale_plan = fixture.build_plan(stale_request).expect("plans");
    let (runtime, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, &stale_plan, &sealed);
    assert!(matches!(outcome, FlowSolveOutcome::Partial(_)));
    assert!(outcome.warm_candidate().is_none());
}

#[test]
fn stale_basis_or_foreign_contract_cannot_complete() {
    let (fixture, plan) = planned();
    let (runtime, sealed) = drive_to_completion(&plan);

    // Every basis axis the finalize-time comparison covers: a plan rebuilt
    // from a request differing in any one of them must not complete against
    // this runtime's sealed artifact.
    let stale_plans: Vec<(&str, FlowDemandPlan)> = {
        let mut legs: Vec<(&str, FlowDemandRequest)> = Vec::new();

        let mut input = base_request();
        let SemanticQueryKey::FlowReturn(key) = &mut input.query else {
            unreachable!()
        };
        key.input = FlowInputContext {
            contextual_parameters: Arc::from(vec![solve_value().return_type()]),
        };
        legs.push(("query input", input));

        let mut profile = base_request();
        let SemanticQueryKey::FlowReturn(key) = &mut profile.query else {
            unreachable!()
        };
        key.context.type_env_hash = [9; 16];
        legs.push(("query profile", profile));

        let mut input_basis = base_request();
        input_basis.input_basis = test_input_basis(2);
        legs.push(("input basis", input_basis));

        let mut result_contract = base_request();
        result_contract.result_contract = foreign_result_contract(3);
        legs.push(("result contract", result_contract));

        legs.into_iter()
            .map(|(name, request)| (name, fixture.build_plan(request).expect("the demand plans")))
            .collect()
    };
    for (name, stale_plan) in &stale_plans {
        let outcome = finalize_flow_solve(&runtime, stale_plan, &sealed);
        assert!(
            matches!(outcome, FlowSolveOutcome::Partial(_)),
            "a stale {name} must not complete: {outcome:?}"
        );
        assert!(outcome.warm_candidate().is_none());
    }

    // A result contract foreign to the operation's registered contract must
    // not complete even when installed, sealed, and finalized consistently.
    let mut foreign_request = base_request();
    foreign_request.result_contract = foreign_result_contract(3);
    let foreign_plan = fixture
        .build_plan(foreign_request)
        .expect("the demand plans");
    let (foreign_runtime, foreign_sealed) = drive_to_completion(&foreign_plan);
    let outcome = finalize_flow_solve(&foreign_runtime, &foreign_plan, &foreign_sealed);
    assert!(
        matches!(
            outcome,
            FlowSolveOutcome::Partial(ref partial)
                if partial.reason == FlowPartialReason::ResultContractMismatch
        ),
        "a foreign result contract must not complete: {outcome:?}"
    );
}

#[test]
fn result_contract_id_tracks_the_complete_contract() {
    let base = *flow_operation_contract(SemanticQueryKeyTag::FlowReturn)
        .expect("FlowReturn is a registered flow operation");
    let base_id = flow_result_contract_id(&base);
    // Deterministic: the same closed contract mints the same identity.
    assert_eq!(base_id, flow_result_contract_id(&base));

    let cases: Vec<(&str, FlowOperationContract)> = vec![
        (
            "role",
            FlowOperationContract {
                role: FlowOperationRole::SemanticSuboperation,
                ..base
            },
        ),
        (
            "status",
            FlowOperationContract {
                status: FlowOperationStatus::PendingReducer,
                ..base
            },
        ),
        (
            "domains",
            FlowOperationContract {
                required_domains: &[FlowDomain::ReachingValue],
                ..base
            },
        ),
        (
            "domain order",
            FlowOperationContract {
                required_domains: &[
                    FlowDomain::ReachingType,
                    FlowDomain::ReachingValue,
                    FlowDomain::Narrowing,
                    FlowDomain::Completion,
                    FlowDomain::ClosureCapture,
                    FlowDomain::Freshness,
                    FlowDomain::Effects,
                    FlowDomain::CallResolution,
                    FlowDomain::Relation,
                ],
                ..base
            },
        ),
        (
            "fact families",
            FlowOperationContract {
                required_fact_families: &[FlowFactFamily::BindingSlot],
                ..base
            },
        ),
        (
            "finalizer",
            FlowOperationContract {
                result: FlowResultContractDescriptor {
                    finalizer: FlowFinalizerKind::TypedGapOnly,
                    ..base.result
                },
                ..base
            },
        ),
        (
            "accepted gaps",
            FlowOperationContract {
                result: FlowResultContractDescriptor {
                    accepted_gaps: &[],
                    ..base.result
                },
                ..base
            },
        ),
    ];
    for (name, contract) in &cases {
        assert_ne!(
            base_id,
            flow_result_contract_id(contract),
            "the result-contract identity must change when the {name} semantics change"
        );
    }
}

#[test]
fn obligation_budget_trips_at_first_excess() {
    let (fixture, plan) = planned();
    let full = plan.work_order.len() as u32;
    assert!(
        full > 9,
        "the fixture expands past the initial domain obligations"
    );

    // Expansion stops at the first excess: the planner reports the
    // would-be NEXT count, not the full population it never constructed.
    let mut request = base_request();
    request.resources = FlowResourcePolicy {
        max_obligations: full - 2,
        ..FlowResourcePolicy::default()
    };
    let Err(FlowDemandPlanError::ObligationBudget { limit, observed }) =
        fixture.build_plan(request)
    else {
        panic!("a tightened obligation budget must trip")
    };
    assert_eq!(limit, full - 2);
    assert_eq!(
        observed,
        full - 1,
        "the planner stops at the first excess instead of building the full population ({full})"
    );

    // `additional_requirements` is unbounded caller input: it is counted
    // BEFORE any obligation construction, so the report names the counted
    // base (contract domains + additional), never the expanded population.
    let mut request = base_request();
    request.additional_requirements = (0..20)
        .map(|_| FlowRequirement {
            operation: SemanticQueryKeyTag::FlowReturn,
            requirement: FlowRequirementKind::FactFamily(FlowFactFamily::BindingSlot),
        })
        .collect();
    request.resources = FlowResourcePolicy {
        max_obligations: 28,
        ..FlowResourcePolicy::default()
    };
    let Err(FlowDemandPlanError::ObligationBudget { limit, observed }) =
        fixture.build_plan(request)
    else {
        panic!("an oversized additional-requirements vector must trip the budget")
    };
    assert_eq!(limit, 28);
    assert_eq!(
        observed, 29,
        "the counted base (9 domains + 20 additional) trips before any node or edge obligation is constructed"
    );
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
