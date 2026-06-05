//! Smoke tests for the `CaptureToken` test harness.
//!
//! Validates the §16 harness contract:
//! - `CaptureToken::start_for_query(...)` binds a per-request token
//! - `CaptureGuard::end()` returns an immutable snapshot
//! - `with_active_capture(...)` is a no-op when no token is bound
//! - Origin-edge ledger dedups on the full identity tuple
//! - Parse-count snapshot records per canonical id
//! - Nested `start_for_query` panics
//! - `assert_no_stack_overflow!` catches deliberate overflow
//! - `KeyFamily::matches` resolves names via the active scope

use std::sync::Arc;

use verter_session::for_tests::{
    assert_no_stack_overflow, with_active_capture, CaptureToken, EdgeIdentity, KeyFamily,
};

// Re-export the shared semantic types the harness consumes.
use verter_session::semantic_query::{
    DeclIdentity, OriginEdgeKind, ResolveDeclKey, ScopeId, SemanticNodeId, SemanticQueryKey,
};

#[test]
fn empty_snapshot_has_no_counters() {
    let guard = CaptureToken::start_for_query("empty_snapshot");
    let snap = guard.end();
    assert_eq!(snap.counters.len(), 0);
    assert_eq!(snap.parse_count.len(), 0);
    assert_eq!(snap.edge_ledger.len(), 0);
    assert_eq!(snap.dispatch_log.len(), 0);
    assert_eq!(snap.edge_count(), 0);
    assert_eq!(snap.duplicate_edge_count(), 0);
    assert_eq!(snap.counter("any_name"), 0);
}

#[test]
fn counter_increment_under_active_token() {
    let guard = CaptureToken::start_for_query("counter_increment");
    for _ in 0..3 {
        with_active_capture(|t| t.record_counter("foo", 1));
    }
    let snap = guard.end();
    assert_eq!(snap.counter("foo"), 3);
    assert_eq!(snap.counter("bar"), 0);
}

#[test]
fn parse_count_records_per_canonical_id() {
    let guard = CaptureToken::start_for_query("parse_count");
    with_active_capture(|t| t.record_parse("/foo.ts"));
    with_active_capture(|t| t.record_parse("/foo.ts"));
    with_active_capture(|t| t.record_parse("/bar.ts"));
    let snap = guard.end();
    assert_eq!(snap.parse_count_for("/foo.ts"), 2);
    assert_eq!(snap.parse_count_for("/bar.ts"), 1);
    assert_eq!(snap.parse_count_for("/missing.ts"), 0);
}

#[test]
fn origin_edge_ledger_dedups_by_identity_tuple() {
    let guard = CaptureToken::start_for_query("origin_edge_dedup");
    let result = SemanticNodeId(42);
    let src = SemanticNodeId(7);
    let kind = OriginEdgeKind::AliasResolve;
    // Same identity tuple recorded twice → 1 unique edge, 1 duplicate.
    let id_a = EdgeIdentity::new(result, kind, vec![src], 0xC0FFEE, 0xBEEF);
    let id_b = EdgeIdentity::new(result, kind, vec![src], 0xC0FFEE, 0xBEEF);
    with_active_capture(|t| t.record_edge(id_a));
    with_active_capture(|t| t.record_edge(id_b));
    // Different dep_signature → legitimate different derivation, NOT a duplicate.
    let id_c = EdgeIdentity::new(result, kind, vec![src], 0xDEADBEEF, 0xBEEF);
    with_active_capture(|t| t.record_edge(id_c));
    // Different metadata_hash → also legitimate, NOT a duplicate.
    let id_d = EdgeIdentity::new(result, kind, vec![src], 0xC0FFEE, 0xCAFE);
    with_active_capture(|t| t.record_edge(id_d));
    let snap = guard.end();
    // Three distinct identity tuples present in the ledger.
    assert_eq!(snap.edge_count(), 3);
    // One duplicate detected (id_b matched id_a).
    assert_eq!(snap.duplicate_edge_count(), 1);
}

