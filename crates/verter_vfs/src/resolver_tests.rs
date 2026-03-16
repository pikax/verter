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
