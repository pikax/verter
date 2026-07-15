use super::*;
use crate::canonical_path::CanonicalPath;
use crate::membership::{ConfiguredMembership, FallbackMembership, StaticMembershipSpec};
use crate::normalized_glob::{CompiledGlob, NormalizedGlob};
use crate::resolver::{IdeProjectCompilerOptions, ProjectMembership};
use crate::workspace_snapshot::{
    ConfiguredOwnerResolution, OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration,
};

// ── Helpers ──

fn compiled(raw: &str) -> CompiledGlob {
    CompiledGlob::new(NormalizedGlob::new(raw))
}

fn compiled_from_root(root: &CanonicalPath, pattern: &str) -> CompiledGlob {
    CompiledGlob::new(NormalizedGlob::from_root_and_pattern(root, pattern))
}

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
                exclude: vec![compiled_from_root(&root_cp, "node_modules/**")].into(),
            },
        },
    }
}

fn spec_with_files(files: &[&str]) -> StaticMembershipSpec {
    StaticMembershipSpec {
        files: files.iter().map(|s| CanonicalPath::new(s)).collect(),
        include: Vec::new(),
        exclude: Vec::new().into(),
    }
}

fn spec_with_include_exclude(include: &[&str], exclude: &[&str]) -> StaticMembershipSpec {
    StaticMembershipSpec {
        files: Vec::new(),
        include: include.iter().map(|s| compiled(s)).collect(),
        exclude: exclude.iter().map(|s| compiled(s)).collect(),
    }
}

fn default_spec(root: &str) -> StaticMembershipSpec {
    StaticMembershipSpec::with_typescript_defaults(&CanonicalPath::new(root))
}

/// The supported-extension set the production builder uses (carrier extensions
/// from the live registry, JS off). Used to drive `membership_to_spec` through
/// the real expansion path.
fn test_supported() -> crate::membership::SupportedExtensions {
    supported_extensions_for(&IdeProjectCompilerOptions::default())
}

// ── membership_to_spec conversion ──

#[test]
fn match_all_becomes_typescript_defaults() {
    let root = CanonicalPath::new("d:/project");
    let spec = membership_to_spec(&root, &ProjectMembership::MatchAll, &test_supported());

    // The default `**/*` include expands into one glob per supported extension
    // (the explicit-extension model) — never zero, all carrying the `**/*` body.
    assert!(
        !spec.include.is_empty(),
        "default include must expand to at least one supported-extension glob"
    );
    assert!(
        spec.include.iter().all(|g| g.as_str().contains("**/*")),
        "every expanded default include keeps the `**/*` body"
    );
    // A `.ts` and every carrier extension are covered; an unknown extension is not.
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")));
    for ext in supported_extensions_for(&IdeProjectCompilerOptions::default()).extensions() {
        let path = CanonicalPath::new(&format!("d:/project/src/foo{ext}"));
        assert!(
            spec.matches(&path),
            "default include must own a supported-extension file `foo{ext}`"
        );
    }
    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/src/foo.unknownext")),
        "default include must NOT own an unknown extension"
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
        &test_supported(),
    );

    assert_eq!(spec.files.len(), 1);
    // The bare-star include expands per supported extension (never empty).
    assert!(
        !spec.include.is_empty(),
        "bare-star include must expand to at least one supported-extension glob"
    );
    assert_eq!(spec.exclude.len(), 1);
    // The exact membership semantics: the named file + supported siblings own,
    // the excluded dir does not.
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/main.ts")));
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/widget.tsx")));
    assert!(!spec.matches(&CanonicalPath::new("d:/project/dist/out.ts")));
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
        &test_supported(),
    );

    assert!(
        spec.exclude.len() >= 2,
        "empty exclude should be filled with TS defaults"
    );
}

// ── FIX 1: default-include + path reconciliation, end-to-end through the
// production config parse chain, asserting BOTH membership paths AGREE ──

