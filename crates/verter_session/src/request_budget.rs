//! Request-scoped projection budget.
//!
//! This module owns the per-request projection-operation fuse used by
//! component-meta entry points and semantic dispatch. The budget itself
//! is stored on [`crate::request_context::RequestContext`], so scheduler
//! worker propagation uses the same request-context TLS bridge as audit
//! and cache counters.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Request-scoped projection-operation fuse.
///
/// Tracks the per-request projection-op count that the legacy engine's
/// `FuseBudgets::projection_op_count` rail used to terminate utility
/// and projection recursion before the call stack exhausts. The cap is
/// constructor-time on `HostConfig::projection_op_budget`; a value of
/// `0` preserves the legacy default of 2000.
#[derive(Debug)]
pub struct RequestBudget {
    /// Projection-operation budget for the request.
    pub projection_op_budget: usize,
    projection_ops_executed: AtomicUsize,
}

impl RequestBudget {
    /// Construct a new per-request budget with a zeroed counter and the
    /// supplied cap.
    #[must_use]
    pub fn new(projection_op_budget: usize) -> Arc<Self> {
        Arc::new(Self {
            projection_op_budget,
            projection_ops_executed: AtomicUsize::new(0),
        })
    }

    /// Increment the projection-op counter and return `true` when the
    /// request has exceeded its cap.
    pub fn check_projection_op_count(&self) -> bool {
        let current = self
            .projection_ops_executed
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        current > self.effective_projection_op_budget()
    }

    /// Return the configured cap after applying the legacy default.
    #[must_use]
    pub fn effective_projection_op_budget(&self) -> usize {
        if self.projection_op_budget == 0 {
            2000
        } else {
            self.projection_op_budget
        }
    }

    /// Read-only view of the executed projection-op counter.
    #[must_use]
    pub(crate) fn projection_ops_executed_count(&self) -> usize {
        self.projection_ops_executed.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::RequestBudget;

    #[test]
    fn request_budget_check_increments_until_cap_then_returns_true() {
        let budget = RequestBudget::new(3);
        assert!(!budget.check_projection_op_count(), "1st call (1 of 3)");
        assert!(!budget.check_projection_op_count(), "2nd call (2 of 3)");
        assert!(!budget.check_projection_op_count(), "3rd call (3 of 3)");
        assert!(budget.check_projection_op_count(), "4th call exceeds 3");
        assert_eq!(
            budget.projection_ops_executed_count(),
            4,
            "counter must persist past the trip; the trip should not silently reset"
        );
    }

    #[test]
    fn request_budget_zero_cap_falls_back_to_default_2000() {
        let budget = RequestBudget::new(0);
        for _ in 0..1999 {
            assert!(!budget.check_projection_op_count());
        }
        assert!(!budget.check_projection_op_count(), "2000th call at cap");
        assert!(
            budget.check_projection_op_count(),
            "2001st call exceeds default"
        );
    }
}
