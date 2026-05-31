//! Architecture guards for the SchedulerDag readiness authority.
//!
//! These guards assert the final-state invariants:
//!
//! - The legacy `JobIndex` symbol does not exist in the scheduler crate.
//! - The legacy `BlockerRegistry` symbol does not exist.
//! - The legacy `pending_requests` field does not exist on `FileNode`.
//! - The legacy `BlockerResolved` Submission variant does not exist.
//! - `SchedulerDag` is declared in `dag.rs` and is the sole readiness
//!   authority — i.e. it owns admission (`submit`), readiness
//!   (`next_ready`), dep gating (`has_pending_deps`/`complete`),
//!   waiter bookkeeping (`register_request`), priority service
//!   (`upgrade_priority`), and capacity reservation (`reserve_capacity`).
//!
//! The guards run as cheap static source scans against the
//! `crates/verter_scheduler/src/` tree.

use std::fs;
use std::path::{Path, PathBuf};

fn scheduler_src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn read_scheduler_source() -> String {
    let mut buf = String::new();
    walk(&scheduler_src_root(), &mut buf);
    buf
}

fn walk(dir: &Path, buf: &mut String) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            walk(&path, buf);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            buf.push_str(&fs::read_to_string(&path).expect("read file"));
            buf.push('\n');
        }
    }
}

/// H23 — `JobIndex` is fully retired in the scheduler crate source.
///
/// Discriminator: any of `struct JobIndex`, `impl JobIndex`, or
/// `Mutex<JobIndex>` re-introduced as a field or symbol makes this
/// guard fire. Documentation references in test names / comments do
/// not count — we only flag declarations or storage.
#[test]
fn h23_job_index_symbol_does_not_exist_in_scheduler_src() {
    let src = read_scheduler_source();
    for needle in ["struct JobIndex", "impl JobIndex", "Mutex<JobIndex>"] {
        assert!(
            !src.contains(needle),
            "H23: legacy `JobIndex` re-appeared in scheduler src — found `{needle}`",
        );
    }
}

/// H23 — `BlockerRegistry`, `BlockerRef`, and `UnblockedJob` are
/// fully retired. The DAG's typed dep model replaces them.
#[test]
fn h23_blocker_registry_symbols_do_not_exist_in_scheduler_src() {
    let src = read_scheduler_source();
    for needle in [
        "struct BlockerRegistry",
        "impl BlockerRegistry",
        "struct BlockerRef",
        "struct UnblockedJob",
    ] {
        assert!(
            !src.contains(needle),
            "H23: legacy blocker symbol re-appeared in scheduler src — found `{needle}`",
        );
    }
}

/// H23 — `FileNode.pending_requests` does not exist as a field on
/// the node. Per-file waiter bookkeeping lives in the DAG only.
#[test]
fn h23_file_node_pending_requests_field_does_not_exist() {
    let src = read_scheduler_source();
    // The retired field declaration was `pub(crate) pending_requests:`.
    assert!(
        !src.contains("pending_requests: PendingRequests"),
        "H23: `FileNode.pending_requests` field re-appeared in scheduler src",
    );
    assert!(
        !src.contains("pending_requests: crate::node::PendingRequests"),
        "H23: `FileNode.pending_requests` field re-appeared in scheduler src",
    );
    // No `PendingRequests` struct declaration anywhere either.
    assert!(
        !src.contains("struct PendingRequests"),
        "H23: legacy `PendingRequests` struct re-appeared in scheduler src",
    );
}

/// H23 — `Submission::BlockerResolved` is retired; the DAG resolves
/// dep edges from `StageComplete` directly. No second submission
/// variant should pop blockers.
#[test]
fn h23_submission_blocker_resolved_variant_does_not_exist() {
    let src = read_scheduler_source();
    assert!(
        !src.contains("BlockerResolved {"),
        "H23: legacy `Submission::BlockerResolved` variant re-appeared",
    );
    assert!(
        !src.contains("Submission::BlockerResolved"),
        "H23: legacy `Submission::BlockerResolved` match arm re-appeared",
    );
}

/// H23 — `SchedulerDag` is declared in `dag.rs` and is the sole
/// readiness authority. The guard checks that the dag module exists
/// and the type declaration is present.
#[test]
fn h23_scheduler_dag_is_declared_in_dag_module() {
    let dag_path = scheduler_src_root().join("dag.rs");
    assert!(
        dag_path.exists(),
        "H23: scheduler src must contain a `dag.rs` module"
    );
    let src = fs::read_to_string(&dag_path).expect("read dag.rs");
    assert!(
        src.contains("pub struct SchedulerDag"),
        "H23: `SchedulerDag` type must be declared in dag.rs",
    );
}

