//! Counter-helper feature-acceptance test.
//!
//! Validates the post-helper invariant: the counter helpers
//! (`record_inflight_aborted_retry` / `record_cold_abort_swept`)
//! write the per-request atomic on the `RequestContext` installed in
//! TLS at the call site. The audit miner reads those per-request
//! atomics at `component_meta_audit/footprint_miner.rs::CacheOutcomeTally`.
//!
//! This file is feature-acceptance (it pins that the helper reaches
//! the right per-request atomic), not characterization. The
//! discriminator for the helper-vs-bypass invariant lives in the
//! architecture guard `audit_counter_single_helper` in
//! `architecture_guards.rs`, which fails on any direct
//! `self.stats.<counter>.fetch_add` outside the helper bodies. The
//! production architecture has supported per-request attribution via
//! scheduler-trait dispatch (`record_cache_event` in
//! `request_context.rs`) before the helper landed and continues to
//! support it after — so these runtime tests are not characterization
//! and would not by themselves discriminate pre-change from
//! post-change. They provide regression coverage for the helper's
//! per-request mirror reaching the right atomic counter.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_session::for_tests::{
    empty_signature_for_tests, test_trigger_inflight_abort, SemanticGraphStore,
};
use verter_session::request_context::{RequestContext, RequestContextGuard};
use verter_session::semantic_query::{
    PrimitiveKind, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticQueryKey,
};

fn scope(canonical: &str) -> ScopeId {
    ScopeId {
        canonical_id: Arc::from(canonical),
        local_scope: None,
    }
}

/// Drive the production cold-abort path with an installed
/// `RequestContext` and confirm the per-request `cold_aborts_swept`
/// counter is bumped — this is what the audit miner reads at
/// `component_meta_audit/footprint_miner.rs::CacheOutcomeTally`.
///
/// The counter helper consults `current_request_context()` directly
/// and bumps both global stats AND per-request when a context is
/// installed. The architecture-guard `audit_counter_single_helper`
/// proves the helper is the only writer; this test proves the
/// per-request mirror lands on the right atomic counter — without it
/// the audit miner's `CacheOutcomeTally` would report 0 for these
/// two counters even when a request was active.
#[test]
fn cold_abort_sweep_attributes_to_per_request_context() {
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    // Install a request context on the calling thread. The cold-abort
    // path runs synchronously on this thread under
    // `execute_cooperative`, so `current_request_context()` is `Some`
    // exactly when the helper fires.
    let ctx = RequestContext::new(7, Arc::from("/c.vue"), false, None);
    let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));

    // Force the cold-abort sweep deterministically. The flag drives
    // the TOCTOU branch in `execute_cooperative` that bumps
    // `cold_aborts_swept` exactly once for a successful cold build.
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

    // Global is bumped (existing invariant — must not regress).
    let snap = store.stats_snapshot();
    assert_eq!(
        snap.cold_aborts_swept, 1,
        "global stats.cold_aborts_swept must still increment for non-audited and audited callers \
         (got {})",
        snap.cold_aborts_swept,
    );

    // Per-request counter is bumped via the helper's
    // `current_request_context()` consult.
    let per_request = ctx.cold_aborts_swept.load(Ordering::Relaxed);
    assert_eq!(
        per_request, 1,
        "ctx.cold_aborts_swept must increment when a RequestContext is \
         installed during a cold-abort sweep — this is what the audit \
         miner reads (got {per_request}). If this is 0, the helper \
         failed to mirror to per-request, or the production site is \
         no longer routing through the helper (architecture guard \
         `audit_counter_single_helper` should have caught the latter)."
    );
}

/// Drive the inflight-aborted-retry path with an installed
/// `RequestContext` and confirm the per-request
/// `inflight_aborted_retries` counter is bumped.
///
/// The driver is the same shape as the in-crate
/// `semantic_graph_stats_inflight_aborted_retries_increments_on_retry_loop`
/// test, but installed under a `RequestContextGuard` so the helper
/// can mirror the bump to per-request.
#[test]
fn inflight_aborted_retry_attributes_to_per_request_context() {
    use std::sync::mpsc;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    // The retry path runs on the joiner thread. Install context
    // there — that is the thread on which the helper fires.
    let ctx = RequestContext::new(11, Arc::from("/r.vue"), false, None);

    let (tx_in_build, rx_in_build) = mpsc::channel::<()>();
    let (tx_finish_build, rx_finish_build) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_key = key.clone();
    let winner = thread::spawn(move || {
        winner_store.execute_cooperative(
            winner_key,
            || {
                winner_store.intern_node(SemanticNodeData::Opaque(
                    verter_session::semantic_query::QueryError::Miss,
                ))
            },
            || {
                tx_in_build.send(()).expect("winner signal in_build");
                rx_finish_build.recv().expect("winner signal finish");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature_for_tests())
            },
        )
    });

    rx_in_build.recv().expect("winner entered build");

    let joiner_store = Arc::clone(&store);
    let joiner_key = key.clone();
    let joiner_ctx = Arc::clone(&ctx);
    let joiner = thread::spawn(move || {
        // Install the context on the JOINER thread — that's where
        // the retry-bump helper runs.
        let _ctx_guard = RequestContextGuard::install(Arc::clone(&joiner_ctx));
        joiner_store.execute_cooperative(
            joiner_key,
            || {
                joiner_store.intern_node(SemanticNodeData::Opaque(
                    verter_session::semantic_query::QueryError::Miss,
                ))
            },
            || {
                let id =
                    joiner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), empty_signature_for_tests())
            },
        )
    });

    thread::sleep(std::time::Duration::from_millis(50));
    let aborted = test_trigger_inflight_abort(&store, &key);
    assert!(aborted, "inflight entry must have been present to abort");

    tx_finish_build.send(()).expect("release winner");
    let _ = winner.join().expect("winner joined");
    let _ = joiner.join().expect("joiner joined");

    let snap = store.stats_snapshot();
    assert!(
        snap.inflight_aborted_retries >= 1,
        "global stats.inflight_aborted_retries must increment on retry loop (got {})",
        snap.inflight_aborted_retries,
    );

    let per_request = ctx.inflight_aborted_retries.load(Ordering::Relaxed);
    assert!(
        per_request >= 1,
        "ctx.inflight_aborted_retries must increment when a RequestContext \
         is installed on the joiner thread — this is what the audit miner \
         reads at `component_meta_audit/footprint_miner.rs::CacheOutcomeTally` \
         (got {per_request}). If this regresses, the helper has stopped \
         consulting `current_request_context()` and the per-request mirror \
         is dropping bumps that the global counter still records — the \
         audit miner will report 0 for inflight_aborted_retries even when \
         a request was active. The architecture-guard \
         `audit_counter_single_helper` only rejects direct \
         `self.stats.fetch_add` outside the helper, so it does not catch a \
         helper that silently stops mirroring; this runtime test does."
    );
}
