use super::*;
use crate::changes::WorkspaceChange;
use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
use crate::resolver::{IdeProjectCompilerOptions, ProjectMembership};
use crate::traits::WorkspaceAccess;
use crate::types::{ExactResolution, FileKind, ParsedEdge, ProjectOwnership, ResolveRequestKind};

// ── MemorySnapshot tests ──

#[test]
fn read_returns_none_for_unknown() {
    let snapshot = MemorySnapshot::new();
    assert!(snapshot.read("src/foo.vue").is_none());
    assert!(!snapshot.contains("src/foo.vue"));
}

#[test]
fn inject_and_read() {
    let mut snapshot = MemorySnapshot::new();
    let source: Arc<str> = Arc::from("<template>hello</template>");
    let changed = snapshot.inject("src/foo.vue".to_string(), source);

    assert!(changed, "first inject should report changed");
    assert_eq!(
        snapshot.read("src/foo.vue").as_deref(),
        Some("<template>hello</template>")
    );
    assert!(snapshot.contains("src/foo.vue"));
}

#[test]
fn inject_same_content_reports_no_change() {
    let mut snapshot = MemorySnapshot::new();
    snapshot.inject("src/foo.vue".to_string(), Arc::from("content"));

    let changed = snapshot.inject("src/foo.vue".to_string(), Arc::from("content"));
    assert!(!changed, "same content should report no change");
}

#[test]
fn inject_different_content_reports_change() {
    let mut snapshot = MemorySnapshot::new();
    snapshot.inject("src/foo.vue".to_string(), Arc::from("old"));

    let changed = snapshot.inject("src/foo.vue".to_string(), Arc::from("new"));
    assert!(changed, "different content should report changed");
    assert_eq!(snapshot.read("src/foo.vue").as_deref(), Some("new"));
}

#[test]
fn remove_existing_file() {
    let mut snapshot = MemorySnapshot::new();
    snapshot.inject("src/foo.vue".to_string(), Arc::from("content"));

    assert!(
        snapshot.remove("src/foo.vue"),
        "should return true for existing file"
    );
    assert!(
        snapshot.read("src/foo.vue").is_none(),
        "removed file should return None"
    );
    assert!(!snapshot.contains("src/foo.vue"));
}

#[test]
fn remove_unknown_file() {
    let mut snapshot = MemorySnapshot::new();
    assert!(
        !snapshot.remove("src/foo.vue"),
        "should return false for unknown file"
    );
}

#[test]
fn len_and_empty() {
    let mut snapshot = MemorySnapshot::new();
    assert!(snapshot.is_empty());
    assert_eq!(snapshot.len(), 0);

    snapshot.inject("src/a.vue".to_string(), Arc::from("a"));
    assert!(!snapshot.is_empty());
    assert_eq!(snapshot.len(), 1);

    snapshot.inject("src/b.vue".to_string(), Arc::from("b"));
    assert_eq!(snapshot.len(), 2);

    snapshot.remove("src/a.vue");
    assert_eq!(snapshot.len(), 1);
}

#[test]
fn ids_iteration() {
    let mut snapshot = MemorySnapshot::new();
    snapshot.inject("src/a.vue".to_string(), Arc::from("a"));
    snapshot.inject("src/b.vue".to_string(), Arc::from("b"));

    let mut ids: Vec<&str> = snapshot.ids().collect();
    ids.sort();
    assert_eq!(ids, vec!["src/a.vue", "src/b.vue"]);
}

// ── MemoryWorkspace creation tests ──

#[test]
fn memory_workspace_creation() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    // Should start empty
    assert!(ws.read_file("d:/project/src/foo.vue").is_none());
    assert!(!ws.file_exists("d:/project/src/foo.vue"));
}

// ── MemoryWorkspace::read_file ──

#[test]
fn read_file_returns_injected_content() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/foo.vue".to_string(),
        Arc::from("<template>hi</template>"),
    );

    let content = ws.read_file("d:/project/src/foo.vue");
    assert_eq!(content.as_deref(), Some("<template>hi</template>"));
}

#[test]
fn read_file_overlay_takes_precedence_over_snapshot() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/foo.vue".to_string(),
        Arc::from("snapshot content"),
    );

    // Set overlay
    ws.apply_changes(vec![WorkspaceChange::OverlaySet {
        canonical_id: "d:/project/src/foo.vue".to_string(),
        source: Arc::from("overlay content"),
    }]);

    let content = ws.read_file("d:/project/src/foo.vue");
    assert_eq!(content.as_deref(), Some("overlay content"));
    // Must NOT return snapshot content
    assert_ne!(content.as_deref(), Some("snapshot content"));
}

