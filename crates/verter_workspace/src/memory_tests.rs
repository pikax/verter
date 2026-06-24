use super::*;
use crate::changes::WorkspaceChange;
use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
use crate::resolver::{IdeProjectCompilerOptions, ProjectMembership};
use crate::traits::{WorkspaceAccess, WorkspaceRead};
use crate::types::{
    ExactResolution, ParsedEdge, ProjectOwnership, ResolutionContext, ResolvePhase,
    ResolveRequestKind,
};

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
    let lang = ws.classify_file("d:/project/src/foo.vue");
    assert_eq!(lang, verter_language::FileLanguage::vue());
    assert!(
        lang.is_framework_carrier(),
        ".vue file must classify as a framework carrier"
    );
}

#[test]
fn classify_file_non_vue() {
    use verter_language::{FileLanguage, ScriptSourceType};
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let lang_ts = ws.classify_file("d:/project/src/utils.ts");
    assert_eq!(lang_ts, FileLanguage::script(ScriptSourceType::Ts));
    assert!(
        !lang_ts.is_framework_carrier(),
        ".ts file must not classify as a framework carrier"
    );

    assert_eq!(
        ws.classify_file("d:/project/src/utils.tsx"),
        FileLanguage::script(ScriptSourceType::Tsx)
    );
    assert_eq!(
        ws.classify_file("d:/project/src/utils.js"),
        FileLanguage::script(ScriptSourceType::js())
    );
    // Negative: .vue should never classify as a plain script.
    assert!(ws
        .classify_file("d:/project/src/comp.vue")
        .is_framework_carrier());
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
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("d:/project/src/utils.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let result = ws.resolve_import(
        "d:/project/src/app.vue",
        "./utils",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
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
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
            resolved_canonical_id: None,
            possible_canonical_ids: vec![],
        }],
    );

    let result = ws.resolve_import(
        "d:/project/src/app.vue",
        "./missing",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
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
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
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
            ..Default::default()
        },
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }]);
    ws.set_project_graph(graph);

    let result = ws.resolve_import(
        "d:/project/src/app.vue",
        "@/components/Button.vue",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
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
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );

    assert!(result.is_none(), "should return None for unknown specifier");
}

#[test]
fn resolve_import_populates_engine_manifest_cache() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("/repo/src/a.vue".to_string(), Arc::from("<template/>"));
    ws.inject_file("/repo/src/b.vue".to_string(), Arc::from("<template/>"));
    ws.inject_file(
        "/repo/node_modules/vue/package.json".to_string(),
        Arc::from(r#"{"module":"dist/vue.esm.js"}"#),
    );
    ws.inject_file(
        "/repo/node_modules/vue/dist/vue.esm.js".to_string(),
        Arc::from("export default {}"),
    );

    let ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let first = ws.resolve_import("/repo/src/a.vue", "vue", ctx);
    let second = ws.resolve_import("/repo/src/b.vue", "vue", ctx);

    assert!(first.is_some(), "first importer should resolve vue");
    assert!(second.is_some(), "second importer should resolve vue");
    assert_eq!(
        ws.engine.package_index.read().found_count(),
        1,
        "package manifests should be cached in Engine::package_index"
    );
}

#[test]
fn package_manifest_cache_invalidates_after_package_json_write() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("/repo/src/App.vue".to_string(), Arc::from("<template/>"));
    ws.inject_file(
        "/repo/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{"module":"dist/old.js"}"#),
    );
    ws.inject_file(
        "/repo/node_modules/pkg/dist/old.js".to_string(),
        Arc::from("export const oldValue = 1;"),
    );
    ws.inject_file(
        "/repo/node_modules/pkg/dist/new.js".to_string(),
        Arc::from("export const newValue = 1;"),
    );

    let ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let first = ws
        .resolve_import("/repo/src/App.vue", "pkg", ctx)
        .expect("initial package.json should resolve");
    assert_eq!(first.source_id, "/repo/node_modules/pkg/dist/old.js");
    assert_eq!(
        ws.engine.package_index.read().found_count(),
        1,
        "initial resolution should populate the manifest cache"
    );

    ws.write_file(
        "/repo/node_modules/pkg/package.json",
        r#"{"module":"dist/new.js"}"#,
    )
    .expect("package.json rewrite should succeed");

    assert_eq!(
        ws.engine.package_index.read().found_count(),
        0,
        "writing package.json should invalidate the cached manifest"
    );

    let second = ws
        .resolve_import("/repo/src/App.vue", "pkg", ctx)
        .expect("updated package.json should resolve");
    assert_eq!(
        second.source_id, "/repo/node_modules/pkg/dist/new.js",
        "resolution should observe the new manifest after invalidation"
    );
}

