//! Architectural fuse budgets for bounded operation.
//!
//! Fuses prevent unbounded work in resolver and component-meta paths.
//! When a fuse trips, the operation returns a bounded degraded result
//! with provenance attached, instead of hanging or replaying proof paths.
//!
//! Fuse-tripped results MUST NOT be published to shared DB layers.

/// Default fuse budgets. These are architecture rules, not tuning hints.
pub struct FuseBudgets {
    /// Maximum wildcard sources per barrel before aborting route surface build.
    pub wildcard_route_fanout: usize,
    /// Maximum root proofs per request.
    pub imported_root_fanout: usize,
    /// Maximum transitive names per request during registry deepening.
    pub registry_deepening_fanout: usize,
    /// Maximum member-surface recursion levels.
    pub member_surface_recursion_depth: usize,
    /// Maximum projection ops per request.
    pub projection_op_count: usize,
    /// Maximum union branches per member before collapsing.
    pub union_member_explosion: usize,
}

impl Default for FuseBudgets {
    fn default() -> Self {
        Self {
            wildcard_route_fanout: 500,
            imported_root_fanout: 200,
            registry_deepening_fanout: 300,
            member_surface_recursion_depth: 10,
            projection_op_count: 2000,
            union_member_explosion: 100,
        }
    }
}

/// Provenance attached to fuse-tripped results.
///
/// This stays request-local — never stored in shared DB layers.
#[derive(Debug, Clone)]
pub struct FuseTrip {
    pub fuse_name: &'static str,
    pub budget: usize,
    pub actual: usize,
}

impl FuseTrip {
    pub fn new(fuse_name: &'static str, budget: usize, actual: usize) -> Self {
        Self {
            fuse_name,
            budget,
            actual,
        }
    }
}

/// Request-scoped fuse state tracker.
///
/// Each request creates one of these. Counters are incremented during
/// work and checked against budgets. When a fuse trips, the trip is
/// recorded and the operation returns a degraded result.
#[derive(Debug, Default)]
pub struct FuseState {
    pub wildcard_sources_processed: usize,
    pub imported_roots_resolved: usize,
    pub registry_names_enqueued: usize,
    pub current_member_recursion_depth: usize,
    pub projection_ops_executed: usize,
    pub union_members_processed: usize,
    pub trips: Vec<FuseTrip>,
}