#[test]
fn read_file_reverts_to_snapshot_after_overlay_clear() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/foo.vue".to_string(),
        Arc::from("snapshot content"),
    );

    ws.apply_changes(vec![WorkspaceChange::OverlaySet {
        canonical_id: "d:/project/src/foo.vue".to_string(),
        source: Arc::from("overlay content"),
    }]);

    // Clear overlay
    ws.apply_changes(vec![WorkspaceChange::OverlayClear {
        canonical_id: "d:/project/src/foo.vue".to_string(),
    }]);

    let content = ws.read_file("d:/project/src/foo.vue");
    assert_eq!(content.as_deref(), Some("snapshot content"));
    assert_ne!(content.as_deref(), Some("overlay content"));
}

#[test]
fn read_file_returns_none_for_nonexistent() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    assert!(ws.read_file("d:/project/src/nonexistent.vue").is_none());
}

// ── MemoryWorkspace::file_exists ──

#[test]
fn file_exists_in_snapshot() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("d:/project/src/foo.vue".to_string(), Arc::from("content"));
    assert!(ws.file_exists("d:/project/src/foo.vue"));
    assert!(!ws.file_exists("d:/project/src/bar.vue"));
}

#[test]
fn file_exists_in_overlay() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.apply_changes(vec![WorkspaceChange::OverlaySet {
        canonical_id: "d:/project/src/new.vue".to_string(),
        source: Arc::from("overlay only"),
    }]);
    assert!(ws.file_exists("d:/project/src/new.vue"));
}

// ── MemoryWorkspace::classify_file ──

#[test]
fn classify_file_vue() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let kind = ws.classify_file("d:/project/src/foo.vue");
    assert_eq!(kind, FileKind::VueSfc);
    assert_ne!(
        kind,
        FileKind::NonSfc,
        ".vue file must not be classified as NonSfc"
    );
}

#[test]
fn classify_file_non_vue() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let kind_ts = ws.classify_file("d:/project/src/utils.ts");
    assert_eq!(kind_ts, FileKind::NonSfc);
    assert_ne!(
        kind_ts,
        FileKind::VueSfc,
        ".ts file must not be classified as VueSfc"
    );

    assert_eq!(
        ws.classify_file("d:/project/src/utils.tsx"),
        FileKind::NonSfc
    );
    assert_eq!(
        ws.classify_file("d:/project/src/utils.js"),
        FileKind::NonSfc
    );
    // Negative: .vue should never return NonSfc
    assert_ne!(
        ws.classify_file("d:/project/src/comp.vue"),
        FileKind::NonSfc
    );
}

// ── MemoryWorkspace::realpath ──

#[test]
fn realpath_returns_id_if_exists() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("d:/project/src/foo.vue".to_string(), Arc::from("content"));

    assert_eq!(
        ws.realpath("d:/project/src/foo.vue"),
        Some("d:/project/src/foo.vue".to_string())
    );
}

#[test]
fn realpath_returns_none_if_not_exists() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    assert!(ws.realpath("d:/project/src/nonexistent.vue").is_none());
}

// ── MemoryWorkspace::resolve_import with exact resolutions ──

#[test]
fn resolve_import_exact_resolution_found() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/utils.ts".to_string(),
        Arc::from("export const x = 1;"),
    );

    ws.set_exact_resolutions(
        "d:/project/src/app.vue",
        vec![ExactResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: Some("d:/project/src/utils.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let result = ws.resolve_import(
        "d:/project/src/app.vue",
        "./utils",
        ResolveRequestKind::EsmImport,
    );

    assert!(result.is_some(), "exact resolution should return Some");
    let result = result.unwrap();
    assert_eq!(result.source_id, "d:/project/src/utils.ts");
}

#[test]
fn resolve_import_exact_resolution_authoritative_none() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    // Exact resolution says "not found" — authoritative, should NOT fall through
    ws.set_exact_resolutions(
        "d:/project/src/app.vue",
        vec![ExactResolution {
            specifier: "./missing".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec![],
        }],
    );

    let result = ws.resolve_import(
        "d:/project/src/app.vue",
        "./missing",
        ResolveRequestKind::EsmImport,
    );

    assert!(
        result.is_none(),
        "authoritative exact resolution with None should return None"
    );
}

// ── MemoryWorkspace::resolve_import with project resolver ──

#[test]
fn resolve_import_via_project_resolver() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    // Inject files
    ws.inject_file(
        "d:/project/src/app.vue".to_string(),
        Arc::from("<template></template>"),
    );
    ws.inject_file(
        "d:/project/src/utils.ts".to_string(),
        Arc::from("export const x = 1;"),
    );

    // Set up project graph with a project that covers d:/project
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

    let result = ws.resolve_import(
        "d:/project/src/app.vue",
        "./utils",
        ResolveRequestKind::EsmImport,
    );

    assert!(result.is_some(), "resolver should find ./utils.ts");
    let result = result.unwrap();
    assert_eq!(result.source_id, "d:/project/src/utils.ts");
}

