//! Terminal-mode-only-expansion tests for the path-projection closures.
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
//! Uses the test-only `dispatch_trace_for(&key)` →
//! `DispatchTrace::path_decomposition()` → `SubKey::mode()`
//! instrumentation surface which lives behind bare `#[cfg(test)]`.

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
            declared_in_macro_type_arg: false,
            merge_role: crate::semantic_query::MemberMergeRole::Authored,
            spans: Default::default(),
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
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

/// 5f §5.D.3 — `fallthrough_inheritance` intermediate hops Navigate,
/// terminal hop Expanded. Mirror of the 5e test for the
/// fallthrough-inheritance closure: a multi-hop ProjectPath in
/// Expanded mode must populate intermediate cache entries in
/// Navigate and only the terminal in Expanded.
#[test]
fn intermediate_hops_navigate_terminal_only_expanded_for_fallthrough_inheritance() {
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let _ = host.semantic_dispatch().execute(key.clone());

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
                "fallthrough hop {i} (terminal={terminal}) ran in {mode:?} (expected Navigate for intermediate, Expanded for terminal)"
            ),
        }
    }
}

/// 5h §5.D.3 — `userland_shadowing_pick` intermediate hops Navigate,
/// terminal hop Expanded. Mirror of the 5e/5f tests for the
/// userland-shadowing closure: a multi-hop ProjectPath in Expanded
/// mode must populate intermediate cache entries in Navigate and
/// only the terminal in Expanded. The path-precise contract
/// (CLAUDE.md "Macro Type Traversal Rule") applies to ALL
/// path-projection sub-phases including 5h's resolver-context
/// shadow-gate thread.
///
/// The shadow-gate thread itself does not change the
/// path-decomposition behaviour (terminal-mode-only expansion is a
/// dispatch-engine invariant, not a shadow-gate concern), so the
/// test re-uses the four-hop Object chain pattern. The
/// discriminating proof remains: a path ending at the terminal
/// hop runs the terminal in caller's mode, intermediates in
/// Navigate.
#[test]
fn intermediate_hops_navigate_terminal_only_expanded_for_userland_shadowing_pick() {
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let _ = host.semantic_dispatch().execute(key.clone());

    let trace = host.dispatch_trace_for(&key);
    let decomposition = trace.path_decomposition();
    assert_eq!(
        decomposition.len(),
        4,
        "trace should decompose the 4-segment path into 4 hops for the userland-shadowing scenario (got {})",
        decomposition.len()
    );

    for (i, sub_key) in decomposition.iter().enumerate() {
        let is_terminal = i == decomposition.len() - 1;
        match (sub_key.mode(), is_terminal) {
            (ProjectionMode::Navigate, false) => {} // expected
            (ProjectionMode::Expanded, true) => {}  // expected
            (mode, terminal) => panic!(
                "userland-shadowing hop {i} (terminal={terminal}) ran in {mode:?} \
                 (expected Navigate for intermediate, Expanded for terminal — \
                 the resolver-context shadow gate must NOT alter path-decomposition modes)"
            ),
        }
    }
}

/// 5i §5.D.3 — `Exclude<>` / `Extract<>` reduction intermediate
/// hops Navigate, terminal hop Expanded. Mirror of the 5e/5f/5h
/// tests for the literal-type reduction landed in 5i: a multi-hop
/// ProjectPath in Expanded mode must populate intermediate cache
/// entries in Navigate and only the terminal in Expanded. The
/// path-precise contract (CLAUDE.md "Macro Type Traversal Rule")
/// applies to ALL path-projection sub-phases including 5i's
/// `Exclude` / `Extract` reduction.
///
/// The Exclude/Extract reduction itself does not change the
/// path-decomposition behaviour (terminal-mode-only expansion is a
/// dispatch-engine invariant; the reduction lives behind
/// `build_builtin_utility` and is invoked from the Instantiate
/// dispatch, not from path projection). The discriminating proof
/// remains: a path ending at the terminal hop runs the terminal
/// in caller's mode, intermediates in Navigate.
#[test]
fn intermediate_hops_navigate_terminal_only_expanded_for_exclude_extract_reduction() {
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let _ = host.semantic_dispatch().execute(key.clone());

    let trace = host.dispatch_trace_for(&key);
    let decomposition = trace.path_decomposition();
    assert_eq!(
        decomposition.len(),
        4,
        "trace should decompose the 4-segment path into 4 hops for the Exclude/Extract reduction scenario (got {})",
        decomposition.len()
    );

    for (i, sub_key) in decomposition.iter().enumerate() {
        let is_terminal = i == decomposition.len() - 1;
        match (sub_key.mode(), is_terminal) {
            (ProjectionMode::Navigate, false) => {} // expected
            (ProjectionMode::Expanded, true) => {}  // expected
            (mode, terminal) => panic!(
                "Exclude/Extract reduction hop {i} (terminal={terminal}) ran in {mode:?} \
                 (expected Navigate for intermediate, Expanded for terminal — \
                 the new Extract/Exclude arm must NOT alter path-decomposition modes)"
            ),
        }
    }
}