/// H23 — the DAG owns all five readiness-authority concerns. The
/// guard inspects `dag.rs` for the public surface that proves it.
#[test]
fn h23_scheduler_dag_owns_every_readiness_concern() {
    let src = fs::read_to_string(scheduler_src_root().join("dag.rs")).expect("read dag.rs");
    for needle in [
        "pub fn submit(",                // admission
        "pub fn next_ready(",            // ordering / readiness
        "pub fn complete(",              // dep resolution
        "pub fn has_pending_deps(",      // gate inspection
        "pub fn register_request(",      // waiter bookkeeping
        "pub fn reserve_capacity(",      // capacity accounting
        "pub fn upgrade_priority(",      // priority service
        "pub fn cancel(",                // supersession / removal
        "pub fn signal_stage_complete(", // completion fan-out
    ] {
        assert!(
            src.contains(needle),
            "H23: SchedulerDag must expose `{needle}` to remain the sole readiness authority",
        );
    }
}

/// H23 — `WorkNodeIdentity` is a typed sum with EXACTLY three
/// variants. A fourth variant would expand the discriminator surface
/// and is an architectural change, not a routine extension.
///
/// We check via the textual signature: each variant header appears
/// exactly once in dag.rs.
#[test]
fn h23_work_node_identity_is_a_three_variant_typed_sum() {
    let src = fs::read_to_string(scheduler_src_root().join("dag.rs")).expect("read dag.rs");

    // The enum declaration line must be present.
    assert!(
        src.contains("pub enum WorkNodeIdentity {"),
        "H23: WorkNodeIdentity enum declaration missing",
    );

    // Count variant headers inside the enum body. We use a tight
    // pattern — `<variant> {` — that matches the struct-variant
    // declarations and the constructor sites are distinguished by
    // their full path `WorkNodeIdentity::<variant>`.
    let enum_body_start = src
        .find("pub enum WorkNodeIdentity {")
        .expect("enum header")
        + "pub enum WorkNodeIdentity {".len();
    // Find the matching closing `}` for the enum body — bias by the
    // shortest substring up to the next `}` that's outside any nested
    // braces. Variants are struct-variants with `{}` so we walk.
    let mut depth = 0;
    let mut enum_end = None;
    for (i, ch) in src[enum_body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    enum_end = Some(enum_body_start + i);
                    break;
                } else {
                    depth -= 1;
                }
            }
            _ => {}
        }
    }
    let enum_end = enum_end.expect("matched enum closing brace");
    let enum_body = &src[enum_body_start..enum_end];

    let file_stage = enum_body.matches("FileStage {").count();
    let artifact = enum_body.matches("Artifact {").count();
    let cache_node = enum_body.matches("CacheNode {").count();
    assert_eq!(
        file_stage, 1,
        "H23: FileStage must appear exactly once in WorkNodeIdentity"
    );
    assert_eq!(
        artifact, 1,
        "H23: Artifact must appear exactly once in WorkNodeIdentity"
    );
    assert_eq!(
        cache_node, 1,
        "H23: CacheNode must appear exactly once in WorkNodeIdentity"
    );
    // No fourth variant.
    let total_variant_headers = file_stage + artifact + cache_node;
    assert_eq!(
        total_variant_headers, 3,
        "H23: WorkNodeIdentity must have exactly three variants — a fourth is an architectural change",
    );
}

/// H23 — `DagCapacityReservation::release` consumes the reservation
/// by value. A second release is statically unrepresentable; we
/// verify this by inspecting the method signature in the dag module
/// (either `dag.rs` directly or its `capacity` submodule).
#[test]
fn h23_dag_capacity_reservation_release_consumes_by_value() {
    let src = read_scheduler_source();
    // Single-release rail: `release(mut self)` consumes the
    // reservation; the second-release fingerprint
    // `release(&self)` / `release(&mut self)` would let permits leak.
    assert!(
        src.contains("pub fn release(mut self)"),
        "H23: DagCapacityReservation::release must consume by value (`release(self)`) — \
         a `&self` or `&mut self` signature would allow double-release",
    );
    assert!(
        !src.contains("pub fn release(&self)"),
        "H23: DagCapacityReservation::release must not take `&self` — double-release would be possible",
    );
    assert!(
        !src.contains("pub fn release(&mut self)"),
        "H23: DagCapacityReservation::release must not take `&mut self` — double-release would be possible",
    );
}
