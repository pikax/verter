use super::*;
use crate::canonical_path::CanonicalPath;
use crate::membership::{FallbackMembership, StaticMembershipSpec};
use crate::normalized_glob::NormalizedGlob;
use crate::resolver::{IdeProjectCompilerOptions, ProjectMembership};
use crate::workspace_snapshot::{
    ConfiguredOwnerResolution, OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration,
};

// ── Helpers ──

fn make_configured(
    root: &str,
    tsconfig: &str,
    spec: StaticMembershipSpec,
    files: &[&str],
) -> OwnershipProject {
    let root_cp = CanonicalPath::new(root);
    let mut materialized = FxHashSet::default();
    for f in files {
        materialized.insert(CanonicalPath::new(f));
    }

    OwnershipProject {
        id: ProjectId(0), // re-assigned by builder
        root: root_cp.clone(),
        workspace_root: root_cp,
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(tsconfig),
            membership: ConfiguredMembership {
                spec,
                materialized_files: materialized,
            },
            compiler_options: IdeProjectCompilerOptions::default(),
            references: Vec::new(),
            workspace_aliases: Vec::new(),
        },
    }
}

fn make_fallback(root: &str) -> OwnershipProject {
    let root_cp = CanonicalPath::new(root);
    OwnershipProject {
        id: ProjectId(0),
        root: root_cp.clone(),
        workspace_root: root_cp.clone(),
        payload: ProjectPayload::Fallback {
            membership: FallbackMembership {
                root: root_cp.clone(),
                exclude: vec![NormalizedGlob::from_root_and_pattern(
                    &root_cp,
                    "node_modules/**",
                )],
            },
        },
    }
}

fn spec_with_files(files: &[&str]) -> StaticMembershipSpec {
    StaticMembershipSpec {
        files: files.iter().map(|s| CanonicalPath::new(s)).collect(),
        include: Vec::new(),
        exclude: Vec::new(),
    }
}

fn spec_with_include_exclude(include: &[&str], exclude: &[&str]) -> StaticMembershipSpec {
    StaticMembershipSpec {
        files: Vec::new(),
        include: include.iter().map(|s| NormalizedGlob::new(s)).collect(),
        exclude: exclude.iter().map(|s| NormalizedGlob::new(s)).collect(),
    }
}

fn default_spec(root: &str) -> StaticMembershipSpec {
    StaticMembershipSpec::with_typescript_defaults(&CanonicalPath::new(root))
}

// ── membership_to_spec conversion ──

#[test]
fn match_all_becomes_typescript_defaults() {
    let root = CanonicalPath::new("d:/project");
    let spec = membership_to_spec(&root, &ProjectMembership::MatchAll);

    // Should have default include pattern
    assert_eq!(spec.include.len(), 1, "should have 1 include pattern");
    assert!(
        spec.include[0].as_str().contains("**/*"),
        "include should be **/*"
    );

    // Should have default exclude patterns
    assert!(
        spec.exclude.len() >= 2,
        "should have at least node_modules + bower excludes"
    );
    assert!(
        spec.exclude
            .iter()
            .any(|e| e.as_str().contains("node_modules")),
        "should exclude node_modules"
    );
}

#[test]
fn include_exclude_converts_directly() {
    let root = CanonicalPath::new("d:/project");
    let spec = membership_to_spec(
        &root,
        &ProjectMembership::IncludeExclude {
            files: vec!["d:/project/src/main.ts".to_string()],
            include: vec!["d:/project/src/**/*".to_string()],
            exclude: vec!["d:/project/dist/**".to_string()],
        },
    );

    assert_eq!(spec.files.len(), 1);
    assert_eq!(spec.include.len(), 1);
    assert_eq!(spec.exclude.len(), 1);
}

#[test]
fn empty_exclude_fills_defaults() {
    let root = CanonicalPath::new("d:/project");
    let spec = membership_to_spec(
        &root,
        &ProjectMembership::IncludeExclude {
            files: vec![],
            include: vec!["d:/project/src/**/*".to_string()],
            exclude: vec![], // empty → should fill TS defaults
        },
    );

    assert!(
        spec.exclude.len() >= 2,
        "empty exclude should be filled with TS defaults"
    );
}

