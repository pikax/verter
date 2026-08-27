//! The observation interface does not extend `ResolverContext` and cannot be
//! built holding a host/scheduler reference. The trait's crate-private seal
//! enforces that ownership invariant at the type level. If a change
//! accidentally unseals `ResolverObservation`, or turns it into a
//! subtrait of a host-capable trait, this fixture starts compiling and the
//! test fails.

#[test]
fn host_shaped_type_cannot_implement_resolver_observation() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/host_holding_type_does_not_satisfy_resolver_observation.rs");
    t.compile_fail("tests/compile-fail/attempt_view_rejects_live_callback_capture.rs");
}
