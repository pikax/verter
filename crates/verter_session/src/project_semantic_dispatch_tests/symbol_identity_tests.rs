use super::carrier_head_resolution_tests::bare_ref_carrier;
use super::carrier_head_resolution_tests::file_scope;
use super::carrier_head_resolution_tests::host;
use super::carrier_head_resolution_tests::import_type_carrier;
use super::carrier_head_resolution_tests::upsert_ts;
use std::sync::Arc;
use verter_session_query::inputs::budget::BudgetDomain;
use verter_session_query::inputs::budget::BudgetExceededFailure;
use verter_type_engine::project_semantic_dispatch::symbol_identity::*;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::request_context::RequestContext;
use verter_type_engine::request_context::RequestContextGuard;
use verter_type_engine::semantic_query::PartialReasonSet;
use verter_type_engine::semantic_query::QueryError;
use verter_type_engine::semantic_query::SemanticNodeData;
use verter_type_expr::PropCallableRoleUnresolvedReason;
use verter_type_expr::ResolvedSymbolIdentity;

fn expected() -> ResolvedSymbolIdentity {
    ResolvedSymbolIdentity {
        canonical_id: Arc::from("/node_modules/svelte/index.d.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        symbol: Arc::from("Snippet"),
    }
}

fn demand_error(error: QueryError) -> SymbolIdentityDemandOutcome {
    let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let node = dispatch
        .graph()
        .intern_node(SemanticNodeData::Opaque(error));
    dispatch.demand_symbol_identity(node, &[expected()])
}

#[test]
fn partial_identity_reasons_are_typed_and_carry_no_node() {
    assert_eq!(
        demand_error(QueryError::RecursiveRef {
            name: Arc::from("Cycle"),
            args: std::sync::Arc::from([]),
        }),
        SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::Cycle)
    );
    assert_eq!(
        demand_error(QueryError::BudgetExceeded(BudgetExceededFailure {
            domain: BudgetDomain::ProjectionOperation,
            limit: 1,
            actual: 2,
            context: "identity-demand".to_string(),
        })),
        SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::BudgetExceeded)
    );
    assert_eq!(
        demand_error(QueryError::Miss),
        SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::MissingDependency)
    );
}

/// `Cancelled` / `UnstableState` carriers keep their DEDICATED partial
/// classes through the shared carrier classification — the same bits the
/// runtime-side classifier returns for the same carriers — so consumers
/// that branch on the exact class (`partial_failure`'s Cancelled /
/// UnstableState arms, the connected-limit cancel fast path) observe the
/// operational cause rather than the generic semantic-fault class. The
/// coarse role outcome stays `Fault` (the role enum has no cancel arm).
#[test]
fn cancelled_and_unstable_carriers_keep_dedicated_partial_classes() {
    assert_eq!(
        query_error_partial_reasons(&QueryError::Cancelled),
        PartialReasonSet::CANCELLED,
        "a cancelled carrier must classify as CANCELLED, not SEMANTIC_QUERY_FAULT"
    );
    assert_eq!(
        query_error_partial_reasons(&QueryError::UnstableState { attempts: 3 }),
        PartialReasonSet::UNSTABLE_STATE,
        "a torn-state carrier must classify as UNSTABLE_STATE, not SEMANTIC_QUERY_FAULT"
    );
    assert_eq!(
        demand_error(QueryError::Cancelled),
        SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::Fault),
        "the coarse role classification keeps its Fault arm"
    );
}

#[test]
fn unsupported_and_work_limited_identity_demands_fail_closed() {
    let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let raw = dispatch.graph().intern_node(SemanticNodeData::RawFallback {
        value: verter_type_expr::UnknownValue::wire_opaque("unsupported"),
    });
    assert_eq!(
        dispatch.demand_symbol_identity(raw, &[expected()]),
        SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::Unsupported)
    );

    let terminal = dispatch.graph().intern_node(SemanticNodeData::Primitive(
        verter_type_engine::semantic_query::PrimitiveKind::String,
    ));
    let alias = dispatch
        .graph()
        .intern_node(SemanticNodeData::Alias(terminal));
    dispatch.set_connected_limits_for_tests(
        0,
        verter_type_engine::project_semantic_dispatch::connected_demand::MAX_CONNECTED_QUERY_DEPTH,
    );
    let _scope = verter_type_engine::request_context::ColdComputeCompletenessScope::enter();
    assert_eq!(
        dispatch.demand_symbol_identity(alias, &[expected()]),
        SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::WorkLimitExceeded)
    );
    assert!(
        verter_type_engine::request_context::current_cold_compute_completeness().is_partial(),
        "a partial identity demand must refuse warm admission"
    );
}

/// Mutation recipe: map the partial observation returned by
/// `resolve_carrier_subject_node_capturing_suppress` to `Fault`; the
/// work-limited assertion must fail while the missing control stays green.
#[test]
fn real_carrier_path_preserves_partial_reason_classes() {
    let host = host();
    upsert_ts(
        &host,
        "/identity.ts",
        "export type Target = { value: string };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/identity.ts");

    let missing = bare_ref_carrier(&dispatch, "Missing", scope.clone(), &[]);
    assert_eq!(
        dispatch.demand_symbol_identity(missing, &[expected()]),
        SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::MissingDependency),
        "an unresolved real carrier must preserve MissingDependency"
    );

    let target = bare_ref_carrier(&dispatch, "Target", scope, &[]);
    dispatch.set_connected_limits_for_tests(1, u16::MAX);
    assert_eq!(
        dispatch.demand_symbol_identity(target, &[expected()]),
        SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::WorkLimitExceeded),
        "a nested real-carrier query must preserve its work-limit reason"
    );

    let limited_host = crate::VerterHost::new_standalone(crate::types::HostConfig {
        projection_op_budget: 1,
        ..crate::types::HostConfig::default()
    });
    upsert_ts(
        &limited_host,
        "/dep.ts",
        "export namespace Surface { export type Target = { value: string } }\n",
    );
    upsert_ts(&limited_host, "/limited.ts", "export type Seed = string;\n");
    let limited_dispatch = ProjectSemanticDispatch::new(&limited_host);
    let limited_scope = file_scope(&limited_dispatch, "/limited.ts");
    let import_carrier = import_type_carrier(
        &limited_dispatch,
        "./dep",
        &["Surface", "Target"],
        &[],
        false,
        limited_scope,
    );
    let request = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/limited.ts"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        1,
    );
    let _request_guard = RequestContextGuard::install(request);
    assert_eq!(
        limited_dispatch.demand_symbol_identity(import_carrier, &[expected()]),
        SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::WorkLimitExceeded),
        "a projection-work-truncated import carrier must preserve WorkLimitExceeded"
    );
}