// ── build_workspace_snapshot_simple ──

#[test]
fn simple_build_sorts_by_precedence() {
    let snap = build_workspace_snapshot_simple(
        vec![
            make_fallback("d:/project"),
            make_configured(
                "d:/project",
                "d:/project/tsconfig.json",
                default_spec("d:/project"),
                &["d:/project/src/main.ts"],
            ),
        ],
        SnapshotGeneration(1),
    );

    // Configured should be before fallback
    assert!(snap.projects[0].is_configured());
    assert!(snap.projects[1].is_fallback());
}

#[test]
fn simple_build_reassigns_ids() {
    let snap = build_workspace_snapshot_simple(
        vec![
            make_fallback("d:/project"),
            make_configured(
                "d:/project",
                "d:/project/tsconfig.json",
                default_spec("d:/project"),
                &[],
            ),
        ],
        SnapshotGeneration(1),
    );

    // IDs should match indices after sorting
    for (i, p) in snap.projects.iter().enumerate() {
        assert_eq!(p.id, ProjectId(i as u32));
    }
}

#[test]
fn simple_build_creates_resolver() {
    let snap = build_workspace_snapshot_simple(
        vec![make_configured(
            "d:/project",
            "d:/project/tsconfig.json",
            default_spec("d:/project"),
            &["d:/project/src/main.ts"],
        )],
        SnapshotGeneration(1),
    );

    // Resolver should find owner for files under project root
    let owner = snap.resolver.owner_for_file("d:/project/src/main.ts");
    assert!(owner.is_some());
}

// ── Plan test matrix: Static membership ──

#[test]
fn files_only_no_implicit_include() {
    let spec = spec_with_files(&["d:/project/src/main.ts"]);
    let snap = build_workspace_snapshot_simple(
        vec![make_configured(
            "d:/project",
            "d:/project/tsconfig.json",
            spec,
            &["d:/project/src/main.ts"], // only this file materialized
        )],
        SnapshotGeneration(1),
    );

    // main.ts is owned
    let owners = snap.owners_for_file("d:/project/src/main.ts");
    assert_eq!(owners.len(), 1);

    // other.ts is NOT owned (no implicit include)
    let owners = snap.owners_for_file("d:/project/src/other.ts");
    assert!(owners.is_empty());
}

#[test]
fn files_immune_to_exclude_in_snapshot() {
    let spec = StaticMembershipSpec {
        files: vec![CanonicalPath::new("d:/project/src/main.ts")],
        include: vec![NormalizedGlob::new("d:/project/src/**/*")],
        exclude: vec![NormalizedGlob::new("d:/project/src/**/*")], // excludes everything
    };

    // But main.ts is in files, so it's immune
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/main.ts")));
}