#[test]
fn resolve_import_via_tsconfig_paths() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    ws.inject_file(
        "d:/project/src/components/Button.vue".to_string(),
        Arc::from("<template></template>"),
    );
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
        compiler_options: IdeProjectCompilerOptions {
            base_url: Some("d:/project".to_string()),
            paths: vec![("@/*".to_string(), vec!["./src/*".to_string()])],
        },
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }]);
    ws.set_project_graph(graph);

    let result = ws.resolve_import(
        "d:/project/src/app.vue",
        "@/components/Button.vue",
        ResolveRequestKind::EsmImport,
    );

    assert!(
        result.is_some(),
        "should resolve @/components/Button.vue via tsconfig paths"
    );
    let result = result.unwrap();
    assert_eq!(result.source_id, "d:/project/src/components/Button.vue");
}

#[test]
fn resolve_import_returns_none_for_unknown() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    let result = ws.resolve_import(
        "d:/project/src/app.vue",
        "./nonexistent",
        ResolveRequestKind::EsmImport,
    );

    assert!(result.is_none(), "should return None for unknown specifier");
}

// ── MemoryWorkspace::record_parsed_edges ──

#[test]
fn record_parsed_edges_relative_updates_forward_reverse() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    // Inject files so resolution works
    ws.inject_file(
        "d:/project/src/app.vue".to_string(),
        Arc::from("<template></template>"),
    );
    ws.inject_file(
        "d:/project/src/utils.ts".to_string(),
        Arc::from("export const x = 1;"),
    );

    // Set up project
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

    // Record edges
    ws.record_parsed_edges(
        "d:/project/src/app.vue",
        &[ParsedEdge::Relative {
            specifier: "./utils".to_string(),
            kind: ResolveRequestKind::EsmImport,
        }],
    );

    // Forward deps of app.vue should include utils.ts
    let forward = ws.forward_deps_for("d:/project/src/app.vue");
    assert!(
        forward.contains(&"d:/project/src/utils.ts".to_string()),
        "forward deps should include utils.ts, got: {:?}",
        forward
    );

    // Reverse deps of utils.ts should include app.vue
    let reverse = ws.reverse_deps_for("d:/project/src/utils.ts");
    assert!(
        reverse.contains(&"d:/project/src/app.vue".to_string()),
        "reverse deps should include app.vue, got: {:?}",
        reverse
    );
}

#[test]
fn record_parsed_edges_bare_stored_not_resolved() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    ws.record_parsed_edges(
        "d:/project/src/app.vue",
        &[ParsedEdge::Bare {
            specifier: "vue".to_string(),
            kind: ResolveRequestKind::EsmImport,
        }],
    );

    // Forward deps should be empty (bare specifiers are not eagerly resolved)
    let forward = ws.forward_deps_for("d:/project/src/app.vue");
    assert!(
        forward.is_empty(),
        "bare specifiers should not appear in forward deps, got: {:?}",
        forward
    );
}

#[test]
fn record_parsed_edges_external_src_resolved() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    ws.record_parsed_edges(
        "d:/project/src/app.vue",
        &[ParsedEdge::ExternalSrc {
            specifier: "./script.ts".to_string(),
            resolved_path: Some("d:/project/src/script.ts".to_string()),
        }],
    );

    let forward = ws.forward_deps_for("d:/project/src/app.vue");
    assert!(
        forward.contains(&"d:/project/src/script.ts".to_string()),
        "ExternalSrc with resolved_path should appear in forward deps"
    );
}

// ── MemoryWorkspace::owner_for_file ──

#[test]
fn owner_for_file_with_project_graph() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: "d:/project".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some("d:/project/tsconfig.json".to_string()),
        root_files: vec![],
        extensions: vec![],
        workspace_root: "d:/project".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }]);
    ws.set_project_graph(graph);

    let owner = ws.owner_for_file("d:/project/src/foo.vue");
    assert_eq!(
        owner,
        Some(ProjectOwnership {
            project_root: "d:/project".to_string(),
            tsconfig_path: Some("d:/project/tsconfig.json".to_string()),
        })
    );

    // File outside the project should have no owner
    let no_owner = ws.owner_for_file("d:/other/src/bar.vue");
    assert!(
        no_owner.is_none(),
        "file outside project should have no owner"
    );
}

// ── MemoryWorkspace::apply_changes ──

