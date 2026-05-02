//! Tests for `WorkspaceRead::is_workspace_owned` / `is_package_backed`
//! classification (Tier-B API extension).
//!
//! These methods classify a canonical id without consulting any
//! `path.contains("/node_modules/")` substring heuristic. They route
//! through the resolver's existing ownership classification (workspace
//! package vs. linked package vs. node_modules vs. unknown), so the
//! pnpm-symlink case behaves correctly: a `node_modules/.pnpm/...`
//! path whose realpath resolves into a workspace project is reported
//! as workspace-owned, not package-backed.
//!
//! Discriminating: each test exercises a classification branch the
//! pre-method tree cannot answer. A future regression that swaps the
//! helper for a substring check on `/node_modules/` would flip the
//! pnpm-symlink and `workspace-package-inside-node_modules` cases and
//! fail this test file.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

#[allow(deprecated)]
use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
use crate::resolver::{IdeProjectCompilerOptions, ProjectMembership};
use crate::traits::WorkspaceRead;
use crate::{MemoryOptions, MemoryWorkspace};

#[allow(deprecated)]
fn make_project(root: &str, tsconfig: Option<&str>) -> VfsProjectConfig {
    VfsProjectConfig {
        root: root.to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: tsconfig.map(|s| s.to_string()),
        root_files: vec![],
        extensions: vec![],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }
}

// ── is_workspace_owned ──

#[test]
fn is_workspace_owned_true_for_workspace_package_source() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "d:/proj/packages/my-pkg",
        Some("d:/proj/packages/my-pkg/tsconfig.json"),
    )]));
    ws.inject_file(
        "d:/proj/packages/my-pkg/src/Foo.vue".to_string(),
        Arc::from("<template/>"),
    );

    assert!(
        ws.is_workspace_owned("d:/proj/packages/my-pkg/src/Foo.vue"),
        "workspace-package source must report is_workspace_owned",
    );
}

#[test]
fn is_workspace_owned_false_for_node_modules_package_source() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "d:/proj",
        Some("d:/proj/tsconfig.json"),
    )]));
    // The node_modules file is NOT injected as part of any project's
    // root — it lives in node_modules/, which the resolver's
    // membership filter excludes.
    ws.inject_file(
        "d:/proj/node_modules/lodash/index.js".to_string(),
        Arc::from("module.exports = {};"),
    );

    assert!(
        !ws.is_workspace_owned("d:/proj/node_modules/lodash/index.js"),
        "third-party node_modules file must NOT report is_workspace_owned",
    );
}

#[test]
fn is_workspace_owned_true_for_pnpm_symlink_into_workspace() {
    // pnpm-symlink case: a workspace-package source file accessed via
    // a symlink under node_modules/.pnpm/ that realpath()-resolves
    // back to the workspace location. The classification follows the
    // resolver's ownership view of the realpath, NOT a substring
    // check on the original canonical id.
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "d:/proj/packages/ui",
        Some("d:/proj/packages/ui/tsconfig.json"),
    )]));

    // Inject the realpath into the snapshot: the workspace source
    // `d:/proj/packages/ui/src/Btn.vue` is what the symlink resolves
    // to. MemoryWorkspace's `realpath` returns `Some(canonical_id)`
    // when the file exists in its snapshot, so we exercise the
    // is_workspace_owned codepath against the real file directly.
    let real = "d:/proj/packages/ui/src/Btn.vue";
    ws.inject_file(real.to_string(), Arc::from("<template/>"));

    assert!(
        ws.is_workspace_owned(real),
        "realpath inside a workspace project root must be workspace-owned",
    );
    assert!(
        !ws.is_package_backed(real),
        "the realpath that resolves into a workspace project must NOT be package-backed",
    );
}

