//! The in-flight charge rail is REQUIRED production accounting, not
//! optional measurement: a plain consumer with no feature enabled — no
//! `test-support`, no `soak-support`, so no ACTIVE-occupancy window and no
//! provenance anywhere — reserves in-flight bytes unconditionally, never
//! sees an active-class refusal, and reads the current ACTIVE occupancy
//! off the account's public snapshot.

use std::sync::Arc;

use verter_session_query::retention::{
    ChargeClass, RetentionAdmission, SemanticRetentionAccount,
};

fn in_flight_footprint(account: &Arc<SemanticRetentionAccount>) -> usize {
    let charge = account.reserve_in_flight(2048);
    let snapshot = account.snapshot();
    assert_eq!(snapshot.active_bytes, 2048);
    assert_eq!(snapshot.refusals_active, 0);
    charge.bytes()
}

fn main() {
    let account = SemanticRetentionAccount::process_local();
    let bytes = in_flight_footprint(&account);
    assert_eq!(bytes, 2048);
    // The discretionary classes keep their own refusal outcomes; only the
    // active class is unconditional.
    assert!(matches!(
        account.reserve(ChargeClass::Retained, usize::MAX),
        RetentionAdmission::Refused(_)
    ));
}
