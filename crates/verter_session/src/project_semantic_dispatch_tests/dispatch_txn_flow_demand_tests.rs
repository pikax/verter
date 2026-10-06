//! Flow-demand carrier and obligation-ledger transitions driven through a
//! session-built flow-graph fixture.

use std::sync::Arc;
use verter_type_engine::project_semantic_dispatch::dispatch_txn::*;
use verter_type_engine::semantic_query::FlowReturnFailure;

/// The flow-demand carriers: the in-flight flow frame and the deferred
/// SCC member each carry the demand's `FlowDemandCarrier` (handle + plan +
/// provenance) — the member's demand SURVIVES the pop so the component
/// close finalizes the member against exactly its own demand. The slot
/// defaults to `None` (a demand the planner refused installs nothing) and
/// round-trips a carrier when set.
#[test]
fn flow_demand_carriers_default_none_and_round_trip() {
    use crate::for_tests::flow_graph_fixture_for_tests;
    use verter_session_query::flow::policy::{FlowReturnPolicy, NullabilityPolicy};
    use verter_type_engine::project_semantic_dispatch::flow_solve::{
        flow_return_result_contract_id, FlowDemandRequest, FlowResourcePolicy,
    };
    use verter_type_engine::semantic_query::{
        CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowInputContext, FlowReturnContext,
        FlowReturnKey, ReturnProjectionDemand, SemanticQueryKey,
    };

    let fixture = flow_graph_fixture_for_tests("function carry_me(x) { return x; }\n", 31);
    let query = SemanticQueryKey::FlowReturn(Box::new(FlowReturnKey {
        function: FlowFunctionSlotIdentity {
            declaration_slot:
                verter_type_engine::semantic_query::ResolvedDeclSlotIdentity::value_slot(
                    Arc::from("/flow_solve_fixture.ts"),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    Arc::from("carry_me"),
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
        result_contract: flow_return_result_contract_id(),
    }));
    let provenance = verter_type_engine::project_semantic_dispatch::dispatch_txn::flow_obligation_state::FlowEvaluationProvenance::new(7, 3, 5, 0);
    let plan = fixture
        .build_plan(FlowDemandRequest {
            ancestry: Default::default(),
            query,
            input_basis: verter_identity::identity::InputBasisId::from_canonical(&provenance),
            resources: FlowResourcePolicy::default(),
            additional_requirements: Arc::from([]),
        })
        .expect("the carrier fixture plans");
    let mut runtime = ObligationRuntime::default();
    let carrier =
        verter_type_engine::project_semantic_dispatch::dispatch_txn::flow_obligation_state::FlowDemandCarrier {
            handle: runtime.install_flow_demand(&plan),
            plan: Arc::new(plan),
            provenance,
        };

    // The in-flight frame carrier.
    let mut frame = FlowReturnFrameState::default();
    assert!(
        frame.flow_demand.is_none(),
        "a fresh flow frame carries no demand carrier"
    );
    frame.flow_demand = Some(carrier.clone());
    assert_eq!(
        frame.flow_demand.as_ref().map(|carrier| carrier.handle),
        Some(carrier.handle),
        "the frame carrier round-trips its handle"
    );

    // The deferred SCC member carrier.
    let mut pending = FlowReturnPendingState {
        plan_refusal: None,
        outcome: FlowReturnPendingOutcome::NoValue {
            failure: FlowReturnFailure::Budget(
                verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
            ),
            degradation: None,
        },
        inline_flight: None,
        holds: Vec::new(),
        self_roots: Vec::new(),
        materialized: verter_type_engine::semantic_query::demand::MaterializedSet::default(),
        fresh_seed: false,
        flow_demand: None,
        discharge: None,
        provenance,
    };
    assert!(
        pending.flow_demand.is_none(),
        "a deferred flow member carries no demand carrier by default"
    );
    pending.flow_demand = Some(carrier.clone());
    assert_eq!(
        pending.flow_demand.as_ref().map(|carrier| carrier.handle),
        Some(carrier.handle),
        "the pending carrier round-trips its handle"
    );
}

/// A demand with ZERO installed obligations is an EMPTY proof universe, not
/// a trivially proved one: `iter().all(Discharged)` is vacuously true over
/// it, so without an explicit emptiness refusal the demand would pass the
/// convergence gate on its first observation and seal a completion whose
/// `proofs` slice is empty — an evidence-free artifact from the sole
/// warm-admission authority. No constructible `FlowDemandPlan` plans an
/// empty spec set (family-coverage + domain obligations come from the
/// closed contract registry: "proved empty" is a DISCHARGED obligation,
/// never an absent one), so the state is reachable only through the
/// test-only installer — and both runtime gates must refuse it fail-closed.
#[test]
fn zero_obligation_demand_never_converges_or_seals() {
    use crate::for_tests::flow_graph_fixture_for_tests;
    use verter_session_query::flow::policy::{FlowReturnPolicy, NullabilityPolicy};
    use verter_type_engine::project_semantic_dispatch::dispatch_txn::flow_obligation_state::{
        FlowSealError, FlowTransitionError,
    };
    use verter_type_engine::project_semantic_dispatch::flow_solve::{
        flow_return_result_contract_id, FlowDemandRequest, FlowResourcePolicy,
    };
    use verter_type_engine::semantic_query::{
        CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowInputContext, FlowReturnContext,
        FlowReturnKey, ReturnProjectionDemand, SemanticQueryKey,
    };

    let fixture = flow_graph_fixture_for_tests("function seal_me(x) { return x; }\n", 33);
    let query = SemanticQueryKey::FlowReturn(Box::new(FlowReturnKey {
        function: FlowFunctionSlotIdentity {
            declaration_slot:
                verter_type_engine::semantic_query::ResolvedDeclSlotIdentity::value_slot(
                    Arc::from("/flow_solve_fixture.ts"),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    Arc::from("seal_me"),
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
        result_contract: flow_return_result_contract_id(),
    }));
    let provenance = verter_type_engine::project_semantic_dispatch::dispatch_txn::flow_obligation_state::FlowEvaluationProvenance::new(11, 3, 5, 0);
    let plan = fixture
        .build_plan(FlowDemandRequest {
            ancestry: Default::default(),
            query,
            input_basis: verter_identity::identity::InputBasisId::from_canonical(&provenance),
            resources: FlowResourcePolicy::default(),
            additional_requirements: Arc::from([]),
        })
        .expect("the fixture plans");
    assert!(
        !plan.obligation_specs().is_empty(),
        "the production planner never yields an empty obligation set — the \
         empty demand below is reachable only through the test installer"
    );

    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand_without_obligations_for_tests(&plan);

    // Convergence is never observed over an EMPTY obligation universe: the
    // all-discharged gate must not pass by vacuous truth.
    assert!(
        matches!(
            runtime.observe_flow_iteration(handle, false),
            Err(FlowTransitionError::IllegalTransition)
        ),
        "a zero-obligation demand must not converge — vacuous all-discharged \
         is not a discharged universe"
    );

    // The seal refuses the same universe: no evidence-free completion mints.
    let graph = verter_type_engine::semantic_query_memo::SemanticGraphStore::new();
    let number = graph.intern_node(
        verter_type_engine::semantic_query::SemanticNodeData::Primitive(
            verter_type_engine::semantic_query::PrimitiveKind::Number,
        ),
    );
    let value = verter_type_engine::semantic_query::FlowReturnResult::new(
        &graph,
        number,
        verter_session_query::flow::completion::NormalCompletion::minted_for_fixture(false),
        None,
    );
    assert!(
        matches!(
            runtime.seal_flow_completion(handle, value),
            Err(FlowSealError::UndischargedObligations)
        ),
        "a zero-obligation demand must never seal — an empty proofs slice is \
         evidence-free, not trivially proved"
    );
}