#[test]
fn lazy_import_resolution_cache_invalidates_after_package_json_write() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("/repo/src/App.vue".to_string(), Arc::from("<template/>"));
    ws.inject_file(
        "/repo/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{"module":"dist/old.js"}"#),
    );
    ws.inject_file(
        "/repo/node_modules/pkg/dist/old.js".to_string(),
        Arc::from("export const oldValue = 1;"),
    );
    ws.inject_file(
        "/repo/node_modules/pkg/dist/new.js".to_string(),
        Arc::from("export const newValue = 1;"),
    );

    let ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let first = ws
        .resolve_import("/repo/src/App.vue", "pkg", ctx)
        .expect("initial package.json should resolve");
    let after_first = ws.engine.vfs_provenance.snapshot();
    let second = ws
        .resolve_import("/repo/src/App.vue", "pkg", ctx)
        .expect("warm lazy cache lookup should resolve");
    let after_second = ws.engine.vfs_provenance.snapshot();

    assert_eq!(first.source_id, "/repo/node_modules/pkg/dist/old.js");
    assert_eq!(second.source_id, first.source_id);
    assert_eq!(
        after_first.import_resolution_cache_miss_count, 1,
        "first resolve should seed the lazy import cache",
    );
    assert_eq!(
        after_second.import_resolution_cache_hit_count, 1,
        "second resolve should come from the lazy import cache",
    );

    ws.inject_file(
        "/repo/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{"module":"dist/new.js"}"#),
    );

    let third = ws
        .resolve_import("/repo/src/App.vue", "pkg", ctx)
        .expect("updated package.json should resolve");
    let after_third = ws.engine.vfs_provenance.snapshot();
    assert_eq!(
        third.source_id, "/repo/node_modules/pkg/dist/new.js",
        "content-generation invalidation should drop the stale lazy import cache entry",
    );
    assert_eq!(
        after_third.import_resolution_cache_miss_count, 2,
        "post-write resolve should rebuild the lazy import cache instead of serving stale data",
    );
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

#[test]
fn owner_for_file_returns_none_for_ambiguous_configured_projects() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    let graph = ProjectGraph::from_configs(vec![
        VfsProjectConfig {
            root: "d:/project".to_string(),
            rank: ProjectRank::Explicit,
            tsconfig_path: Some("d:/project/tsconfig.app.json".to_string()),
            root_files: vec![],
            extensions: vec![],
            workspace_root: "d:/project".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: ProjectMembership::IncludeExclude {
                files: vec!["d:/project/src/shared.ts".to_string()],
                include: vec![],
                exclude: vec![],
            },
        },
        VfsProjectConfig {
            root: "d:/project".to_string(),
            rank: ProjectRank::Explicit,
            tsconfig_path: Some("d:/project/tsconfig.vitest.json".to_string()),
            root_files: vec![],
            extensions: vec![],
            workspace_root: "d:/project".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: ProjectMembership::IncludeExclude {
                files: vec!["d:/project/src/shared.ts".to_string()],
                include: vec![],
                exclude: vec![],
            },
        },
    ]);
    ws.set_project_graph(graph);

    assert!(
        ws.owner_for_file("d:/project/src/shared.ts").is_none(),
        "workspace single-owner API must not collapse overlapping configured owners"
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
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
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
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert!(result.is_some());
    assert_eq!(result.unwrap().source_id, "d:/project/src/utils.ts");
}

