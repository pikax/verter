//! Fuse / budget / cache-length / debug accessors for
//! [`ComponentMetaQueryEngine`] — leaf read accessors over the engine's fuse
//! state, per-request fanout budgets, and cache-size / debug counters. Each
//! reads an engine field (or the ctx-owned store) and returns a budget verdict,
//! a cache length, or a counter; none resolves types or touches the dispatch.

use super::ComponentMetaQueryEngine;
use crate::resolver_core::FuseTrip;

impl<'a> ComponentMetaQueryEngine<'a> {
    pub fn enter_member_surface(&mut self) -> bool {
        self.fuse_state.push_member_recursion();
        !self
            .fuse_state
            .check_member_recursion_depth(&self.fuse_budgets)
    }

    pub fn exit_member_surface(&mut self) {
        self.fuse_state.pop_member_recursion();
    }

    /// `pub(crate)` accessor for the projection-op fuse budget check. The bridge
    /// helpers in `meta_resolve.rs` call it to gate the same projection-op
    /// budget the engine itself enforces.
    pub(crate) fn projection_op_budget_exhausted(&mut self) -> bool {
        self.fuse_state
            .check_projection_op_count(&self.fuse_budgets)
    }

    /// Check wildcard route fanout budget. Returns `true` if within budget.
    pub fn allow_wildcard_route(&mut self) -> bool {
        !self
            .fuse_state
            .check_wildcard_route_fanout(&self.fuse_budgets)
    }

    /// Check imported-root fanout budget. Returns `true` if within budget.
    pub fn allow_imported_root(&mut self) -> bool {
        !self
            .fuse_state
            .check_imported_root_fanout(&self.fuse_budgets)
    }

    /// Check registry deepening fanout budget. Returns `true` if within budget.
    pub fn allow_registry_deepening(&mut self) -> bool {
        !self
            .fuse_state
            .check_registry_deepening_fanout(&self.fuse_budgets)
    }

    /// Check union/member explosion budget. Returns `true` if within budget.
    pub fn allow_union_member(&mut self) -> bool {
        !self
            .fuse_state
            .check_union_member_explosion(&self.fuse_budgets)
    }

    /// Reset union member counter for per-member branch counting.
    pub fn reset_union_members(&mut self) {
        self.fuse_state.reset_union_members();
    }

    /// Whether any fuse has tripped.
    pub fn has_fuse_tripped(&self) -> bool {
        self.fuse_state.has_tripped()
    }

    /// Get fuse trip details for provenance/tracing.
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
