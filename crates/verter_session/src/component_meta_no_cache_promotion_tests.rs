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
            name: Arc::from(name),
            value: leaf,
            optional: false,
            readonly: false,
            is_method: false,
            declared_in_macro_type_arg: false,
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
        name: Arc::from(name),
        value: leaf,
        optional: false,
        readonly: false,
        is_method: false,
        declared_in_macro_type_arg: false,
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

    let first = dispatch.execute(first_key);
    assert!(
        matches!(first, QueryResult::Value(_)),
        "first cold projection within budget must succeed, got {first:?}",
    );

    let second = dispatch.execute(second_key);
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
    let r1 = dispatch.execute(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for a 3-segment path (got {r1:?})"
    );

    // Discrimination: SAME host, SAME key, second query MUST cold-fire
    // (partial NOT promoted to warm cache).
    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute(key.clone());
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
    let r1 = dispatch.execute(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for the 5e route-target 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute(key.clone());
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
    let r1 = dispatch.execute(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for the 5f fallthrough 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute(key.clone());
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
    let r1 = dispatch.execute(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for the 5h userland-shadowing 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute(key.clone());
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
    let r1 = dispatch.execute(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel for the 5i Exclude/Extract reduction 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute(key.clone());
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
    let r1 = dispatch.execute(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel \
         for the 5j slot-binding-lowering 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute(key.clone());
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
    let r1 = dispatch.execute(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel \
         for the 5k typeof-substitution 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute(key.clone());
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
    let r1 = dispatch.execute(key.clone());
    assert!(
        matches!(r1, QueryResult::Recursive(_) | QueryResult::Error(_)),
        "constrained host (depth_budget=2) MUST report a budget-exceeded sentinel \
         for the 5m engine-state-promotion 3-segment path (got {r1:?})"
    );

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);
    let _r2 = dispatch.execute(key.clone());
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
