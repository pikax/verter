//! Architectural fuse budgets for bounded operation.
//!
//! Fuses prevent unbounded work in resolver and component-meta paths.
//! When a fuse trips, the operation returns a bounded degraded result
//! with provenance attached, instead of hanging or replaying proof paths.
//!
//! Fuse-tripped results MUST NOT be published to shared DB layers.
//!
//! Only route and registry fan-out work that no semantic query counts keeps
//! a fuse here. Projection work is charged to the request's operation
//! account ([`crate::request_budget::RequestBudget`]), which semantic
//! dispatch charges per counted query and replays stored receipts into; it
//! has no second count here.

/// Default fuse budgets. These are architecture rules, not tuning hints.
pub struct FuseBudgets {
    /// Maximum wildcard sources per barrel before aborting route surface build.
    pub wildcard_route_fanout: usize,
    /// Maximum root proofs per request.
    pub imported_root_fanout: usize,
    /// Maximum transitive names per request during registry deepening.
    pub registry_deepening_fanout: usize,
}

impl Default for FuseBudgets {
    fn default() -> Self {
        Self {
            wildcard_route_fanout: 500,
            imported_root_fanout: 200,
            registry_deepening_fanout: 300,
        }
    }
}

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
/// Provenance attached to fuse-tripped results.
///
/// This stays request-local — never stored in shared DB layers.
#[derive(Debug, Clone)]
pub struct FuseTrip {
    pub fuse_name: &'static str,
    pub budget: usize,
    pub actual: usize,
}

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
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
    /// Trip history, observation only: refusals come from the check booleans.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub trips: Vec<FuseTrip>,
}

impl FuseState {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub fn has_tripped(&self) -> bool {
        !self.trips.is_empty()
    }

    /// `M` is the checking unit's capture execution: only a captured one
    /// records the fanout into its request.
    pub fn check_wildcard_route_fanout<M: verter_audit::observe::ObserveExec>(
        &mut self,
        observation: &crate::request_observers::CaptureTargets,
        budgets: &FuseBudgets,
    ) -> bool {
        let _ = observation;
        self.wildcard_sources_processed += 1;
        // Per-request audit attribution: one wildcard-route fanout
        // expansion observed. Bumped before the budget check so the
        // counter reflects total expansions even when the fuse trips.
        verter_audit::observe!(M => on observation.observer.as_deref() => |obs| {
            obs.record_event(verter_audit::AuditEvent::RouteDbWildcardFanout);
        });
        if self.wildcard_sources_processed > budgets.wildcard_route_fanout {
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
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
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
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
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            self.trips.push(FuseTrip::new(
                "registry_deepening_fanout",
                budgets.registry_deepening_fanout,
                self.registry_names_enqueued,
            ));
            return true;
        }
        false
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
    }

    #[test]
    fn fuse_trips_after_budget_exceeded() {
        let budgets = FuseBudgets {
            wildcard_route_fanout: 3,
            ..FuseBudgets::default()
        };
        let mut state = FuseState::new();

        assert!(
            !state.check_wildcard_route_fanout::<verter_audit::observe::Uncaptured>(
                &crate::request_observers::CaptureTargets::EMPTY,
                &budgets
            )
        );
        assert!(
            !state.check_wildcard_route_fanout::<verter_audit::observe::Uncaptured>(
                &crate::request_observers::CaptureTargets::EMPTY,
                &budgets
            )
        );
        assert!(
            !state.check_wildcard_route_fanout::<verter_audit::observe::Uncaptured>(
                &crate::request_observers::CaptureTargets::EMPTY,
                &budgets
            )
        );
        assert!(
            state.check_wildcard_route_fanout::<verter_audit::observe::Uncaptured>(
                &crate::request_observers::CaptureTargets::EMPTY,
                &budgets
            )
        ); // 4th call trips
        assert!(state.has_tripped());
        assert_eq!(state.trips.len(), 1);
        assert_eq!(state.trips[0].fuse_name, "wildcard_route_fanout");
    }

    #[test]
    fn multiple_fuses_can_trip() {
        let budgets = FuseBudgets {
            imported_root_fanout: 1,
            registry_deepening_fanout: 1,
            ..FuseBudgets::default()
        };
        let mut state = FuseState::new();

        state.check_imported_root_fanout(&budgets); // 1 (at budget)
        state.check_imported_root_fanout(&budgets); // 2 (trips)
        state.check_registry_deepening_fanout(&budgets); // 1 (at budget)
        state.check_registry_deepening_fanout(&budgets); // 2 (trips)

        assert_eq!(state.trips.len(), 2);
        assert_eq!(state.trips[0].fuse_name, "imported_root_fanout");
        assert_eq!(state.trips[1].fuse_name, "registry_deepening_fanout");
    }
}
