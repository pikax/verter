//! `host_view_env_hashes_for(canonical)` returns real env-hash
//! bundles from the workspace's published-snapshot tables.
//!
//! Contract: `host_view_env_hashes_for(canonical)` consults
//! the workspace-published env-hash table keyed by the owning project's
//! `ProjectId`, returning a non-default, project-derived bundle.
//!
//! The negative assertions verify the bundle is NOT all-zero, NOT equal to
//! `EnvHashes::default()`, and is internally distinguishable across
//! dimensions (R21 salt invariant).

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_session::{HostConfig, VerterHost};
use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectMembership,
    ProjectRank, VfsProjectConfig, WorkspaceAccess,
};

fn build_host_with_single_project() -> (Arc<VerterHost>, &'static str) {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let canonical = "/projA/src/main.ts";
    let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: "/projA".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some("/projA/tsconfig.json".to_string()),
        root_files: vec![canonical.to_string()],
        extensions: vec![".ts".to_string(), ".vue".to_string()],
        workspace_root: "/projA".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions {
            base_url: Some("/projA".to_string()),
            paths: vec![("@/*".to_string(), vec!["./src/*".to_string()])],
            ..Default::default()
        },
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }]);
    workspace.set_project_graph(graph);
    workspace.inject_file(canonical.to_string(), Arc::from("export const X = 1;\n"));
    let workspace: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    (host, canonical)
}

#[test]
fn host_view_env_hashes_for_canonical_returns_non_default_bundle() {
    let (host, canonical) = build_host_with_single_project();
    let env = host.host_view_env_hashes_for(canonical);
    let default_env = verter_session::session_view::EnvHashes::default();
    assert_ne!(
        env, default_env,
        "host_view_env_hashes_for must NOT collapse to EnvHashes::default() \
         for a canonical owned by a published project",
    );
    assert_ne!(
        env.parse_env_hash, [0u8; 16],
        "parse_env_hash must mix workspace parser fingerprint (non-zero)",
    );
    assert_ne!(
        env.resolve_env_hash, [0u8; 16],
        "resolve_env_hash must mix workspace resolve-extensions + project paths (non-zero)",
    );
    assert_ne!(
        env.type_env_hash, [0u8; 16],
        "type_env_hash must mix project type-semantic config (non-zero)",
    );
    assert_ne!(
        env.lib_env_hash, [0u8; 16],
        "lib_env_hash must mix workspace ambient corpus fingerprint (non-zero)",
    );
}

#[test]
fn host_view_env_hashes_for_canonical_distinguishes_dimensions() {
    let (host, canonical) = build_host_with_single_project();
    let env = host.host_view_env_hashes_for(canonical);
    // R21 salt invariant: the four dimensions are derived with distinct
    // per-dimension salt bytes, so even for an "empty" project they MUST
    // disagree across dimensions.
    assert_ne!(
        env.parse_env_hash, env.resolve_env_hash,
        "parse vs resolve env-hash MUST differ (per-dimension salt)",
    );
    assert_ne!(
        env.parse_env_hash, env.type_env_hash,
        "parse vs type env-hash MUST differ (per-dimension salt)",
    );
    assert_ne!(
        env.parse_env_hash, env.lib_env_hash,
        "parse vs lib env-hash MUST differ (per-dimension salt)",
    );
    assert_ne!(
        env.resolve_env_hash, env.type_env_hash,
        "resolve vs type env-hash MUST differ (per-dimension salt)",
    );
    assert_ne!(
        env.resolve_env_hash, env.lib_env_hash,
        "resolve vs lib env-hash MUST differ (per-dimension salt)",
    );
    assert_ne!(
        env.type_env_hash, env.lib_env_hash,
        "type vs lib env-hash MUST differ (per-dimension salt)",
    );
}
