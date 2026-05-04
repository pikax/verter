//! Cross-crate self-test for the
//! [`verter_session::tests::audit_tls_harness`] TLS observer
//! propagation harness.
//!
//! `verter_session`'s `RequestContextGuard::install` plants the
//! audit observer into the substrate's TLS slot. This test proves
//! the wiring is visible from a DIFFERENT crate (`verter_compiler`),
//! so when real production producers ship in lower crates and
//! instrument themselves with `verter_audit::current_observer()`,
//! those producers see `Some(...)` rather than `None`. Without this
//! cross-crate self-test, every consumer-crate change that fails to
//! emit could be blamed on the harness rather than on the new
//! producer's TLS handling; confirming the harness works across
//! crate boundaries upstream of consumer instrumentation isolates
//! that failure mode.
//!
//! The probe lives at
//! [`verter_compiler::_audit_harness_probe`] — a minimal `pub fn`
//! that returns `current_observer().is_some()`. It is removed once
//! production component-compile audit producers ship from
//! `verter_compiler`; that landing also introduces an architecture
//! guard requiring real production probes.
//!
//! Discrimination contract:
//! - **Pre-change tree (no substrate TLS wiring):**
//!   `_audit_harness_probe()` returns `false` even when the harness
//!   installs `RequestContextGuard`, because nothing planted the
//!   observer into the substrate's TLS slot →
//!   `cross_crate_observer_reaches_compiler` fails.
//! - **Post-change tree (current tree):**
//!   `RequestContextGuard::install` plants the observer into the
//!   substrate slot, the probe sees `Some(observer)`, returns
//!   `true`, and the test passes.

use verter_session::tests::audit_tls_harness::assert_observer_reaches;

#[test]
fn cross_crate_observer_reaches_compiler() {
    let mut probe_saw_observer = false;
    let report = assert_observer_reaches(true, || {
        probe_saw_observer = verter_compiler::_audit_harness_probe();
    });

    assert!(
        probe_saw_observer,
        "verter_compiler's `_audit_harness_probe` must see Some(observer) when \
         the harness installs audit; pre-change tree (no substrate TLS plumbing \
         in RequestContextGuard::install) would leave the slot empty and the \
         probe would return false. report = {report:?}",
    );
    assert!(
        report.observer_seen_on_calling_thread,
        "harness must record the calling-thread observation as Some \
         when the cross-crate probe returns true: {report:?}",
    );
}

#[test]
fn cross_crate_observer_absent_when_audit_disabled() {
    let mut probe_saw_observer = true;
    let report = assert_observer_reaches(false, || {
        probe_saw_observer = verter_compiler::_audit_harness_probe();
    });

    assert!(
        !probe_saw_observer,
        "verter_compiler's `_audit_harness_probe` must see None when audit is \
         NOT installed; a tautological probe always returning true would fail \
         this discriminator. report = {report:?}",
    );
    assert!(
        !report.observer_seen_on_calling_thread,
        "harness must record the calling-thread observation as None when audit \
         is disabled: {report:?}",
    );
}