#[test]
fn include_exclude_standard() {
    let spec =
        spec_with_include_exclude(&["d:/project/src/**/*"], &["d:/project/src/generated/**"]);

    assert!(spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")));
    assert!(!spec.matches(&CanonicalPath::new("d:/project/src/generated/types.ts")));
}

#[test]
fn solution_style_empty_files_empty_include() {
    // { "files": [], "references": [...] } → matches nothing
    let spec = StaticMembershipSpec {
        files: Vec::new(),
        include: Vec::new(),
        exclude: Vec::new(),
    };

    assert!(!spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")));
}

#[test]
fn no_filter_config_excludes_node_modules_via_defaults() {
    let spec = default_spec("d:/project");

    assert!(spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")));
    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/node_modules/vue/index.ts")),
        "default exclude should filter node_modules"
    );
}

// ── Plan test matrix: Multi-owner ──

#[test]
fn non_overlapping_tsconfigs_unique_owners() {
    let snap = build_workspace_snapshot_simple(
        vec![
            make_configured(
                "d:/project",
                "d:/project/tsconfig.app.json",
                spec_with_include_exclude(&["d:/project/src/**/*"], &[]),
                &["d:/project/src/main.ts", "d:/project/src/app.vue"],
            ),
            make_configured(
                "d:/project",
                "d:/project/tsconfig.vitest.json",
                spec_with_include_exclude(&["d:/project/tests/**/*"], &[]),
                &["d:/project/tests/foo.spec.ts"],
            ),
            make_fallback("d:/project"),
        ],
        SnapshotGeneration(1),
    );

    // src file → only tsconfig.app
    let res = snap.configured_owner_resolution_for_file("d:/project/src/main.ts");
    assert!(matches!(res, ConfiguredOwnerResolution::Unique(_)));

    // test file → only tsconfig.vitest
    let res = snap.configured_owner_resolution_for_file("d:/project/tests/foo.spec.ts");
    assert!(matches!(res, ConfiguredOwnerResolution::Unique(_)));
}

#[test]
fn overlapping_tsconfigs_returns_ambiguous() {
    let snap = build_workspace_snapshot_simple(
        vec![
            make_configured(
                "d:/project",
                "d:/project/tsconfig.app.json",
                default_spec("d:/project"),
                &["d:/project/src/shared.ts"],
            ),
            make_configured(
                "d:/project",
                "d:/project/tsconfig.vitest.json",
                default_spec("d:/project"),
                &["d:/project/src/shared.ts"],
            ),
            make_fallback("d:/project"),
        ],
        SnapshotGeneration(1),
    );

    let res = snap.configured_owner_resolution_for_file("d:/project/src/shared.ts");
    match res {
        ConfiguredOwnerResolution::Ambiguous(ids) => {
            assert_eq!(ids.len(), 2, "both configured projects should claim");
        }
        other => panic!("expected Ambiguous, got {:?}", other),
    }
}

// ── Plan test matrix: Fallback ──

#[test]
fn fallback_claims_when_excluded_by_configured() {
    let snap = build_workspace_snapshot_simple(
        vec![
            make_configured(
                "d:/project",
                "d:/project/tsconfig.json",
                spec_with_include_exclude(&["d:/project/src/**/*"], &[]),
                &["d:/project/src/main.ts"], // scripts/ not materialized
            ),
            make_fallback("d:/project"),
        ],
        SnapshotGeneration(1),
    );

    // scripts/build.ts not in configured → fallback claims
    let owners = snap.owners_for_file("d:/project/scripts/build.ts");
    assert_eq!(owners.len(), 1);
    let project = snap.project(owners[0]);
    assert!(project.is_fallback());
}

#[test]
fn fallback_no_tsconfig_no_aliases_no_configured_settings() {
    let snap =
        build_workspace_snapshot_simple(vec![make_fallback("d:/project")], SnapshotGeneration(1));

    let owners = snap.owners_for_file("d:/project/src/foo.ts");
    assert_eq!(owners.len(), 1);

    let project = snap.project(owners[0]);
    assert!(project.is_fallback());
    assert!(snap.tsconfig_path(owners[0]).is_none());
}

// ── spec_to_membership round-trip ──

#[test]
fn spec_round_trips_through_membership() {
    let original = StaticMembershipSpec {
        files: vec![CanonicalPath::new("d:/project/src/main.ts")],
        include: vec![NormalizedGlob::new("d:/project/src/**/*")],
        exclude: vec![NormalizedGlob::new("d:/project/dist/**")],
    };

    let membership = spec_to_membership(&original);
    let root = CanonicalPath::new("d:/project");
    let back = membership_to_spec(&root, &membership);

    assert_eq!(back.files.len(), original.files.len());
    assert_eq!(back.include.len(), original.include.len());
    // Note: exclude may differ because empty exclude fills defaults
    // In this case exclude is non-empty so it should round-trip
    assert_eq!(back.exclude.len(), original.exclude.len());
}

// ── materialize_from_spec ──

#[test]
fn materialize_includes_files_entries() {
    let root = CanonicalPath::new("d:/project");
    let spec = StaticMembershipSpec {
        files: vec![
            CanonicalPath::new("d:/project/src/main.ts"),
            CanonicalPath::new("d:/project/src/app.vue"),
        ],
        include: Vec::new(),
        exclude: Vec::new(),
    };

    let materialized = materialize_from_spec(&spec, &root, None);
    assert_eq!(materialized.len(), 2);
    assert!(materialized.contains(&CanonicalPath::new("d:/project/src/main.ts")));
    assert!(materialized.contains(&CanonicalPath::new("d:/project/src/app.vue")));
}

#[test]
fn materialize_empty_spec_produces_empty_set() {
    let root = CanonicalPath::new("d:/project");
    let spec = StaticMembershipSpec {
        files: Vec::new(),
        include: Vec::new(),
        exclude: Vec::new(),
    };

    let materialized = materialize_from_spec(&spec, &root, None);
    assert!(materialized.is_empty());
}

// ── materialize_from_spec with filesystem walking ──

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn materialize_walks_filesystem_and_finds_matching_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/main.ts"), "").unwrap();
    std::fs::write(project.join("src/app.vue"), "").unwrap();
    std::fs::write(project.join("src/utils.ts"), "").unwrap();

    let project_str = project.to_string_lossy().replace('\\', "/");
    let root = CanonicalPath::new(&project_str);
    let spec = StaticMembershipSpec::with_typescript_defaults(&root);

    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );

    let materialized = materialize_from_spec(&spec, &root, Some(&ws));

    // Positive: all 3 files should be found
    assert!(
        materialized.len() >= 3,
        "should find at least 3 files, got {}",
        materialized.len()
    );
    let main_path = CanonicalPath::new(&format!("{}/src/main.ts", project_str));
    let app_path = CanonicalPath::new(&format!("{}/src/app.vue", project_str));
    let utils_path = CanonicalPath::new(&format!("{}/src/utils.ts", project_str));
    assert!(materialized.contains(&main_path), "should contain main.ts");
    assert!(materialized.contains(&app_path), "should contain app.vue");
    assert!(
        materialized.contains(&utils_path),
        "should contain utils.ts"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn materialize_excludes_node_modules_via_default_excludes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join("node_modules/vue")).unwrap();
    std::fs::write(project.join("src/main.ts"), "").unwrap();
    std::fs::write(project.join("node_modules/vue/index.ts"), "").unwrap();

    let project_str = project.to_string_lossy().replace('\\', "/");
    let root = CanonicalPath::new(&project_str);
    let spec = StaticMembershipSpec::with_typescript_defaults(&root);

    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );

    let materialized = materialize_from_spec(&spec, &root, Some(&ws));

    // Positive: src/main.ts found
    let main_path = CanonicalPath::new(&format!("{}/src/main.ts", project_str));
    assert!(
        materialized.contains(&main_path),
        "should contain src/main.ts"
    );

    // Negative: node_modules file excluded
    let nm_path = CanonicalPath::new(&format!("{}/node_modules/vue/index.ts", project_str));
    assert!(
        !materialized.contains(&nm_path),
        "node_modules files must NOT be materialized"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn materialize_files_entries_always_included_regardless_of_excludes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join("dist")).unwrap();
    std::fs::write(project.join("src/main.ts"), "").unwrap();
    std::fs::write(project.join("dist/output.js"), "").unwrap();

    let project_str = project.to_string_lossy().replace('\\', "/");
    let root = CanonicalPath::new(&project_str);
    let dist_file = CanonicalPath::new(&format!("{}/dist/output.js", project_str));

    // The files entry points to dist/output.js, but include only covers src/
    // and exclude covers dist/. Files entries are immune to exclude.
    let spec = StaticMembershipSpec {
        files: vec![dist_file.clone()],
        include: vec![NormalizedGlob::from_root_and_pattern(&root, "src/**/*")],
        exclude: vec![NormalizedGlob::from_root_and_pattern(&root, "dist/**")],
    };

    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );

    let materialized = materialize_from_spec(&spec, &root, Some(&ws));

    // Positive: files entry always materialized
    assert!(
        materialized.contains(&dist_file),
        "files entries must always be materialized even if excluded"
    );

    // Positive: src/main.ts matched by include
    let main_path = CanonicalPath::new(&format!("{}/src/main.ts", project_str));
    assert!(
        materialized.contains(&main_path),
        "included files should be materialized"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn materialize_empty_include_produces_files_only() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/main.ts"), "").unwrap();
    std::fs::write(project.join("src/other.ts"), "").unwrap();

    let project_str = project.to_string_lossy().replace('\\', "/");
    let root = CanonicalPath::new(&project_str);
    let main_file = CanonicalPath::new(&format!("{}/src/main.ts", project_str));

    // Only files, no include → no walking, just the files entries
    let spec = StaticMembershipSpec {
        files: vec![main_file.clone()],
        include: Vec::new(),
        exclude: Vec::new(),
    };

    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );

    let materialized = materialize_from_spec(&spec, &root, Some(&ws));

    // Positive: files entry present
    assert!(
        materialized.contains(&main_file),
        "files entry should be materialized"
    );

    // Negative: other.ts NOT present (no include patterns, so no walking)
    let other_path = CanonicalPath::new(&format!("{}/src/other.ts", project_str));
    assert!(
        !materialized.contains(&other_path),
        "without include patterns, walk should not discover additional files"
    );

    assert_eq!(
        materialized.len(),
        1,
        "only files entries should be materialized"
    );
}

