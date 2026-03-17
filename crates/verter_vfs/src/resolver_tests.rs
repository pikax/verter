use super::*;
use crate::types::ResolvePhase;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn project(
    root: &str,
    workspace_root: &str,
    tsconfig_path: Option<&str>,
    membership: ProjectMembership,
) -> IdeProjectConfig {
    let mut project = IdeProjectConfig::new(
        root.to_string(),
        workspace_root.to_string(),
        tsconfig_path.map(str::to_string),
    );
    project.membership = membership;
    project
}

#[derive(Default)]
struct TestReader {
    files: HashSet<String>,
    texts: HashMap<String, Arc<str>>,
    realpaths: HashMap<String, String>,
}

impl TestReader {
    fn with_files(paths: &[&str]) -> Self {
        let mut reader = Self::default();
        for path in paths {
            let normalized = normalize_canonical_id(path);
            reader.files.insert(normalized.clone());
            reader
                .texts
                .insert(normalized, Arc::<str>::from("// test file"));
        }
        reader
    }

    fn add_file(&mut self, path: &str, text: &str) {
        let normalized = normalize_canonical_id(path);
        self.files.insert(normalized.clone());
        self.texts
            .insert(normalized, Arc::<str>::from(text.to_string()));
    }

    fn add_realpath(&mut self, path: &str, realpath: &str) {
        self.realpaths.insert(
            normalize_canonical_id(path),
            normalize_canonical_id(realpath),
        );
    }
}

impl ProjectResolverReader for TestReader {
    fn read_text(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.texts
            .get(&normalize_canonical_id(canonical_id))
            .cloned()
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.files.contains(&normalize_canonical_id(canonical_id))
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.realpaths
            .get(&normalize_canonical_id(canonical_id))
            .cloned()
            .or_else(|| {
                self.file_exists(canonical_id)
                    .then(|| normalize_canonical_id(canonical_id))
            })
    }
}

#[test]
fn owner_selection_ignores_solution_style_root_membership() {
    let resolver = ProjectResolver::new(vec![
        project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.json"),
            ProjectMembership::IncludeExclude {
                files: Vec::new(),
                include: Vec::new(),
                exclude: Vec::new(),
            },
        ),
        project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::IncludeExclude {
                files: Vec::new(),
                include: vec!["src/**/*".to_string()],
                exclude: vec!["tests/**/*".to_string()],
            },
        ),
    ]);

    let owner = resolver
        .owner_for_file("/workspace/src/App.vue")
        .expect("src/App.vue should have an owner project");

    assert_eq!(
        owner.tsconfig_path.as_deref(),
        Some("/workspace/tsconfig.app.json"),
        "membership-aware owner selection should skip solution-style tsconfig.json"
    );
    assert_ne!(
        owner.tsconfig_path.as_deref(),
        Some("/workspace/tsconfig.json"),
        "solution-style tsconfig.json must not win when it owns no files"
    );
}

#[test]
fn provider_id_uses_original_path_for_non_vue() {
    let resolver = ProjectResolver::new(vec![
        project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::IncludeExclude {
                files: Vec::new(),
                include: vec!["src/**/*".to_string()],
                exclude: Vec::new(),
            },
        ),
        project(
            "/workspace",
            "/workspace",
            None,
            ProjectMembership::MatchAll,
        ),
    ]);

    let provider_id = resolver
        .provider_id_for_source("/workspace/scripts/tool.ts")
        .expect("unmatched file should still receive a provider path");

    assert_eq!(
        provider_id, "/workspace/scripts/tool.ts",
        "non-Vue provider ID should be the canonical ID itself"
    );
}

#[test]
fn provider_paths_keep_vue_as_public_api_targets() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);

    let provider_id = resolver
        .provider_id_for_source("/workspace/src/App.vue")
        .expect("vue files should be rewritten to public API provider paths");

    assert!(
        provider_id.ends_with("/src/App.vue.ts"),
        "Vue files should resolve to .vue.ts in the provider graph: {provider_id}"
    );
    assert!(
        !provider_id.ends_with("/src/App.vue"),
        "provider graph must not expose raw .vue source IDs"
    );
}

