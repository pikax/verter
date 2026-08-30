//! Completeness-proof discipline of the private flow-solve layer: a flow
//! result is COMPLETE only when every planned obligation of the demand
//! discharged under the exact basis the demand was planned against, with
//! per-spec validated evidence and runtime-observed deterministic
//! convergence, sealed into ONE completion artifact by the obligation
//! runtime itself. Undeclared domain/fact-family requirements become typed
//! gaps — never silently dropped — and no partial, gapped, failed, stale,
//! or non-converged replay is a warm candidate.
//!
//! The plan is SEALED (immutable getters only; the requirement universe —
//! the closed domain→family registry mapping — is fixed at plan time); the
//! runtime drives a one-shot lifecycle (Discharging → ExpansionClosed →
//! Converging → Converged → Sealed); and the finalizer accepts ONLY the
//! runtime-sealed artifact, consuming it: no caller-supplied value, no
//! caller-authored convergence evidence, and no caller-assembled discharge
//! evidence can reach it.

use std::sync::Arc;

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};
use verter_language::{FileLanguage, ScriptSourceType};
use verter_semantic::analysis::flow::flow_graph::FlowEdgeClass;
use verter_session::for_tests::{
    degraded_flow_return_result_for_tests, dispatch_flow_demand_footprint_for_tests,
    finalize_flow_solve, flow_family_route, flow_graph_fixture_for_tests,
    flow_graph_fixture_for_tests_with_language, flow_operation_contract, flow_result_contract_id,
    flow_return_result_contract_id, flow_return_result_for_tests, CompleteFlowResult,
    FlowDemandHandle, FlowDemandPlan, FlowDemandPlanError, FlowDemandRequest, FlowDischargeEntry,
    FlowDischargeReport, FlowDomain, FlowDomainClosure, FlowFactFamily, FlowFailure,
    FlowFailureClass, FlowFinalizerKind, FlowGraphFixtureForTests, FlowObligationBasis,
    FlowObligationId, FlowObligationOrigin, FlowObligationSpec, FlowOperationContract,
    FlowOperationRole, FlowOperationStatus, FlowPartialReason, FlowRequirement,
    FlowRequirementKind, FlowResourcePolicy, FlowResultContractDescriptor, FlowSealError,
    FlowSolveOutcome, FlowSuboperationEvidence, FlowTransitionError, ObligationRuntime,
    ObligationState, SealedFlowCompletion, SemanticGraphStore,
};
use verter_session::semantic_query::demand::{ProjectionPath, SurfaceFacet, SurfaceFacetSet};
use verter_session::semantic_query::{
    CanonicalTypeSubstitution, ContextualTypingKey, FlowFunctionSlotIdentity, FlowGap,
    FlowInputContext, FlowNarrowingKey, FlowReturnContext, FlowReturnKey, FlowReturnPolicy,
    FlowReturnResult, PathSegment, PrimitiveKind, ProgramAnalysisContext, ProgramPointId,
    PropertyKey, ResolvedDeclSlotIdentity, ReturnProjectionDemand, SemanticNodeData,
    SemanticQueryKey, SemanticQueryKeyTag, SemanticSymbolSpace, SubstitutionCanonicalHash,
};
use verter_session::{HostConfig, VerterHost};

/// The fixture body: one parameter, one local, one object-literal return
/// with a call entry, so the demand plan exercises binding-slot, return-site,
/// edge, and call-site expansion — including TWO binding obligations of the
/// same family with distinct provenance. No control flow and no nested
/// function: the guard and capture families are PROVED EMPTY through their
/// coverage obligations.
const FIXTURE_SOURCE: &str = r#"
function solve_me(x) {
  const y = x;
  return { value: y, other: side_effect(y) };
}
"#;

/// The rich fixture: a predicated region (a guard), one expression site
/// carrying MULTIPLE call occurrences, a second call site, and two return
/// sites — so the plan exercises guard, per-ordinal call, relation, and
/// contextual-target expansion. Still no nested function: captures stay
/// proved-empty.
const RICH_FIXTURE_SOURCE: &str = r#"
function rich(x) {
  const y = x;
  if (y) { return { a: side(y) }; }
  return { value: y, other: pair(first(), second()) };
}
"#;