#[test]
fn origin_edge_ledger_normalizes_source_order() {
    let guard = CaptureToken::start_for_query("origin_edge_sort");
    let result = SemanticNodeId(99);
    let kind = OriginEdgeKind::Normalize;
    // Same sources in different order → must collide after sort+dedup.
    let id_a = EdgeIdentity::new(
        result,
        kind,
        vec![SemanticNodeId(1), SemanticNodeId(2), SemanticNodeId(3)],
        0xAA,
        0xBB,
    );
    let id_b = EdgeIdentity::new(
        result,
        kind,
        vec![SemanticNodeId(3), SemanticNodeId(1), SemanticNodeId(2)],
        0xAA,
        0xBB,
    );
    // Duplicates within sources collapse via dedup as well.
    let id_c = EdgeIdentity::new(
        result,
        kind,
        vec![
            SemanticNodeId(1),
            SemanticNodeId(1),
            SemanticNodeId(2),
            SemanticNodeId(3),
        ],
        0xAA,
        0xBB,
    );
    with_active_capture(|t| t.record_edge(id_a));
    with_active_capture(|t| t.record_edge(id_b));
    with_active_capture(|t| t.record_edge(id_c));
    let snap = guard.end();
    assert_eq!(snap.edge_count(), 1);
    assert_eq!(snap.duplicate_edge_count(), 2);
}

#[test]
#[should_panic(expected = "nested CaptureToken::start_for_query")]
fn nested_start_for_query_panics() {
    let _outer = CaptureToken::start_for_query("outer");
    let _inner = CaptureToken::start_for_query("inner");
}

#[test]
fn assert_no_stack_overflow_catches_overflow() {
    // The macro converts a panic message containing "stack overflow"
    // (case-insensitive) into `Err(StackOverflow)`. This tests the
    // detection pathway without relying on the platform-specific OS
    // recovery from a real guard-page hit (which on Windows aborts the
    // entire test process — see `assert_no_stack_overflow` doc).
    let result = assert_no_stack_overflow(|| {
        panic!("stack overflow: simulated guard-page hit");
    });
    assert!(result.is_err(), "expected Err(StackOverflow), got Ok");
}

#[test]
fn assert_no_stack_overflow_propagates_unrelated_panics() {
    // The macro must NOT swallow non-overflow panics — those should
    // propagate so test failures are visible. The propagated panic
    // surfaces here as a #[should_panic]-shaped expectation handled
    // in a child closure: we wrap in `catch_unwind` to observe the
    // re-raised panic without the test framework reporting failure.
    let result = std::panic::catch_unwind(|| {
        let _ = assert_no_stack_overflow(|| {
            panic!("unrelated assertion failure");
        });
    });
    assert!(result.is_err(), "expected re-raised panic, got Ok");
}

#[test]
fn assert_no_stack_overflow_passes_for_normal_closure() {
    let result = assert_no_stack_overflow(|| 1 + 2 + 3);
    assert_eq!(result.expect("non-recursive closure must succeed"), 6);
}

#[test]
fn key_family_matches_resolve_decl_for_resolved_name() {
    // ResolveDecl variant — the harness can't resolve names without a real
    // resolver, so it matches by name string equality on the inner key.
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from("/scope.ts"),
            local_scope: None,
        },
        name: Arc::from("UIMessage"),
    });
    assert!(KeyFamily::AnyDispatch.matches(&key));
    // Non-Instantiate dispatches do not match the Instantiate family even
    // when the name is the same.
    assert!(!KeyFamily::InstantiateForResolvedName("UIMessage").matches(&key));
}

#[test]
fn key_family_matches_instantiate_for_resolved_name() {
    let key = SemanticQueryKey::Instantiate {
        base: DeclIdentity::synthetic("UIMessage").to_type_slot_unscoped(),
        args: Arc::new([]),
        context: verter_session::semantic_query::InstantiateContext::new(verter_session::semantic_query::ProjectionReductionContext::published(
            verter_session::semantic_query::ProjectionMode::Skeleton,
        ), Default::default()),
    };
    assert!(KeyFamily::InstantiateForResolvedName("UIMessage").matches(&key));
    assert!(!KeyFamily::InstantiateForResolvedName("OtherName").matches(&key));
    assert!(KeyFamily::AnyDispatch.matches(&key));
    // Slot bindings never match Instantiate.
    assert!(!KeyFamily::SlotBindingDispatch.matches(&key));
}

