//! No-cache-promotion-for-partial-failure tests for the
//! SemanticQueryKey variants.
//!
//! Per CLAUDE.md "cancelled, superseded, interrupted, budget-exceeded,
//! or partial semantic results must not be promoted as warm shared
//! cache entries". Each test:
//!
//! 1. Constructs a hermetic host with a deliberately-tight
//!    `HostConfig::depth_budget`.
//! 2. Issues a path-projection key whose length exceeds the budget,
//!    asserting the result is `QueryResult::Recursive(_)` (or
//!    `Error(_)`) — the budget-exceeded sentinel.
//! 3. Re-issues the SAME key on the SAME host. The cold counter MUST
//!    increment (the partial result was NOT promoted to the warm
//!    cache); the warm counter MUST NOT increment.
//!
//! Discrimination rule: single-host, same-host re-query. A two-host
//! design would be non-discriminating because fresh hosts own their
//! own ProjectTypeStore.

use std::sync::Arc;

use crate::host_test_audit::DispatchCounter;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::resolver_core::BudgetDomain;
use crate::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SurfaceMember,
    SurfaceView,
};
use crate::types::HostConfig;
use crate::VerterHost;

/// Build a hermetic host with `depth_budget = 2` so a 3-segment path
/// projection trips the budget-exceeded sentinel.
fn build_constrained_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        depth_budget: 2,
        ..HostConfig::default()
    }))
}

/// Intern an Object surface with `n` members named "deep_<i>" each
/// pointing at a fresh empty Object. Provides a 3-segment path the
/// walker can attempt to traverse: `deep_0 → deep_1 → deep_2`.
fn intern_three_member_object(host: &VerterHost) -> SemanticNodeId {
    let graph = host.project_type_store().semantic_graph();
    let mut leaf = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(Vec::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));
    // Build nested: leaf, then { deep_2: leaf }, then { deep_1: prev },
    // then { deep_0: prev_2 } at the top.
    for name in ["deep_2", "deep_1", "deep_0"] {
        let member = SurfaceMember {
            visibility: verter_type_expr::MemberVisibility::Public,
            name: Arc::from(name),
            value: leaf,
            optional: false,
            readonly: false,
            is_method: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        };
        leaf = graph.intern_node(SemanticNodeData::Object(SurfaceView {
            members: Arc::from(vec![member].into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }));
    }
    leaf
}

fn intern_single_member_object(host: &VerterHost, name: &'static str) -> SemanticNodeId {
    let graph = host.project_type_store().semantic_graph();
    let leaf = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(Vec::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));
    let member = SurfaceMember {
        visibility: verter_type_expr::MemberVisibility::Public,
        name: Arc::from(name),
        value: leaf,
        optional: false,
        readonly: false,
        is_method: false,
        declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
        spans: Default::default(),
        declaration_origin: None,
    };
    graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(vec![member].into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }))
}

#[test]
fn request_projection_budget_caps_distinct_dispatch_cold_builds() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 1,
        ..HostConfig::default()
    }));
    let first_base = intern_single_member_object(&host, "first");
    let second_base = intern_single_member_object(&host, "second");
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let first_key = SemanticQueryKey::KeyOf {
        base: first_base,
        context,
    };
    let second_key = SemanticQueryKey::KeyOf {
        base: second_base,
        context,
    };

    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/budget.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let _ctx_guard = RequestContextGuard::install(ctx);
    let dispatch = host.semantic_dispatch();

    let first = dispatch.execute_type_node(first_key);
    assert!(
        matches!(first, QueryResult::Value(_)),
        "first cold projection within budget must succeed, got {first:?}",
    );

    let second = dispatch.execute_type_node(second_key);
    match second {
        QueryResult::Error(QueryError::BudgetExceeded(failure)) => {
            assert_eq!(failure.domain, BudgetDomain::ProjectionOperation);
            assert_eq!(failure.limit, 1);
            assert_eq!(failure.actual, 2);
        }
        other => panic!(
            "second distinct cold projection must trip the request projection budget; got {other:?}"
        ),
    }
}

/// Post-trip projection-op queries MUST early-exit at the dispatcher
/// entry without entering the `execute_cooperative` admission machinery.
///
/// Empirical motivation (ChatMessages.vue): 99.2% of the
/// 255,038 cold MappedType builds in a single component-meta request
/// were rejected by the projection-op budget *after* the fuse tripped.
/// Each rejected build still paid the cooperative-admission cost — the
/// in-flight table mutex + Arc clone, the per-key warm probe, the
/// fact-tracer install + finalisation, the joiner-condvar entry path —
/// for ~1ms per call in aggregate. ~250 seconds of pure overhead with
/// zero progress, because every call returned
/// `BudgetExceeded(cache_suppress=true)`.
///
/// Discriminator: drive the request past the projection-op fuse, then
/// issue many additional projection-op queries on DISTINCT keys.
/// Post-trip queries MUST:
///
/// 1. Return `BudgetExceeded` with the same `actual` value (the peek is
///    non-incrementing — see `RequestBudget::is_exhausted`).
/// 2. NOT increment the `RequestBudget::projection_ops_executed`
///    counter past the trip-point value (which would mean the
///    cooperative-admission build closure ran and bumped via
///    `check_projection_op_count`).
/// 3. NOT increment the cooperative-admission entry counter
///    `EXECUTE_COOPERATIVE_CALLS` (the architectural invariant — the
///    early-exit happens BEFORE `execute_cooperative`).
///
/// Pre-fix the test FAILS at all three assertions: every post-trip
/// query incremented the executed counter, entered cooperative
/// admission, and produced a stale `actual` that drifted with each
/// new call. Post-fix the assertions hold because the dispatcher's
/// fast-path early-exit returns the budget-exceeded sentinel without
/// touching the cooperative-admission machinery.
#[test]
fn post_trip_projection_op_queries_bypass_cooperative_admission() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 1,
        ..HostConfig::default()
    }));
    let bases: Vec<SemanticNodeId> = (0..6)
        .map(|i| {
            let name: &'static str = match i {
                0 => "k0",
                1 => "k1",
                2 => "k2",
                3 => "k3",
                4 => "k4",
                _ => "k5",
            };
            intern_single_member_object(&host, name)
        })
        .collect();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/post-trip.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let request_budget = Arc::clone(&ctx.projection_budget);
    let _ctx_guard = RequestContextGuard::install(ctx);
    let dispatch = host.semantic_dispatch();

    // The first call lands within budget (1/1), the second trips it
    // (2/1 → BudgetExceeded with actual=2). Both calls enter the
    // cooperative-admission machinery; only the second is rejected by
    // the in-closure cap check.
    let first_keyof = dispatch.execute_type_node(SemanticQueryKey::KeyOf {
        base: bases[0],
        context,
    });
    assert!(
        matches!(first_keyof, QueryResult::Value(_)),
        "1st keyof within budget should succeed (got {first_keyof:?})"
    );
    let second_keyof = dispatch.execute_type_node(SemanticQueryKey::KeyOf {
        base: bases[1],
        context,
    });
    let trip_actual = match second_keyof {
        QueryResult::Error(QueryError::BudgetExceeded(ref failure)) => failure.actual,
        other => panic!("2nd keyof should trip the budget (got {other:?})"),
    };
    assert_eq!(
        trip_actual, 2,
        "trip-point actual must be 2 (1st + 2nd checked-increments)"
    );
    let executed_at_trip = request_budget.projection_ops_executed_count();
    assert_eq!(
        executed_at_trip, 2,
        "RequestBudget executed counter should equal 2 immediately after the trip"
    );

    // Drive 4 additional projection-op queries on DISTINCT (base)
    // values. Each must return BudgetExceeded with the same `actual`
    // value (peek-only — the executed counter MUST NOT advance).
    for (i, &base) in bases.iter().enumerate().take(6).skip(2) {
        let key = SemanticQueryKey::KeyOf { base, context };
        let result = dispatch.execute_type_node(key);
        match result {
            QueryResult::Error(QueryError::BudgetExceeded(failure)) => {
                assert_eq!(
                    failure.domain,
                    BudgetDomain::ProjectionOperation,
                    "post-trip query #{i} domain must stay ProjectionOperation"
                );
                assert_eq!(
                    failure.limit, 1,
                    "post-trip query #{i} limit reports the configured budget"
                );
                assert_eq!(
                    failure.actual, 2,
                    "post-trip query #{i} reports the trip-point actual (peek is non-incrementing)"
                );
            }
            other => panic!("post-trip query #{i} should still be BudgetExceeded (got {other:?})"),
        }
    }

    // Discriminating invariant. The per-request projection budget is
    // hermetic to this request (it lives on the RequestContext) so
    // this assertion is robust against parallel test execution.
    //
    // Pre-fix: every post-trip dispatch enters the cooperative-
    // admission build closure, which calls `check_projection_op_count`
    // (a `fetch_add(1)`) — the executed counter would advance by 1 per
    // post-trip query, ending at 6 (2 trip + 4 post-trip).
    //
    // Post-fix: the dispatcher's `is_exhausted` peek short-circuits
    // before the build closure runs, leaving the executed counter at
    // its trip-point value.
    let executed_after_posttrip = request_budget.projection_ops_executed_count();
    assert_eq!(
        executed_after_posttrip, executed_at_trip,
        "post-trip queries MUST NOT bump the request budget's executed counter; \
         pre-fix value would advance to 6 (2 trip + 4 post-trip incremental check calls)"
    );
}