#[test]
fn materialize_none_workspace_falls_back_to_files_only() {
    let root = CanonicalPath::new("d:/project");
    let file = CanonicalPath::new("d:/project/src/main.ts");

    let spec = StaticMembershipSpec {
        files: vec![file.clone()],
        include: vec![NormalizedGlob::from_root_and_pattern(&root, "src/**/*")],
        exclude: Vec::new(),
    };

    // None workspace → no walking, just files entries
    let materialized = materialize_from_spec(&spec, &root, None);

    // Positive: files entry present
    assert!(
        materialized.contains(&file),
        "files entry should be materialized"
    );

    // Negative: only files entries (no walking without workspace)
    assert_eq!(
        materialized.len(),
        1,
        "without workspace, only files entries should be materialized"
    );
}

// ── build_workspace_snapshot with filesystem ──

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn build_from_workspace_roots_discovers_tsconfigs() {
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
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );

    let result = build_workspace_snapshot(
        &ws,
        std::slice::from_ref(&workspace_str),
        SnapshotGeneration(1),
        &vite_opts,
    );

    // Should have configured + fallback
    assert!(
        result.snapshot.projects.len() >= 2,
        "should have configured + fallback projects, got {}",
        result.snapshot.projects.len()
    );

    // At least one configured project
    let has_configured = result.snapshot.projects.iter().any(|p| p.is_configured());
    assert!(
        has_configured,
        "should have at least one configured project"
    );

    // At least one fallback
    let has_fallback = result.snapshot.projects.iter().any(|p| p.is_fallback());
    assert!(has_fallback, "should have a fallback project");

    // Resolver should work
    let owner = result
        .snapshot
        .resolver
        .owner_for_file(&format!("{}/src/foo.ts", workspace_str));
    assert!(owner.is_some(), "resolver should find owner");

    // No trust required
    assert!(result.trust_required.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn build_workspace_snapshot_materializes_files_from_disk() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).unwrap();
    std::fs::create_dir_all(workspace.join("node_modules/vue")).unwrap();
    std::fs::write(workspace.join("src/main.ts"), "").unwrap();
    std::fs::write(workspace.join("src/app.vue"), "").unwrap();
    std::fs::write(workspace.join("node_modules/vue/index.ts"), "").unwrap();

    // Default tsconfig (no files/include/exclude) → TypeScript defaults
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": {} }"#,
    )
    .unwrap();

    let workspace_str = workspace.to_string_lossy().replace('\\', "/");
    let vite_opts = crate::vite_config::ViteConfigOptions {
        enabled: false,
        ..Default::default()
    };
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );

    let result = build_workspace_snapshot(
        &ws,
        std::slice::from_ref(&workspace_str),
        SnapshotGeneration(1),
        &vite_opts,
    );

    // Find the configured project
    let configured = result
        .snapshot
        .projects
        .iter()
        .find(|p| p.is_configured())
        .expect("should have a configured project");

    match &configured.payload {
        ProjectPayload::Configured { membership, .. } => {
            // Positive: materialized_files should be non-empty (walked from disk)
            assert!(
                !membership.materialized_files.is_empty(),
                "materialized_files should be populated by filesystem walk"
            );

            // Positive: src/main.ts should be materialized
            let main_path = CanonicalPath::new(&format!("{}/src/main.ts", workspace_str));
            assert!(
                membership.materialized_files.contains(&main_path),
                "src/main.ts should be materialized"
            );

            // Positive: src/app.vue should be materialized
            let app_path = CanonicalPath::new(&format!("{}/src/app.vue", workspace_str));
            assert!(
                membership.materialized_files.contains(&app_path),
                "src/app.vue should be materialized"
            );

            // Negative: node_modules file should NOT be materialized
            let nm_path =
                CanonicalPath::new(&format!("{}/node_modules/vue/index.ts", workspace_str));
            assert!(
                !membership.materialized_files.contains(&nm_path),
                "node_modules files must NOT be materialized"
            );
        }
        _ => panic!("expected configured project"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn build_from_workspace_roots_empty_roots() {
    let vite_opts = crate::vite_config::ViteConfigOptions::default();
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );

    let result = build_workspace_snapshot(&ws, &[], SnapshotGeneration(1), &vite_opts);
    assert!(result.snapshot.projects.is_empty());
    assert!(result.trust_required.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn build_from_workspace_roots_multiple_tsconfigs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).unwrap();
    std::fs::create_dir_all(workspace.join("tests")).unwrap();

    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "files": [], "references": [{ "path": "./tsconfig.app.json" }, { "path": "./tsconfig.vitest.json" }] }"#,
    )
    .unwrap();
    std::fs::write(
        workspace.join("tsconfig.app.json"),
        r#"{ "include": ["src/**/*"] }"#,
    )
    .unwrap();
    std::fs::write(
        workspace.join("tsconfig.vitest.json"),
        r#"{ "include": ["tests/**/*"] }"#,
    )
    .unwrap();

    let workspace_str = workspace.to_string_lossy().replace('\\', "/");
    let vite_opts = crate::vite_config::ViteConfigOptions {
        enabled: false,
        ..Default::default()
    };
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );

    let result = build_workspace_snapshot(&ws, &[workspace_str], SnapshotGeneration(1), &vite_opts);

    // Should have 3 configured + 1 fallback = 4 projects
    let configured_count = result
        .snapshot
        .projects
        .iter()
        .filter(|p| p.is_configured())
        .count();
    assert!(
        configured_count >= 3,
        "should discover all 3 tsconfigs, got {}",
        configured_count
    );
}