#[test]
fn provider_ide_id_appends_tsx_to_vue() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);

    let provider_id = resolver
        .provider_ide_id_for_source("/workspace/src/App.vue", false)
        .expect("vue IDE files should receive provider IDs");

    assert_eq!(
        provider_id, "/workspace/src/App.vue.tsx",
        "Vue IDE path should be canonical_id.tsx"
    );
    assert_eq!(
        resolver.source_id_from_provider_id(&provider_id).as_deref(),
        Some("/workspace/src/App.vue"),
        "Vue IDE provider paths must round-trip back to the source ID"
    );
    assert_ne!(
        Some(provider_id.clone()),
        resolver.provider_id_for_source("/workspace/src/App.vue"),
        "IDE provider paths must remain distinct from the public .vue.ts API path"
    );
}

#[test]
fn provider_paths_round_trip_back_to_source_ids() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);

    let vue_api = resolver
        .provider_id_for_source("/workspace/src/App.vue")
        .expect("vue API file should resolve");
    let vue_ide = resolver
        .provider_ide_id_for_source("/workspace/src/App.vue", true)
        .expect("vue IDE file should resolve");
    let shadow = resolver
        .provider_id_for_source("/workspace/src/utils.ts")
        .expect("shadow file should resolve");

    assert_eq!(
        resolver.source_id_from_provider_id(&vue_api).as_deref(),
        Some("/workspace/src/App.vue")
    );
    assert_eq!(
        resolver.source_id_from_provider_id(&vue_ide).as_deref(),
        Some("/workspace/src/App.vue")
    );
    assert_eq!(
        resolver.source_id_from_provider_id(&shadow).as_deref(),
        Some("/workspace/src/utils.ts")
    );
}

#[test]
fn resolve_relative_vue_import_returns_real_source_and_provider_api() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "./Foo.vue".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("relative .vue import should resolve");

    assert_eq!(resolved.source_id, "/workspace/src/Foo.vue");
    assert_eq!(resolved.provider_target, ProviderTarget::VuePublicApi);
    assert_eq!(resolved.resolution_kind, ResolutionKind::Relative);
    assert_eq!(resolved.provider_specifier, "./Foo.vue.ts");
    assert!(
        resolved.provider_id.ends_with("/src/Foo.vue.ts"),
        "provider graph should target the materialized .vue.ts API file: {}",
        resolved.provider_id
    );
}

#[test]
fn resolve_workspace_alias_rewrites_to_shadow_provider_file() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    app_project.workspace_aliases = vec![WorkspaceAlias {
        find: "@/".to_string(),
        replacement: "/workspace/src/".to_string(),
    }];
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/utils.ts"]);

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "@/utils".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("workspace alias should resolve");

    assert_eq!(resolved.source_id, "/workspace/src/utils.ts");
    assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
    assert_eq!(resolved.resolution_kind, ResolutionKind::WorkspaceAlias);
    assert_eq!(resolved.provider_specifier, "@/utils");
    assert_eq!(
        resolved.provider_id, "/workspace/src/utils.ts",
        "non-Vue workspace files should resolve to their canonical path: {}",
        resolved.provider_id
    );
}

#[test]
fn resolve_tsconfig_paths_before_base_url() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: Some("/workspace/src".to_string()),
        paths: vec![(
            "shared".to_string(),
            vec!["../generated/shared".to_string()],
        )],
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader =
        TestReader::with_files(&["/workspace/generated/shared.ts", "/workspace/src/shared.ts"]);

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "shared".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("tsconfig paths should resolve before baseUrl fallback");

    assert_eq!(resolved.source_id, "/workspace/generated/shared.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::TsConfigPath);
    assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
    assert_eq!(resolved.provider_specifier, "shared");
    assert_eq!(
        resolved.provider_id, "/workspace/generated/shared.ts",
        "tsconfig paths match must not fall through to baseUrl"
    );
}

#[test]
fn resolve_base_url_when_no_paths_match() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: Some("/workspace/src".to_string()),
        paths: Vec::new(),
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/shared.ts"]);

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "shared".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("baseUrl fallback should resolve when paths has no match");

    assert_eq!(resolved.source_id, "/workspace/src/shared.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::TsConfigPath);
    assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
    assert_eq!(resolved.provider_specifier, "shared");
}

