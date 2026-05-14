//! Block 1.6 — multi-project workspace yields distinct env-hash arrays
//! for the same canonical claimed by overlapping projects.
//!
//! Plan citation: `D:/tmp/verter-stage7-final-cutover-plan.md` § "Correction 3:
//! project-scoped env-hash API". v7's `env_hash_array_for_canonical(canonical)`
//! could not disambiguate workspaces where
//! `WorkspaceSnapshot::owners_for_file(canonical) -> SmallVec<[ProjectId; 2]>`
//! returns multiple projects. v8 keys the env-hash API by `ProjectId`,
//! NOT canonical, so callers with explicit project context get distinct
//! cache identities for the same canonical.
//!
//! Discrimination chain — this test exercises the canonical-keyed-API
//! failure mode that v7's design could not catch:
//!
//! 1. Construct a `MemoryWorkspace` with TWO projects whose roots both
//!    cover the same canonical (`/shared/util.ts`). The published
//!    `WorkspaceSnapshot::owners_for_file("/shared/util.ts")` returns
//!    a non-empty list (each project's `is_under_root` membership claim
//!    matches).
//! 2. Each project has DIFFERENT `compiler_options` (different `paths`)
//!    so their derived `[parse, resolve, type_, lib]` env-hash arrays
//!    differ at the producer site.
//! 3. Query env-hash arrays through the project-keyed API:
//!    `workspace.env_hash_array_for_project(p1)` vs
//!    `workspace.env_hash_array_for_project(p2)`. Assert they DIFFER.
//!
//! A canonical-only API (`env_hash_array_for_canonical(canonical)`)
//! would return the SAME bundle for both queries — that bundle could
//! only encode one project's settings, so cache keys derived from it
//! would collide across the two projects. This test discriminates
//! against that bug by asserting the project-keyed API does NOT
//! collapse to a single bundle.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectId,
    ProjectMembership, ProjectRank, VfsProjectConfig, WorkspaceAccess,
};

fn build_multi_project_workspace() -> Arc<MemoryWorkspace> {
    // Two projects whose roots both cover `/shared` so they overlap on
    // `/shared/util.ts`. Different `paths` maps drive different
    // `resolve_env_hash`; different tsconfig paths drive different
    // `project_identity`.
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let graph = ProjectGraph::from_configs(vec![
        VfsProjectConfig {
            root: "/shared".to_string(),
            rank: ProjectRank::Explicit,
            tsconfig_path: Some("/shared/tsconfig.a.json".to_string()),
            root_files: vec!["/shared/util.ts".to_string()],
            extensions: vec![".ts".to_string()],
            workspace_root: "/shared".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions {
                base_url: Some("/shared".to_string()),
                paths: vec![("@a/*".to_string(), vec!["./src-a/*".to_string()])],
            },
            references: vec![],
            membership: ProjectMembership::MatchAll,
        },
        VfsProjectConfig {
            root: "/shared".to_string(),
            rank: ProjectRank::Explicit,
            tsconfig_path: Some("/shared/tsconfig.b.json".to_string()),
            root_files: vec!["/shared/util.ts".to_string()],
            extensions: vec![".ts".to_string()],
            workspace_root: "/shared".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions {
                base_url: Some("/shared".to_string()),
                paths: vec![("@b/*".to_string(), vec!["./src-b/*".to_string()])],
            },
            references: vec![],
            membership: ProjectMembership::MatchAll,
        },
    ]);
    workspace.set_project_graph(graph);
    workspace
}

#[test]
#[ignore = "block-1.6 RED — closed by same-block implementation"]
fn two_projects_share_canonical_via_owners_for_file() {
    let workspace = build_multi_project_workspace();
    let root = workspace
        .load_published()
        .expect("workspace publishes a snapshot in rebuild_and_publish");
    let owners = root.snapshot.owners_for_file("/shared/util.ts");
    assert!(
        owners.len() >= 2,
        "multi-project workspace MUST report at least 2 owners for an overlapping canonical; \
         got {} owners. WorkspaceSnapshot::owners_for_file returns precedence-ordered list.",
        owners.len(),
    );
}

#[test]
#[ignore = "block-1.6 RED — closed by same-block implementation"]
fn project_keyed_env_hash_api_yields_distinct_bundles_per_project() {
    let workspace = build_multi_project_workspace();
    let root = workspace.load_published().expect("snapshot published");
    let owners = root.snapshot.owners_for_file("/shared/util.ts");
    assert!(
        owners.len() >= 2,
        "fixture invariant: two projects must claim the canonical",
    );

    // Pull the first two distinct projects' env-hash arrays through the
    // PROJECT-KEYED API. This is the v8 contract that v7's canonical-only
    // API could not satisfy.
    let project_a: ProjectId = owners[0];
    let project_b: ProjectId = owners[1];
    assert_ne!(
        project_a, project_b,
        "owners list MUST hold distinct ProjectIds",
    );

    let env_a = workspace
        .env_hash_array_for_project(project_a)
        .expect("published project A has env-hash entry");
    let env_b = workspace
        .env_hash_array_for_project(project_b)
        .expect("published project B has env-hash entry");

    // The two projects differ in tsconfig path and `paths` map, so at
    // minimum `resolve_env_hash` and `project_identity` MUST differ.
    // A canonical-keyed API would return the same bundle here — this
    // assertion discriminates against that failure mode.
    assert_ne!(
        env_a, env_b,
        "project-keyed env-hash API MUST yield distinct arrays for two projects with different \
         compiler_options + tsconfig paths. A canonical-keyed API would return identical bundles \
         here, collapsing the two projects' cache identities.",
    );

    let identity_a = workspace
        .project_identity_hash_for_project(project_a)
        .expect("published project A has identity hash");
    let identity_b = workspace
        .project_identity_hash_for_project(project_b)
        .expect("published project B has identity hash");
    assert_ne!(
        identity_a, identity_b,
        "project identity MUST differ for two projects with different tsconfig paths — \
         project_identity hashes (workspace_root, root, provider_root, tsconfig_path, \
         membership), and the two projects have different tsconfig_paths",
    );
}
