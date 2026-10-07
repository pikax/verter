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
    tests.compile_fail(fixture_root.join("retained_flow_slice_constructor_is_private.rs"));
    // This fixture needs the public test-support constructor, while the
    // reveal accessors it probes must remain private.
    tests.compile_fail(fixture_root.join("instantiate_key_context_not_extractable.rs"));
    // The raw compiler entry must be absent without verter_compiler/test-support.
    tests.compile_fail(fixture_root.join("scanners_replacement_raw_parser_public.rs"));
    // These contracts exercise the actual five request ports and their owned outputs.
    tests.pass(fixture_root.join("engine_ports_actual_host_positive.rs"));
    tests.compile_fail(fixture_root.join("engine_ports_indexed_input_no_worker.rs"));
    tests.compile_fail(fixture_root.join("engine_ports_owned_lowering_no_memo.rs"));
    tests.compile_fail(fixture_root.join("engine_ports_route_no_worker.rs"));
    tests.compile_fail(fixture_root.join("engine_ports_fact_validation_no_workspace.rs"));
    tests.compile_fail(fixture_root.join("engine_ports_cancellation_no_host.rs"));
    tests.compile_fail(fixture_root.join("engine_ports_execution_no_graph.rs"));
    // The engine output authority: no forgery or duplication, no recovery from
    // query access, and no remint from a live engine's recovered stores.
    tests.compile_fail(fixture_root.join("output_authority_not_forgeable.rs"));
    tests.compile_fail(fixture_root.join("output_authority_not_duplicable.rs"));
    tests.compile_fail(fixture_root.join("output_authority_not_recoverable_from_query_access.rs"));
    tests.compile_fail(fixture_root.join("output_authority_not_reminted_from_live_stores.rs"));
}