#[test]
fn resolve_relative_paths_use_realpath_normalization() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&["/workspace/src/linked/util.ts"]);
    reader.add_realpath(
        "/workspace/src/linked/util.ts",
        "/workspace/src/shared/util.ts",
    );

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "./linked/util".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("relative import should resolve through the reader realpath");

    assert_eq!(resolved.source_id, "/workspace/src/shared/util.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::Relative);
    assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
    assert_eq!(resolved.provider_specifier, "./linked/util");
    assert_eq!(
        resolved.provider_id, "/workspace/src/shared/util.ts",
        "provider path should be derived from the canonical realpath target: {}",
        resolved.provider_id
    );
}

#[test]
fn resolve_project_references_after_local_tsconfig_options() {
    let mut app_project = project(
        "/workspace/packages/app",
        "/workspace",
        Some("/workspace/packages/app/tsconfig.json"),
        ProjectMembership::MatchAll,
    );
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: Some("/workspace/packages/app/src".to_string()),
        paths: Vec::new(),
    };
    app_project.references = vec!["/workspace/packages/shared/tsconfig.json".to_string()];

    let mut shared_project = project(
        "/workspace/packages/shared",
        "/workspace",
        Some("/workspace/packages/shared/tsconfig.json"),
        ProjectMembership::MatchAll,
    );
    shared_project.compiler_options = IdeProjectCompilerOptions {
        base_url: Some("/workspace/packages/shared/src".to_string()),
        paths: vec![("shared".to_string(), vec!["index".to_string()])],
    };

    let resolver = ProjectResolver::new(vec![app_project, shared_project]);
    let reader = TestReader::with_files(&["/workspace/packages/shared/src/index.ts"]);

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/packages/app/src/App.ts".to_string(),
                specifier: "shared".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("project references should be consulted after local tsconfig resolution");

    let expected_provider_id = resolver
        .provider_id_for_source("/workspace/packages/shared/src/index.ts")
        .expect("referenced source should receive a provider path");
    assert_eq!(
        resolved.source_id,
        "/workspace/packages/shared/src/index.ts"
    );
    assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
    assert_eq!(resolved.provider_id, expected_provider_id);
    assert_eq!(resolved.provider_specifier, "shared");
    assert_eq!(
        resolved.owner_tsconfig_path.as_deref(),
        Some("/workspace/packages/shared/tsconfig.json")
    );
}

#[test]
fn resolve_package_imports_from_nearest_package_json() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&["/workspace/src/utils.ts"]);
    reader.add_file(
        "/workspace/package.json",
        r##"{
                "imports": {
                    "#utils": "./src/utils.ts"
                }
            }"##,
    );

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "#utils".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("package imports should resolve through the nearest package.json");

    assert_eq!(resolved.source_id, "/workspace/src/utils.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::PackageImports);
    assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
    assert_eq!(resolved.provider_specifier, "#utils");
}

#[test]
fn resolve_package_exports_prefers_types_for_root_imports() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/lib/dist/index.d.ts",
        "/workspace/node_modules/lib/dist/index.mjs",
        "/workspace/node_modules/lib/dist/index.cjs",
    ]);
    reader.add_file(
        "/workspace/node_modules/lib/package.json",
        r#"{
                "exports": {
                    ".": {
                        "types": "./dist/index.d.ts",
                        "import": "./dist/index.mjs",
                        "require": "./dist/index.cjs",
                        "default": "./dist/index.mjs"
                    }
                }
            }"#,
    );

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "lib".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("package exports should resolve package root imports");

    assert_eq!(
        resolved.source_id,
        "/workspace/node_modules/lib/dist/index.d.ts"
    );
    assert_eq!(resolved.resolution_kind, ResolutionKind::PackageExports);
    assert_eq!(resolved.provider_target, ProviderTarget::SourceFile);
    assert_eq!(resolved.provider_specifier, "lib");
    assert_eq!(resolved.provider_id, resolved.source_id);
}