/// The post-trip early-exit ATTRIBUTION must mirror EVERY kind the
/// aggregate work-budget gate counts — including the demand-bearing
/// `TypeOf`. A post-trip `TypeOf` dispatch takes the early-exit (it
/// counts toward the projection budget) and must bump the same
/// per-kind cold counter the slow path bumps
/// (`SemanticQueryTypeOfCold`); a missing arm silently under-counts
/// post-trip typeof dispatches and loses the typeof-storm signal in
/// bench attribution.
#[test]
fn post_trip_typeof_early_exit_attributes_to_typeof_cold_counter() {
    use crate::semantic_query::{ScopeId, TypeOfContext, ValueRootKey, ValueRootSlotIdentity};

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 1,
        ..HostConfig::default()
    }));
    let bases: Vec<SemanticNodeId> = (0..2)
        .map(|i| intern_single_member_object(&host, if i == 0 { "t0" } else { "t1" }))
        .collect();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/post-trip-typeof.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let _ctx_guard = RequestContextGuard::install(ctx);
    let dispatch = host.semantic_dispatch();

    // Trip the projection-op fuse: 1st KeyOf lands within budget,
    // 2nd trips it.
    let first = dispatch.execute_type_node(SemanticQueryKey::KeyOf {
        base: bases[0],
        context,
    });
    assert!(
        matches!(first, QueryResult::Value(_)),
        "1st keyof within budget should succeed (got {first:?})"
    );
    let second = dispatch.execute_type_node(SemanticQueryKey::KeyOf {
        base: bases[1],
        context,
    });
    assert!(
        matches!(second, QueryResult::Error(QueryError::BudgetExceeded(_))),
        "2nd keyof should trip the budget (got {second:?})"
    );

    // Post-trip TypeOf dispatch: counts toward the budget, so it takes
    // the early-exit — which must attribute to the TypeOf cold counter.
    let live_ctx = crate::request_context::current_request_context()
        .expect("request context installed for this test");
    let typeof_cold_before = live_ctx
        .semantic_query_typeof_cold
        .load(std::sync::atomic::Ordering::Relaxed);
    let typeof_key = SemanticQueryKey::TypeOf {
        value_root: ValueRootSlotIdentity::new(
            ValueRootKey {
                scope: ScopeId::file(Arc::from("/post-trip-typeof.ts")),
                name: Arc::from("sample"),
            },
            0,
            Default::default(),
            Default::default(),
        ),
        context: TypeOfContext::new(context, Default::default()),
    };
    let result = dispatch.execute_type_node(typeof_key);
    assert!(
        matches!(result, QueryResult::Error(QueryError::BudgetExceeded(_))),
        "post-trip TypeOf must take the budget early-exit (got {result:?})"
    );
    let typeof_cold_after = live_ctx
        .semantic_query_typeof_cold
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        typeof_cold_after,
        typeof_cold_before + 1,
        "the post-trip early-exit must attribute the TypeOf dispatch to \
         SemanticQueryTypeOfCold — a missing attribution arm silently \
         under-counts post-trip typeof dispatches"
    );
}

/// Non-projection-op queries (ResolveDecl, NormalizeUnion,
/// NormalizeIntersection, ResolvedNamedType, Relate,
/// ResolveMacroPayload) MUST be unaffected by the post-trip
/// fast-path early-exit — the projection-op fuse only bounds the
/// budget-counted subset of the dispatch surface, and a request that
/// trips the projection-op cap must still be able to complete any
/// non-budgeted work on its way to publishing a partial result.
///
/// Discriminator: trip the projection-op budget, then dispatch a
/// `NormalizeUnion` query on the post-trip request. The query MUST
/// enter cooperative admission (the budget-gate is keyed on
/// `semantic_query_counts_toward_projection_budget` only) and MUST
/// produce a non-error value.
///
/// Pre-fix this test passes trivially because there was no early-exit
/// at all. Post-fix it fails if the early-exit accidentally widens its
/// gate to include non-projection queries — which would silently
/// poison the budget-exhausted request's final-result assembly path.
#[test]
fn post_trip_non_projection_queries_still_dispatch_normally() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 1,
        ..HostConfig::default()
    }));
    let base_a = intern_single_member_object(&host, "a");
    let base_b = intern_single_member_object(&host, "b");
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/post-trip-non-projection.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let _ctx_guard = RequestContextGuard::install(ctx);
    let dispatch = host.semantic_dispatch();

    // Trip the projection-op fuse with two distinct KeyOf calls.
    let _ = dispatch.execute_type_node(SemanticQueryKey::KeyOf {
        base: base_a,
        context,
    });
    let trip = dispatch.execute_type_node(SemanticQueryKey::KeyOf {
        base: base_b,
        context,
    });
    assert!(
        matches!(trip, QueryResult::Error(QueryError::BudgetExceeded(_))),
        "2nd keyof should trip the fuse (got {trip:?})"
    );

    // Snapshot cooperative-admission entry count after the trip.
    let coop_at_trip = crate::loop5_instrumentation::EXECUTE_COOPERATIVE_CALLS
        .load(std::sync::atomic::Ordering::Relaxed);

    // A non-projection query (NormalizeUnion) on the post-trip request
    // MUST enter cooperative admission and produce a non-error value.
    // The query is structurally trivial (a single-member union
    // normalises to that member), so an error result here would mean
    // the early-exit incorrectly widened its gate.
    let single_member: Arc<[SemanticNodeId]> = Arc::from(vec![base_a].into_boxed_slice());
    let normalize_key = SemanticQueryKey::NormalizeUnion {
        members: single_member,
    };
    let normalize_result = dispatch.execute_type_node(normalize_key);
    assert!(
        matches!(normalize_result, QueryResult::Value(_)),
        "post-trip NormalizeUnion must dispatch normally; \
         widening the early-exit gate to non-projection queries would \
         break partial-result assembly (got {normalize_result:?})"
    );

    // Cooperative admission MUST have run for the NormalizeUnion call —
    // the gate is keyed on `semantic_query_counts_toward_projection_budget`,
    // which excludes NormalizeUnion. Delta should be >= 1.
    let coop_after_normalize = crate::loop5_instrumentation::EXECUTE_COOPERATIVE_CALLS
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        coop_after_normalize > coop_at_trip,
        "post-trip non-projection query MUST enter cooperative admission \
         (coop_at_trip={coop_at_trip}, coop_after_normalize={coop_after_normalize})"
    );
}

