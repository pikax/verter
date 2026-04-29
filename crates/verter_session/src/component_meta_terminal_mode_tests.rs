//! §5.D.3 terminal-mode-only-expansion tests for the §5.B path-projection
//! closures (Phase 5g-supplement backfill for 5e/5f).
//!
//! Per the path-precise rule: intermediate hops run in `Navigate`,
//! the terminal hop runs in the caller's mode. Each test issues a
//! multi-hop `ProjectPath` query in `Expanded` mode, then peeks the
//! warm cache via `dispatch_trace_for(&key)` to assert that:
//!
//! 1. Every intermediate sub-key was published in `Navigate` mode.
//! 2. Only the terminal sub-key was published in the caller's
//!    `Expanded` mode.
//!
//! Uses the §5.D.0 r17 instrumentation surface
//! (`dispatch_trace_for(&key)` → `DispatchTrace::path_decomposition()`
//! → `SubKey::mode()`) which lives behind bare `#[cfg(test)]` per
//! r17/N12.
//!
//! Plan: §5.D.3 (Phase 5g-supplement.1.B for 5e/5f backfill).

use std::sync::Arc;

use crate::semantic_query::{
    PathSegment, ProjectionMode, SemanticNodeData, SemanticNodeId, SemanticQueryApi,
    SemanticQueryKey, SurfaceMember, SurfaceView,
};
use crate::types::HostConfig;
use crate::VerterHost;

/// Build a fresh host with the default config (depth_budget == MAX
/// so the walker completes the full path).
fn build_test_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }))
}

/// Intern a chain of nested Object surfaces:
///
/// `{ a: { b: { full: { bar: <leaf> } } } }`
///
/// where `<leaf>` is an empty Object. Used to exercise multi-hop
/// `ProjectPath` projections.
fn intern_four_hop_object(host: &VerterHost) -> SemanticNodeId {
    let graph = host.project_type_store().semantic_graph();
    let leaf = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(Vec::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));
    let mut current = leaf;
    for name in ["bar", "full", "b", "a"] {
        let member = SurfaceMember {
            name: Arc::from(name),
            value: current,
            optional: false,
            readonly: false,
            is_method: false,
        };
        current = graph.intern_node(SemanticNodeData::Object(SurfaceView {
            members: Arc::from(vec![member].into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }));
    }
    current
}

/// 5e §5.D.3 — `route_target_pick_omit` intermediate hops Navigate,
/// terminal hop Expanded. Multi-hop `ProjectPath` query with mode
/// `Expanded`; assert via dispatch trace that intermediate sub-keys
/// ran in `Navigate` and only the terminal hop ran in `Expanded`.
#[test]
fn intermediate_hops_navigate_terminal_only_expanded_for_route_target_pick_omit() {
    let host = build_test_host();
    let base = intern_four_hop_object(&host);
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("a")),
                PathSegment::Member(Arc::from("b")),
                PathSegment::Member(Arc::from("full")),
                PathSegment::Member(Arc::from("bar")),
            ]
            .into_boxed_slice(),
        ),
        mode: ProjectionMode::Expanded,
    };

    // Drive the dispatch — this populates the warm cache (terminal
    // entry in Expanded mode, intermediate prefixes in Navigate
    // mode via backfill_prefixes).
    let _ = host.semantic_dispatch().execute(key.clone());

    // Read the post-hoc trace.
    let trace = host.dispatch_trace_for(&key);
    let decomposition = trace.path_decomposition();
    assert_eq!(
        decomposition.len(),
        4,
        "trace should decompose the 4-segment path into 4 hops (got {})",
        decomposition.len()
    );

    for (i, sub_key) in decomposition.iter().enumerate() {
        let is_terminal = i == decomposition.len() - 1;
        match (sub_key.mode(), is_terminal) {
            (ProjectionMode::Navigate, false) => {} // expected
            (ProjectionMode::Expanded, true) => {}  // expected
            (mode, terminal) => panic!(
                "hop {i} (terminal={terminal}) ran in {mode:?} (expected Navigate for intermediate, Expanded for terminal)"
            ),
        }
    }
}
