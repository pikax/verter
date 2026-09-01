//! Compile contracts that require a feature profile different from the
//! ordinary `verter_session` fixture inventory.

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("compile-contract crate must live below the workspace root");
    let fixture_root = root.join("crates/verter_session/tests/cases/compile-fail");
    let tests = trybuild::TestCases::new();
    // These fixtures name test-only proof carriers and therefore require the
    // session crate's explicit `test-support` feature.
    tests.compile_fail(fixture_root.join("complete_flow_result_constructor_is_private.rs"));
    tests.compile_fail(fixture_root.join("flow_solve_plan_and_spec_are_sealed.rs"));
    tests.compile_fail(fixture_root.join("flow_solve_plan_and_spec_no_struct_literal.rs"));
    tests.compile_fail(fixture_root.join("flow_solve_sealed_witnesses_not_constructible.rs"));
    // This fixture needs the public test-support constructor, while the
    // reveal accessors it probes must remain private.
    tests.compile_fail(fixture_root.join("instantiate_key_context_not_extractable.rs"));
    // The raw compiler entry must be absent without verter_compiler/test-support.
    tests.compile_fail(fixture_root.join("scanners_replacement_raw_parser_public.rs"));
}
