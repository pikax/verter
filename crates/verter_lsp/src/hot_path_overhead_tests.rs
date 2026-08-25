//! Structural correctness of the request-stability machinery on the HEALTHY path.
//!
//! These tests assert outputs, identity, and causal ordering. They do not
//! gate correctness on elapsed time. Latency distributions belong in the
//! controlled benchmark lane, not this correctness file.

use std::sync::Arc;
use std::time::Duration;

use crate::server::ImportSyncMemo;

#[test]
fn the_warm_import_set_memo_lookup_is_a_recorded_receipt() {
    let memo = ImportSyncMemo::default();
    let canonical = "/workspace/src/App.vue";
    memo.record_delivered(canonical.to_string(), (7, 3));

    assert!(
        memo.is_fresh_at(canonical, (7, 3)),
        "a recorded receipt must be fresh at its exact key"
    );
    assert!(
        !memo.is_fresh_at(canonical, (7, 4)),
        "a different generation key must miss"
    );
    assert!(
        !memo.is_fresh_at("/workspace/src/Other.vue", (7, 3)),
        "a different document must miss"
    );
}

#[tokio::test]
async fn per_document_singleflight_locks_are_identity_keyed() {
    let memo = ImportSyncMemo::default();
    let a = memo.lock_for("/workspace/src/App.vue");
    let a_again = memo.lock_for("/workspace/src/App.vue");
    let b = memo.lock_for("/workspace/src/Other.vue");

    assert!(
        Arc::ptr_eq(&a, &a_again),
        "the same document must reuse one singleflight lock"
    );
    assert!(
        !Arc::ptr_eq(&a, &b),
        "distinct documents must not share a singleflight lock"
    );

    let _guard = a.lock().await;
}

/// Concurrent requests on DIFFERENT documents must not share a lock.
/// Identity inequality is the structural discriminator; elapsed hold
/// time is not a correctness assertion.
#[test]
fn concurrent_requests_on_different_documents_do_not_share_a_lock() {
    let memo = ImportSyncMemo::default();
    let locks: Vec<_> = (0..8)
        .map(|index| memo.lock_for(&format!("/workspace/src/Doc{index}.vue")))
        .collect();
    for (i, left) in locks.iter().enumerate() {
        for (j, right) in locks.iter().enumerate() {
            if i == j {
                assert!(Arc::ptr_eq(left, right));
            } else {
                assert!(
                    !Arc::ptr_eq(left, right),
                    "documents {i} and {j} must not contend on one lock"
                );
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn the_ambient_deadline_scope_is_readable_without_consuming_time() {
    let start = tokio::time::Instant::now();
    verter_type_runtime::deadline::with_deadline(Duration::from_secs(1), async {
        assert!(
            verter_type_runtime::deadline::remaining().is_some(),
            "an armed deadline must be readable from inside the scope"
        );
    })
    .await;
    assert_eq!(
        tokio::time::Instant::now(),
        start,
        "opening and reading the ambient deadline must not consume virtual time"
    );
}

/// A handler that already has an answer must return that answer without
/// waiting for its own deadline. Paused time makes "no wait" exact.
#[tokio::test(start_paused = true)]
async fn a_succeeding_request_never_waits_for_its_own_deadline() {
    let deadline = Duration::from_secs(5);
    let start = tokio::time::Instant::now();
    let result: tower_lsp_server::jsonrpc::Result<u8> =
        crate::audit_harness::run_with_deadline(deadline, async { Ok(7u8) }).await;
    assert_eq!(result.expect("the body succeeds"), 7);
    assert_eq!(
        tokio::time::Instant::now(),
        start,
        "a request that already had its answer must not consume virtual time"
    );
}
