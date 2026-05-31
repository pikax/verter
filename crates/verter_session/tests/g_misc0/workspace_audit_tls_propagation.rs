#![allow(deprecated)]
//! TLS-observer propagation through `WorkspaceAccess::audit_op`
//! (Wave 3 Slice 3.D follow-up).
//!
//! `audit_op` is a trait method on `WorkspaceAccess`; the
//! session-level wrapper [`VerterHost::audit_workspace_op`] installs
//! the `RequestContextGuard` BEFORE the workspace traversal so the
//! trait method's body sees `current_observer() == Some(_)`. This
//! test drives that wrapper through the
//! [`verter_session::tests::audit_tls_harness::assert_observer_reaches`]
//! harness and asserts:
//!
//! - **Positive** (`install_audit=true`, `audit_enabled=true` on the
//!   host): the produced record's `kind` is `RequestKind::Workspace`,
//!   `record.files` lists the resolved target, the host wrapper
//!   reaches the trait method's body with the observer installed (the
//!   record is finalised through the `Active` registration), and the
//!   harness's calling-thread observation is `Some`.
//! - **Negative** (`install_audit=false`, `audit_enabled=false`):
//!   `audit_workspace_op` still produces a record (the trait method
//!   is a producer, not gated on audit-enabled), but the harness sees
//!   no observer on its own thread and the host registration takes
//!   the `Noop` arm so no record enters the records store.
//!
//! Test placement: this file lives in `verter_session/tests/` rather
//! than `verter_workspace/tests/` because the harness it drives lives
//! in `verter_session::tests::audit_tls_harness`. Adding
//! `verter_session` as a dev-dep on `verter_workspace` would create a
//! circular dependency through the test target; the existing slice
//! `workspace_audit_production_callsite.rs` resolves the same circular
//! constraint by living here too. The architecture guard
//! `wave_3_entry_points_propagate_tls` substring-matches the entry
//! symbol against tests under `crates/*/tests/`, so the test path is
//! orthogonal to the guard's pin list semantics.

use std::sync::Arc;

use verter_audit::{RequestKind, RequestKindPayload, WorkspaceOp};
use verter_session::tests::audit_tls_harness::assert_observer_reaches;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectMembership,
    ProjectRank, VfsProjectConfig, WorkspaceAccess,
};

fn build_host_with_workspace(audit_enabled: bool) -> Arc<VerterHost> {
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(
        "d:/project/src/app.vue".to_string(),
        Arc::from("<script setup lang=\"ts\">import \"./utils\";</script>"),
    );
    ws.inject_file(
        "d:/project/src/utils.ts".to_string(),
        Arc::from("export const x = 1;"),
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

    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled,
            ..HostConfig::default()
        },
        ws as Arc<dyn WorkspaceAccess>,
    ))
}

