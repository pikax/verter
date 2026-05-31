//! Self-test for the
//! [`verter_session::tests::audit_tls_harness`] TLS observer
//! propagation harness, exercised against an in-crate probe.
//!
//! Discrimination contract:
//! - **Pre-change tree** (no substrate plumbing inside
//!   `RequestContextGuard::install`, i.e. no
//!   `verter_audit::install_observer` call): even with
//!   `install_audit = true`,
//!   `verter_audit::current_observer()` returns `None` from inside
//!   the harness's closure →
//!   `observer_seen_on_calling_thread == false` →
//!   `in_crate_observer_reaches_synchronous_probe` fails.
//! - **Post-change tree** (substrate plumbing live, i.e. the current
//!   tree): `RequestContextGuard::install` plants the
//!   `Arc<RequestContext>` into the substrate's TLS slot, so
//!   `current_observer()` returns `Some(...)` and the harness records
//!   `observer_seen_on_calling_thread == true`.
//!
//! Negative case: `install_audit = false` skips the guard entirely;
//! `current_observer()` returns `None`. This proves the positive
//! test is not a tautology — the harness measures the install state,
//! not a constant.

use verter_session::tests::audit_tls_harness::assert_observer_reaches;

/// Minimal in-crate probe that mirrors what production producers do
/// at the audit boundary — read `verter_audit::current_observer()`
/// and confirm a populated slot.
fn in_crate_probe() -> bool {
    verter_audit::current_observer().is_some()
}

#[test]
fn in_crate_observer_reaches_synchronous_probe() {
    let mut probe_saw_observer = false;
    let report = assert_observer_reaches(true, || {
        probe_saw_observer = in_crate_probe();
    });

    assert!(
        probe_saw_observer,
        "in-crate probe must see Some(observer) when audit is installed; \
         pre-change tree (no substrate plumbing in RequestContextGuard::install) \
         would leave the slot empty and this assertion would fire"
    );
    assert!(
        report.observer_seen_on_calling_thread,
        "harness must record the calling-thread observation as Some when audit is installed: {report:?}",
    );
    // No worker reported — confirm the report stays empty rather
    // than silently inheriting state from a prior invocation.
    assert!(
        report.observer_seen_on_worker_threads.is_empty(),
        "no worker spawned, no worker reports expected: {report:?}",
    );
    assert!(
        report.orphaned_call_sites.is_empty(),
        "no orphan call sites expected when no worker reported: {report:?}",
    );
}

#[test]
fn in_crate_observer_absent_when_audit_disabled() {
    let mut probe_saw_observer = true;
    let report = assert_observer_reaches(false, || {
        probe_saw_observer = in_crate_probe();
    });

    assert!(
        !probe_saw_observer,
        "in-crate probe must see None when audit is NOT installed; \
         a tautological harness that always reports Some(observer) \
         would fail this discriminator"
    );
    assert!(
        !report.observer_seen_on_calling_thread,
        "harness must record the calling-thread observation as None when audit is disabled: {report:?}",
    );
}
