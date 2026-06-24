//! Env-hash tables swap atomically with the rest of the
//! published workspace snapshot on workspace-config bumps.
//!
//! Storage is the published snapshot, NOT an `OnceCell`. Env-hash
//! arrays live INSIDE the
//! published `PublishedRoot`. On snapshot republish (workspace-config
//! bump, project graph rebuild), the entire snapshot atomically swaps
//! via `ArcSwapOption<PublishedRoot>`; the env-hash table inside it
//! swaps too. No separate cache; no `OnceCell` staleness.
//!
//! Discrimination chain: this test exercises the atomic-swap contract by
//! reading the published `PublishedRoot` before and after a
//! workspace-config bump and asserting:
//!
//! 1. The pre-bump root carries an internally-coherent state — every
//!    project that has an env-hash array also has an identity hash, and
//!    vice versa (no torn intermediate observable).
//! 2. The post-bump root carries an internally-coherent state.
//! 3. The two roots differ in their env-hash tables (because the bump
//!    changed the project graph's shape) — confirming the swap delivered
//!    new values rather than mutating the old ones in-place.
//!
//! An implementation that mutated the tables in-place (instead of
//! swapping the whole `PublishedRoot`) would either expose torn states
//! to concurrent readers OR fail to show the pre/post difference.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectMembership,
    ProjectRank, VfsProjectConfig,
};

fn build_workspace_at_generation_n() -> Arc<MemoryWorkspace> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: "/projN".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some("/projN/tsconfig.json".to_string()),
        root_files: vec![],
        extensions: vec![".ts".to_string()],
        workspace_root: "/projN".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions {
            base_url: Some("/projN".to_string()),
            paths: vec![("@n/*".to_string(), vec!["./src/*".to_string()])],
            ..Default::default()
        },
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }]);
    workspace.set_project_graph(graph);
    workspace
}

fn bump_workspace_config(workspace: &MemoryWorkspace) {
    // Republish with an additional project — this drives
    // `rebuild_and_publish()` to produce a new `PublishedRoot` with a
    // different `env_hashes_by_project` table.
    let graph = ProjectGraph::from_configs(vec![
        VfsProjectConfig {
            root: "/projN".to_string(),
            rank: ProjectRank::Explicit,
            tsconfig_path: Some("/projN/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![".ts".to_string()],
            workspace_root: "/projN".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions {
                base_url: Some("/projN".to_string()),
                // Different paths — invalidates resolve_env_hash for projN.
                paths: vec![("@new/*".to_string(), vec!["./newsrc/*".to_string()])],
                ..Default::default()
            },
            references: vec![],
            membership: ProjectMembership::MatchAll,
        },
        VfsProjectConfig {
            root: "/projM".to_string(),
            rank: ProjectRank::Explicit,
            tsconfig_path: Some("/projM/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![".ts".to_string()],
            workspace_root: "/projM".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: ProjectMembership::MatchAll,
        },
    ]);
    workspace.set_project_graph(graph);
}

#[test]
fn pre_bump_published_root_is_internally_coherent() {
    let workspace = build_workspace_at_generation_n();
    let root = workspace.load_published().expect("snapshot published");
    // Internal coherence: every project listed in the snapshot must have
    // BOTH an env-hash entry AND an identity-hash entry; sizes must match.
    assert_eq!(
        root.env_hashes_by_project.len(),
        root.project_identity_hashes.len(),
        "env-hash table and identity-hash table MUST share a generation — sizes diverge only if \
         the swap exposed a torn intermediate state",
    );
    for project in &root.snapshot.projects {
        assert!(
            root.env_hashes_by_project.contains_key(&project.id),
            "pre-bump root: every published project MUST have an env-hash entry; project id \
             {:?} missing",
            project.id,
        );
        assert!(
            root.project_identity_hashes.contains_key(&project.id),
            "pre-bump root: every published project MUST have an identity-hash entry; project \
             id {:?} missing",
            project.id,
        );
    }
}

#[test]
fn post_bump_published_root_is_internally_coherent() {
    let workspace = build_workspace_at_generation_n();
    bump_workspace_config(&workspace);
    let root = workspace
        .load_published()
        .expect("post-bump snapshot published");
    assert_eq!(
        root.env_hashes_by_project.len(),
        root.project_identity_hashes.len(),
        "post-bump root: env-hash table and identity-hash table MUST share a generation",
    );
    for project in &root.snapshot.projects {
        assert!(
            root.env_hashes_by_project.contains_key(&project.id),
            "post-bump root: every published project MUST have an env-hash entry",
        );
        assert!(
            root.project_identity_hashes.contains_key(&project.id),
            "post-bump root: every published project MUST have an identity-hash entry",
        );
    }
}

#[test]
fn pre_and_post_bump_roots_differ_in_env_hash_tables() {
    let workspace = build_workspace_at_generation_n();
    let pre_root = workspace.load_published().expect("pre-bump snapshot");
    // Snapshot the pre-bump state by COPYING the env-hash table; the Arc
    // we hold in `pre_root` is a separate handle, so the swap cannot
    // mutate it underneath us — that property is what we exercise.
    let pre_count = pre_root.env_hashes_by_project.len();
    let pre_arrays: Vec<_> = pre_root.env_hashes_by_project.values().copied().collect();

    bump_workspace_config(&workspace);
    let post_root = workspace.load_published().expect("post-bump snapshot");
    let post_count = post_root.env_hashes_by_project.len();

    assert_ne!(
        pre_count, post_count,
        "post-bump table MUST differ in size from pre-bump table — the bump added a project",
    );
    // The pre-bump root still reflects the pre-bump table: confirm the
    // copied values still match the pre-bump root's table (no in-place
    // mutation poisoned the pre-bump observation).
    let pre_arrays_now: Vec<_> = pre_root.env_hashes_by_project.values().copied().collect();
    assert_eq!(
        pre_arrays, pre_arrays_now,
        "ArcSwap atomic swap: the pre-bump root's env-hash table MUST remain stable for the \
         lifetime of its Arc handle. An implementation that mutated the table in-place would \
         show different values here.",
    );
}
