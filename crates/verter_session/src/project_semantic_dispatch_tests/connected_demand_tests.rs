//! The connected-demand ledger's construction-byte rail: reservations are
//! charged before a construction and never refunded, the one past the
//! allowance is refused with the sticky memory reason, and a new connected
//! demand starts from zero.

use super::ProjectSemanticDispatch;
use crate::{HostConfig, VerterHost};
use verter_type_engine::semantic_query::PartialReasonSet;

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

/// Reservations within the allowance are granted and accumulate; the one
/// that would pass it is refused with `CONNECTED_MEMORY_LIMIT`, and the trip
/// is sticky: every later charge on the demand, work or bytes, is refused
/// with it.
#[test]
fn a_reservation_past_the_allowance_trips_the_memory_rail() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    dispatch.connected_demand().set_byte_limit_for_tests(1_000);
    let (_guard, trip) = dispatch.enter_connected_demand(false);
    assert!(trip.is_none(), "a fresh demand has not tripped");
    dispatch
        .connected_demand()
        .reserve_bytes(600)
        .expect("600 of 1,000 bytes are granted");
    dispatch
        .connected_demand()
        .reserve_bytes(400)
        .expect("the allowance is inclusive");
    assert_eq!(dispatch.connected_demand().bytes_used_for_tests(), 1_000);
    let refused = dispatch
        .connected_demand()
        .reserve_bytes(1)
        .expect_err("a byte past the allowance is refused");
    assert!(refused.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT));
    assert_eq!(
        dispatch.connected_demand().bytes_used_for_tests(),
        1_000,
        "a refused reservation charges nothing"
    );
    let work = dispatch
        .connected_demand()
        .charge()
        .expect_err("the trip is sticky for the demand's work too");
    assert!(work.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT));
    assert_eq!(
        dispatch
            .connected_demand()
            .limit_report(PartialReasonSet::CONNECTED_MEMORY_LIMIT),
        (1_000, 1_000, "construction-bytes"),
        "the budget verdict names the memory rail and its numbers"
    );
}

/// A reservation that would overflow the counter is refused, not wrapped.
#[test]
fn an_overflowing_reservation_is_refused() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let (_guard, _) = dispatch.enter_connected_demand(false);
    dispatch
        .connected_demand()
        .reserve_bytes(10)
        .expect("ten bytes are granted");
    let refused = dispatch
        .connected_demand()
        .reserve_bytes(usize::MAX)
        .expect_err("an overflowing reservation is refused");
    assert!(refused.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT));
}

/// Every connected demand starts from zero bytes, and the charges of one
/// never reach the next.
#[test]
fn each_connected_demand_starts_from_zero_bytes() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    dispatch.connected_demand().set_byte_limit_for_tests(100);
    {
        let (_guard, _) = dispatch.enter_connected_demand(false);
        dispatch
            .connected_demand()
            .reserve_bytes(100)
            .expect("the whole allowance is granted");
        assert!(dispatch.connected_demand().reserve_bytes(1).is_err());
    }
    let (_guard, trip) = dispatch.enter_connected_demand(false);
    assert!(trip.is_none(), "the next demand starts untripped");
    assert_eq!(dispatch.connected_demand().bytes_used_for_tests(), 0);
    dispatch
        .connected_demand()
        .reserve_bytes(100)
        .expect("the next demand has its whole allowance");
}
