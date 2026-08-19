#[test]
fn registered_geometry_is_unforgeable() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/cases/compile-fail/registered_artifact_direct_construction.rs");
    tests.compile_fail("tests/cases/compile-fail/registered_artifact_inventory_replacement.rs");
    tests.compile_fail("tests/cases/compile-fail/registered_geometry_state_private.rs");
    tests.compile_fail("tests/cases/compile-fail/registered_projector_direct_invocation.rs");
}
