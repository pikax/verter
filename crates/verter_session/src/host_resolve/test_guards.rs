//! Test-only thread-local guards that forbid the legacy route-frontier path
//! from running on a thread. Used by the `host_resolve_tests` and
//! `host_manage_tests` to assert the production pipeline does not regress
//! through the legacy code path.

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static FORBID_ROUTE_FRONTIER_FOR_TESTS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) struct RouteFrontierGuard;

#[cfg(test)]
impl Drop for RouteFrontierGuard {
    fn drop(&mut self) {
        FORBID_ROUTE_FRONTIER_FOR_TESTS.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[cfg(test)]
pub(crate) fn forbid_route_frontier_for_tests() -> RouteFrontierGuard {
    FORBID_ROUTE_FRONTIER_FOR_TESTS.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
    RouteFrontierGuard
}

#[cfg(test)]
pub(super) fn assert_route_frontier_allowed() {
    assert!(
        !route_frontier_forbidden_for_current_thread(),
        "route/root production path should not fall back through the external-type frontier",
    );
}

#[cfg(test)]
pub(crate) fn route_frontier_forbidden_for_current_thread() -> bool {
    FORBID_ROUTE_FRONTIER_FOR_TESTS.with(|depth| depth.get() > 0)
}

#[cfg(not(test))]
pub(super) fn assert_route_frontier_allowed() {}
