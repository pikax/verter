use super::*;

fn make_project(root: &str, rank: ProjectRank, tsconfig: Option<&str>) -> VfsProjectConfig {
    VfsProjectConfig {
        root: root.to_string(),
        rank,
        tsconfig_path: tsconfig.map(|s| s.to_string()),
        root_files: vec![],
        extensions: vec![".vue".to_string(), ".ts".to_string()],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }
}

fn make_project_with_files(
    root: &str,
    rank: ProjectRank,
    tsconfig: Option<&str>,
    files: Vec<&str>,
) -> VfsProjectConfig {
    VfsProjectConfig {
        root: root.to_string(),
        rank,
        tsconfig_path: tsconfig.map(|s| s.to_string()),
        root_files: files.iter().map(|s| s.to_string()).collect(),
        extensions: vec![".vue".to_string(), ".ts".to_string()],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }
}

// ── Precedence tests ──

#[test]
fn explicit_beats_discovered() {
    let graph = ProjectGraph::from_configs(vec![
        make_project(
            "d:/project",
            ProjectRank::Discovered,
            Some("d:/project/tsconfig.json"),
        ),
        make_project("d:/project", ProjectRank::Explicit, None),
    ]);

    let owner = graph.owner_for_file("d:/project/src/foo.vue").unwrap();
    assert!(
        owner.tsconfig_path.is_none(),
        "explicit project (no tsconfig) should win over discovered"
    );
}

#[test]
fn discovered_beats_inferred() {
    let graph = ProjectGraph::from_configs(vec![
        make_project("d:/project", ProjectRank::Inferred, None),
        make_project(
            "d:/project",
            ProjectRank::Discovered,
            Some("d:/project/tsconfig.json"),
        ),
    ]);

    let owner = graph.owner_for_file("d:/project/src/foo.vue").unwrap();
    assert_eq!(
        owner.tsconfig_path.as_deref(),
        Some("d:/project/tsconfig.json"),
        "discovered should win over inferred"
    );
}

#[test]
fn explicit_beats_inferred() {
    let graph = ProjectGraph::from_configs(vec![
        make_project("d:/project", ProjectRank::Inferred, None),
        make_project(
            "d:/project",
            ProjectRank::Explicit,
            Some("d:/project/custom.json"),
        ),
    ]);

    let owner = graph.owner_for_file("d:/project/src/foo.vue").unwrap();
    assert_eq!(
        owner.tsconfig_path.as_deref(),
        Some("d:/project/custom.json"),
        "explicit should win over inferred"
    );
}

#[test]
fn longest_root_wins_within_same_rank() {
    let graph = ProjectGraph::from_configs(vec![
        make_project(
            "d:/project",
            ProjectRank::Discovered,
            Some("d:/project/tsconfig.json"),
        ),
        make_project(
            "d:/project/packages/ui",
            ProjectRank::Discovered,
            Some("d:/project/packages/ui/tsconfig.json"),
        ),
    ]);

    // File under packages/ui should match the longer root
    let owner = graph
        .owner_for_file("d:/project/packages/ui/src/button.vue")
        .unwrap();
    assert_eq!(owner.project_root, "d:/project/packages/ui");

    // File directly under project root should match the shorter root
    let owner = graph.owner_for_file("d:/project/src/app.vue").unwrap();
    assert_eq!(owner.project_root, "d:/project");
}

#[test]
fn explicit_short_root_beats_discovered_long_root() {
    let graph = ProjectGraph::from_configs(vec![
        make_project(
            "d:/project/packages/ui",
            ProjectRank::Discovered,
            Some("d:/project/packages/ui/tsconfig.json"),
        ),
        make_project(
            "d:/project",
            ProjectRank::Explicit,
            Some("d:/project/explicit.json"),
        ),
    ]);

    // Explicit at short root beats discovered at long root
    let owner = graph
        .owner_for_file("d:/project/packages/ui/src/button.vue")
        .unwrap();
    assert_eq!(
        owner.project_root, "d:/project",
        "explicit project at shorter root should win over discovered at longer root"
    );
}

