//! Block 1.6 — `host_view_project_identity_for(canonical)` returns the
//! published project's identity hash, not the all-zero stub.
//!
//! Plan citation: `D:/tmp/verter-stage7-final-cutover-plan.md` § "Block 1.6".
//!
//! Discrimination chain:
//! - Pre-Block-1.6 stub: `host_view_project_identity()` returns
//!   `ProjectIdentity([0u8; 16])` regardless of canonical.
//! - Post-Block-1.6 GREEN: `host_view_project_identity_for(canonical)`
//!   maps canonical → ProjectId via the published snapshot, then looks
//!   up the project-identity hash on `PublishedRoot::project_identity_hashes`.
//!
//! This test FAILS against the pre-change tree (the `_for(canonical)`
//! method does not exist) and PASSES against the post-GREEN state. The
//! negative assertion verifies the returned identity is NOT the all-zero
//! placeholder that R21 forbids in production.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_session::file_artifact_store::ProjectIdentity;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectMembership,
    ProjectRank, VfsProjectConfig, WorkspaceAccess,
};

fn build_host_with_single_project() -> (Arc<VerterHost>, &'static str) {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let canonical = "/projZ/src/lib.ts";
    let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: "/projZ".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some("/projZ/tsconfig.json".to_string()),
        root_files: vec![canonical.to_string()],
        extensions: vec![".ts".to_string()],
        workspace_root: "/projZ".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }]);
    workspace.set_project_graph(graph);
    workspace.inject_file(canonical.to_string(), Arc::from("export const Z = 0;\n"));
    let workspace: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    (host, canonical)
}

#[test]
fn host_view_project_identity_for_canonical_returns_real_hash() {
    let (host, canonical) = build_host_with_single_project();
    let identity = host.host_view_project_identity_for(canonical);
    assert_ne!(
        identity,
        ProjectIdentity([0u8; 16]),
        "post-Block-1.6 GREEN: host_view_project_identity_for must NOT collapse to \
         ProjectIdentity([0u8; 16]) for a canonical owned by a published project — R21 forbids \
         all-zero project identities in production paths",
    );
}

#[test]
fn host_view_project_identity_for_canonical_is_stable_across_calls() {
    let (host, canonical) = build_host_with_single_project();
    let identity_a = host.host_view_project_identity_for(canonical);
    let identity_b = host.host_view_project_identity_for(canonical);
    assert_eq!(
        identity_a, identity_b,
        "project identity is a pure function of the published snapshot — repeated calls under \
         a stable workspace MUST yield the same hash",
    );
}