impl FuseState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_tripped(&self) -> bool {
        !self.trips.is_empty()
    }

    pub fn check_wildcard_route_fanout(&mut self, budgets: &FuseBudgets) -> bool {
        self.wildcard_sources_processed += 1;
        // Per-request audit attribution: one wildcard-route fanout
        // expansion observed. Bumped before the budget check so the
        // counter reflects total expansions even when the fuse trips.
        if let Some(obs) = verter_audit::current_observer() {
            obs.record_event(verter_audit::AuditEvent::RouteDbWildcardFanout);
        }
        if self.wildcard_sources_processed > budgets.wildcard_route_fanout {
            self.trips.push(FuseTrip::new(
                "wildcard_route_fanout",
                budgets.wildcard_route_fanout,
                self.wildcard_sources_processed,
            ));
            return true;
        }
        false
    }

    pub fn check_imported_root_fanout(&mut self, budgets: &FuseBudgets) -> bool {
        self.imported_roots_resolved += 1;
        if self.imported_roots_resolved > budgets.imported_root_fanout {
            self.trips.push(FuseTrip::new(
                "imported_root_fanout",
                budgets.imported_root_fanout,
                self.imported_roots_resolved,
            ));
            return true;
        }
        false
    }

    pub fn check_registry_deepening_fanout(&mut self, budgets: &FuseBudgets) -> bool {
        self.registry_names_enqueued += 1;
        if self.registry_names_enqueued > budgets.registry_deepening_fanout {
            self.trips.push(FuseTrip::new(
                "registry_deepening_fanout",
                budgets.registry_deepening_fanout,
                self.registry_names_enqueued,
            ));
            return true;
        }
        false
    }

    pub fn push_member_recursion(&mut self) -> usize {
        self.current_member_recursion_depth += 1;
        self.current_member_recursion_depth
    }

    pub fn pop_member_recursion(&mut self) {
        self.current_member_recursion_depth = self.current_member_recursion_depth.saturating_sub(1);
    }

    pub fn check_member_recursion_depth(&mut self, budgets: &FuseBudgets) -> bool {
        if self.current_member_recursion_depth > budgets.member_surface_recursion_depth {
            self.trips.push(FuseTrip::new(
                "member_surface_recursion_depth",
                budgets.member_surface_recursion_depth,
                self.current_member_recursion_depth,
            ));
            return true;
        }
        false
    }

    pub fn check_projection_op_count(&mut self, budgets: &FuseBudgets) -> bool {
        self.projection_ops_executed += 1;
        if self.projection_ops_executed > budgets.projection_op_count {
            self.trips.push(FuseTrip::new(
                "projection_op_count",
                budgets.projection_op_count,
                self.projection_ops_executed,
            ));
            return true;
        }
        false
    }

    pub fn check_union_member_explosion(&mut self, budgets: &FuseBudgets) -> bool {
        self.union_members_processed += 1;
        if self.union_members_processed > budgets.union_member_explosion {
            self.trips.push(FuseTrip::new(
                "union_member_explosion",
                budgets.union_member_explosion,
                self.union_members_processed,
            ));
            return true;
        }
        false
    }

    /// Reset union member counter (called per-member to track per-member branch count).
    pub fn reset_union_members(&mut self) {
        self.union_members_processed = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_budgets() {
        let budgets = FuseBudgets::default();
        assert_eq!(budgets.wildcard_route_fanout, 500);
        assert_eq!(budgets.imported_root_fanout, 200);
        assert_eq!(budgets.registry_deepening_fanout, 300);
        assert_eq!(budgets.member_surface_recursion_depth, 10);
        assert_eq!(budgets.projection_op_count, 2000);
        assert_eq!(budgets.union_member_explosion, 100);
    }

    #[test]
    fn fuse_trips_after_budget_exceeded() {
        let budgets = FuseBudgets {
            wildcard_route_fanout: 3,
            ..FuseBudgets::default()
        };
        let mut state = FuseState::new();

        assert!(!state.check_wildcard_route_fanout(&budgets));
        assert!(!state.check_wildcard_route_fanout(&budgets));
        assert!(!state.check_wildcard_route_fanout(&budgets));
        assert!(state.check_wildcard_route_fanout(&budgets)); // 4th call trips
        assert!(state.has_tripped());
        assert_eq!(state.trips.len(), 1);
        assert_eq!(state.trips[0].fuse_name, "wildcard_route_fanout");
    }

    #[test]
    fn member_recursion_depth_tracking() {
        let budgets = FuseBudgets {
            member_surface_recursion_depth: 2,
            ..FuseBudgets::default()
        };
        let mut state = FuseState::new();

        state.push_member_recursion();
        assert!(!state.check_member_recursion_depth(&budgets));
        state.push_member_recursion();
        assert!(!state.check_member_recursion_depth(&budgets));
        state.push_member_recursion();
        assert!(state.check_member_recursion_depth(&budgets)); // Depth 3 > budget 2
        state.pop_member_recursion();
        assert_eq!(state.current_member_recursion_depth, 2);
    }

    #[test]
    fn multiple_fuses_can_trip() {
        let budgets = FuseBudgets {
            imported_root_fanout: 1,
            projection_op_count: 1,
            ..FuseBudgets::default()
        };
        let mut state = FuseState::new();

        state.check_imported_root_fanout(&budgets); // 1 (at budget)
        state.check_imported_root_fanout(&budgets); // 2 (trips)
        state.check_projection_op_count(&budgets); // 1 (at budget)
        state.check_projection_op_count(&budgets); // 2 (trips)

        assert_eq!(state.trips.len(), 2);
        assert_eq!(state.trips[0].fuse_name, "imported_root_fanout");
        assert_eq!(state.trips[1].fuse_name, "projection_op_count");
    }

    #[test]
    fn union_member_explosion_trips_and_resets() {
        let budgets = FuseBudgets {
            union_member_explosion: 2,
            ..FuseBudgets::default()
        };
        let mut state = FuseState::new();

        assert!(!state.check_union_member_explosion(&budgets));
        assert!(!state.check_union_member_explosion(&budgets));
        assert!(state.check_union_member_explosion(&budgets)); // 3rd trips
        assert!(state.has_tripped());
        assert_eq!(state.trips[0].fuse_name, "union_member_explosion");

        state.reset_union_members();
        assert_eq!(state.union_members_processed, 0);
    }
}