#[test]
fn apply_changes_file_changed_with_source() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    let result = ws.apply_changes(vec![WorkspaceChange::FileChanged {
        canonical_id: "d:/project/src/foo.vue".to_string(),
        source: Some(Arc::from("new content")),
    }]);

    assert!(
        result
            .invalidated_files
            .contains(&"d:/project/src/foo.vue".to_string()),
        "FileChanged should invalidate the file"
    );

    // Content should be readable from the snapshot
    let content = ws.read_file("d:/project/src/foo.vue");
    assert_eq!(content.as_deref(), Some("new content"));
}

#[test]
fn apply_changes_file_changed_skipped_when_overlay_active() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    // Set overlay first
    ws.apply_changes(vec![WorkspaceChange::OverlaySet {
        canonical_id: "d:/project/src/foo.vue".to_string(),
        source: Arc::from("overlay content"),
    }]);

    // FileChanged while overlay is active — should be skipped
    let result = ws.apply_changes(vec![WorkspaceChange::FileChanged {
        canonical_id: "d:/project/src/foo.vue".to_string(),
        source: Some(Arc::from("disk content")),
    }]);

    assert!(
        !result
            .invalidated_files
            .contains(&"d:/project/src/foo.vue".to_string()),
        "FileChanged should be skipped when overlay is active"
    );

    // Overlay content should still be returned
    let content = ws.read_file("d:/project/src/foo.vue");
    assert_eq!(content.as_deref(), Some("overlay content"));
}

#[test]
fn apply_changes_file_deleted() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("d:/project/src/foo.vue".to_string(), Arc::from("content"));

    let result = ws.apply_changes(vec![WorkspaceChange::FileDeleted {
        canonical_id: "d:/project/src/foo.vue".to_string(),
    }]);

    assert!(
        result
            .invalidated_files
            .contains(&"d:/project/src/foo.vue".to_string()),
        "FileDeleted should invalidate the file"
    );

    assert!(
        ws.read_file("d:/project/src/foo.vue").is_none(),
        "deleted file should not be readable"
    );
    assert!(!ws.file_exists("d:/project/src/foo.vue"));
}

#[test]
fn apply_changes_config_changed() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    let result = ws.apply_changes(vec![WorkspaceChange::ConfigChanged {
        canonical_id: "d:/project/tsconfig.json".to_string(),
    }]);

    assert!(result.graph_rebuilt);
    assert!(result.generation.is_some());
}

// ── MemoryWorkspace::set_exact_resolutions ──

#[test]
fn set_exact_resolutions_stores_and_retrieves() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    let exact_result = ws.set_exact_resolutions(
        "d:/project/src/app.vue",
        vec![ExactResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: Some("d:/project/src/utils.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    assert!(
        exact_result
            .newly_resolved
            .contains(&"d:/project/src/utils.ts".to_string()),
        "newly_resolved should contain utils.ts"
    );

    // Should be retrievable via resolve_import
    let result = ws.resolve_import(
        "d:/project/src/app.vue",
        "./utils",
        ResolveRequestKind::EsmImport,
    );
    assert!(result.is_some());
    assert_eq!(result.unwrap().source_id, "d:/project/src/utils.ts");
}

// ── MemoryWorkspace::set_project_graph ──

#[test]
fn set_project_graph_updates_resolver() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/utils.ts".to_string(),
        Arc::from("export const x = 1;"),
    );

    // No project graph yet — resolve should fail (no resolver)
    let result_before = ws.resolve_import(
        "d:/project/src/app.vue",
        "./utils",
        ResolveRequestKind::EsmImport,
    );
    assert!(
        result_before.is_none(),
        "should fail to resolve without project graph"
    );

    // Now set the project graph
    let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: "d:/project".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: None,
        root_files: vec![],
        extensions: vec![],
        workspace_root: "d:/project".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }]);
    ws.set_project_graph(graph);

    // Now it should resolve
    let result_after = ws.resolve_import(
        "d:/project/src/app.vue",
        "./utils",
        ResolveRequestKind::EsmImport,
    );
    assert!(
        result_after.is_some(),
        "should resolve after project graph is set"
    );
}

// ── MemoryWorkspace::add_explicit_project ──

#[test]
fn add_explicit_project() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/utils.ts".to_string(),
        Arc::from("export const x = 1;"),
    );

    ws.add_explicit_project(VfsProjectConfig {
        root: "d:/project".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: None,
        root_files: vec![],
        extensions: vec![],
        workspace_root: "d:/project".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    });

    let owner = ws.owner_for_file("d:/project/src/utils.ts");
    assert!(
        owner.is_some(),
        "should own file after adding explicit project"
    );
}
