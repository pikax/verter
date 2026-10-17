//! Allocation and fact work per step of a growing tail run.
//!
//! `Loop<T>` applies itself to `[T]` in both branches, so its run wraps the
//! argument once per step and never repeats a state: growth alone proves
//! nothing, and the run goes on until the tail allowance stops it. Each
//! step decides `T extends 0` over an argument one level deeper than the
//! last. What a step costs must not depend on how deep the argument has
//! grown: a decision that rereads the whole argument (its free type
//! variables, its membership on the active path) costs the square of the
//! run's length in all.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::project_semantic_dispatch::connected_demand::TailBudgetForTests;
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::{alloc_bytes, alloc_count, reset_alloc_counter};

const SCENARIO: &str = "type Loop<T> = T extends 0 ? Loop<[T]> : Loop<[T]>;\n\
export type Probe = Loop<0>;\n";

/// What one cold `Loop<0>` demand under a tail allowance of `steps` costs
/// on this thread.
struct ColdRun {
    allocations: u64,
    bytes: u64,
    fact_work: usize,
}

fn cold_run(steps: u32) -> ColdRun {
    let path = "/growth/scenario.ts".to_string();
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file(path.clone(), Arc::from(SCENARIO));
    let workspace: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(path.clone()),
        input_id: path.clone(),
        source: Arc::from(SCENARIO),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _budget = TailBudgetForTests::install(steps);
    reset_alloc_counter();
    let fact_work_before = verter_session_query::facts::fact_read_set::signature_work_for_tests();
    let (outcome, _) = host
        .resolve_named_symbol_wire_with_audit(
            &path,
            "Probe",
            &[],
            Some(verter_type_engine::semantic_query::ProjectionMode::Expanded),
        )
        .into_parts();
    let run = ColdRun {
        allocations: alloc_count(),
        bytes: alloc_bytes(),
        fact_work: verter_session_query::facts::fact_read_set::signature_work_for_tests()
            - fact_work_before,
    };
    // The run stops at its allowance with the checker's TS2589 recovery.
    assert!(
        matches!(outcome, Ok(Some(_))),
        "Loop<0> answers under a tail allowance of {steps}: {outcome:?}"
    );
    run
}

/// Doubling the run doubles its allocations, the bytes it allocates and
/// its fact work, within a margin: per-step work is flat however deep the
/// argument has grown.
#[test]
fn growing_tail_run_work_per_step_stays_flat() {
    let sizes = [250u32, 500, 1000];
    let runs = sizes.map(cold_run);
    let measured: Vec<(u32, u64, u64, usize)> = sizes
        .iter()
        .zip(&runs)
        .map(|(steps, run)| (*steps, run.allocations, run.bytes, run.fact_work))
        .collect();
    eprintln!("RECURSIVE_GROWTH (steps, allocations, bytes, fact_work) = {measured:?}");
    for pair in runs.windows(2) {
        let [shorter, longer] = pair else {
            unreachable!("windows of two")
        };
        // Twice the steps: twice the work, plus a margin for the setup the
        // two runs share and the containers that grow by doubling.
        assert!(
            longer.allocations * 10 <= shorter.allocations * 21,
            "allocations must grow linearly with the run: {measured:?}"
        );
        assert!(
            longer.bytes * 10 <= shorter.bytes * 21,
            "allocated bytes must grow linearly with the run: {measured:?}"
        );
        assert!(
            longer.fact_work * 10 <= shorter.fact_work * 21,
            "fact work must grow linearly with the run: {measured:?}"
        );
    }
}
