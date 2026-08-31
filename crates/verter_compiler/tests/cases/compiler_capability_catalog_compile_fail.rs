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
    tests.compile_fail("tests/cases/compile-fail/register_frontend_does_not_take_epoch_value.rs");
    tests.compile_fail("tests/cases/compile-fail/host_epoch_forbidden_on_frontend.rs");
    tests.compile_fail("tests/cases/compile-fail/host_integration_requires_host_epoch.rs");
    tests.compile_fail("tests/cases/compile-fail/register_semantic_rejects_mismatched_epoch.rs");
    tests.compile_fail("tests/cases/compile-fail/epoch_id_is_not_a_framework_epoch.rs");
}

/// Consume-once admission evidence: one issued admission cannot drive a
/// second execution (by-value move), and the per-demand execution grant
/// can be neither forged by struct literal nor minted outside the crate.
#[test]
fn admissions_are_consume_once_and_grants_are_unforgeable() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/cases/compile-fail/compile_admission_not_reexecutable.rs");
    tests.compile_fail("tests/cases/compile-fail/product_execution_grant_not_forgeable.rs");
}
