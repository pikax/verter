//! Compile-fail proofs: absent capabilities are unnameable as present
//! backends; no placeholder runtime impl is required for tooling-only rows.

#[test]
fn absent_capabilities_are_compile_time_truth() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/cases/compile-fail/frontend_only_has_no_runtime_accessor.rs");
    tests.compile_fail("tests/cases/compile-fail/projection_only_has_no_runtime_accessor.rs");
    tests.compile_fail(
        "tests/cases/compile-fail/register_frontend_does_not_take_runtime_identity.rs",
    );
    tests.compile_fail("tests/cases/compile-fail/host_epoch_forbidden_on_frontend.rs");
    tests.compile_fail("tests/cases/compile-fail/host_integration_requires_host_epoch.rs");
}
