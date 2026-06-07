#![allow(deprecated)]
//! Discriminating test for `WorkspaceAccess::audit_op`
//! covering the `DepGraphTraverse` operation.
//!
//! Wires a real forward-dep graph (`root.vue` → `mid.ts` → `leaf.ts`)
//! into a `MemoryWorkspace` via `record_parsed_edges`, then drives
//! `audit_op(DepGraphTraverse { root })` and asserts:
//!   * the produced record's kind is the matching `Workspace { op }`,
//!   * `dep_edges_traversed > 0` — the traversal walked the live edge
//!     store, not a stub,
//!   * the touched-file set contains every transitively-reachable
//!     canonical from the root.
//!
//! **Discriminating**: the test exercises `audit_op`; a stub that
//! returned a zero edge count would fail the `dep_edges_traversed`
//! assertions.

use std::sync::Arc;

use verter_audit::{RequestKind, WorkspaceOp};
use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ParsedEdge, ProjectGraph,
    ProjectMembership, ProjectRank, ResolveRequestKind, VfsProjectConfig, WorkspaceAccess,
    WorkspaceRead,
};

fn project_config() -> VfsProjectConfig {
    VfsProjectConfig {
        root: "d:/project".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: None,
        root_files: vec![],
        extensions: vec![".vue".to_string(), ".ts".to_string()],
        workspace_root: "d:/project".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }
}

#[test]
fn audit_op_dep_graph_traverse_walks_real_edges_and_reports_count() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    // Root → mid → leaf chain, plus an unreachable file the
    // traversal must NOT touch.
    ws.inject_file(
        "d:/project/src/root.vue".to_string(),
        Arc::from("<script setup lang=\"ts\">import \"./mid\";</script>"),
    );
    ws.inject_file(
        "d:/project/src/mid.ts".to_string(),
        Arc::from("import \"./leaf\"; export const x = 1;"),
    );
    ws.inject_file(
        "d:/project/src/leaf.ts".to_string(),
        Arc::from("export const y = 2;"),
    );
    ws.inject_file(
        "d:/project/src/unreachable.ts".to_string(),
        Arc::from("export const z = 3;"),
    );

    ws.set_project_graph(ProjectGraph::from_configs(vec![project_config()]));

    // Wire forward edges: root → mid → leaf. `unreachable.ts` has no
    // edges and is NOT pointed at by anyone.
    ws.record_parsed_edges(
        "d:/project/src/root.vue",
        &[ParsedEdge::Relative {
            specifier: "./mid".to_string(),
            kind: ResolveRequestKind::EsmImport,
        }],
    );
    ws.record_parsed_edges(
        "d:/project/src/mid.ts",
        &[ParsedEdge::Relative {
            specifier: "./leaf".to_string(),
            kind: ResolveRequestKind::EsmImport,
        }],
    );
    // No edges for leaf.ts (terminal) or unreachable.ts (orphan).

    // Sanity: forward edges populated as expected before audit_op runs.
    let root_fwd = ws.forward_deps_for("d:/project/src/root.vue");
    assert!(
        root_fwd.contains(&"d:/project/src/mid.ts".to_string()),
        "sanity: root.vue forward deps should contain mid.ts; got {root_fwd:?}"
    );

    let op = WorkspaceOp::DepGraphTraverse {
        root: "d:/project/src/root.vue".to_string(),
    };
    let record = ws.audit_op(op.clone());

    match &record.kind {
        RequestKind::Workspace { op: kind_op } => {
            assert_eq!(kind_op, &op);
        }
        other => panic!("expected RequestKind::Workspace, got {other:?}"),
    }

    let payload = record.workspace_payload().expect("workspace payload");

    // Discriminating: the BFS must traverse at least the root→mid and
    // mid→leaf edges (2 edges total). A stub that returned `0`
    // would fail this assertion.
    assert!(
        payload.dep_edges_traversed > 0,
        "DepGraphTraverse must walk live forward-dep edges; got dep_edges_traversed={}",
        payload.dep_edges_traversed,
    );
    assert!(
        payload.dep_edges_traversed >= 2,
        "DepGraphTraverse on `root.vue → mid.ts → leaf.ts` must traverse at least 2 edges, \
         got {}",
        payload.dep_edges_traversed,
    );

    // Touched-file set must contain root + reachable transitive deps.
    let touched: Vec<&str> = record
        .files
        .iter()
        .map(|f| f.canonical_id.as_str())
        .collect();
    assert!(
        touched.contains(&"d:/project/src/root.vue"),
        "touched set must include the BFS root; got {touched:?}"
    );
    assert!(
        touched.contains(&"d:/project/src/mid.ts"),
        "touched set must include directly-reachable mid.ts; got {touched:?}"
    );
    assert!(
        touched.contains(&"d:/project/src/leaf.ts"),
        "touched set must include transitively-reachable leaf.ts; got {touched:?}"
    );

    // Negative: orphaned unreachable.ts must not appear.
    assert!(
        !touched.contains(&"d:/project/src/unreachable.ts"),
        "orphaned file must not be touched by BFS; got {touched:?}"
    );

    // files_touched mirrors the `record.files` length.
    assert_eq!(
        payload.files_touched as usize,
        record.files.len(),
        "WorkspacePayload::files_touched must mirror record.files.len()"
    );
}

#[test]
fn audit_op_dep_graph_traverse_orphan_root_zero_edges() {
    // A root with no forward edges produces zero edges traversed, but
    // the root itself is still recorded as visited.
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/orphan.ts".to_string(),
        Arc::from("export const z = 1;"),
    );
    ws.set_project_graph(ProjectGraph::from_configs(vec![project_config()]));

    let op = WorkspaceOp::DepGraphTraverse {
        root: "d:/project/src/orphan.ts".to_string(),
    };
    let record = ws.audit_op(op);
    let payload = record.workspace_payload().expect("workspace payload");
    assert_eq!(
        payload.dep_edges_traversed, 0,
        "orphan root has no forward edges; expected dep_edges_traversed==0"
    );
    assert_eq!(
        payload.files_touched, 1,
        "BFS still visits the root itself; expected files_touched==1"
    );
}
