//! Fuse / budget / cache-length / debug accessors for
//! [`ComponentMetaQueryEngine`] — leaf read accessors over the engine's fuse
//! state, per-request fanout budgets, and cache-size / debug counters. Each
//! reads an engine field (or the ctx-owned store) and returns a budget verdict,
//! a cache length, or a counter; none resolves types or touches the dispatch.

use super::ComponentMetaQueryEngine;
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
use verter_type_engine::resolver_core::FuseTrip;

impl<'a, M: verter_audit::observe::ObserveExec> ComponentMetaQueryEngine<'a, M> {
    /// Whether the request's operation account has passed its cap. The
    /// engine keeps no count of its own: every projection it enters is a
    /// semantic dispatch charged to that account, so this only reads it.
    /// An exhausted account marks the request's result budget-partial, so
    /// the surface the skipped projection would have fed is never admitted
    /// warm.
    pub(crate) fn projection_op_budget_exhausted(&mut self) -> bool {
        let exhausted = verter_type_engine::request_context::current_request_budget()
            .is_some_and(|budget| budget.is_exhausted());
        if exhausted {
            verter_type_engine::request_context::mark_request_result_inference_budget_exceeded();
        }
        exhausted
    }

    /// Check wildcard route fanout budget. Returns `true` if within budget; a trip marks the request result fan-out partial, the class the imported-root and registry-deepening fuses record, so the symbol it never looked up is never admitted warm.
    pub fn allow_wildcard_route(&mut self) -> bool {
        let tripped = self.fuse_state.check_wildcard_route_fanout::<M>(
            verter_type_engine::resolver_core::resolver_context::capture_targets(self.ctx),
            &self.fuse_budgets,
        );
        if tripped {
            verter_type_engine::request_context::mark_request_result_fanout_limit();
        }
        !tripped
    }

    /// Check imported-root fanout budget. Returns `true` if within budget; a trip marks the request result fan-out partial so the truncated registry is never admitted warm.
    pub fn allow_imported_root(&mut self) -> bool {
        let tripped = self
            .fuse_state
            .check_imported_root_fanout(&self.fuse_budgets);
        if tripped {
            verter_type_engine::request_context::mark_request_result_fanout_limit();
        }
        !tripped
    }

    /// Check registry deepening fanout budget. Returns `true` if within budget; a trip marks the request result fan-out partial so the truncated registry is never admitted warm.
    pub fn allow_registry_deepening(&mut self) -> bool {
        let tripped = self
            .fuse_state
            .check_registry_deepening_fanout(&self.fuse_budgets);
        if tripped {
            verter_type_engine::request_context::mark_request_result_fanout_limit();
        }
        !tripped
    }

    /// Whether any fuse has tripped.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub fn has_fuse_tripped(&self) -> bool {
        self.fuse_state.has_tripped()
    }

    /// Get fuse trip details for provenance/tracing.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub fn fuse_trips(&self) -> &[FuseTrip] {
        &self.fuse_state.trips
    }

    #[cfg(test)]
    pub(crate) fn imported_registry_symbol_cache_len(&self) -> usize {
        self.imported_registry_symbols.borrow().len()
    }

    /// Pre-consume the wildcard-route fuse so exactly `remaining`
    /// further `allow_wildcard_route()` calls stay within budget. With
    /// `remaining == 1`, the next slow-lane resolution is permitted and
    /// a second would trip `wildcard_route_fanout` — the near-fanout
    /// boundary that discriminates the imported-registry recompute bug.
    #[cfg(test)]
    pub(crate) fn prime_wildcard_route_fuse_for_tests(&mut self, remaining: usize) {
        self.fuse_state.wildcard_sources_processed = self
            .fuse_budgets
            .wildcard_route_fanout
            .saturating_sub(remaining);
    }

    /// Number of `allow_wildcard_route()` calls observed so far — the
    /// live wildcard-route fuse consumption count.
    #[cfg(test)]
    pub(crate) fn wildcard_route_fuse_consumed_for_tests(&self) -> usize {
        self.fuse_state.wildcard_sources_processed
    }
}