#[test]
fn production_paths_are_no_op_without_active_token() {
    // No active token; calls must not panic and must not retain state.
    with_active_capture(|t| t.record_counter("orphan_counter", 1));
    with_active_capture(|t| t.record_parse("/orphan.ts"));
    with_active_capture(|t| {
        t.record_edge(EdgeIdentity::new(
            SemanticNodeId(1),
            OriginEdgeKind::AliasResolve,
            vec![SemanticNodeId(2)],
            0,
            0,
        ))
    });
    // Open a token now — no orphan state should leak in.
    let guard = CaptureToken::start_for_query("post_orphan");
    let snap = guard.end();
    assert_eq!(snap.counters.len(), 0);
    assert_eq!(snap.parse_count.len(), 0);
    assert_eq!(snap.edge_ledger.len(), 0);
}

#[test]
fn dispatch_log_records_under_active_token() {
    let guard = CaptureToken::start_for_query("dispatch_log");
    let key = SemanticQueryKey::Instantiate {
        base: DeclIdentity::synthetic("UIMessage").to_type_slot_unscoped(),
        args: Arc::new([]),
        context: verter_session::semantic_query::InstantiateContext::new(verter_session::semantic_query::ProjectionReductionContext::published(
            verter_session::semantic_query::ProjectionMode::Skeleton,
        ), Default::default()),
    };
    with_active_capture(|t| t.record_dispatch(&key, /* hit */ true));
    with_active_capture(|t| t.record_dispatch(&key, /* hit */ false));
    let snap = guard.end();
    assert_eq!(
        snap.dispatch_count(KeyFamily::InstantiateForResolvedName("UIMessage")),
        2
    );
    assert_eq!(
        snap.dispatch_misses(KeyFamily::InstantiateForResolvedName("UIMessage")),
        1
    );
    assert_eq!(snap.dispatch_count(KeyFamily::AnyDispatch), 2);
    assert_eq!(snap.dispatch_count(KeyFamily::SlotBindingDispatch), 0);
}

#[test]
fn key_family_matches_instantiate_expanded_for_resolved_name() {
    // Mode-aware Instantiate gating used by the Phase 4 field-level
    // fast-path counterfixtures. The variant must match
    // `body_mode == Expanded` and reject `Skeleton` / `Shallow` for
    // the same name, plus reject other names entirely.
    use verter_session::semantic_query::ProjectionMode;

    // body_mode == Expanded → matches when name matches.
    let key_expanded = SemanticQueryKey::Instantiate {
        base: DeclIdentity::synthetic("UIMessage").to_type_slot_unscoped(),
        args: Arc::new([]),
        context: verter_session::semantic_query::InstantiateContext::new(verter_session::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ), Default::default()),
    };
    assert!(KeyFamily::InstantiateExpandedForResolvedName("UIMessage").matches(&key_expanded));
    assert!(!KeyFamily::InstantiateExpandedForResolvedName("OtherName").matches(&key_expanded));
    // Distinguishing from mode-agnostic InstantiateForResolvedName: the
    // unrestricted family also matches the Expanded key (its semantics
    // do not gate on body_mode).
    assert!(KeyFamily::InstantiateForResolvedName("UIMessage").matches(&key_expanded));

    // body_mode == Skeleton → must NOT match the Expanded family.
    let key_skeleton = SemanticQueryKey::Instantiate {
        base: DeclIdentity::synthetic("UIMessage").to_type_slot_unscoped(),
        args: Arc::new([]),
        context: verter_session::semantic_query::InstantiateContext::new(verter_session::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Skeleton,
        ), Default::default()),
    };
    assert!(!KeyFamily::InstantiateExpandedForResolvedName("UIMessage").matches(&key_skeleton));
    // Correct family for Skeleton still matches.
    assert!(KeyFamily::SkeletonForResolvedName("UIMessage").matches(&key_skeleton));

    // body_mode == Shallow → must NOT match the Expanded family.
    let key_shallow = SemanticQueryKey::Instantiate {
        base: DeclIdentity::synthetic("UIMessage").to_type_slot_unscoped(),
        args: Arc::new([]),
        context: verter_session::semantic_query::InstantiateContext::new(verter_session::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ), Default::default()),
    };
    assert!(!KeyFamily::InstantiateExpandedForResolvedName("UIMessage").matches(&key_shallow));
    assert!(KeyFamily::ShallowForResolvedName("UIMessage").matches(&key_shallow));

    // Non-Instantiate dispatch never matches.
    let resolve_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from("/scope.ts"),
            local_scope: None,
        },
        name: Arc::from("UIMessage"),
    });
    assert!(!KeyFamily::InstantiateExpandedForResolvedName("UIMessage").matches(&resolve_key));
}