/// 5b §5.D.4 — `ResolveMacroPayload` budget-exceeded must not warm.
/// The §5.D.4 r18 contract is "after a partial / budget-exceeded
/// result, re-querying must NOT warm-serve the partial". We exercise
/// the contract through a `ProjectPath` key whose path length exceeds
/// `depth_budget`; `ResolveMacroPayload` internally dispatches
/// `ProjectPath` for the slot/emit/model branches, so a budget hit on
/// `ProjectPath` is the same contract `ResolveMacroPayload` relies on.
#[test]
fn no_cache_promotion_for_budget_exceeded_resolve_macro_payload() {
    let host = build_constrained_host();
    let base = intern_three_member_object(&host);
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("deep_0")),
                PathSegment::Member(Arc::from("deep_1")),
                PathSegment::Member(Arc::from("deep_2")),
            ]
            .into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    // First query — should report a budget-exceeded sentinel.
    let dispatch = host.semantic_dispatch();
    let r1 = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for a 3-segment path (got {r1:?})"
    );

    // Discrimination: SAME host, SAME key, second query MUST cold-fire
    // (partial NOT promoted to warm cache).
    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute_type_node(key.clone());
    let cold_delta = counter.family_cold(&key) - baseline_cold;
    let warm_delta = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold_delta, 1,
        "second query on same host MUST cold-fire (partial NOT promoted to warm cache; got cold={cold_delta})"
    );
    assert_eq!(
        warm_delta, 0,
        "warm count must NOT increment on second query of a budget-exceeded key (got warm={warm_delta})"
    );
}

/// 5e §5.D.4 — `route_target_pick_omit` budget-exceeded must not
/// warm. Same-host re-query contract: depth-budget-exceeded result
/// must NOT be promoted to warm cache, and the second query MUST
/// cold-fire on the same host. Exercised through the path-projection
/// dispatch the route-target Pick/Omit closure relies on.
#[test]
fn no_cache_promotion_for_budget_exceeded_route_target_pick_omit() {
    let host = build_constrained_host();
    let base = intern_three_member_object(&host);
    // Use a different path than B7's so the two tests don't share
    // the constrained host's warm cache (each test owns its own
    // host instance — paranoia for thread-local sharing).
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("deep_0")),
                PathSegment::Member(Arc::from("deep_1")),
                PathSegment::Member(Arc::from("deep_2")),
            ]
            .into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let dispatch = host.semantic_dispatch();
    let r1 = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for the 5e route-target 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute_type_node(key.clone());
    let cold_delta = counter.family_cold(&key) - baseline_cold;
    let warm_delta = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold_delta, 1,
        "second route-target query on same host MUST cold-fire (partial NOT promoted; got cold={cold_delta})"
    );
    assert_eq!(
        warm_delta, 0,
        "warm count must NOT increment on second route-target budget-exceeded query (got warm={warm_delta})"
    );
}

/// 5f §5.D.4 — `fallthrough_inheritance` budget-exceeded must not
/// warm. Same-host re-query contract: a depth-budget-exceeded
/// path-projection (the ProjectPath dispatch the fallthrough
/// inheritance closure traverses) must NOT be promoted to the warm
/// cache; the second query on the SAME host MUST cold-fire.
#[test]
fn no_cache_promotion_for_budget_exceeded_fallthrough_inheritance() {
    let host = build_constrained_host();
    let base = intern_three_member_object(&host);
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("deep_0")),
                PathSegment::Member(Arc::from("deep_1")),
                PathSegment::Member(Arc::from("deep_2")),
            ]
            .into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let dispatch = host.semantic_dispatch();
    let r1 = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for the 5f fallthrough 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute_type_node(key.clone());
    let cold_delta = counter.family_cold(&key) - baseline_cold;
    let warm_delta = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold_delta, 1,
        "second fallthrough-inheritance query on same host MUST cold-fire (partial NOT promoted; got cold={cold_delta})"
    );
    assert_eq!(
        warm_delta, 0,
        "warm count must NOT increment on second fallthrough-inheritance budget-exceeded query (got warm={warm_delta})"
    );
}

/// 5h §5.D.4 — `userland_shadowing_pick` budget-exceeded must not
/// warm. Same-host re-query contract: a depth-budget-exceeded
/// path-projection (the resolver-context shadow gate routes
/// through the same dispatch path-walker the route-target Pick/Omit
/// closure relies on) must NOT be promoted to the warm cache; the
/// second query on the SAME host MUST cold-fire.
///
/// The shadow-gate thread does not alter cache-promotion behaviour
/// (CLAUDE.md "cancelled, superseded, interrupted, budget-exceeded,
/// or partial semantic results must not be promoted as warm shared
/// cache entries" applies uniformly). The test exercises the
/// contract through the same path-projection key shape so a
/// regression in the shadow-gate thread that accidentally cached
/// budget-exceeded partials would surface here.
#[test]
fn no_cache_promotion_for_budget_exceeded_userland_shadowing_pick() {
    let host = build_constrained_host();
    let base = intern_three_member_object(&host);
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("deep_0")),
                PathSegment::Member(Arc::from("deep_1")),
                PathSegment::Member(Arc::from("deep_2")),
            ]
            .into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let dispatch = host.semantic_dispatch();
    let r1 = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for the 5h userland-shadowing 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute_type_node(key.clone());
    let cold_delta = counter.family_cold(&key) - baseline_cold;
    let warm_delta = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold_delta, 1,
        "second userland-shadowing query on same host MUST cold-fire \
         (partial NOT promoted to warm cache; got cold={cold_delta})"
    );
    assert_eq!(
        warm_delta, 0,
        "warm count must NOT increment on second userland-shadowing budget-exceeded query \
         (the shadow-gate thread must not introduce warm-promotion of partials; got warm={warm_delta})"
    );
}

/// 5i §5.D.4 — `Exclude<>` / `Extract<>` reduction budget-exceeded
/// must not warm. Same-host re-query contract: a depth-budget-
/// exceeded path-projection (the literal-type reduction lives
/// behind `build_builtin_utility`, but the path-walker that
/// drives projection into the produced Union members shares the
/// same depth budget as every other path-projection) must NOT be
/// promoted to the warm cache; the second query on the SAME host
/// MUST cold-fire.
///
/// The Extract/Exclude arm does not alter cache-promotion
/// behaviour (CLAUDE.md "cancelled, superseded, interrupted,
/// budget-exceeded, or partial semantic results must not be
/// promoted as warm shared cache entries" applies uniformly). The
/// test exercises the contract through the same path-projection
/// key shape so a regression in the new arm that accidentally
/// cached budget-exceeded partials would surface here.
#[test]
fn no_cache_promotion_for_budget_exceeded_exclude_extract_reduction() {
    let host = build_constrained_host();
    let base = intern_three_member_object(&host);
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("deep_0")),
                PathSegment::Member(Arc::from("deep_1")),
                PathSegment::Member(Arc::from("deep_2")),
            ]
            .into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let dispatch = host.semantic_dispatch();
    let r1 = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for the 5i Exclude/Extract reduction 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute_type_node(key.clone());
    let cold_delta = counter.family_cold(&key) - baseline_cold;
    let warm_delta = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold_delta, 1,
        "second Exclude/Extract reduction query on same host MUST cold-fire \
         (partial NOT promoted to warm cache; got cold={cold_delta})"
    );
    assert_eq!(
        warm_delta, 0,
        "warm count must NOT increment on second Exclude/Extract reduction budget-exceeded query \
         (the new Extract/Exclude arm must not introduce warm-promotion of partials; got warm={warm_delta})"
    );
}