#[test]
fn resolve_package_exports_distinguishes_import_and_require() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/lib/dist/feature.mjs",
        "/workspace/node_modules/lib/dist/feature.cjs",
    ]);
    reader.add_file(
        "/workspace/node_modules/lib/package.json",
        r#"{
                "exports": {
                    "./feature": {
                        "import": "./dist/feature.mjs",
                        "require": "./dist/feature.cjs",
                        "default": "./dist/feature.mjs"
                    }
                }
            }"#,
    );

    let esm = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "lib/feature".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("ESM import should resolve package exports");
    let require = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "lib/feature".to_string(),
                kind: ResolveRequestKind::RequireCall,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("require call should resolve package exports");

    assert_eq!(
        esm.source_id,
        "/workspace/node_modules/lib/dist/feature.mjs"
    );
    assert_eq!(
        require.source_id,
        "/workspace/node_modules/lib/dist/feature.cjs"
    );
    assert_eq!(esm.resolution_kind, ResolutionKind::PackageExports);
    assert_eq!(require.resolution_kind, ResolutionKind::PackageExports);
    assert_ne!(
        esm.source_id, require.source_id,
        "import and require must be able to choose different export conditions"
    );
}

#[test]
fn resolve_node_modules_prefers_typings_before_main() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/legacy/dist/index.d.ts",
        "/workspace/node_modules/legacy/dist/index.js",
    ]);
    reader.add_file(
        "/workspace/node_modules/legacy/package.json",
        r#"{
                "typings": "./dist/index.d.ts",
                "main": "./dist/index.js"
            }"#,
    );

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "legacy".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("legacy package resolution should prefer typings before main");

    assert_eq!(
        resolved.source_id,
        "/workspace/node_modules/legacy/dist/index.d.ts"
    );
    assert_eq!(resolved.resolution_kind, ResolutionKind::NodeModules);
    assert_eq!(resolved.provider_target, ProviderTarget::SourceFile);
    assert_eq!(resolved.provider_specifier, "legacy");
}

#[test]
fn resolve_node_modules_falls_back_to_main_without_type_entries() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&["/workspace/node_modules/legacy-main/dist/index.js"]);
    reader.add_file(
        "/workspace/node_modules/legacy-main/package.json",
        r#"{
                "main": "./dist/index.js"
            }"#,
    );

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "legacy-main".to_string(),
                kind: ResolveRequestKind::RequireCall,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("legacy package resolution should fall back to main when no types exist");

    assert_eq!(
        resolved.source_id,
        "/workspace/node_modules/legacy-main/dist/index.js"
    );
    assert_eq!(resolved.resolution_kind, ResolutionKind::NodeModules);
    assert_eq!(resolved.provider_target, ProviderTarget::SourceFile);
    assert_eq!(resolved.provider_specifier, "legacy-main");
}

// Verify backward compat type alias
#[test]
fn native_project_resolver_alias_works() {
    let resolver: NativeProjectResolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        None,
        ProjectMembership::MatchAll,
    )]);
    assert!(
        resolver.owner_for_file("/workspace/src/App.vue").is_some(),
        "NativeProjectResolver alias should be interchangeable with ProjectResolver"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Step 1: Owner-independent resolution tests
// ═══════════════════════════════════════════════════════════════════════════

/// Relative imports should resolve even when the importer has no owning project.
#[test]
fn resolve_relative_without_project_owner() {
    // Only project is /workspace — importer is /other/src/App.ts (unowned)
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&["/other/src/Foo.vue"]);

    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/other/src/App.ts".to_string(),
            specifier: "./Foo.vue".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    );

    let resolved = result.expect("relative import should resolve for unowned importer");
    assert_eq!(resolved.source_id, "/other/src/Foo.vue");
    assert_eq!(resolved.resolution_kind, ResolutionKind::Relative);
    assert!(
        !resolved.source_id.is_empty(),
        "source_id must not be empty"
    );
}

/// When an unowned importer resolves a relative path to a file owned by a project,
/// the result should carry the correct owner metadata (provider_id, provider_target, etc.).
#[test]
fn resolve_relative_unowned_to_owned_target() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            // Importer is outside /workspace — unowned
            importer_id: "/external/tool.ts".to_string(),
            specifier: "../workspace/src/Foo.vue".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    );

    let resolved = result.expect("unowned importer resolving to owned target should work");
    assert_eq!(resolved.source_id, "/workspace/src/Foo.vue");
    assert_eq!(
        resolved.provider_target,
        ProviderTarget::VuePublicApi,
        "Vue target owned by a project should get VuePublicApi"
    );
    assert!(
        resolved.provider_id.ends_with(".vue.ts"),
        "provider_id for owned Vue target should be .vue.ts: {}",
        resolved.provider_id
    );
    assert_eq!(
        resolved.owner_tsconfig_path.as_deref(),
        Some("/workspace/tsconfig.app.json"),
        "owner_tsconfig_path should come from the TARGET's project"
    );
}