#[test]
fn file_outside_all_roots_returns_none() {
    let graph = ProjectGraph::from_configs(vec![make_project(
        "d:/project/packages/a",
        ProjectRank::Discovered,
        None,
    )]);

    assert!(
        graph.owner_for_file("d:/other/src/foo.vue").is_none(),
        "file outside all roots should have no owner"
    );
}

#[test]
fn case_insensitive_matching() {
    let graph = ProjectGraph::from_configs(vec![make_project(
        "D:/Project",
        ProjectRank::Discovered,
        Some("D:/Project/tsconfig.json"),
    )]);

    let owner = graph.owner_for_file("d:/project/src/foo.vue");
    assert!(owner.is_some(), "case-insensitive matching should work");
}

#[test]
fn backslash_normalization() {
    let graph = ProjectGraph::from_configs(vec![make_project(
        "d:/project",
        ProjectRank::Discovered,
        Some("d:/project/tsconfig.json"),
    )]);

    let owner = graph.owner_for_file("d:\\project\\src\\foo.vue");
    assert!(owner.is_some(), "backslash paths should be normalized");
}

// ── Root file listing ──

#[test]
fn list_root_files_filters_by_extension() {
    let graph = ProjectGraph::from_configs(vec![make_project_with_files(
        "d:/project",
        ProjectRank::Discovered,
        None,
        vec![
            "d:/project/src/app.vue",
            "d:/project/src/main.ts",
            "d:/project/src/style.css",
        ],
    )]);

    let vue_files = graph.list_root_files(&[".vue"]);
    assert_eq!(vue_files, vec!["d:/project/src/app.vue"]);

    let ts_files = graph.list_root_files(&[".ts"]);
    assert_eq!(ts_files, vec!["d:/project/src/main.ts"]);

    let all_source = graph.list_root_files(&[".vue", ".ts"]);
    assert_eq!(
        all_source,
        vec!["d:/project/src/app.vue", "d:/project/src/main.ts"]
    );
}

#[test]
fn list_root_files_across_projects() {
    let graph = ProjectGraph::from_configs(vec![
        make_project_with_files(
            "d:/project/packages/a",
            ProjectRank::Discovered,
            None,
            vec!["d:/project/packages/a/src/a.vue"],
        ),
        make_project_with_files(
            "d:/project/packages/b",
            ProjectRank::Discovered,
            None,
            vec!["d:/project/packages/b/src/b.vue"],
        ),
    ]);

    let files = graph.list_root_files(&[".vue"]);
    assert_eq!(files.len(), 2);
    assert!(files.contains(&"d:/project/packages/a/src/a.vue".to_string()));
    assert!(files.contains(&"d:/project/packages/b/src/b.vue".to_string()));
}

// ── Graph properties ──

#[test]
fn empty_graph() {
    let graph = ProjectGraph::new();
    assert!(graph.is_empty());
    assert_eq!(graph.len(), 0);
    assert_eq!(graph.generation(), 0);
    assert!(graph.owner_for_file("d:/anything.vue").is_none());
}

#[test]
fn from_configs_sets_generation() {
    let graph = ProjectGraph::from_configs(vec![make_project(
        "d:/project",
        ProjectRank::Discovered,
        None,
    )]);
    assert_eq!(graph.generation(), 1);
    assert_eq!(graph.len(), 1);
}

#[test]
fn increment_generation() {
    let mut graph = ProjectGraph::from_configs(vec![]);
    assert_eq!(graph.generation(), 1);
    graph.increment_generation();
    assert_eq!(graph.generation(), 2);
}

#[test]
fn root_boundary_not_prefix_match() {
    let graph = ProjectGraph::from_configs(vec![make_project(
        "d:/project",
        ProjectRank::Discovered,
        None,
    )]);

    // "d:/project-extra/foo.vue" should NOT match "d:/project" — it's not under the root
    assert!(
        graph.owner_for_file("d:/project-extra/foo.vue").is_none(),
        "prefix match should require directory boundary"
    );
}

// ── VfsProjectConfig conversion ──