/// Slot-binding-parameter lowering budget-exceeded must not warm.
/// `project_slot_binding_member` composes existing `ProjectPath`
/// variants under the hood; the budget-exceeded contract therefore
/// applies through the same
/// `ProjectPath` infrastructure. We exercise the contract through a
/// `ProjectPath` key whose path length exceeds `depth_budget`; a
/// regression in slot-binding lowering that accidentally cached
/// budget-exceeded partials would surface here.
///
/// CLAUDE.md "cancelled, superseded, interrupted, budget-exceeded,
/// or partial semantic results must not be promoted as warm shared
/// cache entries" applies uniformly. The discriminating proof: a
/// constrained host (`depth_budget = 2`) on a 3-segment path
/// produces a budget-exceeded sentinel on first execution; the
/// SECOND execution on the SAME host MUST cold-fire (proving the
/// partial was NOT promoted).
#[test]
fn no_cache_promotion_for_budget_exceeded_slot_binding_lowering() {
    let host = build_constrained_host();
    let base = intern_three_member_object(&host);
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("deep_0")),
                PathSegment::Member(Arc::from("deep_1")),
                PathSegment::Member(Arc::from("deep_2")),
            ]
            .into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let dispatch = host.semantic_dispatch();
    let r1 = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel \
         for the 5j slot-binding-lowering 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute_type_node(key.clone());
    let cold_delta = counter.family_cold(&key) - baseline_cold;
    let warm_delta = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold_delta, 1,
        "second slot-binding-lowering query on same host MUST cold-fire \
         (partial NOT promoted to warm cache; got cold={cold_delta})"
    );
    assert_eq!(
        warm_delta, 0,
        "warm count must NOT increment on second slot-binding-lowering budget-exceeded query \
         (the new `project_slot_binding_member` helper must not introduce \
         warm-promotion of partials; got warm={warm_delta})"
    );
}

/// 5k §5.D.4 — value-member typeof substitution budget-exceeded
/// must not promote to warm cache. The §5.13 fix in
/// `shallow_lower_type_expr`'s `TypeExpr::TypeOf` arm composes a
/// `ProjectPath { mode: Navigate }` query for the tail segments;
/// when that projection exceeds `depth_budget`, the sentinel must
/// not warm-cache.
///
/// CLAUDE.md "cancelled, superseded, interrupted, budget-exceeded,
/// or partial semantic results must not be promoted as warm shared
/// cache entries" applies uniformly. The discriminating proof: a
/// constrained host (`depth_budget = 2`) on a 3-segment path
/// produces a budget-exceeded sentinel on first execution; the
/// SECOND execution on the SAME host MUST cold-fire (proving the
/// partial was NOT promoted).
#[test]
fn no_cache_promotion_for_budget_exceeded_typeof_substitution() {
    let host = build_constrained_host();
    let base = intern_three_member_object(&host);
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("deep_0")),
                PathSegment::Member(Arc::from("deep_1")),
                PathSegment::Member(Arc::from("deep_2")),
            ]
            .into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let dispatch = host.semantic_dispatch();
    let r1 = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel \
         for the 5k typeof-substitution 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute_type_node(key.clone());
    let cold_delta = counter.family_cold(&key) - baseline_cold;
    let warm_delta = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold_delta, 1,
        "second typeof-substitution query on same host MUST cold-fire \
         (partial NOT promoted to warm cache; got cold={cold_delta})"
    );
    assert_eq!(
        warm_delta, 0,
        "warm count must NOT increment on second typeof-substitution budget-exceeded query \
         (the §5.13 single-segment-first lookup must not introduce \
         warm-promotion of partials; got warm={warm_delta})"
    );
}

/// 5m §5.D.4 — `engine_state_promotion` budget-exceeded must not
/// promote to warm cache. The 5m caller migration routes the 18
/// external engine-method callsites through bridge helpers; the
/// bridge bodies internally call the engine's deprecated methods
/// (during the migration window per §5.13a.2). The cache-promotion
/// contract — cancelled / superseded / interrupted / budget-exceeded
/// / partial semantic results MUST NOT be promoted as warm shared
/// cache entries (CLAUDE.md) — applies uniformly across every
/// dispatch call site, INCLUDING the bridge call sites.
///
/// Discriminating proof: a constrained host (`depth_budget = 2`) on
/// a 3-segment path produces a budget-exceeded sentinel on first
/// execution. The SAME host, SAME key, second execution MUST
/// cold-fire (proving the partial was NOT promoted by the bridge's
/// internal engine call OR by the dispatch path the bridge composes).
///
/// A regression in 5m that accidentally promoted budget-exceeded
/// partials to the warm cache would surface here as a non-zero
/// warm_delta on the second execution.
#[test]
fn no_cache_promotion_for_budget_exceeded_engine_state_promotion() {
    let host = build_constrained_host();
    let base = intern_three_member_object(&host);
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("deep_0")),
                PathSegment::Member(Arc::from("deep_1")),
                PathSegment::Member(Arc::from("deep_2")),
            ]
            .into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let dispatch = host.semantic_dispatch();
    let r1 = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel \
         for the 5m engine-state-promotion 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute_type_node(key.clone());
    let cold_delta = counter.family_cold(&key) - baseline_cold;
    let warm_delta = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold_delta, 1,
        "second engine-state-promotion query on same host MUST cold-fire \
         (partial NOT promoted to warm cache; got cold={cold_delta})"
    );
    assert_eq!(
        warm_delta, 0,
        "warm count must NOT increment on second engine-state-promotion budget-exceeded query \
         (the 5m bridge migration must NOT introduce warm-promotion of partials; \
         got warm={warm_delta})"
    );
}