/// The atomic combined mutator must carry the exact two-call semantics
/// (parsed edges recorded, exacts applied and resolvable) plus the
/// value-idempotency contract: an identical re-push reports
/// `changed: false`, a differing push reports `changed: true`.
#[test]
fn record_parsed_edges_with_exact_resolutions_records_and_applies_atomically() {
    use crate::traits::WorkspaceAccess;

    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let owner = "d:/project/src/app.vue";
    let target = "d:/project/src/utils.ts";
    ws.inject_file(target.to_string(), Arc::from("export const x = 1;"));

    let edges = vec![crate::types::ParsedEdge::Relative {
        specifier: "./utils".to_string(),
        kind: ResolveRequestKind::EsmImport,
    }];
    let exacts = vec![ExactResolution {
        specifier: "./utils".to_string(),
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
        resolved_canonical_id: Some(target.to_string()),
        possible_canonical_ids: vec![],
    }];

    let first = ws.record_parsed_edges_with_exact_resolutions(owner, &edges, exacts.clone());
    assert!(first.changed, "first push must report a table change");

    // Exact resolution is queryable (the set_exact_resolutions half).
    let resolved = ws.resolve_import(
        owner,
        "./utils",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert_eq!(
        resolved.map(|r| r.source_id),
        Some(target.to_string()),
        "exacts must be applied",
    );
    // Reverse edge present (the record_parsed_edges half).
    assert!(
        ws.reverse_deps_for(target).contains(&owner.to_string()),
        "parsed/exact reverse edge must be recorded",
    );

    // Identical re-push: value no-op on both halves.
    let second = ws.record_parsed_edges_with_exact_resolutions(owner, &edges, exacts);
    assert!(
        !second.changed,
        "identical re-push must report changed: false (value-idempotency)",
    );

    // Differing exacts: change reported.
    let third = ws.record_parsed_edges_with_exact_resolutions(
        owner,
        &edges,
        vec![ExactResolution {
            specifier: "./utils".to_string(),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("d:/project/src/other.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );
    assert!(third.changed, "a differing exact push must report changed");
}

/// Every per-canonical content mutator must record the canonical's
/// transition in the workspace's content-transition ledger — the
/// read-side freshness authority consumers compare retained-artifact
/// build generations against. Mutators that bypass host-level wrappers
/// (direct embedder notify, write_file, copy_file) are exactly the
/// perimeter this ledger closes.
#[test]
fn content_mutators_record_per_canonical_transition_generation() {
    use crate::traits::WorkspaceRead;

    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let untouched = "d:/project/src/untouched.ts";
    assert_eq!(
        ws.last_content_transition_generation(untouched),
        0,
        "a never-transitioned canonical reports 0",
    );

    ws.inject_file(
        "d:/project/src/seed.ts".to_string(),
        Arc::from("export const s = 1;"),
    );
    let seed_gen = ws.last_content_transition_generation("d:/project/src/seed.ts");
    assert!(seed_gen > 0, "inject_file records a transition");
    assert_eq!(
        seed_gen,
        ws.content_generation(),
        "the recorded transition is the post-bump generation",
    );

    ws.notify_upsert("d:/project/src/a.ts", Arc::from("export const a = 1;"));
    let a_gen = ws.last_content_transition_generation("d:/project/src/a.ts");
    assert!(a_gen > seed_gen, "notify_upsert records a transition");

    ws.write_file("d:/project/src/b.ts", "export const b = 1;")
        .expect("write_file");
    let b_gen = ws.last_content_transition_generation("d:/project/src/b.ts");
    assert!(b_gen > a_gen, "write_file records a transition");

    ws.copy_file("d:/project/src/b.ts", "d:/project/src/c.ts")
        .expect("copy_file");
    let c_gen = ws.last_content_transition_generation("d:/project/src/c.ts");
    assert!(
        c_gen > b_gen,
        "copy_file records the DESTINATION transition"
    );

    ws.notify_close("d:/project/src/a.ts");
    assert!(
        ws.last_content_transition_generation("d:/project/src/a.ts") > c_gen,
        "notify_close records a transition",
    );

    ws.delete_file("d:/project/src/b.ts").expect("delete_file");
    assert!(
        ws.last_content_transition_generation("d:/project/src/b.ts") > c_gen,
        "delete_file records a transition",
    );

    // Per-canonical isolation: every mutation above left the untouched
    // canonical's ledger entry at 0.
    assert_eq!(
        ws.last_content_transition_generation(untouched),
        0,
        "unrelated mutations never touch another canonical's ledger entry",
    );

    // R22 idempotency: a byte-identical notify_upsert re-push is a TRUE
    // no-op — no transition recorded.
    ws.notify_upsert("d:/project/src/d.ts", Arc::from("export const d = 1;"));
    let d_gen = ws.last_content_transition_generation("d:/project/src/d.ts");
    ws.notify_upsert("d:/project/src/d.ts", Arc::from("export const d = 1;"));
    assert_eq!(
        ws.last_content_transition_generation("d:/project/src/d.ts"),
        d_gen,
        "a byte-identical re-upsert records no transition (R22)",
    );
}

/// Ledger keys normalize at the recording chokepoint: a direct embedder
/// passing a non-canonical key form (backslashes, Windows drive-letter
/// casing) must record under the SAME key the artifact-only freshness
/// gate queries with — an un-normalized record is a silent fresh
/// verdict for the normalized query.
#[test]
fn content_transition_ledger_normalizes_keys_at_recording_and_query() {
    use crate::traits::{WorkspaceAccess, WorkspaceRead};

    let ws = MemoryWorkspace::new(MemoryOptions::default());

    // Record under the RAW backslash + uppercase-drive form a direct
    // NAPI embedder is most likely to pass.
    ws.notify_upsert(
        "D:\\project\\src\\Raw.ts",
        Arc::from("export const raw = 1;"),
    );
    let normalized_query = ws.last_content_transition_generation("d:/project/src/Raw.ts");
    assert!(
        normalized_query > 0,
        "a transition recorded under a backslash/drive-cased raw key MUST be \
         observable under the normalized canonical the gate queries with",
    );

    // And the reverse: record normalized, query raw — the query side
    // normalizes too, so any mixed-form caller pair agrees.
    ws.notify_upsert(
        "d:/project/src/plain.ts",
        Arc::from("export const plain = 1;"),
    );
    assert!(
        ws.last_content_transition_generation("D:\\project\\src\\plain.ts") > 0,
        "the query side must normalize the probed canonical as well",
    );
}

/// A watcher `DirectoryTreeDirty` recovery transitions an UNKNOWN member
/// set — the engine cannot enumerate what changed on disk — so the
/// ledger must record the SUBTREE: every member canonical's
/// `last_content_transition_generation` advances, while canonicals
/// outside the prefix are untouched.
#[test]
fn directory_tree_dirty_records_subtree_content_transition() {
    use crate::changes::WorkspaceChange;
    use crate::traits::WorkspaceRead;

    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let member = "d:/project/src/pkg/member.ts";
    let outside = "d:/project/other/outside.ts";
    ws.inject_file(member.to_string(), Arc::from("export const m = 1;"));
    ws.inject_file(outside.to_string(), Arc::from("export const o = 1;"));
    let member_gen = ws.last_content_transition_generation(member);
    let outside_gen = ws.last_content_transition_generation(outside);

    ws.apply_changes(vec![WorkspaceChange::DirectoryTreeDirty {
        prefix: "d:/project/src/pkg".to_string(),
    }]);

    assert!(
        ws.last_content_transition_generation(member) > member_gen,
        "a DirectoryTreeDirty recovery must advance every member \
         canonical's transition generation — an out-of-band disk change \
         under the prefix may have rewritten any of them, so a retained \
         pre-recovery artifact must stop validating as content-fresh",
    );
    assert_eq!(
        ws.last_content_transition_generation(outside),
        outside_gen,
        "a canonical outside the dirty prefix is untouched (per-canonical \
         isolation preserved)",
    );
}

// ── MemoryWorkspace::set_project_graph ──

#[test]
fn set_project_graph_updates_resolver() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "d:/project/src/utils.ts".to_string(),
        Arc::from("export const x = 1;"),
    );

    // No project graph yet — relative imports still resolve via basic path probing
    // (the engine uses a default empty resolver that handles unowned relative paths).
    let result_before = ws.resolve_import(
        "d:/project/src/app.vue",
        "./utils",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert!(
        result_before.is_some(),
        "relative imports should resolve via basic path probing even without project graph"
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
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
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

// ── WorkspaceAccess trait mutation methods ──

#[test]
fn trait_set_exact_resolutions_delegates_to_engine() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from("<template>hi</template>"),
    );
    ws.inject_file(
        "/src/utils.ts".to_string(),
        Arc::from("export const x = 1;"),
    );

    // Call via trait method.
    let result = WorkspaceAccess::set_exact_resolutions(
        &ws,
        "/src/App.vue",
        vec![ExactResolution {
            specifier: "./utils".to_string(),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/src/utils.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    // Positive: should return without panic; newly_resolved can be empty
    // because the exact resolution doesn't trigger edge recording.
    let _ = result;

    // Positive: resolve_import should now find it via exact resolution.
    let resolved = ws.resolve_import(
        "/src/App.vue",
        "./utils",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert!(
        resolved.is_some(),
        "trait set_exact_resolutions should make specifier resolvable"
    );
    assert_eq!(resolved.unwrap().source_id, "/src/utils.ts");

    // Negative: other specifiers still don't resolve.
    let no_result = ws.resolve_import(
        "/src/App.vue",
        "./other",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert!(
        no_result.is_none(),
        "specifiers not in exact resolutions should not resolve"
    );
}

#[test]
fn trait_configure_resolver_builds_project_resolver() {
    use crate::resolver::IdeProjectConfig;

    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "/proj/src/App.vue".to_string(),
        Arc::from("<template>hi</template>"),
    );
    ws.inject_file(
        "/proj/src/Foo.vue".to_string(),
        Arc::from("<template>foo</template>"),
    );

    // Configure with a path alias: @ -> /proj/src
    let mut project = IdeProjectConfig::new(
        "/proj".to_string(),
        "/proj".to_string(),
        Some("/proj/tsconfig.json".to_string()),
    );
    project.compiler_options.paths = vec![("@/*".to_string(), vec!["/proj/src/*".to_string()])];

    // Call via trait method.
    WorkspaceAccess::configure_resolver(&ws, vec![project]);

    // Positive: workspace resolver should now resolve @/Foo.vue.
    let resolved = ws.resolve_import(
        "/proj/src/App.vue",
        "@/Foo.vue",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert!(
        resolved.is_some(),
        "trait configure_resolver should make alias resolvable"
    );
    assert_eq!(resolved.unwrap().source_id, "/proj/src/Foo.vue");

    // Negative: non-matching alias.
    let no_result = ws.resolve_import(
        "/proj/src/App.vue",
        "~/Bar.vue",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert!(no_result.is_none(), "non-matching alias should not resolve");
}

#[test]
fn trait_configure_resolver_empty_clears_resolver() {
    use crate::resolver::IdeProjectConfig;

    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file(
        "/proj/src/App.vue".to_string(),
        Arc::from("<template>hi</template>"),
    );
    ws.inject_file(
        "/proj/src/Foo.vue".to_string(),
        Arc::from("<template>foo</template>"),
    );

    // First configure with a resolver.
    let mut project = IdeProjectConfig::new(
        "/proj".to_string(),
        "/proj".to_string(),
        Some("/proj/tsconfig.json".to_string()),
    );
    project.compiler_options.paths = vec![("@/*".to_string(), vec!["/proj/src/*".to_string()])];
    WorkspaceAccess::configure_resolver(&ws, vec![project]);

    // Verify it works.
    let resolved = ws.resolve_import(
        "/proj/src/App.vue",
        "@/Foo.vue",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert!(resolved.is_some(), "should resolve before clearing");

    // Clear it.
    WorkspaceAccess::configure_resolver(&ws, vec![]);

    // The resolver is rebuilt from empty configs, so aliases should no longer resolve.
    let after_clear = ws.resolve_import(
        "/proj/src/App.vue",
        "@/Foo.vue",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert!(
        after_clear.is_none(),
        "clearing resolver should stop alias resolution"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 5): changing the
/// default resolve-extension list is a resolve-config mutation. RouteDb
/// effective-export-set entries are keyed on `resolve_env_hash`, and the
/// session-side route-surface edge-currency and known-miss staleness
/// gates key on `content_generation`. The extension setter must
/// therefore (a) recompose + republish the per-project env-hash tables
/// so the new `resolve_env_hash` takes effect (old-keyed entries become
/// unreachable), and (b) advance `content_generation` so those
/// downstream freshness gates invalidate.
///
/// FAILS pre-fix: `set_default_resolve_extensions` only swapped the
/// stored list and never republished env hashes or bumped the epoch, so
/// `resolve_env_hash` stayed identical (stale serve) and the generation
/// did not move. PASSES post-fix: the setter republishes and bumps once.
#[test]
fn changing_default_resolve_extensions_republishes_resolve_env_hash() {
    use crate::resolver::IdeProjectConfig;
    use crate::workspace_snapshot::ProjectId;

    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let project = IdeProjectConfig::new(
        "/proj".to_string(),
        "/proj".to_string(),
        Some("/proj/tsconfig.json".to_string()),
    );
    WorkspaceAccess::configure_resolver(&ws, vec![project]);

    // index 1 of the per-project env-hash array is `resolve_env_hash`.
    let read_resolve_env_hash = || {
        ws.engine
            .load_published()
            .expect("published state after configure_resolver")
            .env_hashes_by_project
            .get(&ProjectId(0))
            .copied()
            .expect("project 0 env-hash array")[1]
    };

    let hash_before = read_resolve_env_hash();
    let gen_before = ws.content_generation();

    // `.custom` is NOT in `probe_extensions()`, so the merged extension
    // set genuinely changes — `resolve_env_hash` MUST change iff the
    // setter republishes the env-hash tables.
    WorkspaceAccess::set_default_resolve_extensions(&ws, vec![".custom".to_string()]);

    let hash_after = read_resolve_env_hash();
    let gen_after = ws.content_generation();

    assert_ne!(
        hash_before, hash_after,
        "changing default resolve extensions MUST republish the project's \
         resolve_env_hash so RouteDb effective-export-set entries keyed on the \
         old hash are no longer reachable"
    );
    assert!(
        gen_after > gen_before,
        "an extension-list change is a resolve-config mutation: it must advance \
         content_generation so the downstream edge-currency and known-miss \
         staleness gates invalidate"
    );
}

// ── Context-keyed exact resolution tests ──

/// Exact overrides keyed by (specifier, phase, kind): different contexts
/// resolve the same specifier to different targets on the same importer.
#[test]
fn exact_resolution_context_keyed_different_targets() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("d:/project/src/app.vue".to_string(), Arc::from("// app"));

    // Set two exact resolutions for the same specifier but different contexts
    ws.set_exact_resolutions(
        "d:/project/src/app.vue",
        vec![
            ExactResolution {
                specifier: "pkg".to_string(),
                phase: ResolvePhase::CodegenBlocker,
                kind: ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some("node_modules/pkg/index.js".to_string()),
                possible_canonical_ids: vec![],
            },
            ExactResolution {
                specifier: "pkg".to_string(),
                phase: ResolvePhase::ProviderGraph,
                kind: ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some("node_modules/pkg/index.d.ts".to_string()),
                possible_canonical_ids: vec![],
            },
        ],
    );

    // CodegenBlocker + EsmImport → index.js
    let codegen_result = ws.resolve_import(
        "d:/project/src/app.vue",
        "pkg",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert_eq!(
        codegen_result.as_ref().map(|r| r.source_id.as_str()),
        Some("node_modules/pkg/index.js"),
        "CodegenBlocker exact should resolve to .js"
    );

    // ProviderGraph + EsmImport → index.d.ts
    let provider_result = ws.resolve_import(
        "d:/project/src/app.vue",
        "pkg",
        ResolutionContext {
            phase: ResolvePhase::ProviderGraph,
            kind: ResolveRequestKind::EsmImport,
        },
    );
    assert_eq!(
        provider_result.as_ref().map(|r| r.source_id.as_str()),
        Some("node_modules/pkg/index.d.ts"),
        "ProviderGraph exact should resolve to .d.ts"
    );
}

// ── Path identity mismatch reproduction tests ──
//
// These tests document that overlay stores keys as-is without normalizing
// drive letter case. `canonicalize_id()` lowercases `C:` to `c:`, but
// `notify_upsert`/`apply_changes` don't normalize, so a file upserted as
// `C:/repo/App.vue` won't be found by a resolver looking up `c:/repo/App.vue`.

/// Documents regression: overlay content stored via uppercase drive letter
/// is not found when looked up via lowercase drive letter.
#[test]
fn overlay_case_mismatch_loses_content() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    // Upsert with uppercase drive letter (simulates LSP uri_to_canonical_id output)
    ws.notify_upsert("C:/repo/App.vue", Arc::from("<template>hi</template>"));

    // Lookup with lowercase drive letter (simulates resolver normalize_canonical_id)
    let content = ws.read_file("c:/repo/App.vue");

    // FAILS today: overlay stores "C:/repo/App.vue", lookup for "c:/repo/App.vue" misses
    assert!(
        content.is_some(),
        "overlay content should be found regardless of drive letter case"
    );
}

/// Documents regression: `apply_changes` with `OverlaySet` stores keys
/// without normalization, so case-mismatched lookups fail.
#[test]
fn apply_changes_overlay_set_case_mismatch() {
    let engine = crate::engine::Engine::new();
    engine.apply_changes(vec![WorkspaceChange::OverlaySet {
        canonical_id: "C:/repo/App.vue".to_string(),
        source: Arc::from("content"),
    }]);

    let has = engine.overlay.read().has_overlay("c:/repo/App.vue");
    // FAILS today: key stored as-is, lookup misses
    assert!(
        has,
        "overlay should normalize keys — C: and c: should match"
    );
}

/// Documents regression: `notify_close` with a different-case drive letter
/// does not clear the overlay set by `notify_upsert`.
#[test]
fn notify_close_case_mismatch_clears_overlay() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.notify_upsert("C:/repo/App.vue", Arc::from("content"));

    // Close with normalized (lowercase) ID
    ws.notify_close("c:/repo/App.vue");

    // Should be cleared regardless of case
    let content = ws.read_file("C:/repo/App.vue");
    // FAILS today: close("c:/...") doesn't clear overlay set by "C:/..."
    assert!(
        content.is_none(),
        "overlay should be cleared after close with different case"
    );
}

// ── §4.2 — MemoryWorkspace integration tests for the new dep model ──

/// §4.2 #1 — Workspace-only oracle for §4.6 regressors. No project graph;
/// `record_parsed_edges` with `Relative { "./types" }`; assert
/// `reverse_deps_for("/src/types.ts")` returns the importer.
#[test]
fn memory_unresolved_relative_records_stem_without_published_root() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.record_parsed_edges(
        "/src/Comp.vue",
        &[crate::types::ParsedEdge::Relative {
            specifier: "./types".to_string(),
            kind: crate::types::ResolveRequestKind::EsmImport,
        }],
    );
    // /src/types.ts strips `.ts` → /src/types — finds the stem bucket.
    assert_eq!(
        ws.reverse_deps_for("/src/types.ts"),
        vec!["/src/Comp.vue"],
        "unresolved relative must record stem visible by extension-stripping query"
    );
}

/// §4.2 #2 — Successful resolve populates canonical only; stem axis empty.
#[test]
fn memory_resolved_relative_does_not_leak_stem() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    // Inject the file content + project graph so resolver succeeds.
    ws.inject_file("/src/types.ts".to_string(), Arc::from("export {}"));
    ws.set_project_graph(crate::project_graph::ProjectGraph::from_configs(vec![
        crate::project_graph::VfsProjectConfig {
            root: "/src".to_string(),
            rank: crate::project_graph::ProjectRank::Explicit,
            tsconfig_path: None,
            root_files: vec![],
            extensions: vec![".ts".into()],
            workspace_root: "/src".to_string(),
            workspace_aliases: vec![],
            compiler_options: crate::resolver::IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: crate::resolver::ProjectMembership::default(),
        },
    ]));
    ws.record_parsed_edges(
        "/src/Comp.vue",
        &[crate::types::ParsedEdge::Relative {
            specifier: "./types".to_string(),
            kind: crate::types::ResolveRequestKind::EsmImport,
        }],
    );
    // Canonical hit — direct query.
    let got = ws.reverse_deps_for("/src/types.ts");
    assert_eq!(got, vec!["/src/Comp.vue"]);
    // Stem-only query — empty (canonical hit doesn't leak into stem axis).
    let snap = ws
        .dependency_snapshot("/src/Comp.vue")
        .expect("snapshot present");
    assert!(
        snap.parsed_unresolved_relatives.is_empty(),
        "successfully-resolved relatives must NOT populate parsed_unresolved_relatives",
    );
}

/// §4.2 #3 — F1.5: ambient deps survive parse re-record.
/// Pre-fix this fails because record_ambient_dependency routed to
/// lazy_resolved which is cleared on every parse re-record.
#[test]
fn memory_record_ambient_dependency_uses_ambient_class() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.record_ambient_dependency("/src/Comp.vue", "ambient:/Cabc/lib.es5.d.ts");
    // Parse-edge re-record with no edges (empty array).
    ws.record_parsed_edges("/src/Comp.vue", &[]);
    assert_eq!(
        ws.reverse_deps_for("ambient:/Cabc/lib.es5.d.ts"),
        vec!["/src/Comp.vue"],
        "F1.5: ambient deps must SURVIVE parse re-record"
    );
}

/// §4.2 #4 — R4 expanded merge contract:
/// (a) `.vue` strips (from probe — pins F3 fix);
/// (b) `.tsx` strips (from probe — workspace owns this, host config CANNOT narrow);
/// (c) `.ts` strips (host configured AND probe — natural intersection);
/// (d) `.svelte` does NOT strip (truly unknown extension).
/// The workspace's merge policy is additive — host config ADDS to probe.
#[test]
fn memory_default_resolve_extensions_merges_with_probe_authoritatively() {
    let ws = MemoryWorkspace::new(MemoryOptions {
        default_resolve_extensions: Some(vec![".ts".to_string()]),
        ..MemoryOptions::default()
    });

    // (a) `.vue` strips (from probe).
    ws.record_parsed_edges(
        "/src/A.vue",
        &[crate::types::ParsedEdge::Relative {
            specifier: "./Child".to_string(),
            kind: crate::types::ResolveRequestKind::EsmImport,
        }],
    );
    assert_eq!(
        ws.reverse_deps_for("/src/Child.vue"),
        vec!["/src/A.vue"],
        "(a) .vue must strip (from probe — F3 fix)"
    );

    // (b) `.tsx` strips (from probe).
    ws.record_parsed_edges(
        "/src/B.vue",
        &[crate::types::ParsedEdge::Relative {
            specifier: "./Helper".to_string(),
            kind: crate::types::ResolveRequestKind::EsmImport,
        }],
    );
    assert_eq!(
        ws.reverse_deps_for("/src/Helper.tsx"),
        vec!["/src/B.vue"],
        "(b) .tsx must strip (from probe — workspace owns this)"
    );

    // (c) `.ts` strips (host configured + probe).
    ws.record_parsed_edges(
        "/src/C.vue",
        &[crate::types::ParsedEdge::Relative {
            specifier: "./util".to_string(),
            kind: crate::types::ResolveRequestKind::EsmImport,
        }],
    );
    assert_eq!(
        ws.reverse_deps_for("/src/util.ts"),
        vec!["/src/C.vue"],
        "(c) .ts must strip (host + probe)"
    );

    // (d) `.svelte` does NOT strip.
    ws.record_parsed_edges(
        "/src/D.vue",
        &[crate::types::ParsedEdge::Relative {
            specifier: "./Mystery".to_string(),
            kind: crate::types::ResolveRequestKind::EsmImport,
        }],
    );
    assert!(
        ws.reverse_deps_for("/src/Mystery.svelte").is_empty(),
        "(d) .svelte must NOT strip (unknown extension)"
    );
}

/// §4.2 #6 — `replace_semantic_transitive` populates canonical reverse axis.
#[test]
fn memory_replace_semantic_transitive_creates_canonical_reverse_bucket() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let deps: std::collections::BTreeSet<String> =
        std::iter::once("/src/shared.ts".to_string()).collect();
    ws.replace_semantic_transitive("/src/Comp.vue", deps);
    assert_eq!(ws.reverse_deps_for("/src/shared.ts"), vec!["/src/Comp.vue"],);
}

/// §4.2 #7 — Combined: record stem for `./types`, then `set_exact_resolutions`
/// with `./types → /lib/types.ts`. Stem axis empty; canonical axis populated.
#[test]
fn memory_set_exact_resolutions_dampens_active_stem_canonical_works() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.record_parsed_edges(
        "/src/Comp.vue",
        &[crate::types::ParsedEdge::Relative {
            specifier: "./types".to_string(),
            kind: crate::types::ResolveRequestKind::EsmImport,
        }],
    );
    assert_eq!(
        ws.reverse_deps_for("/src/types.ts"),
        vec!["/src/Comp.vue"],
        "stem present (matches via .ts strip)"
    );
    ws.set_exact_resolutions(
        "/src/Comp.vue",
        vec![crate::types::ExactResolution {
            specifier: "./types".to_string(),
            phase: crate::types::ResolvePhase::CodegenBlocker,
            kind: crate::types::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/lib/types.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );
    // Stem dampened.
    assert!(
        ws.reverse_deps_for("/src/types.ts").is_empty(),
        "stem must be dampened after bundler resolves",
    );
    // Canonical populated.
    assert_eq!(ws.reverse_deps_for("/lib/types.ts"), vec!["/src/Comp.vue"],);
}

// ── delete_dir_all / subtree-transition prefix normalization ──

/// `delete_dir_all("/")` must remove every file in the snapshot. The
/// enumeration filter routes through `path_matches_prefix`, which
/// normalizes the trailing slash; a naive `format!("{path}/")` prefix
/// turns the root into `"//"` and matches nothing.
#[test]
fn delete_dir_all_root_removes_all_files() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("/a/x.ts".to_string(), Arc::from("export const x = 1\n"));
    ws.inject_file("/b/y.ts".to_string(), Arc::from("export const y = 2\n"));

    ws.delete_dir_all("/")
        .expect("root delete_dir_all succeeds");

    assert!(
        ws.read_file("/a/x.ts").is_none(),
        "delete_dir_all(\"/\") must remove /a/x.ts"
    );
    assert!(
        ws.read_file("/b/y.ts").is_none(),
        "delete_dir_all(\"/\") must remove /b/y.ts"
    );
}

/// Path-boundary correctness: deleting `/a` removes the `/a` subtree
/// only — a sibling whose name merely starts with the same bytes
/// (`/ab.ts`) survives.
#[test]
fn delete_dir_all_respects_path_component_boundaries() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("/a/x.ts".to_string(), Arc::from("export const x = 1\n"));
    ws.inject_file("/ab.ts".to_string(), Arc::from("export const ab = 3\n"));

    ws.delete_dir_all("/a")
        .expect("subtree delete_dir_all succeeds");

    assert!(
        ws.read_file("/a/x.ts").is_none(),
        "delete_dir_all(\"/a\") must remove the subtree member /a/x.ts"
    );
    assert!(
        ws.read_file("/ab.ts").is_some(),
        "delete_dir_all(\"/a\") must NOT remove the byte-prefix sibling /ab.ts"
    );
}

/// A recorded ROOT subtree transition (`"/"`) folds into every
/// canonical's `last_content_transition_generation`. The read-side
/// subtree filter routes through `path_matches_prefix`; a raw
/// `canonical.as_bytes()[prefix.len()] == b'/'` boundary byte check
/// can never match the root prefix (the byte after `"/"` is the first
/// name character, not another `'/'`).
#[test]
fn root_subtree_transition_folds_into_every_canonical() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    assert_eq!(
        ws.engine
            .last_content_transition_generation("/never/touched.ts"),
        0,
        "untouched canonical starts with no transition"
    );

    ws.engine.record_subtree_content_transition("/");

    assert!(
        ws.engine
            .last_content_transition_generation("/never/touched.ts")
            > 0,
        "a root subtree transition must fold into every canonical under /"
    );
}

/// Non-root subtree transitions stay component-boundary-precise: a
/// `/src` record folds into `/src/a.ts` but not into the byte-prefix
/// sibling `/srcx.ts`.
#[test]
fn subtree_transition_respects_path_component_boundaries() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.engine.record_subtree_content_transition("/src");

    assert!(
        ws.engine.last_content_transition_generation("/src/a.ts") > 0,
        "subtree member must observe the recorded transition"
    );
    assert_eq!(
        ws.engine.last_content_transition_generation("/srcx.ts"),
        0,
        "byte-prefix sibling outside the subtree must NOT observe it"
    );
}

/// Trailing-slash input normalizes to the same semantics as the bare
/// path: `delete_dir_all("/a/")` removes the exact entry `"/a"` just
/// like `delete_dir_all("/a")` does (the enumeration filter strips the
/// trailing slash through `path_matches_prefix` instead of comparing
/// the raw input string against entry ids).
#[test]
fn delete_dir_all_trailing_slash_matches_exact_entry() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.inject_file("/a".to_string(), Arc::from("export const a = 0\n"));
    ws.inject_file("/a/x.ts".to_string(), Arc::from("export const x = 1\n"));

    ws.delete_dir_all("/a/")
        .expect("trailing-slash delete_dir_all succeeds");

    assert!(
        ws.read_file("/a/x.ts").is_none(),
        "delete_dir_all(\"/a/\") must remove the subtree member /a/x.ts"
    );
    assert!(
        ws.read_file("/a").is_none(),
        "delete_dir_all(\"/a/\") must remove the exact entry /a — same \
         semantics as delete_dir_all(\"/a\")"
    );
}
