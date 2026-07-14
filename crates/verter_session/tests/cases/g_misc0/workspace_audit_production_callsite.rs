#![allow(deprecated)]
//! Production-path discriminator for workspace-op auditing.
//!
//! Drives a real `WorkspaceOp::AuditResolve` through
//! [`verter_session::VerterHost::audit_workspace_op`] (the
//! session-level production callsite that wraps
//! [`verter_workspace::WorkspaceAccess::audit_op`] with
//! [`verter_session::host_audit_runtime::AuditRequestRegistration::new`])
//! and asserts:
//!   * the record published to the host's records store carries
//!     `RequestKind::Workspace { op: AuditResolve { ... } }`,
//!   * the active-request registry was drained on finalize,
//!   * `record.files` lists exactly the resolved target,
//!   * `record.request_id` matches the host's monotonic stamp.
//!
//! **Discriminating against pre-change tree**: neither
//! `audit_workspace_op` nor `WorkspaceAccess::audit_op` exist before
//! this slice; this test would not even compile.

use std::sync::Arc;

use verter_audit::{RequestKind, WorkspaceOp};
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectRank,
    VfsProjectConfig,
};

#[test]
fn audit_workspace_op_finalizes_registration_and_publishes_record() {
    // Build a workspace populated with a relative-import target so
    // `WorkspaceOp::AuditResolve` can succeed against real resolver
    // state. A second unrelated file probes the macro-traversal
    // invariant (must NOT appear in record.files).
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(
        "d:/project/src/app.vue".to_string(),
        Arc::from("<script setup lang=\"ts\">import \"./utils\";</script>"),
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
        membership: verter_workspace::ConfiguredMembership::match_all_under_root(
            &verter_workspace::CanonicalPath::new("d:/project"),
        ),
    }]);
    ws.set_project_graph(graph);

    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            ..HostConfig::default()
        },
        ws.clone() as Arc<dyn verter_workspace::WorkspaceAccess>,
    ));

    // Pre-state.
    let snap_before = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap_before.active_request_count, 0,
        "pre-state: registry must be empty"
    );
    assert_eq!(
        snap_before.records_store_size, 0,
        "pre-state: records store must be empty"
    );

    let op = WorkspaceOp::AuditResolve {
        specifier: "./utils".to_string(),
        from: "d:/project/src/app.vue".to_string(),
    };
    let record = host.audit_workspace_op(op.clone());

    // Returned record matches the requested op.
    match &record.kind {
        RequestKind::Workspace { op: kind_op } => {
            assert_eq!(kind_op, &op);
        }
        other => panic!("expected RequestKind::Workspace, got {other:?}"),
    }
    let touched: Vec<&str> = record
        .files
        .iter()
        .map(|f| f.canonical_id.as_str())
        .collect();
    assert_eq!(
        touched,
        vec!["d:/project/src/utils.ts"],
        "record.files must list EXACTLY the resolved target"
    );
    assert!(
        !touched.contains(&"d:/project/src/unrelated.ts"),
        "macro-traversal invariant: unrelated files must not be touched"
    );
    assert_ne!(
        record.request_id, 0,
        "request_id must be stamped non-zero by audit_workspace_op"
    );

    // Post-state: the registration's `finalize` removed the
    // active-request slot AND inserted the record into the
    // records store. Pre-change tree (no audit_workspace_op
    // wiring) leaves the records store empty here.
    let snap_after = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap_after.active_request_count, 0,
        "post-state: registry must be drained by finalize"
    );
    assert!(
        !snap_after.contains_active_request(record.request_id),
        "post-state: request_id must not appear in active registry"
    );
    assert_eq!(
        snap_after.records_store_size, 1,
        "post-state: records store must hold the finalized record. \
         Pre-change tree (no AuditRequestRegistration wiring on the \
         workspace audit path) reports 0 here."
    );

    // Drain the record and verify it matches our request_id and op.
    let taken = host
        .host_audit_runtime()
        .take_record(record.request_id)
        .expect("records store must hold the finalised record");
    assert_eq!(taken.request_id, record.request_id);
    match &taken.kind {
        RequestKind::Workspace { op: kind_op } => {
            assert_eq!(kind_op, &op, "drained record must carry the same op");
        }
        other => panic!("drained record kind: expected Workspace, got {other:?}"),
    }
}