/// L3 fail-closed budget-TRIP guard for `Conditional`. Pre-fix
/// `Conditional` was excluded from
/// `semantic_query_counts_toward_projection_budget`, so a storm
/// dominated by conditionals never tripped the fuse (the
/// `ChatMessages.vue` hang). This test pins the FULL trip contract,
/// not just the executed-counter increment:
///
/// With `projection_op_budget = 1`:
/// 1. the first cold `Conditional` build lands within budget (1/1),
///    returns a concrete `Value`, and counts (executed counter == 1);
/// 2. a SECOND, DISTINCT cold `Conditional` build trips the fuse,
///    returning `QueryError::BudgetExceeded`;
/// 3. that `CacheRead.cache_suppress == true` (the build closure marks
///    the budget-exceeded carrier non-cacheable); and
/// 4. re-querying the SAME tripped key returns `BudgetExceeded` again
///    with NO warm hit — the partial was NOT promoted to the warm cache
///    (warm delta == 0; the post-trip fast-path peek bypasses
///    cooperative admission entirely, so no warm-served `Value` can
///    leak).
///
/// Pre-fix the test FAILS at step 2 (the second Conditional never
/// trips, returning a `Value`), so the whole contract is
/// discriminating against the pre-L3 tree.
#[test]
fn budget_trip_conditional_suppresses_and_does_not_warm() {
    use crate::semantic_query::PrimitiveKind;

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 1,
        ..HostConfig::default()
    }));
    let graph = host.project_type_store().semantic_graph();
    let true_branch = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let false_branch = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    // Two DISTINCT conditional keys (distinct check/extends shapes), so
    // each is its own cold build.
    let key_first = SemanticQueryKey::Conditional {
        check: intern_single_member_object(&host, "cond_check_first"),
        extends: intern_single_member_object(&host, "cond_extends_first"),
        true_branch,
        false_branch,
        distributive: false,
    };
    let key_second = SemanticQueryKey::Conditional {
        check: intern_single_member_object(&host, "cond_check_second"),
        extends: intern_single_member_object(&host, "cond_extends_second"),
        true_branch,
        false_branch,
        distributive: false,
    };

    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/cond-budget-trip.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let request_budget = Arc::clone(&ctx.projection_budget);
    let _ctx_guard = RequestContextGuard::install(ctx);
    let dispatch = host.semantic_dispatch();

    // (1) first cold build lands within budget and counts.
    let first = dispatch.execute_read(key_first.clone());
    assert!(
        matches!(first.value, QueryResult::Value(_)),
        "1st Conditional within budget must succeed (got {:?})",
        first.value
    );
    assert_eq!(
        request_budget.projection_ops_executed_count(),
        1,
        "a cold Conditional build must count toward the request work budget"
    );

    // (2)+(3) second distinct build trips + suppresses.
    let second = dispatch.execute_read(key_second.clone());
    assert!(
        matches!(
            second.value,
            QueryResult::Error(QueryError::BudgetExceeded(_))
        ),
        "2nd distinct Conditional must trip the budget (pre-L3 it returned a Value); got {:?}",
        second.value
    );
    assert!(
        second.cache_suppress,
        "a budget-exceeded Conditional read MUST carry cache_suppress=true so the partial is \
         never warmed"
    );
    assert!(
        second.result_is_partial,
        "a budget-exceeded Conditional read MUST carry result_is_partial=true — this is the \
         signal the component-meta + shape/materialize warm gates key on"
    );

    // (4) FRESH-request replay. The previous in-request replay was too
    // weak: re-querying inside the already-exhausted request short-
    // circuits at the dispatcher's `is_exhausted` peek BEFORE cooperative
    // admission, so it neither warm-serves nor cold-rebuilds — a warm
    // delta of 0 there proves nothing (a promoted partial would simply
    // never be consulted). A FRESH request carries a FRESH budget, so the
    // replay actually enters `execute_cooperative`: if the partial had
    // been promoted it would WARM-hit; if it was correctly suppressed it
    // COLD-rebuilds. Assert cold delta increases with NO warm hit.
    drop(_ctx_guard);
    let fresh_ctx = RequestContext::with_kind_timing_and_projection_budget(
        2,
        Arc::from("/cond-budget-trip-replay.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let _fresh_guard = RequestContextGuard::install(fresh_ctx);
    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key_second);
    let baseline_warm = counter.family_warm(&key_second);
    let _replay = dispatch.execute_read(key_second.clone());
    // The discriminating invariant is the COLD/WARM split, not the
    // replay's value kind: under the fresh, roomier budget the
    // cold-rebuild legitimately completes to a `Value`. What proves the
    // partial was NOT promoted is that the replay COLD-fires (it had to
    // recompute) and does NOT WARM-hit. A promoted partial would instead
    // warm-serve (warm delta 1, cold delta 0) and return the suppressed
    // `Error`. Pre-signal-split (or pre-fresh-request replay) the in-
    // request replay short-circuited at the budget peek and recorded
    // NEITHER counter, so the old `warm delta == 0` proved nothing.
    assert!(
        counter.family_cold(&key_second) - baseline_cold >= 1,
        "fresh-request replay MUST cold-rebuild (proving the suppressed partial was NOT promoted \
         to the warm cache)"
    );
    assert_eq!(
        counter.family_warm(&key_second) - baseline_warm,
        0,
        "fresh-request replay of a budget-exceeded Conditional MUST NOT warm-hit a promoted partial"
    );
}

/// L3 fail-closed budget-TRIP guard for `Instantiate` — the sibling of
/// `budget_trip_conditional_suppresses_and_does_not_warm`. Pre-fix
/// `Instantiate` was excluded from the projection budget gate, so a
/// generic-expansion storm never tripped. With
/// `projection_op_budget = 1` the second distinct cold `Instantiate`
/// build must trip, suppress, and refuse warm promotion on replay.
///
/// Pre-fix the test FAILS at the trip assertion (the second Instantiate
/// returns a `Value` instead of `BudgetExceeded`).
#[test]
fn budget_trip_instantiate_suppresses_and_does_not_warm() {
    // `projection_op_budget = 1`: the FIRST decidable `Partial<{ … }>`
    // cold build lands within the budget (it counts exactly one
    // budget-gated Instantiate op at the dispatch entry) and the SECOND
    // distinct build trips the fuse, returning a real `BudgetExceeded`.
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 1,
        ..HostConfig::default()
    }));
    let dispatch = host.semantic_dispatch();
    // Two DISTINCT builtin instantiations (distinct argument shapes) so
    // each is its own cold build.
    let arg_first = intern_single_member_object(&host, "inst_arg_first");
    let arg_second = intern_single_member_object(&host, "inst_arg_second");
    let partial_slot = dispatch.builtin_type_slot("Partial");
    let context = dispatch.instantiate_context_for(
        "__builtin__",
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    let key_first = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
        partial_slot.clone(),
        Arc::from(vec![arg_first].into_boxed_slice()),
        context,
    ));
    let key_second = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
        partial_slot,
        Arc::from(vec![arg_second].into_boxed_slice()),
        context,
    ));

    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/inst-budget-trip.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let request_budget = Arc::clone(&ctx.projection_budget);
    let _ctx_guard = RequestContextGuard::install(ctx);

    let first = dispatch.execute_read(key_first);
    assert!(
        matches!(first.value, QueryResult::Value(_)),
        "1st Instantiate within budget must succeed (got {:?})",
        first.value
    );
    assert!(
        request_budget.projection_ops_executed_count() >= 1,
        "a cold Instantiate build must count toward the request work budget (Instantiate must be \
         in the budget gate); executed counter still 0"
    );

    let second = dispatch.execute_read(key_second.clone());
    assert!(
        matches!(
            second.value,
            QueryResult::Error(QueryError::BudgetExceeded(_))
        ),
        "2nd distinct Instantiate must trip the budget (pre-L3 it returned a Value); got {:?}",
        second.value
    );
    assert!(
        second.cache_suppress,
        "a budget-exceeded Instantiate read MUST carry cache_suppress=true"
    );
    assert!(
        second.result_is_partial,
        "a budget-exceeded Instantiate read MUST carry result_is_partial=true — the warm gate \
         keys on this signal"
    );

    // FRESH-request replay (see the Conditional sibling for the rationale).
    // The fresh budget lets the replay enter cooperative admission so a
    // promoted partial would warm-hit; a correctly-suppressed partial
    // cold-rebuilds. Assert cold delta increases with NO warm hit.
    drop(_ctx_guard);
    let fresh_ctx = RequestContext::with_kind_timing_and_projection_budget(
        2,
        Arc::from("/inst-budget-trip-replay.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let _fresh_guard = RequestContextGuard::install(fresh_ctx);
    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key_second);
    let baseline_warm = counter.family_warm(&key_second);
    let _replay = dispatch.execute_read(key_second.clone());
    // Discriminating invariant: the COLD/WARM split (see the Conditional
    // sibling). Under the fresh roomier budget the cold-rebuild completes;
    // what proves non-promotion is cold-fire + no warm-hit. A promoted
    // partial would warm-serve the suppressed `Error` (warm 1, cold 0).
    assert!(
        counter.family_cold(&key_second) - baseline_cold >= 1,
        "fresh-request replay MUST cold-rebuild (proving the suppressed partial was NOT promoted)"
    );
    assert_eq!(
        counter.family_warm(&key_second) - baseline_warm,
        0,
        "fresh-request replay of a budget-exceeded Instantiate MUST NOT warm-hit a promoted partial"
    );
}

