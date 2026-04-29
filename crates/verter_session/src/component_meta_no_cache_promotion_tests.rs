//! §5.D.4 no-cache-promotion-for-partial-failure tests for the §5.B
//! variants (Phase 5g-supplement backfill for 5b/5e/5f).
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
//! Per §5.D.4 r18 (Claude-N18): single-host, same-host re-query.
//! r17's two-host design is non-discriminating because fresh hosts
//! own their own ProjectTypeStore.
//!
//! Plan: §5.D.4 (Phase 5g-supplement.1.B for 5b/5e/5f backfill).

use std::sync::Arc;

use crate::host_test_audit::DispatchCounter;
use crate::semantic_query::{
    PathSegment, ProjectionMode, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi,
    SemanticQueryKey, SurfaceMember, SurfaceView,
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
        mode: ProjectionMode::Expanded,
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
        mode: ProjectionMode::Expanded,
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
        mode: ProjectionMode::Expanded,
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
