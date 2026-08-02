//! ARCH GUARD — no depth sentinel on the `FlowReturn` evaluation path.
//!
//! The whole-function `FlowReturn` producer
//! (`crates/verter_session/src/project_semantic_dispatch/flow_return.rs`)
//! evaluates a demanded function through the owned whole-body flow IR
//! (`crates/verter_session/src/flow_ir.rs`). The evaluation walk —
//! region/statement recursion, the contributor join, the coinductive
//! hold discharge, and the tagged SCC close — carries NO depth counter
//! and NO depth sentinel: recursion is discharged coinductively through
//! the obligation runtime, never by a bounded retry. The only budget on
//! the path is the shallow expression lowering's own depth / work budget
//! inside `verter_semantic`'s `infer_*` leaf lowering (surfaced as the
//! typed `FlowReturnFailure::Budget` reason, exactly the scanner's
//! `Unavailable` verdict for the same leaf) and the obligation runtime's
//! connected-demand cap — neither is a depth counter on the evaluation
//! walk itself.
//!
//! Scan scope: the two producer files. Forbidden markers:
//!
//! - `depth` (any depth-counter parameter / field / local on the walk);
//! - `MAX_SEMANTIC_INFERENCE_DEPTH`, `SEMANTIC_INFERENCE_TRAVERSAL_BUDGET`
//!   (the shallow scanner's sentinel constants);
//! - `InferenceBudget` (the shallow scanner's budget type — the leaf
//!   lowering keeps its own budget inside `verter_semantic`, it never
//!   rides the flow walk).
//!
//! Discrimination: the guard PASSES on the current tree. A planted
//! `depth` marker on either file must FAIL it.

use std::fs;
use std::path::Path;

/// The producer files the sentinel is forbidden in.
const PRODUCER_FILES: &[&str] = &[
    "crates/verter_session/src/project_semantic_dispatch/flow_return.rs",
    "crates/verter_session/src/flow_ir.rs",
];

/// The forbidden sentinel markers.
const SENTINEL_MARKERS: &[&str] = &[
    "depth",
    "MAX_SEMANTIC_INFERENCE_DEPTH",
    "SEMANTIC_INFERENCE_TRAVERSAL_BUDGET",
    "InferenceBudget",
];

fn violations(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for file in PRODUCER_FILES {
        let path = root.join(file);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
        for marker in SENTINEL_MARKERS {
            if source.contains(marker) {
                out.push(format!(
                    "{file}: forbidden depth-sentinel marker `{marker}`"
                ));
            }
        }
    }
    out
}

#[test]
fn no_depth_sentinel_on_flow_return_path() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().expect("workspace root");
    let found = violations(&root);
    assert!(
        found.is_empty(),
        "the FlowReturn evaluation path carries no depth sentinel: {found:?}"
    );
}

#[test]
fn no_depth_sentinel_guard_predicate_flags_a_planted_marker() {
    let dir = std::env::temp_dir().join(format!("u6_depth_sentinel_guard_{}", std::process::id()));
    for file in PRODUCER_FILES {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, "fn eval(&self, depth: usize) {}\n").expect("write");
    }
    let found = violations(&dir);
    fs::remove_dir_all(&dir).ok();
    assert!(
        !found.is_empty(),
        "a planted depth marker on the evaluation path must fail the guard"
    );
}