/// Absolute path imports should resolve for unowned importers.
#[test]
fn resolve_absolute_without_project_owner() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/unowned/tool.ts".to_string(),
            specifier: "/workspace/src/Foo.vue".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    );

    let resolved = result.expect("absolute import should resolve for unowned importer");
    assert_eq!(resolved.source_id, "/workspace/src/Foo.vue");
    assert_eq!(resolved.resolution_kind, ResolutionKind::Relative);
}

/// Bare node_modules imports should resolve for unowned importers.
#[test]
fn resolve_node_modules_without_project_owner() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&["/unowned/node_modules/vue/dist/vue.d.ts"]);
    reader.add_file(
        "/unowned/node_modules/vue/package.json",
        r#"{ "types": "./dist/vue.d.ts" }"#,
    );

    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/unowned/src/App.ts".to_string(),
            specifier: "vue".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    );

    let resolved = result.expect("node_modules import should resolve for unowned importer");
    assert_eq!(
        resolved.source_id,
        "/unowned/node_modules/vue/dist/vue.d.ts"
    );
    assert_eq!(resolved.resolution_kind, ResolutionKind::NodeModules);
    assert!(
        !resolved.source_id.is_empty(),
        "source_id must not be empty"
    );
}

/// Package #imports should resolve for unowned importers.
#[test]
fn resolve_hash_import_without_project_owner() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&["/unowned/src/utils.ts"]);
    reader.add_file(
        "/unowned/package.json",
        r##"{ "imports": { "#app": "./src/utils.ts" } }"##,
    );

    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/unowned/src/main.ts".to_string(),
            specifier: "#app".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    );

    let resolved = result.expect("#imports should resolve for unowned importer");
    assert_eq!(resolved.source_id, "/unowned/src/utils.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::PackageImports);
}

/// Alias-based resolution (tsconfig paths) should NOT work for unowned importers.
#[test]
fn resolve_alias_requires_project_owner() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: None,
        paths: vec![("@/*".to_string(), vec!["/workspace/src/*".to_string()])],
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/unowned/tool.ts".to_string(),
            specifier: "@/Foo.vue".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    );

    assert!(
        result.is_none(),
        "alias resolution must NOT work for unowned importer — got: {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// preferred_specifier tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn preferred_specifier_returns_tsconfig_alias() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: None,
        paths: vec![("@/*".to_string(), vec!["/workspace/src/*".to_string()])],
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    let result =
        resolver.preferred_specifier(&reader, "/workspace/src/App.ts", "/workspace/src/Foo.vue");

    assert_eq!(
        result.as_deref(),
        Some("@/Foo.vue"),
        "should return tsconfig path alias"
    );
}

#[test]
fn preferred_specifier_returns_none_when_no_match() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: None,
        paths: vec![("@/*".to_string(), vec!["/workspace/src/*".to_string()])],
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/other/Foo.vue"]);

    let result = resolver.preferred_specifier(&reader, "/workspace/src/App.ts", "/other/Foo.vue");

    assert!(
        result.is_none(),
        "target outside all aliases should return None — got: {result:?}"
    );
}

#[test]
fn preferred_specifier_prefers_shortest() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: None,
        paths: vec![
            ("@/*".to_string(), vec!["/workspace/src/*".to_string()]),
            (
                "@components/*".to_string(),
                vec!["/workspace/src/components/*".to_string()],
            ),
        ],
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/components/Bar.vue"]);

    let result = resolver.preferred_specifier(
        &reader,
        "/workspace/src/App.ts",
        "/workspace/src/components/Bar.vue",
    );

    assert_eq!(
        result.as_deref(),
        Some("@components/Bar.vue"),
        "should prefer shorter (more specific) alias"
    );
}

#[test]
fn preferred_specifier_round_trips() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: None,
        paths: vec![("@/*".to_string(), vec!["/workspace/src/*".to_string()])],
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    let specifier = resolver
        .preferred_specifier(&reader, "/workspace/src/App.ts", "/workspace/src/Foo.vue")
        .expect("should find alias specifier");

    // Forward-resolve the specifier and verify it matches the original target
    let request = ResolveRequest {
        importer_id: "/workspace/src/App.ts".to_string(),
        specifier: specifier.clone(),
        kind: ResolveRequestKind::EsmImport,
        phase: ResolvePhase::ProviderGraph,
    };
    let forward = resolver
        .resolve_with_reader(&reader, &request)
        .expect("forward resolve of preferred specifier should succeed");

    assert_eq!(
        forward.source_id, "/workspace/src/Foo.vue",
        "round-trip: forward({specifier}) should resolve to original target"
    );
}

