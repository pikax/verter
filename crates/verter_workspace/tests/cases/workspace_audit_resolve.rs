#![allow(deprecated)]
//! Discriminating test for `WorkspaceAccess::audit_op`.
//!
//! Drives a real specifier resolution through `MemoryWorkspace` via
//! the trait method and asserts:
//!   * the returned record's `kind` is `RequestKind::Workspace { op: AuditResolve { ... } }`,
//!   * the typed `WorkspacePayload` carries the same `op` discriminator,
//!   * `record.files` lists EXACTLY the resolved target file (and only
//!     that file) — i.e., the resolver did not walk unrelated imports.
//!
//! Doubles as a regression for the macro-traversal MUST-NOT-walk-unrelated-imports
//! invariant (`CLAUDE.md` "Macro Type Traversal Rule"): if `audit_op`
//! starts touching unrelated files, `record.files` would grow beyond
//! the single resolved target and this test would fail.
//!
//! **Discriminating**: if `audit_op` started touching unrelated files,
//! `record.files` would grow beyond the single resolved target and the
//! assertions below would fail.

use std::sync::Arc;

use verter_audit::{RequestKind, RequestKindPayload, WorkspaceOp};
use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ParsedEdge, ProjectGraph,
    ProjectMembership, ProjectRank, ResolutionContext, ResolvePhase, ResolveRequestKind,
    VfsProjectConfig, WorkspaceAccess, WorkspaceRead,
};

#[test]
fn audit_op_resolve_records_exactly_the_resolved_target_in_files() {
    // Two files exist in the workspace; we resolve `./utils` from
    // `app.vue`. The resolver must touch ONLY `utils.ts`. A third file
    // (`unrelated.ts`) is also present but must NOT appear in
    // `record.files` — that's the macro-traversal invariant we're
    // doubling as a regression test for.
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/app.vue".to_string(),
        Arc::from("<script setup lang=\"ts\">import { x } from \"./utils\";</script>"),
    );
    ws.inject_file(
        "d:/project/src/utils.ts".to_string(),
        Arc::from("export const x = 1;"),
    );
    ws.inject_file(
        "d:/project/src/unrelated.ts".to_string(),
        Arc::from("export const y = 2;"),
    );

    let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
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
    }]);
    ws.set_project_graph(graph);

    // Sanity: confirm the resolver actually finds utils.ts so the
    // record.files assertion below is meaningful (i.e., a successful
    // resolve produced exactly one touched file).
    let resolved = ws
        .resolve_import(
            "d:/project/src/app.vue",
            "./utils",
            ResolutionContext {
                phase: ResolvePhase::CodegenBlocker,
                kind: ResolveRequestKind::EsmImport,
            },
        )
        .expect("./utils must resolve to utils.ts under the project graph");
    assert_eq!(
        resolved.source_id, "d:/project/src/utils.ts",
        "sanity: resolver returned the expected target before audit_op runs"
    );

    // Drive the audited op.
    let op = WorkspaceOp::AuditResolve {
        specifier: "./utils".to_string(),
        from: "d:/project/src/app.vue".to_string(),
    };
    let record = ws.audit_op(op.clone());

    // Kind matches the requested op.
    match &record.kind {
        RequestKind::Workspace { op: kind_op } => {
            assert_eq!(
                kind_op, &op,
                "RequestKind::Workspace {{ op }} must mirror the audited op"
            );
        }
        other => panic!("expected RequestKind::Workspace, got {other:?}"),
    }

    // Typed payload also carries the op (parallel to LspRequestPayload::method).
    let payload = match &record.kind_payload {
        RequestKindPayload::Workspace(p) => p,
        other => panic!("expected RequestKindPayload::Workspace, got {other:?}"),
    };
    assert_eq!(
        payload.op, op,
        "WorkspacePayload::op must mirror the audited op"
    );
    assert!(
        payload.ms >= 0.0,
        "WorkspacePayload::ms must be a non-negative duration"
    );

    // Discriminating assertion on the macro-traversal invariant:
    // exactly one file touched, and it is the resolved target.
    let touched: Vec<&str> = record
        .files
        .iter()
        .map(|f| f.canonical_id.as_str())
        .collect();
    assert_eq!(
        touched,
        vec!["d:/project/src/utils.ts"],
        "audit_op(AuditResolve) must list EXACTLY the resolved target in `record.files`. \
         A future regression that walks unrelated imports would surface here as extra entries."
    );
    assert_eq!(
        payload.files_touched, 1,
        "files_touched must equal the number of files in record.files"
    );

    // Negative assertion: `unrelated.ts` MUST NOT appear in record.files.
    assert!(
        !record
            .files
            .iter()
            .any(|f| f.canonical_id.as_str() == "d:/project/src/unrelated.ts"),
        "macro-traversal invariant: audit_op must not touch unrelated files; \
         `unrelated.ts` is present in the workspace but not on the resolved path \
         and must not appear in record.files"
    );
}

#[test]
fn audit_op_resolve_unresolved_specifier_yields_zero_touched_files() {
    // Negative path: a specifier that doesn't resolve (the project
    // graph does not know about `./does-not-exist`) must still produce
    // a well-formed record, with `files_touched == 0` and an empty
    // `record.files`. This guards the default impl against silently
    // recording phantom entries on the resolution-failure path.
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/app.vue".to_string(),
        Arc::from("<template></template>"),
    );
    let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
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
    }]);
    ws.set_project_graph(graph);

    let _unused_edge = ParsedEdge::Bare {
        specifier: "vue".to_string(),
        kind: ResolveRequestKind::EsmImport,
    };

    let op = WorkspaceOp::AuditResolve {
        specifier: "./does-not-exist".to_string(),
        from: "d:/project/src/app.vue".to_string(),
    };
    let record = ws.audit_op(op);
    assert_eq!(
        record.files.len(),
        0,
        "unresolved specifier must produce zero touched files; got {:?}",
        record.files
    );
    let payload = record
        .workspace_payload()
        .expect("Workspace payload must be present");
    assert_eq!(payload.files_touched, 0);
    assert_eq!(payload.dep_edges_traversed, 0);
}
