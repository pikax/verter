//! Cheap, clonable, thread-safe cancellation flag.
//!
//! A [`CancellationToken`] is a one-shot latch shared between a request
//! handle and the work it admitted. A token rides on a work item; its
//! dispatch path checks the flag, and the owning handle trips it on
//! `Drop`, so dropping a handle cancels its still-pending work.
//!
//! The token is a transparent `Arc<AtomicBool>`: cloning is a refcount
//! bump, `cancel()` is a single store, and `is_cancelled()` is a single
//! load. It is `Send + Sync` so it can ride on a work node shared across
//! the driver and the worker pool.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A cheap, clonable, thread-safe one-shot cancellation flag.
///
/// All clones share the same underlying flag — cancelling any clone is
/// observed by every other clone. `cancel()` is idempotent.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates a fresh, un-cancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Trips the flag. Idempotent — calling more than once leaves the
    /// token cancelled and never panics or toggles back.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Returns whether the token has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_token_is_not_cancelled() {
        assert!(!CancellationToken::new().is_cancelled());
    }

    #[test]
    fn cancel_then_observe() {
        let t = CancellationToken::new();
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[test]
    fn cancel_is_idempotent() {
        let t = CancellationToken::new();
        t.cancel();
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[test]
    fn clones_share_state() {
        let t = CancellationToken::new();
        let c = t.clone();
        t.cancel();
        assert!(c.is_cancelled(), "clone observes a cancel on the sibling");
    }

    #[test]
    fn default_matches_new() {
        assert!(!CancellationToken::default().is_cancelled());
    }
}