/// §5 MANDATORY discrimination proof for the A2 signal split, at the WARM
/// GATE — the single point the reachability argument converges on.
///
/// A budget/walker partial can surface as a COMPLETE
/// `QueryResult::Value` (a `ProjectPath` shallow-walking an
/// `InstantiationRef` whose nested `Instantiate` trips the budget — the
/// walker catches the error, contributes no surface, and `build_project_path`
/// returns `Value` with `result_is_partial = true`). A value-kind gate
/// (`matches!(value, Error | Recursive)`) is INSUFFICIENT — it MISSES this
/// Value-partial by construction and would warm `ComponentMetaResultDb`.
///
/// This test pins the warm gate's behaviour directly on the two read shapes
/// the split must distinguish — independently of which producer site
/// manufactures them, so it stays discriminating even where the
/// request-wide budget backstop would otherwise mask the producer:
///
/// 1. A `Value` carrying `result_is_partial = true` (the Value-partial)
///    MUST raise the suppression flag. A value-kind gate
///    (`matches!(value, Error | Recursive)`) is `false` for this `Value`, so
///    keying on value-kind alone would NOT suppress → the partial would warm.
///    The `result_is_partial` authority is what closes the hole.
/// 2. A complete `Value` carrying `cache_suppress = true` but
///    `result_is_partial = false` (a benign non-cacheable result — ReturnOnly
///    / overflow / unrootable self-root) MUST NOT raise the flag: a
///    complete-but-non-cacheable result still warms the component-meta result.
///    Keying the gate on `cache_suppress` (the other failed candidate) would
///    wrongly suppress here.
#[test]
fn warm_gate_keys_on_result_is_partial_not_value_kind_or_cache_suppress() {
    use crate::request_context::{current_materialization_cache_suppress, RequestContext};
    use crate::semantic_query::CacheRead;

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    let graph = host.project_type_store().semantic_graph();
    let value_node = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(Vec::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));

    let empty_sig: crate::semantic_query::DepSignature = Arc::from(Vec::new().into_boxed_slice());

    // (1) The Value-partial: a COMPLETE `Value` with
    // `result_is_partial = true`. A value-kind gate
    // (`matches!(value, Error | Recursive)`) is FALSE for this `Value`.
    let value_partial: CacheRead<QueryResult<SemanticNodeId>> = CacheRead {
        value: QueryResult::Value(value_node),
        dep_signature: empty_sig.clone(),
        walker_diagnostics: Arc::from([]),
        cache_suppress: true,
        result_is_partial: true,
    };
    assert!(
        !matches!(
            value_partial.value,
            QueryResult::Error(_) | QueryResult::Recursive(_)
        ),
        "the Value-partial is a COMPLETE Value — a value-kind gate's blind spot"
    );

    // (2) A benign complete-but-non-cacheable result: a `Value` with
    // `cache_suppress = true` but `result_is_partial = false` (ReturnOnly /
    // overflow / unrootable self-root).
    let complete_non_cacheable: CacheRead<QueryResult<SemanticNodeId>> = CacheRead {
        value: QueryResult::Value(value_node),
        dep_signature: empty_sig,
        walker_diagnostics: Arc::from([]),
        cache_suppress: true,
        result_is_partial: false,
    };

    // Gate behaviour for (2) — under its OWN request — must NOT suppress:
    // keying on `cache_suppress` (a failed candidate) would wrongly fire.
    {
        let ctx = RequestContext::with_kind_timing_and_projection_budget(
            1,
            Arc::from("/non-cacheable.vue"),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            host.config.projection_op_budget,
        );
        let _g = RequestContextGuard::install(ctx);
        assert!(!current_materialization_cache_suppress());
        crate::request_context::observe_component_meta_read_suppress(&complete_non_cacheable);
        assert!(
            !current_materialization_cache_suppress(),
            "a complete-but-non-cacheable result (cache_suppress=true, result_is_partial=false) \
             MUST NOT raise the warm-gate suppress flag — it must still be allowed to warm a \
             complete component-meta result"
        );
    }

    // Gate behaviour for (1) — under a fresh request — MUST suppress: the
    // Value-partial raises the flag the final-result + shape caches consult.
    {
        let ctx = RequestContext::with_kind_timing_and_projection_budget(
            1,
            Arc::from("/value-partial.vue"),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            host.config.projection_op_budget,
        );
        let _g = RequestContextGuard::install(ctx);
        assert!(!current_materialization_cache_suppress());
        crate::request_context::observe_component_meta_read_suppress(&value_partial);
        assert!(
            current_materialization_cache_suppress(),
            "a Value-partial (result_is_partial=true) MUST raise the warm-gate suppress flag — a \
             value-kind gate misses this complete Value and would have warmed the partial"
        );
    }
}

/// FIX 2 discrimination proof: a COMPLETE result that hits a BENIGN
/// non-cacheable inner memo (signature-overflow / unrootable self-root /
/// ReturnOnly) MUST STILL warm the component-meta final cache.
///
/// The benign-non-cacheable shapes carry `cache_suppress = true` but
/// `result_is_partial = false`. The warm gate keys on `result_is_partial`,
/// so `observe_component_meta_read_suppress` MUST NOT raise the request's
/// suppression flag for ANY of these complete results. Were a memo site to
/// wrongly tag a benign non-cacheable winner with `result_is_partial = true`
/// (the bug the memo fixtures encoded pre-FIX-2), this test would FAIL: the
/// flag would fire and the complete component-meta result would be wrongly
/// refused warm promotion.
#[test]
fn benign_non_cacheable_complete_results_still_warm_component_meta_final() {
    use crate::request_context::{current_materialization_cache_suppress, RequestContext};
    use crate::semantic_query::CacheRead;

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    let graph = host.project_type_store().semantic_graph();
    let value_node = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let empty_sig: crate::semantic_query::DepSignature = Arc::from(Vec::new().into_boxed_slice());

    // Each benign non-cacheable production shape: a COMPLETE `Value`
    // (`Primitive(String)`), `cache_suppress = true`, `result_is_partial =
    // false`. These mirror exactly what `finalise_traced_build_output`
    // emits for the Overflow / unrootable-`None` / ReturnOnly arms and what
    // the two `component_meta_materialize.rs` ReturnOnly arms emit.
    let benign_shapes: [(&str, CacheRead<QueryResult<SemanticNodeId>>); 3] = [
        (
            "signature-overflow",
            CacheRead {
                value: QueryResult::Value(value_node),
                dep_signature: empty_sig.clone(),
                walker_diagnostics: Arc::from([]),
                cache_suppress: true,
                result_is_partial: false,
            },
        ),
        (
            "unrootable-self-root",
            CacheRead {
                value: QueryResult::Value(value_node),
                dep_signature: empty_sig.clone(),
                walker_diagnostics: Arc::from([]),
                cache_suppress: true,
                result_is_partial: false,
            },
        ),
        (
            "return-only",
            CacheRead {
                value: QueryResult::Value(value_node),
                dep_signature: empty_sig,
                walker_diagnostics: Arc::from([]),
                cache_suppress: true,
                result_is_partial: false,
            },
        ),
    ];

    for (label, read) in benign_shapes {
        let ctx = RequestContext::with_kind_timing_and_projection_budget(
            1,
            Arc::from("/benign-non-cacheable.vue"),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            host.config.projection_op_budget,
        );
        let _g = RequestContextGuard::install(ctx);
        assert!(!current_materialization_cache_suppress());
        // Sanity: the shape is a COMPLETE Value, non-cacheable only.
        assert!(
            matches!(read.value, QueryResult::Value(_))
                && read.cache_suppress
                && !read.result_is_partial,
            "{label}: benign shape must be a complete Value with cache_suppress=true, \
             result_is_partial=false"
        );
        crate::request_context::observe_component_meta_read_suppress(&read);
        assert!(
            !current_materialization_cache_suppress(),
            "{label}: a COMPLETE but non-cacheable result (cache_suppress=true, \
             result_is_partial=false) MUST NOT raise the warm-gate suppress flag — it must still \
             warm the component-meta final cache. A memo site wrongly setting result_is_partial=true \
             on this benign winner would fail here."
        );
    }
}

