//! `CancellationToken` — cheap, clonable, thread-safe cancellation flag.
//!
//! This is the substrate the DAG scheduler uses for `DagHandle`-drop
//! cancellation: a node's `cancellation_token` is checked before / during
//! dispatch and tripped when the owning handle drops. This is the leaf
//! primitive only.
//!
//! Pinned properties:
//!
//! 1. Fresh token is not cancelled.
//! 2. `cancel()` → `is_cancelled()` becomes true.
//! 3. `cancel()` is idempotent (calling twice keeps it cancelled, no
//!    panic, no toggle).
//! 4. Clones share the same underlying state — cancelling one clone is
//!    observed by every other clone (shared `Arc<AtomicBool>` semantics).

use std::sync::Arc;
use std::thread;

use verter_scheduler::cancellation::CancellationToken;

/// A freshly constructed token reads as not cancelled.
#[test]
fn fresh_token_is_not_cancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled(), "a fresh token must not be cancelled");
}

/// `cancel()` flips the flag; `is_cancelled()` then returns true.
#[test]
fn cancel_sets_the_flag() {
    let token = CancellationToken::new();
    token.cancel();
    assert!(token.is_cancelled(), "cancel() must set the flag");
}

/// `cancel()` is idempotent: a second cancel leaves it cancelled.
#[test]
fn cancel_is_idempotent() {
    let token = CancellationToken::new();
    token.cancel();
    token.cancel();
    assert!(
        token.is_cancelled(),
        "cancel() must be idempotent — a second call keeps it cancelled",
    );
}

/// Clones observe the same state: cancelling one clone is visible on
/// another, including across a thread boundary (Send + Sync shared flag).
#[test]
fn clones_share_state_across_threads() {
    let token = CancellationToken::new();
    let clone = token.clone();
    assert!(!clone.is_cancelled());

    // Cancel from the original on another thread; the clone must observe it.
    let original = token; // move the original into the thread
    let h = thread::spawn(move || {
        original.cancel();
    });
    h.join().expect("cancel thread joined");

    assert!(
        clone.is_cancelled(),
        "a clone must observe a cancel issued on a sibling handle",
    );
}

/// The token is cheaply clonable and `Send + Sync` so it can ride on a
/// work node shared across the driver + worker pool. We assert the bounds
/// statically and that an `Arc`-shared token propagates cancellation.
#[test]
fn token_is_send_sync_and_arc_shareable() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CancellationToken>();

    let token = Arc::new(CancellationToken::new());
    let t2 = Arc::clone(&token);
    token.cancel();
    assert!(
        t2.is_cancelled(),
        "Arc-shared token must propagate cancellation to every Arc holder",
    );
}