// ── ownership_project_from_vfs_config bridge ──

#[test]
fn bridge_configured_project_from_vfs_config() {
    use crate::project_graph::{ProjectRank, VfsProjectConfig};

    let config = VfsProjectConfig {
        root: "d:/project".to_string(),
        rank: ProjectRank::Discovered,
        tsconfig_path: Some("d:/project/tsconfig.json".to_string()),
        root_files: vec![],
        extensions: vec![],
        workspace_root: "d:/project".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec!["d:/project/tsconfig.app.json".to_string()],
        membership: ProjectMembership::MatchAll,
    };

    let project = ownership_project_from_vfs_config(&config, ProjectId(0));

    assert!(project.is_configured(), "should be configured");
    assert_eq!(project.root.as_str(), "d:/project");
    match &project.payload {
        ProjectPayload::Configured {
            tsconfig_path,
            references,
            ..
        } => {
            assert_eq!(tsconfig_path.as_str(), "d:/project/tsconfig.json");
            assert_eq!(references.len(), 1);
        }
        _ => panic!("expected Configured payload"),
    }
}

#[test]
fn bridge_fallback_project_from_vfs_config() {
    use crate::project_graph::{ProjectRank, VfsProjectConfig};

    let config = VfsProjectConfig {
        root: "d:/project".to_string(),
        rank: ProjectRank::Inferred,
        tsconfig_path: None, // no tsconfig → fallback
        root_files: vec![],
        extensions: vec![],
        workspace_root: "d:/project".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    };

    let project = ownership_project_from_vfs_config(&config, ProjectId(0));

    assert!(project.is_fallback(), "should be fallback");
    assert_eq!(project.root.as_str(), "d:/project");
    // Fallback should NOT have tsconfig
    match &project.payload {
        ProjectPayload::Fallback { membership } => {
            assert!(
                membership.contains(&CanonicalPath::new("d:/project/src/foo.ts")),
                "fallback should match files under root"
            );
            assert!(
                !membership.contains(&CanonicalPath::new("d:/project/node_modules/vue/index.ts")),
                "fallback should exclude node_modules"
            );
        }
        _ => panic!("expected Fallback payload"),
    }
}