#[test]
fn to_ide_project_config_preserves_fields() {
    let config = VfsProjectConfig {
        root: "d:/project".to_string(),
        rank: ProjectRank::Discovered,
        tsconfig_path: Some("d:/project/tsconfig.json".to_string()),
        root_files: vec![],
        extensions: vec![],
        workspace_root: "d:/workspace".to_string(),
        workspace_aliases: vec![WorkspaceAlias {
            find: "@/".to_string(),
            replacement: "d:/project/src".to_string(),
        }],
        compiler_options: IdeProjectCompilerOptions {
            base_url: Some("d:/project".to_string()),
            paths: vec![("@/*".to_string(), vec!["d:/project/src/*".to_string()])],
        },
        references: vec!["d:/project/tsconfig.app.json".to_string()],
        membership: ProjectMembership::IncludeExclude {
            files: vec![],
            include: vec!["d:/project/src/**/*".to_string()],
            exclude: vec!["d:/project/node_modules/**/*".to_string()],
        },
    };

    let ide = config.to_ide_project_config();
    assert_eq!(ide.root, "d:/project");
    assert_eq!(ide.workspace_root, "d:/workspace");
    assert_eq!(
        ide.tsconfig_path.as_deref(),
        Some("d:/project/tsconfig.json")
    );
    assert_eq!(ide.workspace_aliases.len(), 1);
    assert_eq!(ide.workspace_aliases[0].find, "@/");
    assert_eq!(ide.compiler_options.base_url.as_deref(), Some("d:/project"));
    assert_eq!(ide.compiler_options.paths.len(), 1);
    assert_eq!(ide.references.len(), 1);
    assert!(matches!(
        ide.membership,
        ProjectMembership::IncludeExclude { .. }
    ));
}

#[test]
fn to_project_resolver_creates_from_graph() {
    let graph = ProjectGraph::from_configs(vec![make_project(
        "d:/project",
        ProjectRank::Discovered,
        Some("d:/project/tsconfig.json"),
    )]);

    let resolver = graph.to_project_resolver();
    let owner = resolver.owner_for_file("d:/project/src/foo.vue");
    assert!(
        owner.is_some(),
        "resolver should find owner for file under project root"
    );
    assert_eq!(owner.unwrap().root, "d:/project");
}

// ── from_workspace_roots ──

#[test]
fn from_workspace_roots_discovers_tsconfigs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).unwrap();
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["./src/*"] } } }"#,
    )
    .unwrap();

    let workspace_str = workspace.to_string_lossy().replace('\\', "/");
    let vite_opts = crate::vite_config::ViteConfigOptions {
        enabled: false,
        ..Default::default()
    };

    let result = ProjectGraph::from_workspace_roots(&[workspace_str.clone()], &vite_opts);

    // Should have at least 2 projects: discovered + inferred fallback
    assert!(
        result.graph.len() >= 2,
        "should have discovered + inferred projects, got {}",
        result.graph.len()
    );

    // First project (by precedence) should be the discovered one
    let discovered = result
        .graph
        .iter()
        .find(|p| p.rank == ProjectRank::Discovered);
    assert!(discovered.is_some(), "should have a discovered project");
    let discovered = discovered.unwrap();
    assert!(
        discovered.tsconfig_path.is_some(),
        "discovered project should have tsconfig_path"
    );
    assert!(
        !discovered.compiler_options.paths.is_empty(),
        "discovered project should have paths from tsconfig"
    );

    // Should also have an inferred fallback
    let inferred = result
        .graph
        .iter()
        .find(|p| p.rank == ProjectRank::Inferred);
    assert!(
        inferred.is_some(),
        "should have an inferred fallback project"
    );
    assert!(
        inferred.unwrap().tsconfig_path.is_none(),
        "inferred project should not have tsconfig"
    );

    // No trust required since we disabled vite
    assert!(
        result.trust_required.is_empty(),
        "should have no trust_required when vite is disabled"
    );
}