#[test]
fn preferred_specifier_none_for_provider_paths() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: None,
        paths: vec![("@/*".to_string(), vec!["/workspace/src/*".to_string()])],
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    // .vue.ts is a provider path, not a real file — should not match
    let result = resolver.preferred_specifier(
        &reader,
        "/workspace/src/App.ts",
        "/workspace/src/Foo.vue.ts",
    );

    assert!(
        result.is_none(),
        "provider paths (.vue.ts) should return None — got: {result:?}"
    );
}

#[test]
fn preferred_specifier_multi_target_first_wins() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    // "@/*" maps to both src/ and lib/, first target wins
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: None,
        paths: vec![(
            "@/*".to_string(),
            vec![
                "/workspace/src/*".to_string(),
                "/workspace/lib/*".to_string(),
            ],
        )],
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue", "/workspace/lib/Foo.vue"]);

    // Target in src/ — first target, should round-trip successfully
    let result =
        resolver.preferred_specifier(&reader, "/workspace/src/App.ts", "/workspace/src/Foo.vue");
    assert_eq!(
        result.as_deref(),
        Some("@/Foo.vue"),
        "target in first replacement should produce alias"
    );
}

#[test]
fn preferred_specifier_multi_target_shadowed() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    // "@/*" maps to src/ first, then lib/. Target in lib/ is shadowed.
    app_project.compiler_options = IdeProjectCompilerOptions {
        base_url: None,
        paths: vec![(
            "@/*".to_string(),
            vec![
                "/workspace/src/*".to_string(),
                "/workspace/lib/*".to_string(),
            ],
        )],
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    // Only lib/Bar.vue exists (NOT src/Bar.vue)
    let reader = TestReader::with_files(&["/workspace/lib/Bar.vue"]);

    // Target is lib/Bar.vue — @/Bar.vue forward-resolves to lib/Bar.vue
    // (src/Bar.vue doesn't exist, so TypeScript tries lib/ next)
    let result =
        resolver.preferred_specifier(&reader, "/workspace/src/App.ts", "/workspace/lib/Bar.vue");
    assert_eq!(
        result.as_deref(),
        Some("@/Bar.vue"),
        "when first target doesn't exist, second target should round-trip"
    );
}

#[test]
fn preferred_specifier_workspace_alias_fallback() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    // No tsconfig paths, but has a workspace alias
    app_project.workspace_aliases = vec![WorkspaceAlias {
        find: "~/".to_string(),
        replacement: "/workspace/src/".to_string(),
    }];
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    let result =
        resolver.preferred_specifier(&reader, "/workspace/src/App.ts", "/workspace/src/Foo.vue");

    assert_eq!(
        result.as_deref(),
        Some("~/Foo.vue"),
        "workspace alias should be used when no tsconfig paths match"
    );
}

/// Vite normalization stores find with trailing slash (`@/`) and replacement
/// WITHOUT trailing slash (`/workspace/src`). The reverse-alias must not
/// produce double-slash specifiers like `@//Foo.vue`.
#[test]
fn preferred_specifier_workspace_alias_no_double_slash() {
    let mut app_project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    );
    // Simulates Vite's normalize_alias_pair output:
    // find = "@/" (with trailing slash), replacement = "/workspace/src" (NO trailing slash)
    app_project.workspace_aliases = vec![WorkspaceAlias {
        find: "@/".to_string(),
        replacement: "/workspace/src".to_string(),
    }];
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    let result =
        resolver.preferred_specifier(&reader, "/workspace/src/App.ts", "/workspace/src/Foo.vue");

    let specifier = result.expect("should find workspace alias specifier");
    assert_eq!(
        specifier, "@/Foo.vue",
        "must NOT produce double-slash like @//Foo.vue"
    );
    assert!(
        !specifier.contains("//"),
        "specifier must not contain double-slash: {specifier}"
    );
}