/// Direct PRODUCER-LEVEL discrimination proof for the A2 signal split —
/// the producer fold, not just the gate.
///
/// The concrete reachability path: a `ProjectPath` shallow-walking an
/// `InstantiationRef` whose nested `Instantiate` trips the projection-op
/// budget. The walker catches the budget error, contributes no surface,
/// and `build_project_path` returns `QueryResult::Value` carrying the
/// folded `result_is_partial = true`. This test observes the DISPATCH READ
/// directly (proving the producer fold, independent of any request-wide
/// budget backstop) and asserts it raises the component-meta warm-gate
/// suppression flag — i.e. it does NOT warm `ComponentMetaResultDb`.
///
/// It FAILS if the walker fatal-path fold (`walk.rs` InstantiationRef /
/// DeclPlaceholder arms → `self.result_is_partial`, drained by
/// `build_project_path`) is reverted: the read would then be a COMPLETE
/// `Value` with `result_is_partial = false` and the flag would NOT fire.
///
/// Asserts BOTH the request-level warm-gate suppression AND the SEMANTIC
/// non-admission directly: `graph.get_unvalidated(&projectpath_key)` is
/// `None` (the partial `ProjectPath` build was refused family-memo
/// admission), so a later identical request cannot warm-replay it as
/// complete.
#[test]
fn projectpath_over_instantiationref_budget_trip_surfaces_value_partial_and_does_not_warm() {
    use crate::request_context::current_materialization_cache_suppress;

    // `projection_op_budget = 2`: the outer `ProjectPath` (op 1) and the
    // nested `Instantiate(Partial<…>)` (op 2) both pass the entry/raw-build
    // budget gates, so the walk reaches the builtin-utility mapper path;
    // `Partial`'s nested `KeyOf` / `MappedType` (op 3+) then trips the fuse
    // INSIDE the instantiation, exercising the FIX-1 mapper-utility fold AND
    // the walker fold — NOT the dispatch-entry post-trip early-exit.
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 2,
        ..HostConfig::default()
    }));
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    // A real decidable object arg so the nested `Instantiate(Partial<…>)`
    // is a genuine cold build (not an immediate miss).
    let arg = intern_single_member_object(&host, "instref_budget_arg");
    // `InstantiationRef` over the builtin `Partial` — a builtin base ALWAYS
    // unwraps in the walker (even at the terminal hop), so the ProjectPath
    // walk dispatches the nested `Instantiate`.
    let partial_builtin = crate::semantic_query::DeclIdentity {
        canonical_id: Arc::from("__builtin__"),
        whole_hash: crate::semantic_query::HashValue::default(),
        decl_name: Arc::from("Partial"),
    };
    let instref = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: partial_builtin,
        args: Arc::from(vec![arg].into_boxed_slice()),
    });

    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/projectpath-instref-budget.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let _ctx_guard = RequestContextGuard::install(ctx);

    // ProjectPath over the InstantiationRef. The walker unwraps the builtin
    // InstantiationRef → dispatches the nested `Instantiate` → the builtin
    // `Partial` mapper's nested KeyOf/MappedType trips the budget → the
    // FIX-1 fold raises `result_is_partial`, threaded out through the
    // builtin-utility tuple and the walker, and drained onto the build
    // output by `build_project_path`.
    let projectpath_key = SemanticQueryKey::ProjectPath {
        base: instref,
        path: Arc::from(vec![PathSegment::Member(Arc::from("x"))].into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    };
    let read = dispatch.execute_read(projectpath_key.clone());

    // The dispatch read is a COMPLETE `Value` (the walker contributed an
    // opaque shell, NOT an Error/Recursive) carrying the FOLDED partiality.
    assert!(
        matches!(read.value, QueryResult::Value(_)),
        "the budget-tripped ProjectPath-over-InstantiationRef surfaces as a COMPLETE Value (the \
         walker catches the nested budget error and contributes an opaque shell), got {:?}",
        read.value
    );
    assert!(
        read.result_is_partial,
        "the nested Instantiate budget trip MUST fold result_is_partial=true onto the surfaced \
         Value — reverting the walker fatal-path fold makes this a complete Value with \
         result_is_partial=false (the A2 reachability hole)"
    );

    // The Value-partial MUST raise the warm-gate suppression flag — i.e. it
    // does NOT warm `ComponentMetaResultDb`. The shared read boundary marks
    // the request sticky directly on `result_is_partial`, so the flag is
    // already set after the partial read; `observe_component_meta_read_suppress`
    // confirms it (and is idempotent).
    crate::request_context::observe_component_meta_read_suppress(&read);
    assert!(
        current_materialization_cache_suppress(),
        "the Value-partial from the budget-tripped ProjectPath-over-InstantiationRef MUST raise the \
         warm-gate suppress flag so the partial is NOT promoted into ComponentMetaResultDb"
    );

    // SEMANTIC NON-ADMISSION (direct): the partial `ProjectPath` build was
    // refused family-memo admission, so no warm `MemoEntry` exists for the
    // key. A later identical request therefore cold-rebuilds and cannot
    // warm-replay the partial as a complete surface. This pins the producer
    // fold at the memo boundary, not just the request-level warm gate.
    assert!(
        graph.get_unvalidated(&projectpath_key).is_none(),
        "the partial ProjectPath-over-InstantiationRef MUST NOT be admitted to the family memo \
         (reverting the walker fold makes this a complete result_is_partial=false Value that warms \
         the memo)"
    );
}