#[test]
fn from_workspace_roots_vite_fallback() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).unwrap();

    // No tsconfig, but has vite config with static alias
    std::fs::write(
        workspace.join("vite.config.ts"),
        "export default { resolve: { alias: { '@': './src' } } }",
    )
    .unwrap();

    let workspace_str = workspace.to_string_lossy().replace('\\', "/");
    let vite_opts = crate::vite_config::ViteConfigOptions {
        enabled: true,
        ..Default::default()
    };

    let result = ProjectGraph::from_workspace_roots(&[workspace_str.clone()], &vite_opts);

    // Should have 1 inferred project (no tsconfigs found)
    let inferred = result
        .graph
        .iter()
        .find(|p| p.rank == ProjectRank::Inferred);
    assert!(inferred.is_some(), "should have inferred project");
    let inferred = inferred.unwrap();

    // Inferred project should have vite aliases
    assert!(
        !inferred.workspace_aliases.is_empty(),
        "inferred project should have vite aliases"
    );
    assert_eq!(
        inferred.workspace_aliases[0].find, "@/",
        "alias find should be @/"
    );

    // No discovered projects (no tsconfigs)
    assert!(
        result
            .graph
            .iter()
            .all(|p| p.rank != ProjectRank::Discovered),
        "should have no discovered projects"
    );
}

#[test]
fn from_workspace_roots_complex_vite_requires_trust() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).unwrap();

    // Complex vite config (function export)
    std::fs::write(
        workspace.join("vite.config.ts"),
        r#"import { defineConfig } from 'vite'
export default defineConfig(({ mode }) => ({
  resolve: { alias: { '@': './src' } }
}))"#,
    )
    .unwrap();

    let workspace_str = workspace.to_string_lossy().replace('\\', "/");
    let vite_opts = crate::vite_config::ViteConfigOptions {
        enabled: true,
        ..Default::default()
    };

    let result = ProjectGraph::from_workspace_roots(&[workspace_str.clone()], &vite_opts);

    // Should have 1 trust_required entry
    assert_eq!(
        result.trust_required.len(),
        1,
        "should require trust for complex config"
    );
    assert!(
        result.trust_required[0].reason.contains("function")
            || result.trust_required[0].reason.contains("arrow"),
        "reason should mention function/arrow"
    );

    // Inferred project should have NO aliases (complex config not resolved)
    let inferred = result
        .graph
        .iter()
        .find(|p| p.rank == ProjectRank::Inferred);
    assert!(inferred.is_some());
    assert!(
        inferred.unwrap().workspace_aliases.is_empty(),
        "complex config should not produce aliases without trust"
    );
}

#[test]
fn from_workspace_roots_tsconfig_backed_skips_vite() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).unwrap();

    // Both tsconfig and vite config
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["./src/*"] } } }"#,
    )
    .unwrap();
    std::fs::write(
        workspace.join("vite.config.ts"),
        "export default { resolve: { alias: { '~': './lib' } } }",
    )
    .unwrap();

    let workspace_str = workspace.to_string_lossy().replace('\\', "/");
    let vite_opts = crate::vite_config::ViteConfigOptions {
        enabled: true,
        ..Default::default()
    };

    let result = ProjectGraph::from_workspace_roots(&[workspace_str.clone()], &vite_opts);

    // Tsconfig-backed project should NOT have vite aliases
    let discovered = result
        .graph
        .iter()
        .find(|p| p.rank == ProjectRank::Discovered);
    assert!(discovered.is_some());
    assert!(
        discovered.unwrap().workspace_aliases.is_empty(),
        "tsconfig-backed project should not have vite aliases"
    );

    // Inferred fallback should also NOT have vite aliases (has_tsconfigs is true)
    let inferred = result
        .graph
        .iter()
        .find(|p| p.rank == ProjectRank::Inferred);
    assert!(inferred.is_some());
    assert!(
        inferred.unwrap().workspace_aliases.is_empty(),
        "inferred fallback should not have vite aliases when tsconfigs exist"
    );
}

#[test]
fn from_workspace_roots_empty_roots() {
    let vite_opts = crate::vite_config::ViteConfigOptions::default();
    let result = ProjectGraph::from_workspace_roots(&[], &vite_opts);
    assert!(
        result.graph.is_empty(),
        "empty roots should produce empty graph"
    );
    assert!(result.trust_required.is_empty());
}