/// Slot-binding-parameter lowering: intermediate hops Navigate,
/// terminal hop Expanded. The `project_slot_binding_member` helper
/// composes existing variants and explicitly threads `Navigate`
/// through `ProjectPath { base, [Member(slot)], Navigate }`
/// (intermediate hop), then runs the terminal
/// `ProjectPath { base: param0_ty, [Member(binding)], <caller_mode> }`
/// in the caller's mode. The path-precise contract
/// (CLAUDE.md "Macro Type Traversal Rule") applies to ALL
/// path-projection sub-phases including the helper-composed
/// dispatch tested here.
///
/// The slot-binding-parameter lowering itself does not change the
/// path-decomposition behaviour (terminal-mode-only expansion is a
/// dispatch-engine invariant; the helper threads the modes
/// per-hop, not all-or-nothing). The discriminating proof remains:
/// a path ending at the terminal hop runs the terminal in caller's
/// mode, intermediates in Navigate.
#[test]
fn intermediate_hops_navigate_terminal_only_expanded_for_slot_binding_lowering() {
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let _ = host.semantic_dispatch().execute(key.clone());

    let trace = host.dispatch_trace_for(&key);
    let decomposition = trace.path_decomposition();
    assert_eq!(
        decomposition.len(),
        4,
        "trace should decompose the 4-segment path into 4 hops for the slot-binding-lowering scenario (got {})",
        decomposition.len()
    );

    for (i, sub_key) in decomposition.iter().enumerate() {
        let is_terminal = i == decomposition.len() - 1;
        match (sub_key.mode(), is_terminal) {
            (ProjectionMode::Navigate, false) => {} // expected
            (ProjectionMode::Expanded, true) => {}  // expected
            (mode, terminal) => panic!(
                "slot-binding-lowering hop {i} (terminal={terminal}) ran in {mode:?} \
                 (expected Navigate for intermediate, Expanded for terminal — \
                 the `project_slot_binding_member` helper must NOT alter \
                 path-decomposition modes)"
            ),
        }
    }
}

/// Value-member typeof substitution: intermediate hops Navigate,
/// terminal hop Expanded. `shallow_lower_type_expr`'s
/// `TypeExpr::TypeOf` arm explicitly projects the tail segments via
/// `ProjectPath { mode: Navigate }` (intermediate, terminal-as-leaf
/// when only one segment remains). The path-precise contract
/// (CLAUDE.md "Macro Type Traversal Rule") applies: when an outer
/// caller dispatches a deeper `ProjectPath` in `Expanded` mode
/// against the typeof-substituted result, intermediate hops MUST
/// run in `Navigate` and only the terminal hop in `Expanded`.
///
/// The typeof-substitution fix itself does not change
/// path-decomposition behaviour (terminal-mode-only expansion is a
/// dispatch-engine invariant; the new lookup composes the same
/// `ProjectPath` query, not a sibling family). The discriminating
/// proof remains: a path ending at the terminal hop runs the
/// terminal in caller's mode, intermediates in Navigate.
#[test]
fn intermediate_hops_navigate_terminal_only_expanded_for_typeof_substitution() {
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let _ = host.semantic_dispatch().execute(key.clone());

    let trace = host.dispatch_trace_for(&key);
    let decomposition = trace.path_decomposition();
    assert_eq!(
        decomposition.len(),
        4,
        "trace should decompose the 4-segment path into 4 hops for the typeof-substitution scenario (got {})",
        decomposition.len()
    );

    for (i, sub_key) in decomposition.iter().enumerate() {
        let is_terminal = i == decomposition.len() - 1;
        match (sub_key.mode(), is_terminal) {
            (ProjectionMode::Navigate, false) => {} // expected
            (ProjectionMode::Expanded, true) => {}  // expected
            (mode, terminal) => panic!(
                "typeof-substitution hop {i} (terminal={terminal}) ran in {mode:?} \
                 (expected Navigate for intermediate, Expanded for terminal — \
                 the §5.13 single-segment-first lookup must NOT alter \
                 path-decomposition modes)"
            ),
        }
    }
}

/// `engine_state_promotion`: intermediate hops Navigate, terminal
/// hop Expanded. External engine-method callsites that route through
/// the bridge helpers must not change the underlying dispatch
/// path-decomposition contract (CLAUDE.md "Macro Type Traversal
/// Rule"). The discriminating proof: a multi-hop ProjectPath query
/// in `Expanded` mode produces a decomposition where intermediate
/// sub-keys ran in `Navigate` and only the terminal hop ran in
/// `Expanded`.
///
/// A regression in the bridge migration that accidentally promoted
/// intermediate hops to `Expanded` (e.g. by routing route-fast-path
/// through a sibling family that loses the mode-decomposition) would
/// surface here as a non-Navigate intermediate.
#[test]
fn intermediate_hops_navigate_terminal_only_expanded_for_engine_state_promotion() {
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let _ = host.semantic_dispatch().execute(key.clone());

    let trace = host.dispatch_trace_for(&key);
    let decomposition = trace.path_decomposition();
    assert_eq!(
        decomposition.len(),
        4,
        "trace should decompose the 4-segment path into 4 hops for the \
         engine-state-promotion scenario (got {})",
        decomposition.len()
    );

    for (i, sub_key) in decomposition.iter().enumerate() {
        let is_terminal = i == decomposition.len() - 1;
        match (sub_key.mode(), is_terminal) {
            (ProjectionMode::Navigate, false) => {} // expected
            (ProjectionMode::Expanded, true) => {}  // expected
            (mode, terminal) => panic!(
                "engine-state-promotion hop {i} (terminal={terminal}) ran in {mode:?} \
                 (expected Navigate for intermediate, Expanded for terminal — \
                 the 5m bridge migration must NOT alter path-decomposition modes)"
            ),
        }
    }
}
