//! Allocation and deterministic work per step of a growing tail run.
//!
//! `Loop<T>` applies itself to `[T]` in both branches, so its run wraps the
//! argument once per step and never repeats a state: growth alone proves
//! nothing, and the run goes on until the tail allowance stops it. Each
//! step decides `T extends 0` over an argument one level deeper than the
//! last. What a step costs must not depend on how deep the argument has
//! grown: a decision that rereads the whole argument (its free type
//! variables, its membership on the active path) costs the square of the
//! run's length in all.
//!
//! The available counters cover conditional operand-walk visits, structural
//! `TypeExpr` hashing visits, fresh substitution walks and fact-signature
//! evidence work. There is no exposed active-membership candidate counter
//! or total hashing-operation counter; structural hashing visits measure
//! only that walk, not all engine hashing.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::mapper_binder_registry::type_expr_visits_for_tests;
use verter_type_engine::project_semantic_dispatch::connected_demand::TailBudgetForTests;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::{ResolverCapabilities, ResolverContext};
use verter_type_engine::semantic_query::{
    CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    RecoveryBasis, SemanticNodeData,
};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::{alloc_bytes, alloc_count, reset_alloc_counter};

const SCENARIO: &str = "type Loop<T> = T extends 0 ? Loop<[T]> : Loop<[T]>;\n\
export type Probe = Loop<0>;\n";

/// What one cold `Loop<0>` demand under a tail allowance of `steps` costs
/// on this thread.
#[derive(Debug)]
struct ColdRun {
    allocations: u64,
    bytes: u64,
    fact_work: usize,
    operand_visits: usize,
    structural_hash_visits: u64,
    substitution_walks: u64,
}

impl ColdRun {
    fn counters(&self) -> [(&'static str, u64); 6] {
        [
            ("allocations", self.allocations),
            ("allocated bytes", self.bytes),
            ("fact work", self.fact_work as u64),
            ("operand visits", self.operand_visits as u64),
            ("structural hashing visits", self.structural_hash_visits),
            ("substitution walks", self.substitution_walks),
        ]
    }
}

// Infer the host's capability type to read the existing thread-local
// counters without exposing its private concrete capability type.
fn conditional_work<C: ResolverCapabilities>(_: &impl ResolverContext<C>) -> (usize, usize) {
    (
        ProjectSemanticDispatch::<C>::conditional_operand_walk_visits_for_tests(),
        ProjectSemanticDispatch::<C>::conditional_branch_selections_for_tests(),
    )
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
    let graph = host.project_type_store().semantic_graph();
    let stats_before = graph.stats_snapshot();
    let (visits_before, selections_before) = conditional_work(host.as_ref());
    let hash_visits_before = type_expr_visits_for_tests();
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
        operand_visits: conditional_work(host.as_ref()).0 - visits_before,
        structural_hash_visits: type_expr_visits_for_tests() - hash_visits_before,
        substitution_walks: graph.stats_snapshot().substitute_memo_misses
            - stats_before.substitute_memo_misses,
    };
    // This fixture selects one recursive branch per tail step, including
    // the last step, which the budget refuses. A constant early cutoff or
    // an unexpected divergence certificate must not pass the growth bound.
    let selections = conditional_work(host.as_ref()).1 - selections_before;
    assert_eq!(
        selections, steps as usize,
        "every run must reach its requested tail allowance: {steps}"
    );
    let node = outcome
        .expect("the growing tail demand completes")
        .expect("the probe exists");
    let data = graph.node_data(node.id());
    assert!(
        matches!(
            data.as_deref(),
            Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                diagnostic: CheckerDiagnostic {
                    code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
                    operation: CheckerDiagnosticOperation::ConditionalTail,
                },
                basis: RecoveryBasis::Budget,
                ..
            }))
        ),
        "Loop<0> stops at its tail budget with TS2589: {data:?}"
    );
    run
}

/// Doubling the run at most doubles each measured cost, within a margin:
/// per-step work is flat however deep the argument has grown.
#[test]
fn growing_tail_run_work_per_step_stays_flat() {
    let sizes = [250u32, 500, 1000];
    let runs = sizes.map(cold_run);
    let measured: Vec<_> = sizes.iter().zip(&runs).collect();
    eprintln!("RECURSIVE_GROWTH (steps, costs) = {measured:?}");
    for pair in runs.windows(2) {
        let [shorter, longer] = pair else {
            unreachable!("windows of two")
        };
        // Twice the steps: twice the work, plus a margin for the setup the
        // two runs share and the containers that grow by doubling.
        for ((name, shorter), (_, longer)) in shorter.counters().into_iter().zip(longer.counters())
        {
            assert!(
                longer * 10 <= shorter * 21,
                "{name} must grow linearly with the run: {measured:?}"
            );
        }
    }
}
