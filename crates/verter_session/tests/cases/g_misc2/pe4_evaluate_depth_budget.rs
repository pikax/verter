//! Stack-safety discriminator for the heap-owned deferred evaluator.
//!
//! A deep but finite deferred-operator chain is semantic work, not recursion
//! depth. It must complete on a controlled 2 MiB stack without relying on
//! \`RUST_MIN_STACK\`, and its complete result may warm the evaluator memo.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use std::process::Command;
use std::sync::Arc;

use verter_session::semantic_query::{
    PrimitiveKind, ProjectionMode, ProjectionReductionContext, ResultCompleteness, SemanticNodeData,
};
use verter_session::{for_tests, HostConfig, VerterHost};

const CHILD_MARKER: &str = "VERTER_DEFERRED_HEAP_CHILD";
const CHAIN_DEPTH: usize = 10_000;
const CHILD_STACK_BYTES: usize = 2 * 1024 * 1024;

fn run_deep_finite_keyof_child() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let graph = host.project_type_store().semantic_graph();
    let leaf = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let mut root = leaf;
    for _ in 0..CHAIN_DEPTH {
        root = graph.intern_node(SemanticNodeData::KeyOf { base: root });
    }

    let worker_host = Arc::clone(&host);
    let (first_node, first_completeness, second_node, second_completeness) =
        std::thread::Builder::new()
            .name("deferred-evaluator-2mib".to_string())
            .stack_size(CHILD_STACK_BYTES)
            .spawn(move || {
                let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
                let first =
                    for_tests::dispatch_evaluate_deferred_for_tests(&worker_host, root, context);
                let second =
                    for_tests::dispatch_evaluate_deferred_for_tests(&worker_host, root, context);
                (first.0, first.1, second.0, second.1)
            })
            .expect("controlled 2 MiB evaluator worker must spawn")
            .join()
            .expect("heap evaluator worker must return without stack overflow");

    assert_eq!(
        first_completeness,
        ResultCompleteness::Complete,
        "a finite 10,000-operator chain must not be truncated by structural depth"
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

#[test]
fn deep_finite_keyof_chain_completes_on_2_mib_stack_subprocess() {
    if std::env::var(CHILD_MARKER).as_deref() == Ok("1") {
        run_deep_finite_keyof_child();
        return;
    }

    let exe = std::env::current_exe().expect("current integration-test executable");
    let status = Command::new(exe)
        .arg("deep_finite_keyof_chain_completes_on_2_mib_stack_subprocess")
        .arg("--nocapture")
        .env(CHILD_MARKER, "1")
        .env_remove("RUST_MIN_STACK")
        .status()
        .expect("spawn isolated deferred-evaluator child");

    assert!(
        status.success(),
        "the controlled 2 MiB evaluator subprocess must exit normally; status={status}"
    );
}
