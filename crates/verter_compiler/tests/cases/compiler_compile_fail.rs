//! Compile-fail contracts grouped by the feature set of trybuild's nested probe crate.
//!
//! Each fixture keeps its own pinned `.stderr`; grouping only amortizes the generated
//! crate and Cargo bootstrap that `trybuild::TestCases` owns.

#[test]
fn default_compiler_compile_fail_contracts_are_enforced() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/cases/compile-fail/assemble_sequence_requires_validated_fragment.rs");
    tests.compile_fail("tests/cases/compile-fail/registered_artifact_direct_construction.rs");
    tests.compile_fail("tests/cases/compile-fail/registered_artifact_inventory_replacement.rs");
    tests.compile_fail("tests/cases/compile-fail/registered_geometry_state_private.rs");
    tests.compile_fail("tests/cases/compile-fail/registered_projector_direct_invocation.rs");
    tests.compile_fail("tests/cases/compile-fail/verified_plain_css_private_field.rs");
    tests.compile_fail("tests/cases/compile-fail/verified_plain_css_sink_unreachable.rs");
}

/// These fixtures need `verter_compiler/bench` for a live Cargo invocation to reach
/// the narrow item-level visibility wall. The canonical gate excludes the complete
/// trybuild class; direct execution of this group therefore owns the bench-feature
/// diagnostics pinned by these fixtures.
#[test]
#[cfg_attr(
    not(feature = "bench"),
    ignore = "run with --features bench to exercise the narrow visibility walls"
)]
fn bench_compiler_compile_fail_contracts_are_enforced() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/cases/compile-fail/pending_nav_request_unreachable.rs");
    tests.compile_fail("tests/cases/compile-fail/segmented_overwrite_authority_unreachable.rs");
}
