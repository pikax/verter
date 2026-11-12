//! Stack-safety discriminator for the heap-owned deferred evaluator.
//!
//! A deep but finite deferred-operator chain is semantic work, not recursion
//! depth. It must complete on a controlled worker stack without relying on
//! \`RUST_MIN_STACK\`, and its complete result may warm the evaluator memo.
//!
//! The gate runs 5,000 operators on 1 MiB, about 210 bytes of stack an
//! operator, as 10,000 operators on 2 MiB allow, and less once the
//! evaluator's fixed need is taken out: an evaluator that spends stack per
//! operator overflows here wherever it overflows the full size. The chain is
//! past the shared structural depth cap (`MAX_DEPTH`, 4,096), so a depth
//! fuse truncating it fails here too. The 10,000-operator, 2 MiB run is in
//! the nightly scale tier.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use std::process::Command;
use std::sync::Arc;

use verter_session::{for_tests, HostConfig, VerterHost};
use verter_type_engine::semantic_query::{
    PrimitiveKind, ProjectionMode, ProjectionReductionContext, ResultCompleteness, SemanticNodeData,
};

const CHILD_MARKER: &str = "VERTER_DEFERRED_HEAP_CHILD";

fn run_deep_finite_keyof_child(chain_depth: usize, stack_bytes: usize) {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let graph = host.project_type_store().semantic_graph();
    // This thread owns the chain while the worker evaluates it.
    let _scope = graph.enter_node_root_scope();
    let leaf = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let mut root = leaf;
    for _ in 0..chain_depth {
        root = graph.intern_node(SemanticNodeData::KeyOf { base: root });
    }

    let worker_host = Arc::clone(&host);
    let (first_node, first_completeness, second_node, second_completeness) =
        std::thread::Builder::new()
            .name("deferred-evaluator".to_string())
            .stack_size(stack_bytes)
            .spawn(move || {
                let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
                let first =
                    for_tests::dispatch_evaluate_deferred_for_tests(&worker_host, root, context);
                let second =
                    for_tests::dispatch_evaluate_deferred_for_tests(&worker_host, root, context);
                (first.0, first.1, second.0, second.1)
            })
            .expect("controlled evaluator worker must spawn")
            .join()
            .expect("heap evaluator worker must return without stack overflow");

    assert_eq!(
        first_completeness,
        ResultCompleteness::Complete,
        "a finite {chain_depth}-operator chain must not be truncated by structural depth"
    );
    assert_eq!(
        second_completeness,
        ResultCompleteness::Complete,
        "the warm replay of a complete finite chain must remain Complete"
    );
    assert_eq!(
        second_node, first_node,
        "warm replay must preserve the completed semantic result"
    );

    let stats = graph.stats_snapshot();
    assert!(
        stats.evaluate_deferred_memo_hits >= 1,
        "the complete result should be admitted and reused; stats={stats:?}"
    );
}

/// Runs the `chain_depth`-operator chain on a `stack_bytes` worker in a child
/// process that re-runs `test`, the calling test's name.
fn assert_deep_finite_keyof_chain_completes(test: &str, chain_depth: usize, stack_bytes: usize) {
    if std::env::var(CHILD_MARKER).as_deref() == Ok("1") {
        run_deep_finite_keyof_child(chain_depth, stack_bytes);
        return;
    }

    let exe = std::env::current_exe().expect("current integration-test executable");
    let status = Command::new(exe)
        .arg(test)
        .arg("--nocapture")
        .env(CHILD_MARKER, "1")
        .env_remove("RUST_MIN_STACK")
        .status()
        .expect("spawn isolated deferred-evaluator child");

    assert!(
        status.success(),
        "the controlled {} KiB evaluator subprocess must exit normally; status={status}",
        stack_bytes >> 10
    );
}

#[test]
fn deep_finite_keyof_chain_completes_on_1_mib_stack_subprocess() {
    assert_deep_finite_keyof_chain_completes(
        "deep_finite_keyof_chain_completes_on_1_mib_stack_subprocess",
        5_000,
        1 << 20,
    );
}

mod nightly_scale {
    use super::*;

    /// The 10,000-operator chain on a 2 MiB worker, the full size of
    /// [`super::deep_finite_keyof_chain_completes_on_1_mib_stack_subprocess`].
    #[test]
    fn deep_finite_keyof_chain_completes_on_2_mib_stack_subprocess() {
        assert_deep_finite_keyof_chain_completes(
            "deep_finite_keyof_chain_completes_on_2_mib_stack_subprocess",
            10_000,
            2 << 20,
        );
    }
}
