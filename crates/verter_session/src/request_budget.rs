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
/// Tracks the per-request aggregate work-op count used to terminate
/// utility / projection / generic-expansion recursion before the call
/// stack exhausts. The cap is constructor-time on
/// `HostConfig::projection_op_budget`; a value of `0` preserves the
/// legacy default of 2000.
///
/// The set of `SemanticQueryKey` kinds that count is the aggregate
/// work-budget gate
/// (`project_semantic_dispatch::semantic_query_counts_toward_projection_budget`):
/// the projection operators (`ProjectMember` / `IndexedAccess` /
/// `ProjectPath` / `KeyOf` / `MappedType`) PLUS `Instantiate` and
/// `Conditional` — the kinds that dominate an open-generic expansion
/// storm. Counting only projection operators left instantiation /
/// conditional storms unbounded; the aggregate gate makes them fail
/// closed too.
///
/// # Default rationale
///
/// The default `2000` is a **fuse threshold**, not a correctness
/// boundary. It is sized so legitimate component-meta resolutions on
/// representative corpora (nuxt-ui, element-plus, primevue, etc.)
/// complete well under the cap with substantial headroom, while
/// pathological projections — recursive `Pick<...>` chains over
/// untyped barrels, deep `Surface[K1][K2]...[Kn]` walks with missing
/// types, generic-helper instantiation storms — exhaust within a
/// few seconds and surface a partial.
///
/// **Semantic contract on exhaustion**: a request that trips the cap
/// returns a *partial* `ComponentMeta` with the same structural
/// invariants as a complete one (well-formed `props` / `emits` /
/// `slots` lists, opaque sentinels for unresolved members) — NOT a
/// malformed payload. The dispatch return carries
/// `cache_suppress=true`, which propagates through the
/// reducer/materializer pipeline (see
/// `MaterializedTypeExpr.cache_suppress` and
/// `RequestContext::materialization_cache_suppress`) into
/// `ResolvedComponentMetaState.synthesis_should_suppress`. The
/// `ComponentMetaResultDb` admission gate refuses to warm the
/// partial, so a subsequent identical request re-runs the cold
/// compute against fresh budget rather than warm-hitting a
/// poisoned entry.
///
/// **Raising the cap is safe** for users who profile-confirm a
/// legitimate request needs more headroom; lowering it is a way to
/// surface partials earlier on known-pathological inputs. Either
/// direction preserves correctness — the cap only controls when the
/// reducer bails to a partial, not whether the partial is admitted
/// to caches.
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

    /// Peek-only test for budget exhaustion. Returns `true` when the
    /// already-executed projection-op count strictly exceeds the cap,
    /// i.e. when a fresh [`Self::check_projection_op_count`] would also
    /// return `true` *without* the prior incrementing call having been
    /// the one to trip the fuse.
    ///
    /// The dispatcher's `execute_via_cold_build_helper` consults this
    /// peek BEFORE entering the cooperative-admission machinery so that,
    /// once a request trips its fuse, every subsequent projection-op
    /// query short-circuits at the dispatch entry without paying the
    /// `execute_cooperative` admission cost (in-flight table mutex,
    /// joiner-condvar entry, fact-tracer install, per-key warm probe).
    /// Without this gate a runaway request keeps spending μs-per-call
    /// on admission overhead for each rejected MappedType / KeyOf /
    /// ProjectPath dispatch — the empirically-observed 250K rejected
    /// builds on `ChatMessages.vue` translate to ~250 wall-clock
    /// seconds of materialisation-lane time spent past the fuse trip,
    /// none of which makes progress because every call returns
    /// `BudgetExceeded(cache_suppress=true)`.
    ///
    /// Non-incrementing on purpose: the cooperative-admission build
    /// closure remains the single site that bumps the executed
    /// counter via [`Self::check_projection_op_count`], so the trip
    /// point and the reported `BudgetExceededFailure.actual` value
    /// stay invariant across this fast-path peek.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.projection_ops_executed_count() > self.effective_projection_op_budget()
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

    #[test]
    fn request_budget_is_exhausted_tracks_post_trip_state_without_incrementing() {
        let budget = RequestBudget::new(2);
        assert!(
            !budget.is_exhausted(),
            "fresh budget reports !exhausted before any call"
        );
        assert!(!budget.check_projection_op_count(), "1st call (1 of 2)");
        assert!(
            !budget.is_exhausted(),
            "within-budget call leaves !exhausted"
        );
        assert!(!budget.check_projection_op_count(), "2nd call (2 of 2)");
        assert!(
            !budget.is_exhausted(),
            "at-cap call leaves !exhausted (the cap is inclusive)"
        );
        assert!(budget.check_projection_op_count(), "3rd call exceeds 2");
        assert!(
            budget.is_exhausted(),
            "post-trip peek reports exhausted so the dispatcher early-exits"
        );
        // Crucial property: the peek must NOT increment. Without this
        // invariant the dispatcher's fast-path early-exit would inflate
        // `BudgetExceededFailure.actual` past the production value and
        // skew the per-request audit.
        let executed_before = budget.projection_ops_executed_count();
        assert!(budget.is_exhausted());
        assert!(budget.is_exhausted());
        assert!(budget.is_exhausted());
        assert_eq!(
            budget.projection_ops_executed_count(),
            executed_before,
            "is_exhausted is peek-only — it must not bump the executed counter"
        );
    }
}