/// Drive the FULL production chain for one tsconfig body: parse membership from
/// an in-memory workspace, expand to a `StaticMembershipSpec`, and ALSO
/// round-trip the spec back to the live `IdeProjectConfig` shape. Returns
/// `(spec, ide_config)` so a caller can assert BOTH the `StaticMembershipSpec::matches`
/// path AND the live `IdeProjectConfig::matches_file` path on the same parsed config.
fn parse_both_paths(root: &str, tsconfig_body: &str) -> (StaticMembershipSpec, IdeProjectConfig) {
    use crate::memory::{MemoryOptions, MemoryWorkspace};

    let tsconfig = format!("{root}/tsconfig.json");
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![root.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(tsconfig.clone(), std::sync::Arc::<str>::from(tsconfig_body));

    let root_cp = CanonicalPath::new(root);
    let raw = crate::config::load_project_membership(&ws, &tsconfig);
    let spec = membership_to_spec(&root_cp, &raw, &test_supported());

    let mut ide = IdeProjectConfig::new(root.to_string(), root.to_string(), Some(tsconfig));
    ide.membership = ConfiguredMembership {
        spec: spec.clone(),
        materialized_files: rustc_hash::FxHashSet::default(),
    };

    (spec, ide)
}

/// Like `parse_both_paths`, but also injects `{root}/base.json` so the child
/// tsconfig can `extends` it — exercising the inheritance path end-to-end.
fn parse_both_paths_with_base(
    root: &str,
    base_body: &str,
    child_body: &str,
) -> (StaticMembershipSpec, IdeProjectConfig) {
    use crate::memory::{MemoryOptions, MemoryWorkspace};

    let tsconfig = format!("{root}/tsconfig.json");
    let base = format!("{root}/base.json");
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![root.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(base, std::sync::Arc::<str>::from(base_body));
    ws.inject_file(tsconfig.clone(), std::sync::Arc::<str>::from(child_body));

    let root_cp = CanonicalPath::new(root);
    let raw = crate::config::load_project_membership(&ws, &tsconfig);
    let spec = membership_to_spec(&root_cp, &raw, &test_supported());

    let mut ide = IdeProjectConfig::new(root.to_string(), root.to_string(), Some(tsconfig));
    ide.membership = ConfiguredMembership {
        spec: spec.clone(),
        materialized_files: rustc_hash::FxHashSet::default(),
    };

    (spec, ide)
}

#[test]
fn inherited_explicit_empty_files_owns_nothing_on_both_paths() {
    // An `extends` base declaring `"files": []` (solution-style) must keep the
    // child owning NOTHING but references — the default include must NOT be
    // synthesized just because the inherited vectors are empty. Asserted on BOTH
    // the `StaticMembershipSpec::matches` path AND the live
    // `IdeProjectConfig::matches_file` path, which MUST AGREE.
    //
    // DISCRIMINATING: before the fix the producer inherited only the empty
    // files/include vectors (not the declared-ness), so it synthesized `**/*`
    // and OWNED `src/Foo.vue` — the red.
    let (spec, ide) = parse_both_paths_with_base(
        "d:/ws",
        r#"{ "files": [] }"#,
        r#"{ "extends": "./base.json", "exclude": ["dist"] }"#,
    );

    for ext in ["vue", "svelte"] {
        let candidate = CanonicalPath::new(&format!("d:/ws/src/Foo.{ext}"));
        assert!(
            !spec.matches(&candidate),
            "path 1: inherited explicit `files: []` must own NOTHING (`src/Foo.{ext}`)"
        );
        assert!(
            !ide.matches_file(&format!("d:/ws/src/Foo.{ext}")),
            "path 2: inherited explicit `files: []` must own NOTHING (`src/Foo.{ext}`)"
        );
    }
}

#[test]
fn inherited_explicit_empty_include_owns_nothing_on_both_paths() {
    // Sibling of the above for an inherited `"include": []`.
    let (spec, ide) = parse_both_paths_with_base(
        "d:/ws",
        r#"{ "include": [] }"#,
        r#"{ "extends": "./base.json", "exclude": ["dist"] }"#,
    );

    let candidate = CanonicalPath::new("d:/ws/src/Foo.vue");
    assert!(
        !spec.matches(&candidate),
        "path 1: inherited explicit `include: []` must own NOTHING"
    );
    assert!(
        !ide.matches_file("d:/ws/src/Foo.vue"),
        "path 2: inherited explicit `include: []` must own NOTHING"
    );
}

#[test]
fn exclude_only_owns_default_include_minus_exclude_on_both_paths() {
    // FIX 1: `{"exclude":["dist"]}` keeps the implicit default include MINUS the
    // excludes — it OWNS `src/Foo.vue`/`src/Foo.svelte` and REJECTS
    // `dist/Foo.vue`. Asserted on BOTH the `StaticMembershipSpec::matches` path
    // AND the live `IdeProjectConfig::matches_file` path, which MUST AGREE.
    //
    // DISCRIMINATING: before FIX 1 the producer emitted an empty include for an
    // exclude-only config ⇒ `StaticMembershipSpec::matches` owned NOTHING (the
    // red on path 1); the two paths also DIVERGED (the live path's
    // `!exclude.is_empty()` fallback owned everything-not-excluded).
    let (spec, ide) = parse_both_paths("d:/ws", r#"{ "exclude": ["dist"] }"#);

    for ext in ["vue", "svelte"] {
        let owned = CanonicalPath::new(&format!("d:/ws/src/Foo.{ext}"));
        assert!(
            spec.matches(&owned),
            "path 1 (StaticMembershipSpec::matches): exclude-only must OWN `src/Foo.{ext}`"
        );
        assert!(
            ide.matches_file(&format!("d:/ws/src/Foo.{ext}")),
            "path 2 (IdeProjectConfig::matches_file): exclude-only must OWN `src/Foo.{ext}`"
        );
    }

    let excluded = CanonicalPath::new("d:/ws/dist/Foo.vue");
    assert!(
        !spec.matches(&excluded),
        "path 1: exclude-only must REJECT `dist/Foo.vue`"
    );
    assert!(
        !ide.matches_file("d:/ws/dist/Foo.vue"),
        "path 2: exclude-only must REJECT `dist/Foo.vue`"
    );
}

#[test]
fn explicit_empty_files_owns_nothing_on_both_paths() {
    // FIX 1 distinction: an explicit `"files": []` solution-style config owns
    // NOTHING but its references — distinct from "no files key" (which gets the
    // default include). BOTH paths must agree it owns nothing.
    //
    // DISCRIMINATING on path 2: before FIX 4 the `!exclude.is_empty()` fallback
    // made the live path own everything-not-excluded (the TS-default exclude is
    // non-empty) — the red.
    let (spec, ide) = parse_both_paths("d:/ws", r#"{ "files": [], "references": [] }"#);

    let candidate = CanonicalPath::new("d:/ws/src/App.vue");
    assert!(
        !spec.matches(&candidate),
        "path 1: explicit `files: []` must own NOTHING"
    );
    assert!(
        !ide.matches_file("d:/ws/src/App.vue"),
        "path 2: explicit `files: []` must own NOTHING (no everything-not-excluded fallback)"
    );
}

#[test]
fn default_and_include_exclude_agree_on_both_paths() {
    // FIX 1 cases (c) default (no keys) and (d) include+exclude: both paths agree.
    let (spec_default, ide_default) = parse_both_paths("d:/ws", r#"{ "compilerOptions": {} }"#);
    assert!(
        spec_default.matches(&CanonicalPath::new("d:/ws/src/Foo.vue")),
        "path 1: default include owns `src/Foo.vue`"
    );
    assert!(
        ide_default.matches_file("d:/ws/src/Foo.vue"),
        "path 2: default include owns `src/Foo.vue`"
    );

    let (spec_ie, ide_ie) = parse_both_paths(
        "d:/ws",
        r#"{ "include": ["src/**/*"], "exclude": ["src/gen"] }"#,
    );
    assert!(
        spec_ie.matches(&CanonicalPath::new("d:/ws/src/Foo.vue")),
        "path 1: include+exclude owns `src/Foo.vue`"
    );
    assert!(
        ide_ie.matches_file("d:/ws/src/Foo.vue"),
        "path 2: include+exclude owns `src/Foo.vue`"
    );
    assert!(
        !spec_ie.matches(&CanonicalPath::new("d:/ws/src/gen/Bar.vue")),
        "path 1: include+exclude REJECTS `src/gen/Bar.vue`"
    );
    assert!(
        !ide_ie.matches_file("d:/ws/src/gen/Bar.vue"),
        "path 2: include+exclude REJECTS `src/gen/Bar.vue`"
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
    let owner = snap
        .resolver
        .nearest_config_for_path("d:/project/src/main.ts");
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
        include: vec![compiled("d:/project/src/**/*")],
        exclude: vec![compiled("d:/project/src/**/*")].into(), // excludes everything
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
        exclude: Vec::new().into(),
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

// ── membership_to_spec expansion + matching semantics ──

#[test]
fn include_exclude_membership_expands_and_matches_supported_extensions() {
    // A raw `files`/`include`/`exclude` membership expands to a spec whose
    // `matches` honours `files` immunity, bare-star include expansion across the
    // supported extension set, and `exclude` filtering.
    let raw = ProjectMembership::IncludeExclude {
        files: vec!["d:/project/src/main.ts".to_string()],
        include: vec!["d:/project/src/**/*".to_string()],
        exclude: vec!["d:/project/dist/**".to_string()],
    };
    let root = CanonicalPath::new("d:/project");
    let back = membership_to_spec(&root, &raw, &test_supported());

    assert_eq!(back.files.len(), 1);
    // The bare-star include expands per supported extension, so the glob COUNT
    // is one-per-extension; the membership SEMANTICS are what matter: the same
    // supported paths match, the excluded dir does not.
    assert!(!back.include.is_empty());
    assert!(back.matches(&CanonicalPath::new("d:/project/src/main.ts")));
    assert!(back.matches(&CanonicalPath::new("d:/project/src/widget.tsx")));
    assert!(!back.matches(&CanonicalPath::new("d:/project/dist/out.ts")));
    // An explicit non-empty exclude is kept verbatim (no default fill).
    assert_eq!(back.exclude.len(), 1);
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
        exclude: Vec::new().into(),
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
        exclude: Vec::new().into(),
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
        include: vec![compiled_from_root(&root, "src/**/*")],
        exclude: vec![compiled_from_root(&root, "dist/**")].into(),
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
        exclude: Vec::new().into(),
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
        include: vec![compiled_from_root(&root, "src/**/*")],
        exclude: Vec::new().into(),
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
        .nearest_config_for_path(&format!("{}/src/foo.ts", workspace_str));
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
        membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new("d:/project")),
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
        membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new("d:/project")),
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

// ── extends-only base config must not own files (Q-A5 + Q-A2(d)) ──

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn extends_only_alternate_config_is_not_registered_as_file_owner_and_package_wins() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("packages/app/src")).unwrap();

    // Root `tsconfig.base.json`: an extends-only base — compilerOptions only,
    // no files/include/exclude, no references. It is reached by other configs
    // through `extends`, but it must NOT itself become a file-owning project.
    std::fs::write(
        workspace.join("tsconfig.base.json"),
        r#"{ "compilerOptions": { "strict": true, "baseUrl": "." } }"#,
    )
    .unwrap();

    // Root `tsconfig.json`: extends the base, still no files/include/exclude.
    // It is a project config (default name) with MatchAll membership.
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "extends": "./tsconfig.base.json" }"#,
    )
    .unwrap();

    // Package config: extends the base and declares its own include set.
    std::fs::write(
        workspace.join("packages/app/tsconfig.json"),
        r#"{ "extends": "../../tsconfig.base.json", "include": ["src/**/*"] }"#,
    )
    .unwrap();

    let note_vue = workspace.join("packages/app/src/Note.vue");
    std::fs::write(&note_vue, "<template><div/></template>").unwrap();

    let workspace_str = workspace.to_string_lossy().replace('\\', "/");
    // Compare against canonicalized paths — the snapshot stores tsconfig paths
    // through `CanonicalPath::new` (lowercased drive, forward slashes).
    let note_str = CanonicalPath::new(&note_vue.to_string_lossy().replace('\\', "/"))
        .as_str()
        .to_string();
    let base_tsconfig_str = CanonicalPath::new(
        &workspace
            .join("tsconfig.base.json")
            .to_string_lossy()
            .replace('\\', "/"),
    )
    .as_str()
    .to_string();
    let pkg_tsconfig_str = CanonicalPath::new(
        &workspace
            .join("packages/app/tsconfig.json")
            .to_string_lossy()
            .replace('\\', "/"),
    )
    .as_str()
    .to_string();

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

    let configured_tsconfigs: Vec<String> = result
        .snapshot
        .projects
        .iter()
        .filter_map(|p| match &p.payload {
            ProjectPayload::Configured { tsconfig_path, .. } => {
                Some(tsconfig_path.as_str().to_string())
            }
            ProjectPayload::Fallback { .. } => None,
        })
        .collect();

    // Negative: the extends-only base must NOT be registered as a configured
    // (file-owning) project.
    assert!(
        !configured_tsconfigs.contains(&base_tsconfig_str),
        "extends-only tsconfig.base.json must not become a file-owning project, configured={configured_tsconfigs:?}"
    );

    // Positive: the package tsconfig IS registered.
    assert!(
        configured_tsconfigs.contains(&pkg_tsconfig_str),
        "packages/app/tsconfig.json must be registered as a configured project, configured={configured_tsconfigs:?}"
    );

    // Positive: Note.vue resolves uniquely to the package config (nearest root
    // beats the ancestor root tsconfig.json; the base never participates).
    match result.snapshot.configured_owner_resolution_for_file(&note_str) {
        ConfiguredOwnerResolution::Unique(id) => {
            assert_eq!(
                result.snapshot.tsconfig_path(id).map(|p| p.as_str()),
                Some(pkg_tsconfig_str.as_str()),
                "Note.vue's unique configured owner must be the package tsconfig"
            );
        }
        other => panic!(
            "Note.vue must have a unique package owner, got {other:?} (configured={configured_tsconfigs:?})"
        ),
    }
}