/// The capture fixture: a nested function closing over the parameter — the
/// capture set is beyond the skeleton's authority, so the capture subject
/// installs the family's accepted typed gap.
const CAPTURE_FIXTURE_SOURCE: &str = r#"
function with_capture(x) {
  function helper() { return x; }
  return helper();
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

fn flow_return_query_named(env_tag: u8, name: &str) -> SemanticQueryKey {
    SemanticQueryKey::FlowReturn(Box::new(FlowReturnKey {
        function: FlowFunctionSlotIdentity {
            // The bound graph's program key is VALUE-space (a function
            // declaration), so the query's slot must name the value-space
            // slot — the planner proves the query identifies the bound
            // graph's function on every shared axis.
            declaration_slot: ResolvedDeclSlotIdentity::value_slot(
                Arc::from("/flow_solve_fixture.ts"),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from(name),
                0,
                [0; 16],
                [0; 16],
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
        // The key's contract axis: the value the single production key
        // constructor derives from the closed registry row. Tests that
        // need a FOREIGN contract overwrite exactly this field.
        result_contract: flow_return_result_contract_id(),
    }))
}

fn flow_return_query(env_tag: u8) -> SemanticQueryKey {
    flow_return_query_named(env_tag, "solve_me")
}

fn registered_result_contract() -> ResultContractId {
    flow_result_contract_id(
        flow_operation_contract(SemanticQueryKeyTag::FlowReturn)
            .expect("FlowReturn is a registered flow operation"),
    )
}

/// A demand request carries NO graph axis, NO subject axis, and NO
/// result-contract axis: the bound graph pins the body identity, the query
/// payload carries the demand, and the key carries the contract.
fn base_request() -> FlowDemandRequest {
    FlowDemandRequest {
        query: flow_return_query(0),
        input_basis: test_input_basis(1),
        resources: FlowResourcePolicy::default(),
        additional_requirements: Arc::from([]),
    }
}

fn request_named(name: &str) -> FlowDemandRequest {
    FlowDemandRequest {
        query: flow_return_query_named(0, name),
        ..base_request()
    }
}

fn planned() -> (FlowGraphFixtureForTests, FlowDemandPlan) {
    let fixture = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 7);
    let plan = fixture
        .build_plan(base_request())
        .expect("the fixture demand plans within budget");
    (fixture, plan)
}

fn planned_rich() -> (FlowGraphFixtureForTests, FlowDemandPlan) {
    let fixture = flow_graph_fixture_for_tests(RICH_FIXTURE_SOURCE, 9);
    let plan = fixture
        .build_plan(request_named("rich"))
        .expect("the rich fixture demand plans within budget");
    (fixture, plan)
}

fn spec(plan: &FlowDemandPlan, id: FlowObligationId) -> &FlowObligationSpec {
    plan.obligation_specs()
        .iter()
        .find(|spec| spec.id() == id)
        .expect("every planned id has a spec")
}

/// The exact declared dependency evidence of `spec`.
fn declared_dependencies(spec: &FlowObligationSpec) -> Arc<[FlowObligationId]> {
    Arc::from(spec.expected_dependencies())
}

/// The suboperation evidence a faithful solve presents for `spec`: exactly
/// the declared suboperations under the installed result contract.
fn expected_suboperations(
    plan: &FlowDemandPlan,
    spec: &FlowObligationSpec,
) -> Arc<[FlowSuboperationEvidence]> {
    spec.expected_suboperations()
        .iter()
        .map(|operation| FlowSuboperationEvidence {
            operation: *operation,
            result_contract: plan.basis().result_contract.clone(),
        })
        .collect()
}

fn discharge_one(
    runtime: &mut ObligationRuntime,
    handle: FlowDemandHandle,
    plan: &FlowDemandPlan,
    id: FlowObligationId,
) {
    let obligation = spec(plan, id);
    runtime
        .start_flow_obligation(handle, id)
        .expect("a planned pending obligation starts");
    runtime
        .discharge_flow_obligation(
            handle,
            id,
            declared_dependencies(obligation),
            expected_suboperations(plan, obligation),
        )
        .expect("a running obligation discharges with its spec-declared evidence");
}

fn discharge_all(
    runtime: &mut ObligationRuntime,
    handle: FlowDemandHandle,
    plan: &FlowDemandPlan,
    order: &[FlowObligationId],
) {
    for id in order {
        discharge_one(runtime, handle, plan, *id);
    }
}

/// The runtime observes the fixed point: one changing iteration, then one
/// stable one. Convergence is runtime-OBSERVED state, never caller evidence.
fn observe_convergence(runtime: &mut ObligationRuntime, handle: FlowDemandHandle) {
    runtime
        .observe_flow_iteration(handle, true)
        .expect("a changing fixed-point iteration is observed");
    runtime
        .observe_flow_iteration(handle, false)
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
/// work order, let the runtime observe convergence, seal.
fn drive_to_completion(
    plan: &FlowDemandPlan,
) -> (ObligationRuntime, FlowDemandHandle, SealedFlowCompletion) {
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(plan);
    discharge_all(&mut runtime, handle, plan, plan.work_order());
    observe_convergence(&mut runtime, handle);
    let sealed = runtime
        .seal_flow_completion(handle, solve_value())
        .expect("a fully discharged, runtime-converged solve seals");
    (runtime, handle, sealed)
}

#[test]
fn complete_result_requires_every_planned_obligation() {
    let (fixture, plan) = planned();

    // Positive control: the sealed path is the sole construction of a
    // complete, warm-admissible result.
    let (runtime, handle, sealed) = drive_to_completion(&plan);
    let sealed_value = sealed.value().clone();
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
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
    let (runtime, handle, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, handle, &wider_plan, sealed);
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
    let handle = runtime.install_flow_demand(&plan);
    let mut order = plan.work_order().to_vec();
    let held = order.pop().expect("the plan has obligations");
    discharge_all(&mut runtime, handle, &plan, &order);
    assert!(
        matches!(
            runtime.seal_flow_completion(handle, solve_value()),
            Err(FlowSealError::UndischargedObligations)
        ),
        "a pending obligation must block sealing"
    );
    assert!(matches!(
        runtime
            .flow_obligations(handle)
            .expect("the demand is installed")
            .iter()
            .find(|record| record.spec.id() == held)
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
    let handle = runtime.install_flow_demand(&plan);

    let gaps: Vec<_> = runtime
        .flow_obligations(handle)
        .expect("the demand is installed")
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
    assert!(gaps.iter().any(|record| *record.spec.requirement()
        == FlowRequirement {
            operation: SemanticQueryKeyTag::FlowReturn,
            requirement: FlowRequirementKind::Domain(FlowDomain::Coverage),
        }));
    assert!(gaps.iter().any(|record| *record.spec.requirement()
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
    let canonical: Vec<FlowObligationId> = plan.work_order().to_vec();
    assert!(
        canonical.len() >= 4,
        "the fixture must expand to enough obligations to permute"
    );

    // Dependency levels: an obligation's level is 1 + the maximum level of
    // its declared dependencies (level 0 = no dependencies). Dependencies
    // precede their dependents in the work order, so levels resolve in one
    // pass. Any order whose levels are non-decreasing is a legal drive.
    let mut levels: std::collections::BTreeMap<FlowObligationId, usize> =
        std::collections::BTreeMap::new();
    for id in &canonical {
        let level = spec(&plan, *id)
            .expected_dependencies()
            .iter()
            .map(|dependency| levels[dependency] + 1)
            .max()
            .unwrap_or(0);
        levels.insert(*id, level);
    }
    let permute = |reverse_within: bool, rotate_within: bool| -> Vec<FlowObligationId> {
        let mut groups: std::collections::BTreeMap<usize, Vec<FlowObligationId>> =
            std::collections::BTreeMap::new();
        for id in &canonical {
            groups.entry(levels[id]).or_default().push(*id);
        }
        groups
            .into_values()
            .flat_map(|mut group| {
                if reverse_within {
                    group.reverse();
                }
                if rotate_within && group.len() > 1 {
                    group.rotate_left(1);
                }
                group
            })
            .collect()
    };
    let reversed = permute(true, false);
    let rotated = permute(false, true);
    assert_ne!(canonical, reversed);
    assert_ne!(canonical, rotated);
    assert_ne!(reversed, rotated);

    let mut results: Vec<CompleteFlowResult> = Vec::new();
    for order in [&canonical, &reversed, &rotated] {
        let mut runtime = ObligationRuntime::default();
        let handle = runtime.install_flow_demand(&plan);
        discharge_all(&mut runtime, handle, &plan, order);
        observe_convergence(&mut runtime, handle);
        let sealed = runtime
            .seal_flow_completion(handle, solve_value())
            .expect("every legal discharge order seals");
        match finalize_flow_solve(&runtime, handle, &plan, sealed) {
            FlowSolveOutcome::Complete(complete) => results.push(complete),
            other => panic!("every legal discharge order must complete: {other:?}"),
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
        plan_a.basis(),
        plan_b.basis(),
        "distinct bound graphs must yield distinct bases"
    );
    // Replanning over the same bound graph is deterministic.
    let plan_a2 = fixture_a.build_plan(base_request()).expect("plans");
    assert_eq!(plan_a.basis(), plan_a2.basis());
    assert_eq!(plan_a.obligation_specs(), plan_a2.obligation_specs());

    // A solve sealed under one bound graph can never complete against a
    // plan minted over the other, even with identical source.
    let (runtime, handle, sealed) = drive_to_completion(&plan_a);
    let outcome = finalize_flow_solve(&runtime, handle, &plan_b, sealed);
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
    assert!(plan.subject().projection_path.is_empty());

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
        plan.subject()
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
        .filter(|spec| matches!(spec.basis(), FlowObligationBasis::Binding { .. }))
        .collect();
    assert!(
        bindings.len() >= 2,
        "the fixture must plan at least two binding obligations: {}",
        bindings.len()
    );
    // Same family, same origin, same declared evidence — yet distinct
    // semantic subjects. The identity, not the plan-local ordinal, is
    // what separates them.
    assert_eq!(bindings[0].requirement(), bindings[1].requirement());
    assert_eq!(bindings[0].origin(), bindings[1].origin());
    assert_ne!(
        bindings[0].basis(),
        bindings[1].basis(),
        "two binding obligations of one family carry distinct provenance"
    );
    let (
        FlowObligationBasis::Binding { slot: first, .. },
        FlowObligationBasis::Binding { slot: second, .. },
    ) = (bindings[0].basis(), bindings[1].basis())
    else {
        unreachable!()
    };
    assert_ne!(first.binding, second.binding);
    assert_ne!(first.identity, second.identity);

    // Each requires its OWN discharge: discharging the first leaves the
    // second pending, and a second discharge of the same obligation is an
    // illegal transition.
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    discharge_one(&mut runtime, handle, &plan, bindings[0].id());
    assert_eq!(
        runtime.discharge_flow_obligation(
            handle,
            bindings[0].id(),
            declared_dependencies(bindings[0]),
            expected_suboperations(&plan, bindings[0]),
        ),
        Err(FlowTransitionError::IllegalTransition),
        "an already-discharged obligation cannot discharge again"
    );
    assert!(
        matches!(
            runtime.seal_flow_completion(handle, solve_value()),
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
        .find(|spec| matches!(spec.basis(), FlowObligationBasis::CallSite { .. }))
        .expect("the fixture plans a call-site obligation with required suboperations")
        .id();
    let edge_spec = plan
        .obligation_specs()
        .iter()
        .find(|spec| matches!(spec.basis(), FlowObligationBasis::Edge { .. }))
        .expect("the fixture plans an edge obligation with required dependencies")
        .id();

    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);

    // Empty evidence cannot discharge a spec that declares required
    // suboperations — the check is against THIS spec, not a global set.
    runtime
        .start_flow_obligation(handle, call_spec)
        .expect("start");
    assert_eq!(
        runtime.discharge_flow_obligation(handle, call_spec, Arc::from([]), Arc::from([])),
        Err(FlowTransitionError::NonSuboperationEvidence),
        "empty evidence must not discharge a spec declaring a suboperation"
    );
    // A wrong suboperation tag is refused, as is a foreign result contract.
    let valid = expected_suboperations(&plan, spec(&plan, call_spec));
    let mut wrong_tag = valid.to_vec();
    wrong_tag[0].operation = SemanticQueryKeyTag::Relate;
    assert_eq!(
        runtime.discharge_flow_obligation(handle, call_spec, Arc::from([]), Arc::from(wrong_tag)),
        Err(FlowTransitionError::NonSuboperationEvidence)
    );
    let mut foreign = valid.to_vec();
    foreign[0].result_contract = foreign_result_contract(4);
    assert_eq!(
        runtime.discharge_flow_obligation(handle, call_spec, Arc::from([]), Arc::from(foreign)),
        Err(FlowTransitionError::NonSuboperationEvidence)
    );

    // The same holds for declared dependencies: empty or foreign.
    runtime
        .start_flow_obligation(handle, edge_spec)
        .expect("start");
    assert_eq!(
        runtime.discharge_flow_obligation(handle, edge_spec, Arc::from([]), Arc::from([])),
        Err(FlowTransitionError::UnplannedDependency),
        "empty evidence must not discharge a spec declaring a dependency"
    );
    assert_eq!(
        runtime.discharge_flow_obligation(
            handle,
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
            .flow_obligations(handle)
            .expect("the demand is installed")
            .iter()
            .find(|record| record.spec.id() == call_spec)
            .map(|record| &record.state),
        Some(ObligationState::Running)
    ));
    let obligation = spec(&plan, call_spec);
    runtime
        .discharge_flow_obligation(
            handle,
            call_spec,
            declared_dependencies(obligation),
            expected_suboperations(&plan, obligation),
        )
        .expect("the still-running call-site obligation discharges");
    // Dependency readiness: the edge's declared dependency (its source
    // node's primary obligation) must itself be discharged first.
    let edge = spec(&plan, edge_spec);
    let dependency = edge.expected_dependencies()[0];
    discharge_one(&mut runtime, handle, &plan, dependency);
    runtime
        .discharge_flow_obligation(
            handle,
            edge_spec,
            declared_dependencies(edge),
            expected_suboperations(&plan, edge),
        )
        .expect("the still-running edge obligation discharges");
}

#[test]
fn convergence_must_be_runtime_observed() {
    let (_fixture, plan) = planned();

    // Fully discharged, but the runtime never observed a fixed point.
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    discharge_all(&mut runtime, handle, &plan, plan.work_order());
    assert!(
        matches!(
            runtime.seal_flow_completion(handle, solve_value()),
            Err(FlowSealError::NonConverged)
        ),
        "no convergence observation cannot seal"
    );

    // A changing iteration alone is not convergence.
    runtime
        .observe_flow_iteration(handle, true)
        .expect("a changing iteration is observed");
    assert!(
        matches!(
            runtime.seal_flow_completion(handle, solve_value()),
            Err(FlowSealError::NonConverged)
        ),
        "a still-changing fixed point cannot seal"
    );

    // The stable iteration closes convergence; observing past it is an
    // illegal transition (the solve kept running past its fixed point).
    runtime
        .observe_flow_iteration(handle, false)
        .expect("the stable iteration closes convergence");
    assert_eq!(
        runtime.observe_flow_iteration(handle, true),
        Err(FlowTransitionError::IllegalTransition),
        "no iteration exists past the observed fixed point"
    );
    assert!(runtime.seal_flow_completion(handle, solve_value()).is_ok());

    // The iteration budget is enforced at observation time: the
    // (max + 1)-th changing iteration is refused, and a solve that ran
    // into it can never stabilize, so it can never seal.
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    discharge_all(&mut runtime, handle, &plan, plan.work_order());
    for _ in 0..plan.convergence().max_iterations {
        runtime
            .observe_flow_iteration(handle, true)
            .expect("within budget");
    }
    assert_eq!(
        runtime.observe_flow_iteration(handle, true),
        Err(FlowTransitionError::ConvergenceBudget),
        "the first over-budget iteration is refused"
    );
    assert!(matches!(
        runtime.seal_flow_completion(handle, solve_value()),
        Err(FlowSealError::NonConverged)
    ));

    // Observing on a runtime with no demand installed is a typed error,
    // not a default: the handle belongs to ANOTHER runtime and is out of
    // range here.
    let mut idle = ObligationRuntime::default();
    assert_eq!(
        idle.observe_flow_iteration(handle, false),
        Err(FlowTransitionError::NoDemandInstalled)
    );
}

/// The convergence gate: NO observation is admitted while the expansion
/// frontier is open or any required obligation is undischarged.
#[test]
fn convergence_observation_requires_a_closed_discharged_frontier() {
    let (_fixture, plan) = planned();
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);

    // Frontier open, nothing discharged: both changing and stable
    // observations are rejected.
    assert_eq!(
        runtime.observe_flow_iteration(handle, true),
        Err(FlowTransitionError::IllegalTransition)
    );
    assert_eq!(
        runtime.observe_flow_iteration(handle, false),
        Err(FlowTransitionError::IllegalTransition)
    );

    // All but one obligation discharged: still rejected — "almost
    // discharged" is not a closed frontier.
    let order = plan.work_order().to_vec();
    let (last, rest) = order.split_last().expect("the plan has obligations");
    discharge_all(&mut runtime, handle, &plan, rest);
    assert_eq!(
        runtime.observe_flow_iteration(handle, true),
        Err(FlowTransitionError::IllegalTransition),
        "one undischarged obligation keeps the frontier open"
    );

    // Fully discharged: observation is admitted, and the solve completes.
    discharge_one(&mut runtime, handle, &plan, *last);
    observe_convergence(&mut runtime, handle);
    let sealed = runtime
        .seal_flow_completion(handle, solve_value())
        .expect("a fully discharged, runtime-converged solve seals");
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
    assert!(outcome.warm_candidate().is_some());
}

/// A dependent obligation cannot discharge until its EXACT dependencies
/// are themselves discharged.
#[test]
fn dependent_obligation_discharge_requires_discharged_dependencies() {
    let (_fixture, plan) = planned();

    // A domain obligation depends on its family-coverage obligations.
    let domain_spec = plan
        .obligation_specs()
        .iter()
        .find(|spec| matches!(spec.origin(), FlowObligationOrigin::ContractDomain))
        .expect("the plan has contract-domain obligations");
    assert!(
        !domain_spec.expected_dependencies().is_empty(),
        "every domain obligation depends on its closure's coverage obligations"
    );

    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    runtime
        .start_flow_obligation(handle, domain_spec.id())
        .expect("start");
    assert_eq!(
        runtime.discharge_flow_obligation(
            handle,
            domain_spec.id(),
            declared_dependencies(domain_spec),
            Arc::from([]),
        ),
        Err(FlowTransitionError::UndischargedDependency),
        "a domain obligation cannot discharge before its coverage obligations discharge"
    );
    // The obligation is left Running: the refused discharge minted nothing.
    assert!(matches!(
        runtime
            .flow_obligations(handle)
            .expect("the demand is installed")
            .iter()
            .find(|record| record.spec.id() == domain_spec.id())
            .map(|record| &record.state),
        Some(ObligationState::Running)
    ));
    // Discharge the exact dependencies, then the domain discharges.
    for dependency in domain_spec.expected_dependencies() {
        discharge_one(&mut runtime, handle, &plan, *dependency);
    }
    runtime
        .discharge_flow_obligation(
            handle,
            domain_spec.id(),
            declared_dependencies(domain_spec),
            Arc::from([]),
        )
        .expect("with every dependency discharged, the domain discharges");

    // The same gate binds an edge-fact obligation to its source node's
    // obligation.
    let edge_spec = plan
        .obligation_specs()
        .iter()
        .find(|spec| matches!(spec.basis(), FlowObligationBasis::Edge { .. }))
        .expect("the fixture plans an edge obligation");
    let edge_dependency = edge_spec.expected_dependencies()[0];
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    runtime
        .start_flow_obligation(handle, edge_spec.id())
        .expect("start");
    assert_eq!(
        runtime.discharge_flow_obligation(
            handle,
            edge_spec.id(),
            declared_dependencies(edge_spec),
            Arc::from([]),
        ),
        Err(FlowTransitionError::UndischargedDependency),
        "an edge obligation cannot discharge before its source node's obligation"
    );
    discharge_one(&mut runtime, handle, &plan, edge_dependency);
    runtime
        .discharge_flow_obligation(
            handle,
            edge_spec.id(),
            declared_dependencies(edge_spec),
            Arc::from([]),
        )
        .expect("with the dependency discharged, the edge discharges");
}

/// Once convergence begins, no obligation expansion or transition is
/// legal; once sealed, EVERY transition fails and a repeated seal is
/// `AlreadySealed`.
#[test]
fn no_transitions_after_convergence_and_seal_is_one_shot() {
    let (_fixture, plan) = planned();
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    discharge_all(&mut runtime, handle, &plan, plan.work_order());
    runtime
        .observe_flow_iteration(handle, true)
        .expect("the first observation begins convergence");

    // Converging: every obligation transition is rejected.
    let id = plan.work_order()[0];
    let obligation = spec(&plan, id);
    assert_eq!(
        runtime.start_flow_obligation(handle, id),
        Err(FlowTransitionError::IllegalTransition)
    );
    assert_eq!(
        runtime.discharge_flow_obligation(
            handle,
            id,
            declared_dependencies(obligation),
            expected_suboperations(&plan, obligation),
        ),
        Err(FlowTransitionError::IllegalTransition)
    );
    assert_eq!(
        runtime.gap_flow_obligation(handle, id, FlowGap::UnmodeledExpression),
        Err(FlowTransitionError::IllegalTransition)
    );
    assert_eq!(
        runtime.fail_flow_obligation(
            handle,
            id,
            FlowFailure {
                class: FlowFailureClass::Internal
            }
        ),
        Err(FlowTransitionError::IllegalTransition)
    );

    // Converged → Sealed: the artifact mints exactly once.
    runtime
        .observe_flow_iteration(handle, false)
        .expect("the stable iteration closes convergence");
    let sealed = runtime
        .seal_flow_completion(handle, solve_value())
        .expect("a fully discharged, runtime-converged solve seals");

    // Post-seal: every transition fails; a repeated seal is AlreadySealed.
    assert!(
        matches!(
            runtime.seal_flow_completion(handle, solve_value()),
            Err(FlowSealError::AlreadySealed)
        ),
        "the completion artifact is one-shot"
    );
    assert_eq!(
        runtime.observe_flow_iteration(handle, false),
        Err(FlowTransitionError::IllegalTransition)
    );
    assert_eq!(
        runtime.start_flow_obligation(handle, id),
        Err(FlowTransitionError::IllegalTransition)
    );
    assert_eq!(
        runtime.gap_flow_obligation(handle, id, FlowGap::UnmodeledExpression),
        Err(FlowTransitionError::IllegalTransition)
    );
    // Installing again is a NEW, INDEPENDENT demand — nested flow frames
    // and deferred members each hold their own demand, so there is no
    // singleton to refuse a second install. The SEALED demand's handle
    // still rejects every transition; the fresh handle starts Discharging.
    let second = runtime.install_flow_demand(&plan);
    assert_ne!(
        second, handle,
        "every install mints a distinct demand handle"
    );
    assert_eq!(
        runtime.observe_flow_iteration(handle, false),
        Err(FlowTransitionError::IllegalTransition),
        "the sealed demand stays sealed beside its sibling"
    );
    runtime
        .start_flow_obligation(second, id)
        .expect("the sibling demand accepts its own transitions");

    // The minted artifact still finalizes (the runtime is unchanged).
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
    assert!(outcome.warm_candidate().is_some());
}

#[test]
fn foreign_value_and_partial_solve_cannot_seal() {
    let (_fixture, plan) = planned();

    // A value minted over a foreign graph store with NO solve work behind
    // it: nothing discharged, nothing observed — the runtime refuses to
    // seal it.
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    assert!(
        matches!(
            runtime.seal_flow_completion(handle, solve_value()),
            Err(FlowSealError::UndischargedObligations)
        ),
        "a foreign value with no discharge evidence cannot seal"
    );

    // A degraded value can never seal, even over a fully discharged,
    // converged solve.
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    discharge_all(&mut runtime, handle, &plan, plan.work_order());
    observe_convergence(&mut runtime, handle);
    let degraded = {
        let graph = SemanticGraphStore::new();
        let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        degraded_flow_return_result_for_tests(&graph, node)
    };
    assert!(matches!(
        runtime.seal_flow_completion(handle, degraded),
        Err(FlowSealError::DegradedValue)
    ));

    // A runtime that never served a demand has nothing to seal: a handle
    // minted by ANOTHER runtime is out of range here and fails closed.
    let mut idle = ObligationRuntime::default();
    assert!(matches!(
        idle.seal_flow_completion(handle, solve_value()),
        Err(FlowSealError::NoDemandInstalled)
    ));
}

/// Per-demand handles: two installed demands on ONE runtime run fully
/// independent one-shot lifecycles — the model nested flow frames and
/// deferred SCC members need. Sealing one neither rejects the other's
/// install (there is no singleton) nor freezes the other's transitions.
#[test]
fn nested_flow_demands_have_independent_lifecycles() {
    let (_fixture, plan) = planned();
    let (_rich_fixture, rich_plan) = planned_rich();

    let mut runtime = ObligationRuntime::default();
    let outer = runtime.install_flow_demand(&plan);
    let inner = runtime.install_flow_demand(&rich_plan);
    assert_ne!(outer, inner, "every install mints a distinct handle");
    assert_eq!(runtime.flow_demand_count(), 2);

    // Drive the INNER demand to completion while the OUTER stays open.
    discharge_all(&mut runtime, inner, &rich_plan, rich_plan.work_order());
    observe_convergence(&mut runtime, inner);
    let inner_sealed = runtime
        .seal_flow_completion(inner, solve_value())
        .expect("the inner demand seals on its own lifecycle");

    // The outer demand is untouched: every obligation is still Pending.
    let outer_pending = runtime
        .flow_obligations(outer)
        .expect("the outer demand is installed")
        .iter()
        .filter(|record| matches!(record.state, ObligationState::Pending))
        .count();
    assert_eq!(
        outer_pending,
        plan.work_order().len(),
        "the sibling's lifecycle never touched the outer demand"
    );

    // The outer demand completes on its own afterwards.
    discharge_all(&mut runtime, outer, &plan, plan.work_order());
    observe_convergence(&mut runtime, outer);
    let outer_sealed = runtime
        .seal_flow_completion(outer, solve_value())
        .expect("the outer demand seals after its sibling");

    let inner_outcome = finalize_flow_solve(&runtime, inner, &rich_plan, inner_sealed);
    assert!(
        inner_outcome.warm_candidate().is_some(),
        "the inner demand finalizes: {inner_outcome:?}"
    );
    let outer_outcome = finalize_flow_solve(&runtime, outer, &plan, outer_sealed);
    assert!(
        outer_outcome.warm_candidate().is_some(),
        "the outer demand finalizes: {outer_outcome:?}"
    );
}

/// `ResultContractId` is exact production identity on the key: the
/// constructor-derived contract IS the registered contract of the
/// `FlowReturn` registry row, and two otherwise identical keys carrying
/// different contract ids compare — and hash — unequal.
#[test]
fn flow_return_key_result_contract_is_exact_identity() {
    let SemanticQueryKey::FlowReturn(key) = flow_return_query(0) else {
        unreachable!()
    };
    assert_eq!(
        key.result_contract,
        registered_result_contract(),
        "the constructor-derived contract IS the FlowReturn registry row's identity"
    );

    let mut foreign = (*key).clone();
    foreign.result_contract = foreign_result_contract(3);
    assert_ne!(
        *key, foreign,
        "two keys differing only in result contract are distinct identities"
    );
    let hash_of = |key: &FlowReturnKey| {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    };
    assert_ne!(
        hash_of(&key),
        hash_of(&foreign),
        "the contract axis participates in the key hash (the family key embeds the full key)"
    );
}

/// The central report applicator: a complete report discharges every
/// obligation in the plan's deterministic work order regardless of the
/// report's own entry order; a claim naming an obligation the demand
/// never installed fails closed; a partial report never seals.
#[test]
fn apply_flow_discharge_report_applies_in_work_order_and_fails_closed() {
    let (_fixture, plan) = planned();
    let report_for = |plan: &FlowDemandPlan, order: &[FlowObligationId]| {
        FlowDischargeReport::new(
            order
                .iter()
                .map(|id| {
                    let obligation = spec(plan, *id);
                    FlowDischargeEntry {
                        obligation: *id,
                        dependencies: declared_dependencies(obligation),
                        suboperations: expected_suboperations(plan, obligation),
                    }
                })
                .collect(),
        )
    };

    // A complete report — entries in REVERSE work order — still applies in
    // the plan's deterministic work order and completes the solve.
    let reversed: Vec<FlowObligationId> = plan.work_order().iter().rev().copied().collect();
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    runtime
        .apply_flow_discharge_report(handle, &plan, &report_for(&plan, &reversed))
        .expect("a complete report applies");
    observe_convergence(&mut runtime, handle);
    let sealed = runtime
        .seal_flow_completion(handle, solve_value())
        .expect("a fully applied, runtime-converged solve seals");
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
    assert!(
        outcome.warm_candidate().is_some(),
        "the centrally applied report completes: {outcome:?}"
    );

    // A claim naming an obligation this demand never installed fails
    // closed — never silently dropped.
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    let foreign = FlowDischargeReport::new(vec![FlowDischargeEntry {
        obligation: FlowObligationId(u32::MAX),
        dependencies: Arc::from([]),
        suboperations: Arc::from([]),
    }]);
    assert_eq!(
        runtime.apply_flow_discharge_report(handle, &plan, &foreign),
        Err(FlowTransitionError::UnknownObligation),
        "a foreign claim fails closed"
    );

    // A partial report is not a completion: whatever it claimed is
    // discharged, the rest stays pending, and the solve never seals.
    let prefix = &plan.work_order()[..plan.work_order().len() / 2];
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    runtime
        .apply_flow_discharge_report(handle, &plan, &report_for(&plan, prefix))
        .expect("the prefix report applies");
    assert!(matches!(
        runtime.seal_flow_completion(handle, solve_value()),
        Err(FlowSealError::UndischargedObligations)
    ));
}

#[test]
fn partial_replay_never_seals() {
    let (fixture, plan) = planned();

    // Fully discharged control: seals, completes, warms.
    let (runtime, handle, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
    assert!(outcome.warm_candidate().is_some());

    // A gapped obligation: never sealed, never warm.
    let mut gapped_request = base_request();
    gapped_request.additional_requirements = Arc::from(vec![FlowRequirement {
        operation: SemanticQueryKeyTag::FlowReturn,
        requirement: FlowRequirementKind::Domain(FlowDomain::Coverage),
    }]);
    let gapped_plan = fixture.build_plan(gapped_request).expect("plans");
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&gapped_plan);
    let pending: Vec<FlowObligationId> = runtime
        .flow_obligations(handle)
        .expect("the demand is installed")
        .iter()
        .filter(|record| matches!(record.state, ObligationState::Pending))
        .map(|record| record.spec.id())
        .collect();
    discharge_all(&mut runtime, handle, &gapped_plan, &pending);
    assert!(matches!(
        runtime.seal_flow_completion(handle, solve_value()),
        Err(FlowSealError::UndischargedObligations)
    ));

    // A failed obligation — internal failure and cancellation alike:
    // never sealed, never warm.
    for class in [FlowFailureClass::Internal, FlowFailureClass::Cancelled] {
        let mut runtime = ObligationRuntime::default();
        let handle = runtime.install_flow_demand(&plan);
        let mut order = plan.work_order().to_vec();
        let failed = order.pop().expect("the plan has obligations");
        discharge_all(&mut runtime, handle, &plan, &order);
        runtime
            .start_flow_obligation(handle, failed)
            .expect("start");
        runtime
            .fail_flow_obligation(handle, failed, FlowFailure { class })
            .expect("a running obligation fails");
        assert!(matches!(
            runtime.seal_flow_completion(handle, solve_value()),
            Err(FlowSealError::UndischargedObligations)
        ));
    }

    // A partial replay (some obligations never discharged): never sealed.
    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    let partial_prefix: Vec<FlowObligationId> = {
        // Prefix only: every obligation's dependencies precede it in the
        // work order, so any prefix is a legal drive.
        let order = plan.work_order();
        order[..order.len() / 2].to_vec()
    };
    discharge_all(&mut runtime, handle, &plan, &partial_prefix);
    assert!(matches!(
        runtime.seal_flow_completion(handle, solve_value()),
        Err(FlowSealError::UndischargedObligations)
    ));

    // A stale basis still reaches the finalizer as a typed partial, and a
    // partial is never a warm candidate.
    let mut stale_request = base_request();
    stale_request.input_basis = test_input_basis(2);
    let stale_plan = fixture.build_plan(stale_request).expect("plans");
    let (runtime, handle, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, handle, &stale_plan, sealed);
    assert!(matches!(outcome, FlowSolveOutcome::Partial(_)));
    assert!(outcome.warm_candidate().is_none());
}

#[test]
fn stale_basis_or_foreign_contract_cannot_complete() {
    let (fixture, plan) = planned();

    // Every basis axis the finalize-time comparison covers: a plan rebuilt
    // from a request differing in any one of them must not complete against
    // a runtime's sealed artifact.
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
        let SemanticQueryKey::FlowReturn(key) = &mut result_contract.query else {
            unreachable!()
        };
        // The contract axis lives on the KEY: a key carrying a foreign
        // contract is a different demand identity.
        key.result_contract = foreign_result_contract(3);
        legs.push(("result contract", result_contract));

        legs.into_iter()
            .map(|(name, request)| (name, fixture.build_plan(request).expect("the demand plans")))
            .collect()
    };
    for (name, stale_plan) in &stale_plans {
        let (runtime, handle, sealed) = drive_to_completion(&plan);
        let outcome = finalize_flow_solve(&runtime, handle, stale_plan, sealed);
        assert!(
            matches!(outcome, FlowSolveOutcome::Partial(_)),
            "a stale {name} must not complete: {outcome:?}"
        );
        assert!(outcome.warm_candidate().is_none());
    }

    // A result contract foreign to the operation's registered contract must
    // not complete even when installed, sealed, and finalized consistently.
    let mut foreign_request = base_request();
    let SemanticQueryKey::FlowReturn(key) = &mut foreign_request.query else {
        unreachable!()
    };
    key.result_contract = foreign_result_contract(3);
    let foreign_plan = fixture
        .build_plan(foreign_request)
        .expect("the demand plans");
    let (foreign_runtime, foreign_handle, foreign_sealed) = drive_to_completion(&foreign_plan);
    let outcome = finalize_flow_solve(
        &foreign_runtime,
        foreign_handle,
        &foreign_plan,
        foreign_sealed,
    );
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
    let base = flow_operation_contract(SemanticQueryKeyTag::FlowReturn)
        .expect("FlowReturn is a registered flow operation")
        .clone();
    let base_id = flow_result_contract_id(&base);
    // Deterministic: the same closed contract mints the same identity.
    assert_eq!(base_id, flow_result_contract_id(&base));

    // A reordered closure list and a narrowed family list (the closed
    // mapping is order-bearing identity).
    let mut reordered = base.closures.to_vec();
    reordered.reverse();
    let reordered: &'static [FlowDomainClosure] = Box::leak(reordered.into_boxed_slice());
    let mut narrowed = base.closures.to_vec();
    narrowed[0] = FlowDomainClosure {
        families: &[FlowFactFamily::BindingSlot],
        ..narrowed[0].clone()
    };
    let narrowed: &'static [FlowDomainClosure] = Box::leak(narrowed.into_boxed_slice());

    let cases: Vec<(&str, FlowOperationContract)> = vec![
        (
            "role",
            FlowOperationContract {
                role: FlowOperationRole::SemanticSuboperation,
                ..base.clone()
            },
        ),
        (
            "status",
            FlowOperationContract {
                status: FlowOperationStatus::PendingReducer,
                ..base.clone()
            },
        ),
        (
            "domains",
            FlowOperationContract {
                closures: narrowed,
                ..base.clone()
            },
        ),
        (
            "domain order",
            FlowOperationContract {
                closures: reordered,
                ..base.clone()
            },
        ),
        (
            "finalizer",
            FlowOperationContract {
                result: FlowResultContractDescriptor {
                    finalizer: FlowFinalizerKind::TypedGapOnly,
                    ..base.result
                },
                ..base.clone()
            },
        ),
        (
            "accepted gaps",
            FlowOperationContract {
                result: FlowResultContractDescriptor {
                    accepted_gaps: &[],
                    ..base.result
                },
                ..base.clone()
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
    let full = plan.work_order().len() as u32;
    assert!(
        full > 20,
        "the fixture expands past the initial coverage + domain obligations"
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
    // base (family coverage + contract domains + additional), never the
    // expanded population.
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
        observed,
        11 + 9 + 20,
        "the counted base (11 coverage + 9 domains + 20 additional) trips before any concrete obligation is constructed"
    );
}

/// The no-flow allocation contract, at BOTH levels: a default runtime
/// reserves no demand storage, and REAL production dispatch — an ordinary
/// non-flow query and each pending typed-gap root — installs zero demands
/// and reserves zero demand capacity.
#[test]
fn unused_flow_runtime_reserves_no_demand_storage() {
    let runtime = ObligationRuntime::default();
    assert_eq!(runtime.flow_demand_count(), 0);
    assert_eq!(
        runtime.flow_demand_storage_capacity(),
        0,
        "a runtime that never served a flow demand reserves no demand storage"
    );

    let host = VerterHost::new_standalone(HostConfig::default());
    let analysis_context = || ProgramAnalysisContext {
        parse_env_hash: [0; 16],
        resolve_env_hash: [0; 16],
        type_env_hash: [0; 16],
        lib_env_hash: [0; 16],
        project_identity: 0,
        substitution: SubstitutionCanonicalHash::empty(),
    };
    // An ordinary non-flow query.
    let ordinary = {
        let graph = host.project_type_store().semantic_graph();
        let member = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        SemanticQueryKey::NormalizeUnion {
            members: Arc::from(vec![member].into_boxed_slice()),
        }
    };
    // The pending typed-gap roots.
    let pending_roots = [
        SemanticQueryKey::FlowNarrowingAt {
            point: ProgramPointId {
                canonical_id: Arc::from("/flow_solve_no_flow.ts"),
                offset: 0,
            },
            flow: FlowNarrowingKey::empty(),
            context: analysis_context(),
        },
        SemanticQueryKey::ContextualTypeAt {
            point: ProgramPointId {
                canonical_id: Arc::from("/flow_solve_no_flow.ts"),
                offset: 0,
            },
            contextual: ContextualTypingKey::empty(),
            context: analysis_context(),
        },
    ];
    let legs = [
        ("an ordinary non-flow query", ordinary),
        ("the FlowNarrowingAt pending root", pending_roots[0].clone()),
        (
            "the ContextualTypeAt pending root",
            pending_roots[1].clone(),
        ),
    ];
    for (what, key) in legs {
        let (count, capacity) = dispatch_flow_demand_footprint_for_tests(&host, key);
        assert_eq!(
            (count, capacity),
            (0, 0),
            "{what} installs no flow demand and reserves no demand capacity"
        );
    }
}

// ── Registry closure and the closed obligation universe ─────────────────

/// The registry is a CLOSED universe: every family of the vocabulary has
/// exactly one registered expansion route, and every declared family of
/// every registered contract has exactly one family-coverage obligation in
/// the plan, with each domain obligation depending on EXACTLY its
/// closure's coverage obligations.
#[test]
fn registry_declares_a_closed_universe() {
    // The family→route mapping is total over the vocabulary (a
    // wildcard-free match): pin every family's registered route so a
    // route-table edit is a test-visible change.
    let vocabulary = [
        FlowFactFamily::GraphEdge(FlowEdgeClass::ValueDef),
        FlowFactFamily::GraphEdge(FlowEdgeClass::PathWrite),
        FlowFactFamily::GraphEdge(FlowEdgeClass::EvalEffect),
        FlowFactFamily::GraphEdge(FlowEdgeClass::ControlRegion),
        FlowFactFamily::BindingSlot,
        FlowFactFamily::ReturnSite,
        FlowFactFamily::GuardPredicate,
        FlowFactFamily::CallSite,
        FlowFactFamily::ContextualTarget,
        FlowFactFamily::Capture,
        FlowFactFamily::SemanticRelation,
    ];
    for family in &vocabulary {
        let route = flow_family_route(family);
        // Every route resolves to a registered rule with a typed gap —
        // totality is by construction; the pin is on the mapping itself.
        assert!(
            matches!(
                route.accepted_gap,
                FlowGap::GuardNarrowing
                    | FlowGap::NominalRelation
                    | FlowGap::ClosureCapture
                    | FlowGap::AbruptCompletion
                    | FlowGap::UnmodeledExpression
            ),
            "every family route carries a typed gap: {family:?}"
        );
    }

    // Every registered contract: no duplicate domains, and the derived
    // families are exactly the closures' families.
    for tag in [
        SemanticQueryKeyTag::FlowReturn,
        SemanticQueryKeyTag::FlowNarrowingAt,
        SemanticQueryKeyTag::ContextualTypeAt,
        SemanticQueryKeyTag::ResolveCall,
        SemanticQueryKeyTag::Relate,
    ] {
        let contract = flow_operation_contract(tag).expect("registered");
        let domains: Vec<FlowDomain> = contract.required_domains().collect();
        let mut dedup = domains.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(
            domains.len(),
            dedup.len(),
            "{tag:?} declares no duplicate domain"
        );
        for closure in contract.closures {
            assert!(
                !closure.families.is_empty(),
                "every declared domain maps families"
            );
        }
    }

    // The plan carries the registry closure and the derived families.
    let (_fixture, plan) = planned();
    let contract = flow_operation_contract(SemanticQueryKeyTag::FlowReturn).expect("registered");
    assert_eq!(plan.registry_closure(), contract.closures);
    assert_eq!(
        plan.required_fact_families(),
        contract.required_fact_families().as_slice()
    );

    // Exactly ONE coverage obligation per required family, minted through
    // the family's registered expansion rule.
    let families = plan.required_fact_families();
    assert_eq!(plan.coverage_obligations().len(), families.len());
    for (coverage_id, family) in plan.coverage_obligations().iter().zip(families.iter()) {
        let coverage_spec = spec(&plan, *coverage_id);
        assert!(
            matches!(coverage_spec.basis(), FlowObligationBasis::FamilyCoverage { family: f } if f == family),
            "the coverage obligation names its family: {family:?}"
        );
        assert_eq!(
            coverage_spec.origin(),
            &FlowObligationOrigin::Expansion(flow_family_route(family).rule),
            "the coverage obligation rides the family's registered expansion rule"
        );
        assert!(
            coverage_spec.expected_dependencies().is_empty()
                && coverage_spec.expected_suboperations().is_empty(),
            "a coverage obligation discharges on enumeration alone"
        );
    }
    let coverage_specs: Vec<&FlowObligationSpec> = plan
        .obligation_specs()
        .iter()
        .filter(|spec| matches!(spec.basis(), FlowObligationBasis::FamilyCoverage { .. }))
        .collect();
    assert_eq!(
        coverage_specs.len(),
        families.len(),
        "exactly one coverage obligation per family — no more, no fewer"
    );

    // Every domain obligation depends on EXACTLY the coverage obligations
    // of the families its closure maps it to.
    for closure in contract.closures {
        let domain_spec = plan
            .obligation_specs()
            .iter()
            .find(|spec| {
                *spec.requirement()
                    == FlowRequirement {
                        operation: SemanticQueryKeyTag::FlowReturn,
                        requirement: FlowRequirementKind::Domain(closure.domain),
                    }
            })
            .expect("every closure domain has its obligation");
        let expected: Vec<FlowObligationId> = closure
            .families
            .iter()
            .map(|family| {
                let index = families
                    .iter()
                    .position(|candidate| candidate == family)
                    .expect("closure families are required families");
                plan.coverage_obligations()[index]
            })
            .collect();
        assert_eq!(
            domain_spec.expected_dependencies(),
            expected.as_slice(),
            "domain {:?} depends on exactly its closure's coverage obligations",
            closure.domain
        );
    }
}

/// A required family with zero concrete instances still gets its coverage
/// obligation — "proved empty" is a discharged enumeration, never a
/// forgotten family.
#[test]
fn family_coverage_is_explicit_even_when_empty() {
    let (_fixture, plan) = planned();
    for family in [FlowFactFamily::GuardPredicate, FlowFactFamily::Capture] {
        // The fixture has no guard and no capture subject.
        assert!(
            plan.obligation_specs().iter().all(|spec| !matches!(
                spec.basis(),
                FlowObligationBasis::Guard { .. } if family == FlowFactFamily::GuardPredicate
            )),
            "the fixture has no concrete guard"
        );
        assert!(
            plan.obligation_specs().iter().all(|spec| !matches!(
                spec.basis(),
                FlowObligationBasis::Capture { .. } if family == FlowFactFamily::Capture
            )),
            "the fixture has no concrete capture"
        );
        let coverage = plan
            .obligation_specs()
            .iter()
            .find(|spec| matches!(spec.basis(), FlowObligationBasis::FamilyCoverage { family: f } if *f == family))
            .expect("every required family has its coverage obligation even when empty");
        assert!(coverage.expected_dependencies().is_empty());
    }
    // And the solve completes — the empty families discharge their
    // coverage obligations like every other obligation.
    let (runtime, handle, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
    assert!(
        outcome.warm_candidate().is_some(),
        "proved-empty families never block completion: {outcome:?}"
    );
}

/// Every concrete call occurrence gets its OWN identity: one call
/// obligation per (expression site, call ordinal), with the dynamic
/// relation anchored on exactly its call occurrence.
#[test]
fn every_call_occurrence_has_its_own_identity() {
    let (_fixture, plan) = planned_rich();
    let calls: Vec<&FlowObligationSpec> = plan
        .obligation_specs()
        .iter()
        .filter(|spec| matches!(spec.basis(), FlowObligationBasis::CallSite { .. }))
        .collect();
    // The rich fixture's `pair(first(), second())` site carries three call
    // occurrences (pair, first, second) and `side(y)` one more.
    assert!(
        calls.len() >= 4,
        "the rich fixture plans one call obligation per occurrence: {}",
        calls.len()
    );
    // The multi-call site: distinct obligations per (site, ordinal).
    let mut by_site: std::collections::BTreeMap<usize, Vec<u32>> =
        std::collections::BTreeMap::new();
    for call in &calls {
        let FlowObligationBasis::CallSite {
            site, call_ordinal, ..
        } = call.basis()
        else {
            unreachable!()
        };
        by_site.entry(site.index()).or_default().push(*call_ordinal);
    }
    let (multi_site, ordinals) = by_site
        .iter()
        .max_by_key(|(_, ordinals)| ordinals.len())
        .expect("a call site exists");
    assert_eq!(
        ordinals.len(),
        3,
        "the `pair(first(), second())` site has one obligation per call ordinal"
    );
    let mut sorted = ordinals.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![0, 1, 2], "each ordinal is its own obligation");

    // Each call occurrence anchors exactly one dynamic relation, depending
    // on EXACTLY its own call obligation.
    let relations: Vec<&FlowObligationSpec> = plan
        .obligation_specs()
        .iter()
        .filter(|spec| matches!(spec.basis(), FlowObligationBasis::SemanticRelation { .. }))
        .collect();
    assert_eq!(relations.len(), calls.len());
    for call in &calls {
        let FlowObligationBasis::CallSite {
            site, call_ordinal, ..
        } = call.basis()
        else {
            unreachable!()
        };
        let anchored: Vec<&&FlowObligationSpec> = relations
            .iter()
            .filter(|relation| {
                matches!(
                    relation.basis(),
                    FlowObligationBasis::SemanticRelation { site: s, call_ordinal: o, .. }
                        if s == site && o == call_ordinal
                )
            })
            .collect();
        assert_eq!(anchored.len(), 1, "one relation per call occurrence");
        assert_eq!(
            anchored[0].expected_dependencies(),
            &[call.id()],
            "the relation depends on exactly its own call occurrence"
        );
        assert_eq!(
            anchored[0].expected_suboperations(),
            &[SemanticQueryKeyTag::Relate],
            "the relation consumes the registered Relate suboperation"
        );
    }
    let _ = multi_site;

    // The rich solve completes: every occurrence discharged under its own
    // identity.
    let (runtime, handle, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
    assert!(outcome.warm_candidate().is_some());
}

/// A required subject the structural authority cannot name — a nested
/// function's capture set — installs the family's accepted typed gap at
/// install, anchored on the nested function's real binding identity. It is
/// never omitted and never discharged, so the solve can never complete.
#[test]
fn unnameable_capture_subject_installs_the_family_typed_gap() {
    let fixture = flow_graph_fixture_for_tests(CAPTURE_FIXTURE_SOURCE, 11);
    let plan = fixture
        .build_plan(request_named("with_capture"))
        .expect("the capture fixture plans");
    let captures: Vec<&FlowObligationSpec> = plan
        .obligation_specs()
        .iter()
        .filter(|spec| matches!(spec.basis(), FlowObligationBasis::Capture { .. }))
        .collect();
    assert_eq!(
        captures.len(),
        1,
        "the nested function is one capture subject"
    );
    let FlowObligationBasis::Capture { identity, .. } = captures[0].basis() else {
        unreachable!()
    };
    // Anchored on the capturer's real FlowBindingIdentity.
    let identity = identity
        .as_ref()
        .expect("the nested function resolves an identity");
    assert_eq!(identity.name.as_ref(), "helper");

    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    let record = runtime
        .flow_obligations(handle)
        .expect("the demand is installed")
        .iter()
        .find(|record| record.spec.id() == captures[0].id())
        .expect("installed");
    assert_eq!(
        record.state,
        ObligationState::Gap(FlowGap::ClosureCapture),
        "the unnameable capture subject installs the family's accepted typed gap"
    );

    // Discharge everything else; the gapped subject still blocks sealing.
    let pending: Vec<FlowObligationId> = runtime
        .flow_obligations(handle)
        .expect("the demand is installed")
        .iter()
        .filter(|record| matches!(record.state, ObligationState::Pending))
        .map(|record| record.spec.id())
        .collect();
    discharge_all(&mut runtime, handle, &plan, &pending);
    assert!(
        matches!(
            runtime.seal_flow_completion(handle, solve_value()),
            Err(FlowSealError::UndischargedObligations)
        ),
        "a typed-gap subject can never be discharged, so the solve can never seal"
    );
}

/// The universe is closed: dropping ANY planned obligation's discharge —
/// a domain, a family-coverage obligation, a concrete fact, a call
/// ordinal, a guard, or a relation — prevents `Complete`.
#[test]
fn universe_mutation_prevents_complete() {
    let (_fixture, plan) = planned_rich();

    // The rich plan exercises every category.
    let one = |predicate: &dyn Fn(&FlowObligationSpec) -> bool, what: &str| -> FlowObligationId {
        plan.obligation_specs()
            .iter()
            .find(|spec| predicate(spec))
            .unwrap_or_else(|| panic!("the rich plan must contain {what}"))
            .id()
    };
    let domain = one(
        &|spec| matches!(spec.origin(), FlowObligationOrigin::ContractDomain),
        "a domain",
    );
    let coverage = plan.coverage_obligations()[0];
    let binding = one(
        &|spec| matches!(spec.basis(), FlowObligationBasis::Binding { .. }),
        "a binding fact",
    );
    let edge = one(
        &|spec| matches!(spec.basis(), FlowObligationBasis::Edge { .. }),
        "an edge fact",
    );
    let call = one(
        &|spec| {
            matches!(
                spec.basis(),
                FlowObligationBasis::CallSite {
                    call_ordinal: 1,
                    ..
                }
            )
        },
        "the second call ordinal",
    );
    let guard = one(
        &|spec| matches!(spec.basis(), FlowObligationBasis::Guard { .. }),
        "a guard",
    );
    let relation = one(
        &|spec| matches!(spec.basis(), FlowObligationBasis::SemanticRelation { .. }),
        "a relation",
    );
    let contextual = one(
        &|spec| matches!(spec.basis(), FlowObligationBasis::ContextualTarget { .. }),
        "a contextual target",
    );

    // The skip set: the held obligation plus everything that transitively
    // depends on it (those discharges are dependency-blocked, not skipped
    // silently).
    let skip_closure = |held: FlowObligationId| -> Vec<FlowObligationId> {
        let mut skip = vec![held];
        loop {
            let mut grew = false;
            for spec in plan.obligation_specs() {
                if !skip.contains(&spec.id())
                    && spec
                        .expected_dependencies()
                        .iter()
                        .any(|dependency| skip.contains(dependency))
                {
                    skip.push(spec.id());
                    grew = true;
                }
            }
            if !grew {
                return skip;
            }
        }
    };

    for (what, held) in [
        ("domain", domain),
        ("family coverage", coverage),
        ("binding fact", binding),
        ("edge fact", edge),
        ("call ordinal", call),
        ("guard", guard),
        ("relation", relation),
        ("contextual target", contextual),
    ] {
        let skip = skip_closure(held);
        let mut runtime = ObligationRuntime::default();
        let handle = runtime.install_flow_demand(&plan);
        let order: Vec<FlowObligationId> = plan
            .work_order()
            .iter()
            .copied()
            .filter(|id| !skip.contains(id))
            .collect();
        discharge_all(&mut runtime, handle, &plan, &order);
        assert!(
            matches!(
                runtime.seal_flow_completion(handle, solve_value()),
                Err(FlowSealError::UndischargedObligations)
            ),
            "dropping the {what} obligation must prevent sealing"
        );
        assert!(
            matches!(
                runtime.observe_flow_iteration(handle, false),
                Err(FlowTransitionError::IllegalTransition)
            ),
            "dropping the {what} obligation keeps the frontier open"
        );
        assert!(
            matches!(
                runtime
                    .flow_obligations(handle)
                    .expect("the demand is installed")
                    .iter()
                    .find(|record| record.spec.id() == held)
                    .map(|record| &record.state),
                Some(ObligationState::Pending)
            ),
            "the dropped {what} obligation was never discharged"
        );
    }

    // And the intact universe completes — the mutation assertions above
    // are not vacuous.
    let (runtime, handle, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
    assert!(outcome.warm_candidate().is_some());
}

// ── Source identity axes ────────────────────────────────────────────────

/// The parse identity and the runtime language row are demand-basis axes:
/// changing ONLY the parse key (same language, same tagged body hashes,
/// same canonical) or the language row rebinds the basis, and a proof
/// sealed under one basis can never finalize across either boundary.
#[test]
fn parse_key_and_language_axes_rebind_the_demand_basis() {
    let base_fixture = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 7);
    // Only the parse key changes: the same function, the same tagged body
    // hashes, the same language row — but a source whose exact parse
    // identity differs (a trailing comment changes the content axis the
    // parse key binds, and nothing else).
    let commented = format!("{FIXTURE_SOURCE}\n// identity-only comment\n");
    let parse_key_fixture = flow_graph_fixture_for_tests(&commented, 7);
    // The language row changes (a real tsx parse identity of the same
    // source).
    let tsx_fixture = flow_graph_fixture_for_tests_with_language(
        FIXTURE_SOURCE,
        7,
        FileLanguage::script(ScriptSourceType::Tsx),
    );

    let plan_base = base_fixture.build_plan(base_request()).expect("plans");
    let plan_parse = parse_key_fixture.build_plan(base_request()).expect("plans");
    let plan_tsx = tsx_fixture.build_plan(base_request()).expect("plans");
    assert_ne!(
        plan_base.basis(),
        plan_parse.basis(),
        "a different parse key is a different demand basis"
    );
    assert_ne!(
        plan_base.basis(),
        plan_tsx.basis(),
        "a different language row is a different demand basis"
    );

    // A proof sealed under the base basis finalizes against its own plan...
    let (runtime, handle, sealed) = drive_to_completion(&plan_base);
    let outcome = finalize_flow_solve(&runtime, handle, &plan_base, sealed);
    assert!(outcome.warm_candidate().is_some());

    // ...but never across the parse-key boundary...
    let (runtime, handle, sealed) = drive_to_completion(&plan_base);
    let outcome = finalize_flow_solve(&runtime, handle, &plan_parse, sealed);
    assert!(
        matches!(
            outcome,
            FlowSolveOutcome::Partial(ref partial)
                if partial.reason == FlowPartialReason::StaleBasis
        ),
        "a proof cannot finalize across a parse-key boundary: {outcome:?}"
    );
    assert!(outcome.warm_candidate().is_none());

    // ...and never across the language boundary.
    let (runtime, handle, sealed) = drive_to_completion(&plan_base);
    let outcome = finalize_flow_solve(&runtime, handle, &plan_tsx, sealed);
    assert!(
        matches!(
            outcome,
            FlowSolveOutcome::Partial(ref partial)
                if partial.reason == FlowPartialReason::StaleBasis
        ),
        "a proof cannot finalize across a language boundary: {outcome:?}"
    );
    assert!(outcome.warm_candidate().is_none());
}

// ── Query/graph identity coherence ──────────────────────────────────────

/// The query must NAME the bound graph's function and parse environment:
/// every identity axis present in both the query's `FlowReturnKey` and the
/// bound graph's `FlowSliceFunctionKey` — canonical identity, owner,
/// merged-symbol name, symbol space, function part, overload ordinal, and
/// the parse-environment hash — must match. A mismatch on ANY one axis is
/// a typed planning error: a demand planned over a graph its query does
/// not name would discharge graph A's obligations against a query naming
/// function B and seal a `Complete` for the wrong body.
#[test]
fn planning_rejects_a_query_that_does_not_name_the_bound_graph() {
    let fixture = flow_graph_fixture_for_tests(FIXTURE_SOURCE, 7);

    // The matched control plans.
    assert!(
        fixture.build_plan(base_request()).is_ok(),
        "the query naming the bound graph's function plans"
    );

    let mismatched = |mutate: &dyn Fn(&mut FlowReturnKey)| {
        let mut request = base_request();
        let SemanticQueryKey::FlowReturn(key) = &mut request.query else {
            unreachable!()
        };
        mutate(key);
        fixture.build_plan(request)
    };

    assert!(
        matches!(
            mismatched(
                &|key| key.function.declaration_slot.defining_canonical = Arc::from("/foreign.ts")
            ),
            Err(FlowDemandPlanError::BasisKeyMismatch)
        ),
        "a foreign canonical identity must not plan"
    );
    assert!(
        matches!(
            mismatched(&|key| key.function.declaration_slot.owner =
                verter_type_expr::TopLevelOwnerId::module(1)),
            Err(FlowDemandPlanError::BasisKeyMismatch)
        ),
        "a foreign owner must not plan"
    );
    assert!(
        matches!(
            mismatched(&|key| key.function.declaration_slot.merged_symbol_name = Arc::from("other")),
            Err(FlowDemandPlanError::BasisKeyMismatch)
        ),
        "a foreign merged-symbol name must not plan"
    );
    assert!(
        matches!(
            mismatched(
                &|key| key.function.declaration_slot.symbol_space = SemanticSymbolSpace::Type
            ),
            Err(FlowDemandPlanError::BasisKeyMismatch)
        ),
        "a foreign symbol space must not plan"
    );
    assert!(
        matches!(
            mismatched(&|key| key.function.function_part =
                verter_type_expr::facts::FunctionPartIdentity::Initializer),
            Err(FlowDemandPlanError::BasisKeyMismatch)
        ),
        "a foreign function part must not plan"
    );
    assert!(
        matches!(
            mismatched(&|key| key.function.overload_ordinal = 1),
            Err(FlowDemandPlanError::BasisKeyMismatch)
        ),
        "a foreign overload ordinal must not plan"
    );
    assert!(
        matches!(
            mismatched(&|key| key.context.parse_env_hash = [9; 16]),
            Err(FlowDemandPlanError::BasisKeyMismatch)
        ),
        "a foreign parse environment must not plan"
    );
}

// ── Closure-expression capture subjects ─────────────────────────────────

/// An arrow returning an outer parameter: the skeleton authority records
/// the capture on the closure's expression site.
const ARROW_CAPTURE_FIXTURE_SOURCE: &str = r#"
function arrow_capture(x) {
  return () => x;
}
"#;

/// A function expression capturing an outer `let`.
const EXPRESSION_CAPTURE_FIXTURE_SOURCE: &str = r#"
function expression_capture(x) {
  let y = x;
  return function () { return y; };
}
"#;

/// A nested double closure: the inner arrow's free read is a capture of
/// the outer arrow's frame too, so the enclosing function's closure site
/// captures the parameter transitively.
const DOUBLE_CLOSURE_FIXTURE_SOURCE: &str = r#"
function double_capture(x) {
  return () => () => x;
}
"#;

/// A closure that captures nothing: the capture family stays proved-empty.
const NO_CAPTURE_FIXTURE_SOURCE: &str = r#"
function no_capture(x) {
  return () => 1;
}
"#;

/// A closure capturing a destructured parameter: the binding is real but
/// the cross-frame inventory cannot name it — the family's accepted typed
/// gap, never silence.
const DESTRUCTURED_CAPTURE_FIXTURE_SOURCE: &str = r#"
function destructured_capture({a}) {
  return () => a;
}
"#;

/// The concrete capture subjects of one fixture's plan: the real
/// cross-frame identities of the captured bindings.
fn concrete_captures(
    source: &str,
    body_hash_tag: u8,
    name: &str,
) -> Vec<verter_semantic::analysis::function_program::FlowBindingIdentity> {
    let fixture = flow_graph_fixture_for_tests(source, body_hash_tag);
    let plan = fixture
        .build_plan(request_named(name))
        .expect("the capture fixture plans");
    plan.obligation_specs()
        .iter()
        .filter_map(|spec| match spec.basis() {
            FlowObligationBasis::CapturedBinding { identity, .. } => Some(identity.clone()),
            _ => None,
        })
        .collect()
}

/// Closure expressions are CONCRETE capture subjects: one capture
/// obligation per (closure site, captured binding), carrying the captured
/// binding's real cross-frame identity — never the falsely-empty family
/// the old planner "proved" by examining no closure site at all.
#[test]
fn closure_expression_captures_are_concrete_subjects() {
    use verter_semantic::analysis::function_program::FunctionBindingKind;

    // An arrow returning the outer parameter captures exactly `x`.
    let captures = concrete_captures(ARROW_CAPTURE_FIXTURE_SOURCE, 21, "arrow_capture");
    assert_eq!(captures.len(), 1, "one (closure site, binding) subject");
    assert_eq!(captures[0].name.as_ref(), "x");
    assert_eq!(captures[0].kind, FunctionBindingKind::Param);
    assert_eq!(
        captures[0].defining_function.declaration.name.as_ref(),
        "arrow_capture",
        "the capture identity names the defining frame"
    );

    // A function expression captures the outer `let`, not the parameter
    // it was initialized from.
    let captures = concrete_captures(EXPRESSION_CAPTURE_FIXTURE_SOURCE, 22, "expression_capture");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].name.as_ref(), "y");
    assert_eq!(captures[0].kind, FunctionBindingKind::Let);

    // A nested double closure captures the outer parameter transitively:
    // the inner arrow's free read is free in the middle frame too.
    let captures = concrete_captures(DOUBLE_CLOSURE_FIXTURE_SOURCE, 23, "double_capture");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].name.as_ref(), "x");
    assert_eq!(captures[0].kind, FunctionBindingKind::Param);

    // The concrete subjects are dischargeable: the arrow-capture solve
    // drives to a sealed completion.
    let fixture = flow_graph_fixture_for_tests(ARROW_CAPTURE_FIXTURE_SOURCE, 21);
    let plan = fixture
        .build_plan(request_named("arrow_capture"))
        .expect("the capture fixture plans");
    let (runtime, handle, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
    assert!(
        outcome.warm_candidate().is_some(),
        "a solve whose capture subjects are concrete discharges and completes: {outcome:?}"
    );
}

/// A no-capture closure yields the family's explicit empty-coverage
/// marker ONLY — no concrete subject, no gap — and the solve completes.
#[test]
fn a_no_capture_closure_proves_the_family_empty() {
    let fixture = flow_graph_fixture_for_tests(NO_CAPTURE_FIXTURE_SOURCE, 24);
    let plan = fixture
        .build_plan(request_named("no_capture"))
        .expect("the no-capture fixture plans");
    assert!(
        plan.obligation_specs().iter().all(|spec| !matches!(
            spec.basis(),
            FlowObligationBasis::CapturedBinding { .. } | FlowObligationBasis::Capture { .. }
        )),
        "a no-capture closure has no concrete capture subject and no gap"
    );
    assert!(
        plan.obligation_specs().iter().any(|spec| matches!(
            spec.basis(),
            FlowObligationBasis::FamilyCoverage { family } if *family == FlowFactFamily::Capture
        )),
        "the capture family's coverage obligation is the explicit proved-empty marker"
    );
    let (runtime, handle, sealed) = drive_to_completion(&plan);
    let outcome = finalize_flow_solve(&runtime, handle, &plan, sealed);
    assert!(
        outcome.warm_candidate().is_some(),
        "the proved-empty capture family never blocks completion: {outcome:?}"
    );
}

/// A captured binding the cross-frame inventory cannot name (a
/// destructured parameter) installs the family's accepted typed gap —
/// anchored on the resolved lexical binding — never silence.
#[test]
fn unnameable_captured_binding_installs_the_family_typed_gap() {
    let fixture = flow_graph_fixture_for_tests(DESTRUCTURED_CAPTURE_FIXTURE_SOURCE, 25);
    let plan = fixture
        .build_plan(request_named("destructured_capture"))
        .expect("the destructured-capture fixture plans");
    let gaps: Vec<&FlowObligationSpec> = plan
        .obligation_specs()
        .iter()
        .filter(|spec| matches!(spec.basis(), FlowObligationBasis::Capture { .. }))
        .collect();
    assert_eq!(gaps.len(), 1, "exactly the destructured capture gaps");
    let FlowObligationBasis::Capture { identity, .. } = gaps[0].basis() else {
        unreachable!()
    };
    assert!(
        identity.is_none(),
        "the destructured binding has no cross-frame identity"
    );

    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand(&plan);
    let record = runtime
        .flow_obligations(handle)
        .expect("the demand is installed")
        .iter()
        .find(|record| record.spec.id() == gaps[0].id())
        .expect("installed");
    assert_eq!(
        record.state,
        ObligationState::Gap(FlowGap::ClosureCapture),
        "the unnameable captured binding installs the family's accepted typed gap"
    );
}