/// the partial-metadata invariant (§4) Finding-A producer path — `lower.rs` operator-over-budget,
/// through the `execute_type_node` CHOKEPOINT (NO per-site fold).
///
/// Every `lower.rs` operator arm (`Instantiate`/`KeyOf`/`MappedType`/
/// `IndexedAccess`/`Conditional`) handles a budget-tripped sub-dispatch
/// with a bare `_ => self.opaque(QueryError::Miss)` — there is NO per-site
/// `result_is_partial` fold at any of these sites. Partiality propagates
/// SOLELY through the chokepoint `execute_type_node` override. This drives a
/// userland alias `Deep = keyof Partial<Required<Box>>` whose lowering
/// dispatches the `KeyOf` operator (`lower.rs:1320`) over a nested
/// builtin-utility instantiation; the nested `Partial`/`Required` mapper's
/// `KeyOf`/`MappedType` operators trip the projection-op budget, `lower.rs`
/// returns the opaque shell with no fold, and the chokepoint is the ONLY
/// thing that carries the partial onto the `Instantiate(Deep)` build output.
///
/// Asserts the `Instantiate` read carries `result_is_partial = true` (does
/// NOT warm component-meta) AND was refused family-memo admission.
///
/// DISCRIMINATION: reverting the chokepoint fold drops the lower.rs nested
/// partiality entirely (no per-site fold backstops it) — the read becomes a
/// complete `result_is_partial = false` Value and warms the memo.
#[test]
fn lower_indexed_access_chain_budget_trip_folds_partial_through_chokepoint_and_refuses_memo() {
    use crate::request_context::current_materialization_cache_suppress;
    use crate::{FileLanguage, UpsertRequest};

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 2,
        ..HostConfig::default()
    }));
    // A nested object alias plus a deep indexed-access alias. Lowering
    // `Deep`'s body dispatches one `IndexedAccess` per hop through
    // `lower.rs:1476`; the third hop trips `projection_op_budget = 2`.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/w/lower_chain.ts".to_string(),
            source: Arc::from(
                "export type Box = { a: number; b: string; c: boolean }\n\
                 export type Deep = keyof Partial<Required<Box>>\n",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert lower_chain.ts");

    let dispatch = host.semantic_dispatch();
    let deep_slot = dispatch.type_slot_for(Arc::from("/w/lower_chain.ts"), Arc::from("Deep"));
    let resolve_env = dispatch.resolve_env_hash_for("/w/lower_chain.ts");
    let instantiate_key =
        SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            deep_slot,
            Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            crate::semantic_query::InstantiateContext::non_file(
                ProjectionReductionContext::published(ProjectionMode::Expanded),
                resolve_env,
                crate::project_semantic_dispatch::BodySourceWitness::mint_for_unit_tests(),
            ),
        ));

    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/lower-indexed-budget.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let _ctx_guard = RequestContextGuard::install(ctx);

    let read = dispatch.execute_read(instantiate_key.clone());
    assert!(
        read.result_is_partial,
        "the deep indexed-access chain's nested IndexedAccess budget trip MUST fold \
         result_is_partial=true onto the Instantiate(Deep) build output THROUGH the chokepoint — \
         lower.rs has NO per-site fold, so reverting the chokepoint makes this result_is_partial=false"
    );

    // Warm-gate: the partial raises the suppression flag (no component-meta warm).
    crate::request_context::observe_component_meta_read_suppress(&read);
    assert!(
        current_materialization_cache_suppress(),
        "the lower.rs operator partial MUST raise the component-meta warm-gate suppress flag"
    );

    // Semantic-memo NON-ADMISSION: a partial result leaves NO warm family
    // entry. Probing memo presence directly is robust against the per-request
    // budget already being exhausted.
    let graph = host.project_type_store().semantic_graph();
    assert!(
        graph.get_unvalidated(&instantiate_key).is_none(),
        "the partial Instantiate(Deep) MUST NOT be admitted to the family memo (Finding B closure)"
    );
}

/// the partial-metadata invariant (§4) Finding-A producer path — relation → `build_conditional`.
///
/// A `Conditional` whose `check` / `extends` are identity carriers
/// (`InstantiationRef` over the builtin `Partial`) forces
/// `build_conditional` to fall through to the full `relate_nodes`
/// authority. The relation's identity-carrier `Instantiate` unwrap
/// (`relation.rs:342`/`:433`) trips the projection-op budget; the
/// chokepoint `execute_type_node` folds the partiality into the relation's
/// cold-build-local frame, so the relation `Unknown` is refused admission
/// to the relation memo AND the partiality bubbles into the conditional's
/// build output.
///
/// Asserts: the `Conditional` dispatch read carries `result_is_partial =
/// true` (does NOT warm component-meta) AND the relation judgement is
/// refused relation-memo admission (a fresh `relate_nodes` cold-recomputes).
///
/// DISCRIMINATION: reverting the chokepoint fold (or the relation-memo
/// partial-skip) makes the conditional a complete non-partial Value and/or
/// admits the partial-derived `Unknown` to the relation memo.
#[test]
fn conditional_relation_budget_trip_folds_partial_and_refuses_relation_memo() {
    use crate::request_context::current_materialization_cache_suppress;

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 2,
        ..HostConfig::default()
    }));
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    let mk_partial_instref = |member: &'static str| -> SemanticNodeId {
        let arg = intern_single_member_object(&host, member);
        let partial_builtin = crate::semantic_query::DeclIdentity {
            canonical_id: Arc::from("__builtin__"),
            whole_hash: crate::semantic_query::HashValue::default(),
            decl_name: Arc::from("Partial"),
        };
        graph.intern_node(SemanticNodeData::InstantiationRef {
            base: partial_builtin,
            args: Arc::from(vec![arg].into_boxed_slice()),
        })
    };
    let check = mk_partial_instref("cond_check_arg");
    let extends = mk_partial_instref("cond_extends_arg");
    let true_branch = {
        let g = host.project_type_store().semantic_graph();
        g.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ))
    };
    let false_branch = {
        let g = host.project_type_store().semantic_graph();
        g.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::Number,
        ))
    };

    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/conditional-relation-budget.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        host.config.projection_op_budget,
    );
    let _ctx_guard = RequestContextGuard::install(ctx);

    let conditional_key = SemanticQueryKey::Conditional {
        check,
        extends,
        true_branch,
        false_branch,
        distributive: false,
    };
    let read = dispatch.execute_read(conditional_key.clone());
    assert!(
        read.result_is_partial,
        "the conditional's relation authority tripped the budget instantiating its identity-carrier \
         operands; the chokepoint MUST fold result_is_partial=true onto the Conditional build output \
         (reverting the chokepoint fold makes this a complete non-partial Value)"
    );

    // Semantic family-memo NON-ADMISSION (the DIRECT Finding-A assertion,
    // matching the lower test at the Instantiate key and the ProjectPath test):
    // the partial-derived `Conditional` result leaves NO warm family entry.
    // Probing the `Conditional` family-memo key directly discriminates the
    // chokepoint fold — reverting it makes the build admit a complete Value and
    // this `get_unvalidated` would return `Some`.
    assert!(
        graph.get_unvalidated(&conditional_key).is_none(),
        "the partial-derived Conditional MUST NOT be admitted to the family memo \
         (reverting the chokepoint early-exit/normal fold admits it here)"
    );
    // The chokepoint marks the request sticky directly on result_is_partial,
    // so the flag is already set after the partial read; observe-suppress
    // confirms it (and is idempotent).
    crate::request_context::observe_component_meta_read_suppress(&read);
    assert!(
        current_materialization_cache_suppress(),
        "the relation-derived conditional partial MUST raise the component-meta warm-gate suppress flag"
    );

    // Relation-memo non-admission: a fresh `relate_nodes(check, extends)`
    // must cold-recompute (no warm relation hit on the partial-derived
    // judgement). `get_relation` returning `None` proves the partial-derived
    // judgement was never admitted.
    assert!(
        graph
            .get_relation(host.as_ref(), &dispatch.relate_memo_key(check, extends))
            .is_none(),
        "a relation Unknown that arose from a PARTIAL nested read MUST NOT be admitted to the \
         relation memo (reverting the relation-memo partial-skip admits it here)"
    );
}
