//! Slice 0.2 (Wave 0) — global counter regression test.
//!
//! The Decision #5 helper writes the global stat unconditionally and
//! mirrors to the per-request stat only when a context is installed.
//! This test pins the unconditional global write — it is what
//! discriminates the v4.1 single-helper design from the rejected v4
//! drop-time aggregation design (which would have moved the global
//! into a finalize step and broken non-audited callers).

use std::sync::Arc;

use verter_session::for_tests::{empty_signature_for_tests, SemanticGraphStore};
use verter_session::request_context::current_request_context;
use verter_session::semantic_query::{
    PrimitiveKind, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticQueryKey,
};

fn scope(canonical: &str) -> ScopeId {
    ScopeId {
        canonical_id: Arc::from(canonical),
        local_scope: None,
    }
}

/// Without any `RequestContext` installed, the global
/// `cold_aborts_swept` counter must still tick — the helper is
/// dual-target, not exclusive-target. Existing global-stats
/// observers (telemetry, Prometheus exporters, debug dumps) rely on
/// this invariant.
#[test]
fn cold_abort_sweep_global_counter_increments_without_request_context() {
    // Sanity: this test runs without an audited request scope.
    assert!(
        current_request_context().is_none(),
        "test prelude expects an empty TLS — found an installed context. \
         Another test leaked its RequestContextGuard."
    );

    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    let _force_guard = SemanticGraphStore::test_force_cold_abort_sweep();

    let result = store.execute_cooperative(
        key.clone(),
        || {
            store.intern_node(SemanticNodeData::Opaque(
                verter_session::semantic_query::QueryError::Miss,
            ))
        },
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature_for_tests())
        },
    );
    assert!(matches!(result.value, QueryResult::Value(_)));

    let snap = store.stats_snapshot();
    assert_eq!(
        snap.cold_aborts_swept, 1,
        "global stats.cold_aborts_swept must increment for non-audited callers \
         (got {}). If this regresses, Slice 0.2's helper has accidentally \
         moved the global write behind a per-request guard — that would \
         break every existing telemetry consumer that reads stats_snapshot.",
        snap.cold_aborts_swept,
    );
}