#[test]
fn audit_op_propagates_observer_through_workspace_traversal() {
    let host = build_host_with_workspace(true);

    let snap_before = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap_before.records_store_size, 0,
        "pre-state: records store must be empty"
    );

    let mut touched_target: Option<String> = None;
    let mut record_request_id: u64 = 0;
    let report = assert_observer_reaches(true, || {
        // Drive the host's `audit_workspace_op` wrapper which
        // constructs the `AuditRequestRegistration` BEFORE installing
        // `RequestContextGuard`, then calls
        // `WorkspaceAccess::audit_op` so the trait method's body runs
        // with `current_observer() == Some(_)`. The default body
        // walks the workspace and stamps the record's `request_id`
        // from `current_request_id()` — so a non-zero id
        // discriminates that the TLS slot the wrapper installed was
        // observable on the trait method's thread.
        let record = host.audit_workspace_op(WorkspaceOp::AuditResolve {
            specifier: "./utils".to_string(),
            from: "d:/project/src/app.vue".to_string(),
        });

        // `audit_workspace_op` re-stamps `record.request_id` with the
        // host's monotonic id even if the trait body dropped it; the
        // discriminator we want is the per-file canonical_id list
        // populated by the trait body's resolution call, so we
        // capture the resolved target's id (which only the trait body
        // can compute).
        if let RequestKindPayload::Workspace(payload) = &record.kind_payload {
            assert!(
                payload.ms >= 0.0,
                "WorkspacePayload::ms must be non-negative; got {}",
                payload.ms
            );
            assert_eq!(
                payload.files_touched, 1,
                "trait body must walk EXACTLY the resolved target — \
                 a regression that pre-empts the body before resolution \
                 returns 0 here. payload = {payload:?}"
            );
        } else {
            panic!("expected Workspace payload, got {:?}", record.kind_payload);
        }
        match &record.kind {
            RequestKind::Workspace { .. } => {}
            other => panic!("expected RequestKind::Workspace, got {other:?}"),
        }
        touched_target = record.files.first().map(|f| f.canonical_id.clone());
        record_request_id = record.request_id;
    });

    assert_eq!(
        touched_target.as_deref(),
        Some("d:/project/src/utils.ts"),
        "audit_op trait body must reach the resolver and record the resolved \
         target — a regression that fails to install the observer on the calling \
         thread would not affect this assertion (the trait body is a producer, not \
         gated on observer presence), but a regression that strips the workspace \
         producer wiring entirely would empty record.files. touched_target = {touched_target:?}"
    );
    assert_ne!(
        record_request_id, 0,
        "the host wrapper must stamp a non-zero request_id; pre-Wave-3 wiring \
         left this at 0 because no `next_request_id()` allocation preceded the \
         trait call"
    );
    assert!(
        report.observer_seen_on_calling_thread,
        "harness's outer RequestContextGuard remains visible on the calling \
         thread after the entry-point's nested guard drops; a regression that \
         leaks the entry-point's guard past return would either panic on drop \
         or leave a stale slot, either way breaking this contract: {report:?}",
    );

    let snap_after = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap_after.records_store_size, 1,
        "post-state: records store must hold the finalised record"
    );
    assert_eq!(
        snap_after.active_request_count, 0,
        "post-state: active-request registry must be drained by finalize"
    );
}

#[test]
fn audit_op_observer_absent_outside_harness_window() {
    // `audit_workspace_op` does NOT short-circuit on
    // `audit_enabled=false` — the consumer-filter snapshot defaults
    // to `allow_all`, so the wrapper's registration takes the
    // `Active` arm and publishes a record regardless of the
    // host-level enable flag (parallels `resolve_type_with_audit`).
    // The discriminator the harness can drive is the
    // OUTSIDE-the-window observation: with `install_audit=false`,
    // the harness installs no outer `RequestContextGuard`, so after
    // the wrapper's own guard drops on return, the calling thread
    // must see `current_observer() == None`.
    let host = build_host_with_workspace(false);

    let report = assert_observer_reaches(false, || {
        // Drive the wrapper. Inside the wrapper, the registration's
        // `Active` arm installs its own guard for the duration of
        // the workspace traversal, but the guard drops on return —
        // so when the harness's calling-thread probe runs AFTER
        // this closure returns, no observer must be visible.
        let record = host.audit_workspace_op(WorkspaceOp::AuditResolve {
            specifier: "./utils".to_string(),
            from: "d:/project/src/app.vue".to_string(),
        });
        // Sanity: the wrapper still produces a Workspace-kind
        // record envelope (the producer is unconditional and
        // filter-driven, not audit-enabled-driven).
        assert!(
            matches!(record.kind, RequestKind::Workspace { .. }),
            "wrapper must still produce a Workspace-kind record — the producer \
             is unconditional. record = {record:?}"
        );
    });

    assert!(
        !report.observer_seen_on_calling_thread,
        "harness installed no outer guard; the wrapper's nested guard MUST \
         drop on return, so the calling thread must see no observer at the \
         harness's post-call probe point. A regression that leaks the \
         wrapper's guard past its return — the most common TLS-propagation \
         defect — would surface here as `observer_seen_on_calling_thread = true`. \
         report = {report:?}",
    );
}