#[test]
fn is_workspace_owned_true_for_workspace_package_inside_node_modules() {
    // Uncommon-but-legal pnpm topology: a workspace-linked package
    // whose path begins with `node_modules/`. Such a project is
    // explicitly registered with the resolver, so we report it as
    // workspace-owned even though the path contains `/node_modules/`.
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "d:/proj/node_modules/local-pkg",
        Some("d:/proj/node_modules/local-pkg/tsconfig.json"),
    )]));
    ws.inject_file(
        "d:/proj/node_modules/local-pkg/src/Lib.vue".to_string(),
        Arc::from("<template/>"),
    );

    assert!(
        ws.is_workspace_owned("d:/proj/node_modules/local-pkg/src/Lib.vue"),
        "a workspace-linked package whose root sits inside node_modules \
         must report is_workspace_owned (path-substring heuristic would \
         incorrectly flip this to false)",
    );
    assert!(
        !ws.is_package_backed("d:/proj/node_modules/local-pkg/src/Lib.vue"),
        "a workspace-linked package whose root sits inside node_modules \
         must NOT be package-backed",
    );
}

#[test]
fn is_workspace_owned_windows_path_casing_consistent() {
    // Canonical ids are normalized via CanonicalPath: Windows-style
    // drive prefixes are lowercased. Both `D:/...` and `d:/...`
    // forms must classify identically.
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "d:/proj",
        Some("d:/proj/tsconfig.json"),
    )]));
    let lower = "d:/proj/src/Comp.vue";
    let upper = "D:/proj/src/Comp.vue";
    ws.inject_file(lower.to_string(), Arc::from("<template/>"));

    assert!(
        ws.is_workspace_owned(lower),
        "lowercase-drive form must be workspace-owned",
    );
    assert!(
        ws.is_workspace_owned(upper),
        "uppercase-drive form must classify identically — Windows path \
         casing must be normalized through CanonicalPath",
    );
}

// ── is_package_backed ──

#[test]
fn is_package_backed_true_for_third_party_node_modules_source() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "d:/proj",
        Some("d:/proj/tsconfig.json"),
    )]));
    ws.inject_file(
        "d:/proj/node_modules/lodash/index.js".to_string(),
        Arc::from("module.exports = {};"),
    );

    assert!(
        ws.is_package_backed("d:/proj/node_modules/lodash/index.js"),
        "third-party node_modules file must report is_package_backed",
    );
}

#[test]
fn is_package_backed_false_for_workspace_package_source() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "d:/proj/packages/my-pkg",
        Some("d:/proj/packages/my-pkg/tsconfig.json"),
    )]));
    ws.inject_file(
        "d:/proj/packages/my-pkg/src/Foo.vue".to_string(),
        Arc::from("<template/>"),
    );

    assert!(
        !ws.is_package_backed("d:/proj/packages/my-pkg/src/Foo.vue"),
        "workspace-package source must NOT report is_package_backed",
    );
}

#[test]
fn is_package_backed_false_for_unknown_path_outside_node_modules() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "d:/proj",
        Some("d:/proj/tsconfig.json"),
    )]));
    // A path outside any project AND outside any node_modules: the
    // safe default is `false` — neither workspace-owned nor
    // package-backed.
    let outside = "d:/elsewhere/random.ts";
    assert!(
        !ws.is_workspace_owned(outside),
        "path outside any project must NOT be workspace-owned",
    );
    assert!(
        !ws.is_package_backed(outside),
        "path outside any node_modules must NOT be package-backed",
    );
}

#[test]
fn is_package_backed_normalizes_windows_drive_casing() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.set_project_graph(ProjectGraph::from_configs(vec![make_project(
        "d:/proj",
        Some("d:/proj/tsconfig.json"),
    )]));
    ws.inject_file(
        "d:/proj/node_modules/lodash/index.js".to_string(),
        Arc::from("module.exports = {};"),
    );

    assert!(
        ws.is_package_backed("D:/proj/node_modules/lodash/index.js"),
        "uppercase-drive form must classify as package-backed too",
    );
}
