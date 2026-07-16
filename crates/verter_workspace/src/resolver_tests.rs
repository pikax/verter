use super::*;
use crate::canonical_path::CanonicalPath;
use crate::membership::ConfiguredMembership;
use crate::types::ResolvePhase;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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
    project.membership = crate::snapshot_builder::configured_membership_from_raw(
        root,
        &membership,
        &project.compiler_options,
    );
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

impl crate::traits::WorkspaceRead for TestReader {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
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

    fn reverse_deps_for(&self, _id: &str) -> Vec<String> {
        Vec::new()
    }
    fn forward_deps_for(&self, _id: &str) -> Vec<String> {
        Vec::new()
    }
    fn dependency_snapshot(
        &self,
        _id: &str,
    ) -> Option<crate::exact_resolution::DependencySnapshotView> {
        None
    }
}

impl crate::traits::WorkspaceAccess for TestReader {
    // Reader-only stub overrides (R6/R7). Rationale: `TestReader` is
    // constructed only by resolver unit tests that exercise resolver-side
    // concerns (file reads, realpath); they don't touch dep-flow.
    fn record_parsed_edges(&self, _id: &str, _edges: &[crate::types::ParsedEdge]) {}
    fn set_exact_resolutions(
        &self,
        _id: &str,
        _resolutions: Vec<crate::types::ExactResolution>,
    ) -> crate::types::ExactResolutionResult {
        crate::types::ExactResolutionResult::default()
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        _id: &str,
        _edges: &[crate::types::ParsedEdge],
        _resolutions: Vec<crate::types::ExactResolution>,
    ) -> crate::types::ExactResolutionResult {
        crate::types::ExactResolutionResult::default()
    }
    fn replace_semantic_transitive(&self, _id: &str, _deps: std::collections::BTreeSet<String>) {}
    fn set_default_resolve_extensions(&self, _host_extensions: Vec<String>) {}
    fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
}

// ── CountingReader: WorkspaceAccess with call counters ──

struct CountingReader {
    files: HashSet<String>,
    texts: HashMap<String, Arc<str>>,
    realpaths: HashMap<String, String>,
    read_file_count: AtomicU64,
    file_exists_count: AtomicU64,
    realpath_count: AtomicU64,
    /// Per-path read_file call counts for isolating specific file reads.
    read_file_by_path: Mutex<HashMap<String, u64>>,
    package_manifest_cache: Mutex<HashMap<String, crate::types::PackageManifest>>,
}

impl CountingReader {
    fn with_files(paths: &[&str]) -> Self {
        let mut files = HashSet::new();
        let mut texts = HashMap::new();
        for path in paths {
            let normalized = normalize_canonical_id(path);
            files.insert(normalized.clone());
            texts.insert(normalized, Arc::<str>::from("// test file"));
        }
        Self {
            files,
            texts,
            realpaths: HashMap::new(),
            read_file_count: AtomicU64::new(0),
            file_exists_count: AtomicU64::new(0),
            realpath_count: AtomicU64::new(0),
            read_file_by_path: Mutex::new(HashMap::new()),
            package_manifest_cache: Mutex::new(HashMap::new()),
        }
    }

    fn add_file(&mut self, path: &str, text: &str) {
        let normalized = normalize_canonical_id(path);
        self.files.insert(normalized.clone());
        self.texts
            .insert(normalized, Arc::<str>::from(text.to_string()));
    }

    #[allow(dead_code)]
    fn add_realpath(&mut self, path: &str, realpath: &str) {
        self.realpaths.insert(
            normalize_canonical_id(path),
            normalize_canonical_id(realpath),
        );
    }

    fn read_file_calls(&self) -> u64 {
        self.read_file_count.load(Ordering::Relaxed)
    }

    fn file_exists_calls(&self) -> u64 {
        self.file_exists_count.load(Ordering::Relaxed)
    }

    #[allow(dead_code)]
    fn realpath_calls(&self) -> u64 {
        self.realpath_count.load(Ordering::Relaxed)
    }

    /// Get the number of read_file calls for a specific path.
    fn read_file_calls_for(&self, path: &str) -> u64 {
        let normalized = normalize_canonical_id(path);
        *self
            .read_file_by_path
            .lock()
            .unwrap()
            .get(&normalized)
            .unwrap_or(&0)
    }

    #[allow(dead_code)]
    fn reset_counters(&self) {
        self.read_file_count.store(0, Ordering::Relaxed);
        self.file_exists_count.store(0, Ordering::Relaxed);
        self.realpath_count.store(0, Ordering::Relaxed);
        self.read_file_by_path.lock().unwrap().clear();
        self.package_manifest_cache.lock().unwrap().clear();
    }
}

impl crate::traits::WorkspaceRead for CountingReader {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.read_file_count.fetch_add(1, Ordering::Relaxed);
        let normalized = normalize_canonical_id(canonical_id);
        *self
            .read_file_by_path
            .lock()
            .unwrap()
            .entry(normalized.clone())
            .or_insert(0) += 1;
        self.texts.get(&normalized).cloned()
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.file_exists_count.fetch_add(1, Ordering::Relaxed);
        self.files.contains(&normalize_canonical_id(canonical_id))
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.realpath_count.fetch_add(1, Ordering::Relaxed);
        self.realpaths
            .get(&normalize_canonical_id(canonical_id))
            .cloned()
            .or_else(|| {
                self.files
                    .contains(&normalize_canonical_id(canonical_id))
                    .then(|| normalize_canonical_id(canonical_id))
            })
    }

    fn read_package_manifest(&self, canonical_id: &str) -> Option<crate::types::PackageManifest> {
        let normalized = normalize_canonical_id(canonical_id);
        if let Some(manifest) = self.package_manifest_cache.lock().unwrap().get(&normalized) {
            return Some(manifest.clone());
        }

        let source = self.read_file(&normalized)?;
        let manifest = crate::package_index::parse_package_json(&source);
        self.package_manifest_cache
            .lock()
            .unwrap()
            .insert(normalized, manifest.clone());
        Some(manifest)
    }

    fn reverse_deps_for(&self, _id: &str) -> Vec<String> {
        Vec::new()
    }
    fn forward_deps_for(&self, _id: &str) -> Vec<String> {
        Vec::new()
    }
    fn dependency_snapshot(
        &self,
        _id: &str,
    ) -> Option<crate::exact_resolution::DependencySnapshotView> {
        None
    }
}

impl crate::traits::WorkspaceAccess for CountingReader {
    // Reader-only stub overrides (R6/R7). Rationale: `CountingReader` is a
    // resolver unit-test fixture for read_file/file_exists call counting;
    // it never participates in dep-flow.
    fn record_parsed_edges(&self, _id: &str, _edges: &[crate::types::ParsedEdge]) {}
    fn set_exact_resolutions(
        &self,
        _id: &str,
        _resolutions: Vec<crate::types::ExactResolution>,
    ) -> crate::types::ExactResolutionResult {
        crate::types::ExactResolutionResult::default()
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        _id: &str,
        _edges: &[crate::types::ParsedEdge],
        _resolutions: Vec<crate::types::ExactResolution>,
    ) -> crate::types::ExactResolutionResult {
        crate::types::ExactResolutionResult::default()
    }
    fn replace_semantic_transitive(&self, _id: &str, _deps: std::collections::BTreeSet<String>) {}
    fn set_default_resolve_extensions(&self, _host_extensions: Vec<String>) {}
    fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
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
        .nearest_config_for_path("/workspace/src/App.vue")
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
fn live_resolver_files_are_immune_to_exclude() {
    // FIX 4: in the LIVE `IdeProjectConfig::matches_file` path, an explicitly
    // listed `files` entry under an excluded directory must still be OWNED —
    // `files` are immune to `exclude` (matching TS + the new `StaticMembershipSpec`).
    //
    // DISCRIMINATING: before FIX 4 the exclude check ran BEFORE the files check,
    // so `Keep.vue` (under the excluded `src/excluded`) was wrongly rejected ⇒
    // `nearest_config_for_path` returned `None` (the red). After the fix the file is
    // owned.
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::IncludeExclude {
            files: vec!["/workspace/src/excluded/Keep.vue".to_string()],
            include: vec!["src/**/*".to_string()],
            exclude: vec!["src/excluded".to_string()],
        },
    )]);

    let owner = resolver.nearest_config_for_path("/workspace/src/excluded/Keep.vue");
    assert!(
        owner.is_some(),
        "an explicit `files` entry under an excluded dir must be OWNED \
         (files are immune to exclude in the live resolver path)"
    );
    assert_eq!(
        owner.unwrap().tsconfig_path.as_deref(),
        Some("/workspace/tsconfig.json")
    );

    // Negative control: a NON-files file under the excluded dir is still excluded.
    assert!(
        resolver
            .nearest_config_for_path("/workspace/src/excluded/Other.vue")
            .is_none(),
        "a non-`files` file under the excluded dir stays excluded"
    );
}

#[test]
fn live_resolver_exclude_only_owns_default_include_minus_exclude() {
    // FIX 1 (live path / path 2): an exclude-only config keeps the implicit
    // default include MINUS excludes. The producer synthesizes the default
    // include into the membership, so the LIVE resolver owns `src/Foo.vue` and
    // `src/Foo.svelte` and REJECTS `dist/Foo.vue`. (This mirrors what the new
    // spec / `StaticMembershipSpec::matches` produces — the two paths AGREE.)
    //
    // The membership shape here is exactly what `load_project_membership` +
    // `spec_to_membership` round-trip to for `{"exclude":["dist"]}`: a default
    // `**/*` include plus the user exclude.
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::IncludeExclude {
            files: Vec::new(),
            include: vec!["**/*".to_string()],
            exclude: vec!["dist".to_string()],
        },
    )]);

    for ext in ["vue", "svelte"] {
        let owned = format!("/workspace/src/Foo.{ext}");
        assert!(
            resolver.nearest_config_for_path(&owned).is_some(),
            "exclude-only (default include) must OWN `src/Foo.{ext}`"
        );
    }
    assert!(
        resolver
            .nearest_config_for_path("/workspace/dist/Foo.vue")
            .is_none(),
        "exclude-only must REJECT `dist/Foo.vue` (under the exclude)"
    );
}

#[test]
fn live_resolver_explicit_empty_files_owns_nothing() {
    // FIX 1 distinction (live path): an explicit `files: []` solution-style
    // config (with only the TS-default excludes, no include) owns NOTHING but
    // its references — it must NOT fall back to owning everything-not-excluded.
    //
    // DISCRIMINATING: before FIX 4 the `!exclude.is_empty()` fallback made this
    // own every non-excluded file (because the TS-default exclude is non-empty)
    // — the red. After the fix an empty-include + empty-files membership owns
    // nothing.
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::IncludeExclude {
            files: Vec::new(),
            include: Vec::new(),
            // Only the TS-default excludes (what `membership_to_spec` fills for
            // an explicit `files: []` with no user exclude).
            exclude: vec!["node_modules/**".to_string()],
        },
    )]);

    assert!(
        resolver
            .nearest_config_for_path("/workspace/src/App.vue")
            .is_none(),
        "an explicit `files: []` solution-style config must own NOTHING but references \
         (no everything-not-excluded fallback)"
    );
}

#[test]
fn ambiguous_configured_owners_are_preserved_non_collapsing() {
    // Two configured projects at the SAME root both claim `shared.ts` via `files`.
    // The resolver is NON-COLLAPSING: `effective_configs_for_path` must PRESERVE both
    // candidates rather than invent a single winner (or collapse to None). The
    // fail-closed ownership authority (`WorkspaceSnapshot::configured_owner_resolution_for_file`)
    // consumes this overlap and reports `Ambiguous`; the resolver's job is only to not
    // lose it. Import resolution (`nearest_config_for_path`) is NOT the ownership
    // authority, so it is not asserted here.
    //
    // DISCRIMINATING: a collapsing single-owner API returns 1 (invents a winner) or 0
    // (loses the file); the non-collapsing contract returns BOTH.
    let resolver = ProjectResolver::new(vec![
        project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::IncludeExclude {
                files: vec!["/workspace/src/shared.ts".to_string()],
                include: Vec::new(),
                exclude: Vec::new(),
            },
        ),
        project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.vitest.json"),
            ProjectMembership::IncludeExclude {
                files: vec!["/workspace/src/shared.ts".to_string()],
                include: Vec::new(),
                exclude: Vec::new(),
            },
        ),
    ]);

    let candidates = resolver.effective_configs_for_path("/workspace/src/shared.ts");
    assert_eq!(
        candidates.len(),
        2,
        "the non-collapsing resolver must PRESERVE both overlapping configured owners, \
         not invent a single winner"
    );
    let tsconfigs: Vec<&str> = candidates
        .iter()
        .filter_map(|c| c.tsconfig_path.as_deref())
        .collect();
    assert!(
        tsconfigs.contains(&"/workspace/tsconfig.app.json")
            && tsconfigs.contains(&"/workspace/tsconfig.vitest.json"),
        "both overlapping configs must survive: {tsconfigs:?}"
    );
}

#[test]
fn descendant_configured_owner_wins_over_ancestor_configured_owner() {
    // Mirrors a pnpm monorepo opened at one root: a root tsconfig whose
    // membership (e.g. via an `extends`-driven MatchAll) reaches every
    // descendant, plus a real package tsconfig that also claims the file.
    // The nearest (deepest-root) configured owner wins — the ancestor must
    // not make the package file ambiguous.
    let resolver = ProjectResolver::new(vec![
        project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.json"),
            ProjectMembership::MatchAll,
        ),
        project(
            "/workspace/packages/app",
            "/workspace",
            Some("/workspace/packages/app/tsconfig.json"),
            ProjectMembership::MatchAll,
        ),
    ]);

    let owner = resolver
        .nearest_config_for_path("/workspace/packages/app/src/Note.vue")
        .expect("descendant package config must own the package file");
    // Positive: the package config wins.
    assert_eq!(
        owner.tsconfig_path.as_deref(),
        Some("/workspace/packages/app/tsconfig.json"),
        "nearest-root configured owner must win over the ancestor root config"
    );
    // Negative: the ancestor root config must NOT be selected.
    assert_ne!(
        owner.tsconfig_path.as_deref(),
        Some("/workspace/tsconfig.json"),
        "ancestor root config must lose to the descendant package config"
    );

    // A file owned only by the ancestor root still resolves to the root.
    let root_owner = resolver
        .nearest_config_for_path("/workspace/scripts/build.ts")
        .expect("ancestor root config still owns files outside descendant packages");
    assert_eq!(
        root_owner.tsconfig_path.as_deref(),
        Some("/workspace/tsconfig.json"),
        "ancestor root config owns files that no descendant package claims"
    );
}

#[test]
fn incomparable_configured_roots_each_own_only_their_own_files() {
    // Sibling-root isolation in the RESOLVER path: `IdeProjectConfig::matches_file`
    // applies `normalized_starts_with(file, root)` FIRST, so two configs with
    // incomparable roots can NEVER both claim the same file — genuine
    // incomparable ambiguity is unreachable through `nearest_config_for_path` (it IS
    // reachable in the SNAPSHOT path, exercised by
    // `workspace_snapshot_tests::incomparable_configured_roots_overlap_is_ambiguous`).
    // The real reachable property here: each config owns ONLY files under its own
    // root, and a file under neither root resolves to None.
    let resolver = ProjectResolver::new(vec![
        project(
            "/workspace/packages/a",
            "/workspace",
            Some("/workspace/packages/a/tsconfig.json"),
            ProjectMembership::MatchAll,
        ),
        project(
            "/workspace/packages/b",
            "/workspace",
            Some("/workspace/packages/b/tsconfig.json"),
            ProjectMembership::MatchAll,
        ),
    ]);

    // A file under packages/a is owned by the `a` config alone.
    assert_eq!(
        resolver
            .nearest_config_for_path("/workspace/packages/a/src/Note.vue")
            .and_then(|o| o.tsconfig_path.as_deref()),
        Some("/workspace/packages/a/tsconfig.json"),
        "a file under packages/a is owned by the a config alone"
    );
    // Negative: the `b` config must NOT cross-claim packages/a's file.
    assert_ne!(
        resolver
            .nearest_config_for_path("/workspace/packages/a/src/Note.vue")
            .and_then(|o| o.tsconfig_path.as_deref()),
        Some("/workspace/packages/b/tsconfig.json"),
        "the b config must not cross-claim a file under packages/a"
    );
    // Symmetric: a file under packages/b is owned by the `b` config alone.
    assert_eq!(
        resolver
            .nearest_config_for_path("/workspace/packages/b/src/Panel.vue")
            .and_then(|o| o.tsconfig_path.as_deref()),
        Some("/workspace/packages/b/tsconfig.json"),
        "a file under packages/b is owned by the b config alone"
    );
    // A file under NEITHER root resolves to None (no config claims it).
    assert!(
        resolver
            .nearest_config_for_path("/workspace/packages/c/src/Other.vue")
            .is_none(),
        "a file under neither sibling root must resolve to None"
    );
}

#[test]
fn resolve_for_project_uses_owner_tsconfig_paths_without_importer_file() {
    let mut configured = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    );
    configured.compiler_options.base_url = Some("/workspace".to_string());
    configured
        .compiler_options
        .paths
        .push(("vue".to_string(), vec!["types/vue/index.d.ts".to_string()]));
    configured
        .compiler_options
        .paths
        .push(("vue/*".to_string(), vec!["types/vue/*.d.ts".to_string()]));

    let resolver = ProjectResolver::new(vec![configured]);
    let reader = TestReader::with_files(&[
        "/workspace/types/vue/index.d.ts",
        "/workspace/types/vue/jsx.d.ts",
    ]);
    let owner = crate::types::ProjectOwnership {
        project_root: "/workspace".to_string(),
        tsconfig_path: Some("/workspace/tsconfig.json".to_string()),
    };
    let ctx = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: crate::types::ResolveRequestKind::TypeImport,
    };

    let vue = resolver
        .resolve_for_project_with_reader(&reader, &owner, "vue", ctx)
        .expect("project-owned resolution should honor exact tsconfig path entries");
    let vue_jsx = resolver
        .resolve_for_project_with_reader(&reader, &owner, "vue/jsx", ctx)
        .expect("project-owned resolution should honor wildcard tsconfig path entries");

    assert_eq!(vue.source_id, "/workspace/types/vue/index.d.ts");
    assert_eq!(vue_jsx.source_id, "/workspace/types/vue/jsx.d.ts");
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
        provider_id.ends_with("/src/App.vue.verter.ts"),
        "Vue files should resolve to .vue.verter.ts in the provider graph: {provider_id}"
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
        "IDE provider paths must remain distinct from the public .vue.verter.ts API path"
    );
}

#[test]
fn provider_paths_derive_both_virtual_files_for_svelte_carriers() {
    // The carrier-extension generalization: a `.svelte` file receives BOTH the
    // api virtual file (`.svelte.verter.ts`, the reserved redirect-reached
    // infix) and the IDE virtual file (`.svelte.tsx`), derived from the registry
    // carrier-extension set — not a `.vue`-literal.
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);

    let api = resolver
        .provider_id_for_source("/workspace/src/Comp.svelte")
        .expect("svelte files receive an api provider path");
    assert_eq!(api, "/workspace/src/Comp.svelte.verter.ts");

    let ide = resolver
        .provider_ide_id_for_source("/workspace/src/Comp.svelte", false)
        .expect("svelte IDE files receive a provider path");
    assert_eq!(ide, "/workspace/src/Comp.svelte.tsx");
    let js_ide = resolver
        .provider_ide_id_for_source("/workspace/src/Comp.svelte", true)
        .expect("JavaScript svelte IDE files receive a provider path");
    assert_eq!(js_ide, "/workspace/src/Comp.svelte.jsx");

    // Both round-trip back to the `.svelte` source.
    assert_eq!(
        resolver.source_id_from_provider_id(&api).as_deref(),
        Some("/workspace/src/Comp.svelte")
    );
    assert_eq!(
        resolver.source_id_from_provider_id(&ide).as_deref(),
        Some("/workspace/src/Comp.svelte")
    );
    assert_eq!(
        resolver.source_id_from_provider_id(&js_ide).as_deref(),
        Some("/workspace/src/Comp.svelte")
    );
}

#[test]
fn rune_modules_are_not_carriers_and_serve_their_own_provider_path() {
    // `.svelte.ts` / `.svelte.js` rune modules are NOT carriers, so
    // `path_is_carrier` MUST exclude them (the watcher-glob / virtual-file
    // routing source) and `carrier_extensions` MUST NOT list them. A rune
    // module serves its OWN canonical path (no `{carrier}.tsx`/`.ts` virtual
    // dual-file model) so a consumer resolving it from disk finds it directly.
    assert!(
        !path_is_carrier("/workspace/src/store.svelte.ts"),
        "a `.svelte.ts` rune module must NOT be a carrier path"
    );
    assert!(
        !path_is_carrier("/workspace/src/store.svelte.js"),
        "a `.svelte.js` rune module must NOT be a carrier path"
    );
    // The bare `.svelte` component IS a carrier (discrimination is real).
    assert!(path_is_carrier("/workspace/src/Box.svelte"));

    let extensions = verter_language::LanguageRegistry::global().carrier_extensions();
    assert!(!extensions.contains(&"svelte.ts"));
    assert!(!extensions.contains(&"svelte.js"));

    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    // A rune module's provider id is its OWN path (NOT a `{carrier}.ts` dual
    // file) and it has NO IDE virtual file (it is not a component carrier).
    assert_eq!(
        resolver
            .provider_id_for_source("/workspace/src/store.svelte.ts")
            .as_deref(),
        Some("/workspace/src/store.svelte.ts"),
        "a rune module serves its own canonical path"
    );
    assert_eq!(
        resolver.provider_ide_id_for_source("/workspace/src/store.svelte.ts", false),
        None,
        "a rune module has no component IDE virtual file"
    );
}

#[test]
fn strip_carrier_extension_is_registry_backed_for_every_carrier() {
    // The registry-backed carrier strip yields the bare stem for EVERY
    // carrier extension (`.vue`, `.svelte`), longest-suffix-first.
    assert_eq!(
        strip_carrier_extension("/project/src/components/MyButton.vue"),
        "/project/src/components/MyButton"
    );
    assert_eq!(
        strip_carrier_extension("/project/src/components/MyButton.svelte"),
        "/project/src/components/MyButton"
    );
    assert_eq!(strip_carrier_extension("Foo.svelte"), "Foo");
    assert_eq!(strip_carrier_extension("Foo.vue"), "Foo");
    // A non-carrier path is returned UNCHANGED (discrimination: the caller
    // distinguishes carrier from non-carrier by stem != input).
    assert_eq!(strip_carrier_extension("util.ts"), "util.ts");
    assert_eq!(
        strip_carrier_extension("store.svelte.ts"),
        "store.svelte.ts"
    );
    // A bare `.svelte` / `.vue` (no stem) does not strip to empty wrongly —
    // the helper requires at least one stem char before the extension.
    assert_eq!(strip_carrier_extension(".svelte"), ".svelte");
}

#[test]
fn carrier_api_provider_path_appends_verter_ts_to_full_carrier() {
    // The API virtual path is the FULL carrier canonical + the reserved
    // `.verter.ts` infix for every carrier — never a hardcoded `.vue.ts`.
    assert_eq!(
        carrier_api_provider_path("/workspace/src/App.vue"),
        "/workspace/src/App.vue.verter.ts"
    );
    assert_eq!(
        carrier_api_provider_path("/workspace/src/App.svelte"),
        "/workspace/src/App.svelte.verter.ts"
    );
    // Mirrors the IDE derivation's carrier-genericity.
    assert_eq!(
        carrier_ide_provider_path("/workspace/src/App.svelte", false),
        "/workspace/src/App.svelte.tsx"
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
    assert_eq!(resolved.provider_target, ProviderTarget::CarrierPublicApi);
    assert_eq!(resolved.resolution_kind, ResolutionKind::Relative);
    assert_eq!(resolved.provider_specifier, "./Foo.vue.verter.ts");
    assert!(
        resolved.provider_id.ends_with("/src/Foo.vue.verter.ts"),
        "provider graph should target the materialized .vue.verter.ts API file: {}",
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
fn resolve_package_imports_subpath_substitutes_js_specifier_to_ts_sibling() {
    // `nodenext` extension rewrite: a `.js` import specifier resolves to its
    // `.ts` sibling. The fixture maps `#internal/*` -> `./src/internal/*` and the
    // consumer imports `#internal/InternalComp.js`; the real file is
    // `InternalComp.ts`, so the resolver must substitute `.js` -> `.ts`.
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&["/workspace/src/internal/InternalComp.ts"]);
    reader.add_file(
        "/workspace/package.json",
        r##"{
                "imports": {
                    "#internal/*": "./src/internal/*"
                }
            }"##,
    );

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "#internal/InternalComp.js".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("a `#imports` subpath with a `.js` specifier must resolve to its `.ts` sibling");

    assert_eq!(
        resolved.source_id,
        "/workspace/src/internal/InternalComp.ts"
    );
    assert_eq!(resolved.resolution_kind, ResolutionKind::PackageImports);
}

#[test]
fn resolve_relative_js_specifier_substitutes_to_ts_sibling() {
    // The extension rewrite is general (not `#imports`-specific): a relative
    // `./x.js` import resolves to `./x.ts` when only the `.ts` sibling exists.
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&["/workspace/src/mod.ts"]);

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "./mod.js".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("a relative `.js` specifier must resolve to its `.ts` sibling");

    assert_eq!(resolved.source_id, "/workspace/src/mod.ts");
}

#[test]
fn resolve_js_specifier_prefers_source_ts_over_colocated_dts() {
    // TS extension substitution: when BOTH `./x.ts` and `./x.d.ts` exist, a
    // `./x.js` specifier resolves to the SOURCE `./x.ts`, not the declaration.
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&["/workspace/src/mod.ts", "/workspace/src/mod.d.ts"]);

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "./mod.js".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("a relative `.js` specifier must resolve");

    assert_eq!(
        resolved.source_id, "/workspace/src/mod.ts",
        "the source `.ts` sibling must win over a co-located `.d.ts` for a `.js` specifier"
    );
}

#[test]
fn sfc_src_attr_js_does_not_substitute_to_ts_sibling() {
    // An SFC `src="./setup.js"` reads the LITERAL file bytes — it is NOT TS
    // import resolution. When both `setup.js` and `setup.ts` exist it MUST
    // resolve to the literal `.js`, never substitute to `.ts`.
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&["/workspace/src/setup.js", "/workspace/src/setup.ts"]);

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.vue".to_string(),
                specifier: "./setup.js".to_string(),
                kind: ResolveRequestKind::SfcSrcAttr,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect("an SFC `src=\"./setup.js\"` must resolve to the literal file");

    assert_eq!(
        resolved.source_id, "/workspace/src/setup.js",
        "an SFC `src=` must NOT substitute `.js` -> `.ts` (reads literal bytes)"
    );
}

#[test]
fn sfc_src_attr_js_does_not_substitute_to_ts_even_when_js_absent() {
    // An SFC `src="./setup.js"` is a LITERAL file reference, not TS import
    // resolution — the `.js` -> `.ts` extension rewrite never applies. With only
    // `setup.ts` present (no `setup.js`), the `src` does NOT resolve to the
    // `.ts` sibling: it stays unresolved (a missing-file `src=`), distinguishing
    // the literal-bytes semantics from import substitution.
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&["/workspace/src/setup.ts"]);

    let resolved = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/workspace/src/App.vue".to_string(),
            specifier: "./setup.js".to_string(),
            kind: ResolveRequestKind::SfcSrcAttr,
            phase: ResolvePhase::CodegenBlocker,
        },
    );

    assert!(
        resolved.is_none(),
        "an SFC `src=\"./setup.js\"` must NOT substitute to the `.ts` sibling \
         (literal file reference, not TS import resolution); got: {resolved:?}"
    );
}

#[test]
fn esm_import_js_still_substitutes_to_ts_when_js_absent() {
    // Contrast with the SFC src case: an ESM IMPORT `./setup.js` DOES substitute
    // to the `.ts` sibling (TS file-extension-substitution).
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&["/workspace/src/setup.ts"]);

    let resolved = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "./setup.js".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("an ESM import `./setup.js` must substitute to its `.ts` sibling");

    assert_eq!(resolved.source_id, "/workspace/src/setup.ts");
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
        resolver
            .nearest_config_for_path("/workspace/src/App.vue")
            .is_some(),
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
        ProviderTarget::CarrierPublicApi,
        "Vue target owned by a project should get CarrierPublicApi"
    );
    assert!(
        resolved.provider_id.ends_with(".vue.verter.ts"),
        "provider_id for owned Vue target should be .vue.verter.ts: {}",
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
        ..Default::default()
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
// tsconfig paths with non-standard patterns (Nuxt #imports, etc.)
// ═══════════════════════════════════════════════════════════════════════════

/// Tsconfig paths must resolve ANY pattern — `#imports`, `#app/*`, `$lib/*`,
/// `~/*`, etc. The resolver must not short-circuit based on prefix characters.
#[test]
fn resolve_tsconfig_paths_arbitrary_patterns() {
    let mut p = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    );
    p.compiler_options = IdeProjectCompilerOptions {
        base_url: None,
        paths: vec![
            (
                "#imports".to_string(),
                vec!["/workspace/.nuxt/imports".to_string()],
            ),
            (
                "#app".to_string(),
                vec!["/workspace/node_modules/nuxt/dist/app".to_string()],
            ),
            (
                "#app/*".to_string(),
                vec!["/workspace/node_modules/nuxt/dist/app/*".to_string()],
            ),
        ],
        ..Default::default()
    };
    let resolver = ProjectResolver::new(vec![p]);
    let reader = TestReader::with_files(&[
        "/workspace/.nuxt/imports.d.ts",
        "/workspace/node_modules/nuxt/dist/app/index.d.ts",
        "/workspace/node_modules/nuxt/dist/app/composables/index.d.ts",
    ]);

    // Exact match (no wildcard)
    let r = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/workspace/app/App.vue".to_string(),
            specifier: "#imports".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    );
    let resolved = r.expect("#imports must resolve via tsconfig paths");
    assert_eq!(resolved.source_id, "/workspace/.nuxt/imports.d.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::TsConfigPath);

    // Exact match (no wildcard), probes index file
    let r = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/workspace/app/App.vue".to_string(),
            specifier: "#app".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    );
    let resolved = r.expect("#app must resolve via tsconfig paths");
    assert_eq!(
        resolved.source_id,
        "/workspace/node_modules/nuxt/dist/app/index.d.ts"
    );
    assert_eq!(resolved.resolution_kind, ResolutionKind::TsConfigPath);

    // Wildcard match
    let r = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/workspace/app/App.vue".to_string(),
            specifier: "#app/composables".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    );
    let resolved = r.expect("#app/composables must resolve via tsconfig paths wildcard");
    assert_eq!(
        resolved.source_id,
        "/workspace/node_modules/nuxt/dist/app/composables/index.d.ts"
    );
    assert_eq!(resolved.resolution_kind, ResolutionKind::TsConfigPath);
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
    };
    let resolver = ProjectResolver::new(vec![app_project]);
    let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

    // .vue.verter.ts is a provider path, not a real file — should not match
    let result = resolver.preferred_specifier(
        &reader,
        "/workspace/src/App.ts",
        "/workspace/src/Foo.vue.verter.ts",
    );

    assert!(
        result.is_none(),
        "provider paths (.vue.verter.ts) should return None — got: {result:?}"
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
        ..Default::default()
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
        ..Default::default()
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

// ── Context-aware resolution tests ──

/// Same specifier → different target depending on (phase, kind).
/// Package with `exports: { ".": { "types": "./d.ts", "import": "./index.js" } }`
///
/// - `(CodegenBlocker, EsmImport)` → index.js  (runtime entry)
/// - `(ProviderGraph, EsmImport)` → d.ts       (type entry)
/// - `(CodegenBlocker, TypeImport)` → d.ts      (type entry)
#[test]
fn context_aware_exports_codegen_esm_picks_runtime() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/pkg/index.js",
        "/workspace/node_modules/pkg/d.ts",
    ]);
    reader.add_file(
        "/workspace/node_modules/pkg/package.json",
        r#"{ "exports": { ".": { "types": "./d.ts", "import": "./index.js" } } }"#,
    );

    // CodegenBlocker + EsmImport → runtime entry (index.js)
    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "pkg".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect("CodegenBlocker + EsmImport should resolve");
    assert_eq!(
        result.source_id, "/workspace/node_modules/pkg/index.js",
        "CodegenBlocker+EsmImport should pick runtime entry (import condition)"
    );
}

#[test]
fn context_aware_exports_provider_esm_picks_types() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/pkg/index.js",
        "/workspace/node_modules/pkg/d.ts",
    ]);
    reader.add_file(
        "/workspace/node_modules/pkg/package.json",
        r#"{ "exports": { ".": { "types": "./d.ts", "import": "./index.js" } } }"#,
    );

    // ProviderGraph + EsmImport → type entry (d.ts)
    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "pkg".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("ProviderGraph + EsmImport should resolve");
    assert_eq!(
        result.source_id, "/workspace/node_modules/pkg/d.ts",
        "ProviderGraph should pick type entry (types condition)"
    );
}

#[test]
fn context_aware_exports_codegen_type_import_picks_types() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/pkg/index.js",
        "/workspace/node_modules/pkg/d.ts",
    ]);
    reader.add_file(
        "/workspace/node_modules/pkg/package.json",
        r#"{ "exports": { ".": { "types": "./d.ts", "import": "./index.js" } } }"#,
    );

    // CodegenBlocker + TypeImport → type entry (d.ts)
    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "pkg".to_string(),
                kind: ResolveRequestKind::TypeImport,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect("CodegenBlocker + TypeImport should resolve");
    assert_eq!(
        result.source_id, "/workspace/node_modules/pkg/d.ts",
        "CodegenBlocker+TypeImport should pick type entry (types condition)"
    );
}

/// Legacy package (no exports field): `{ "types": "./t.d.ts", "main": "./m.js" }`
///
/// - `(CodegenBlocker, EsmImport)` → m.js  (module/main keys)
/// - `(ProviderGraph, EsmImport)` → t.d.ts (types key)
#[test]
fn context_aware_legacy_codegen_picks_main() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/legacy/m.js",
        "/workspace/node_modules/legacy/t.d.ts",
    ]);
    reader.add_file(
        "/workspace/node_modules/legacy/package.json",
        r#"{ "types": "./t.d.ts", "main": "./m.js" }"#,
    );

    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "legacy".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect("CodegenBlocker legacy should resolve");
    assert_eq!(
        result.source_id, "/workspace/node_modules/legacy/m.js",
        "CodegenBlocker+EsmImport legacy should pick main"
    );
}

#[test]
fn context_aware_legacy_provider_picks_types() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/legacy/m.js",
        "/workspace/node_modules/legacy/t.d.ts",
    ]);
    reader.add_file(
        "/workspace/node_modules/legacy/package.json",
        r#"{ "types": "./t.d.ts", "main": "./m.js" }"#,
    );

    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "legacy".to_string(),
                kind: ResolveRequestKind::EsmImport,
                phase: ResolvePhase::ProviderGraph,
            },
        )
        .expect("ProviderGraph legacy should resolve");
    assert_eq!(
        result.source_id, "/workspace/node_modules/legacy/t.d.ts",
        "ProviderGraph legacy should pick types"
    );
}

/// RequireCall → CJS entry via "require" export condition.
#[test]
fn context_aware_require_picks_cjs() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/dual/index.mjs",
        "/workspace/node_modules/dual/index.cjs",
    ]);
    reader.add_file(
        "/workspace/node_modules/dual/package.json",
        r#"{ "exports": { ".": { "import": "./index.mjs", "require": "./index.cjs" } } }"#,
    );

    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.ts".to_string(),
                specifier: "dual".to_string(),
                kind: ResolveRequestKind::RequireCall,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect("RequireCall should resolve");
    assert_eq!(
        result.source_id, "/workspace/node_modules/dual/index.cjs",
        "RequireCall should pick CJS entry via require condition"
    );
}

/// TypeImport + CodegenBlocker → resolves "types" condition in package exports.
/// This is critical for macro type deps (defineProps<ExternalType>()) where the
/// import source is a bare module with `exports: { ".": { "types": "..." } }`.
#[test]
fn type_import_codegen_blocker_resolves_types_condition() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&["/workspace/node_modules/motion/dist/index.d.ts"]);
    reader.add_file(
        "/workspace/node_modules/motion/package.json",
        r#"{ "name": "motion", "exports": { ".": { "types": "./dist/index.d.ts" } } }"#,
    );

    // Positive: TypeImport should resolve via "types" condition
    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/workspace/src/App.vue".to_string(),
            specifier: "motion".to_string(),
            kind: ResolveRequestKind::TypeImport,
            phase: ResolvePhase::CodegenBlocker,
        },
    );
    assert!(
        result.is_some(),
        "TypeImport+CodegenBlocker should resolve 'types' export condition"
    );
    assert_eq!(
        result.unwrap().source_id,
        "/workspace/node_modules/motion/dist/index.d.ts",
        "TypeImport should resolve to the types entry point"
    );

    // Negative: EsmImport+CodegenBlocker should NOT resolve types-only exports
    let esm_result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/workspace/src/App.vue".to_string(),
            specifier: "motion".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::CodegenBlocker,
        },
    );
    assert!(
        esm_result.is_none(),
        "EsmImport+CodegenBlocker should NOT resolve types-only exports (no 'import'/'default' condition)"
    );
}

#[test]
fn type_import_codegen_blocker_falls_back_to_manifest_types_when_exports_lack_types() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&["/workspace/node_modules/fancy/dist/index.d.ts"]);
    reader.add_file(
        "/workspace/node_modules/fancy/package.json",
        r#"{
            "name": "fancy",
            "types": "./dist/index.d.ts",
            "exports": {
                ".": {
                    "import": "./dist/index.js",
                    "require": "./dist/index.cjs"
                }
            }
        }"#,
    );

    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.vue".to_string(),
                specifier: "fancy".to_string(),
                kind: ResolveRequestKind::TypeImport,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect(
            "TypeImport should honor package.json types before falling back to runtime exports",
        );

    assert_eq!(
        result.source_id,
        "/workspace/node_modules/fancy/dist/index.d.ts"
    );
}

#[test]
fn type_import_relative_js_specifier_prefers_declaration_companion() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/fancy/package.json",
        "/workspace/node_modules/fancy/dist/index.d.ts",
        "/workspace/node_modules/fancy/dist/index3.d.ts",
        "/workspace/node_modules/fancy/dist/index3.js",
    ]);
    reader.add_file(
        "/workspace/node_modules/fancy/package.json",
        r#"{
            "name": "fancy",
            "types": "./dist/index.d.ts"
        }"#,
    );

    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
                specifier: "./index3.js".to_string(),
                kind: ResolveRequestKind::TypeImport,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect("TypeImport should prefer declaration companions over runtime JS");

    assert_eq!(
        result.source_id,
        "/workspace/node_modules/fancy/dist/index3.d.ts"
    );
}

#[test]
fn type_import_relative_package_follow_requires_package_manifest_confirmation() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let reader = TestReader::with_files(&[
        "/workspace/node_modules/fancy/dist/index.d.ts",
        "/workspace/node_modules/fancy/dist/index3.d.ts",
        "/workspace/node_modules/fancy/dist/index3.js",
    ]);

    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
            specifier: "./index3.js".to_string(),
            kind: ResolveRequestKind::TypeImport,
            phase: ResolvePhase::CodegenBlocker,
        },
    );

    assert!(
        result.is_none(),
        "package-internal follow-on imports should not resolve without confirming the package via package.json"
    );
}

// ── CountingReader infrastructure + regression tests ──

#[test]
fn counting_reader_tracks_calls() {
    let reader = CountingReader::with_files(&["/repo/src/a.ts"]);
    assert_eq!(reader.file_exists_calls(), 0);
    assert_eq!(reader.read_file_calls(), 0);
    assert_eq!(reader.realpath_calls(), 0);

    <dyn crate::traits::WorkspaceAccess>::file_exists(&reader, "/repo/src/a.ts");
    assert_eq!(reader.file_exists_calls(), 1);

    <dyn crate::traits::WorkspaceAccess>::read_file(&reader, "/repo/src/a.ts");
    assert_eq!(reader.read_file_calls(), 1);
    assert_eq!(reader.read_file_calls_for("/repo/src/a.ts"), 1);

    // Second read of same path increments per-path counter
    <dyn crate::traits::WorkspaceAccess>::read_file(&reader, "/repo/src/a.ts");
    assert_eq!(reader.read_file_calls(), 2);
    assert_eq!(reader.read_file_calls_for("/repo/src/a.ts"), 2);

    // Different path tracked separately
    <dyn crate::traits::WorkspaceAccess>::read_file(&reader, "/repo/src/nonexistent.ts");
    assert_eq!(reader.read_file_calls(), 3);
    assert_eq!(reader.read_file_calls_for("/repo/src/a.ts"), 2);
    assert_eq!(reader.read_file_calls_for("/repo/src/nonexistent.ts"), 1);
}

/// Resolving through the workspace manifest API should not re-read the same
/// package.json for every importer.
#[test]
fn bare_package_json_reread_per_importer() {
    use crate::engine::Engine;
    use crate::types::{ResolutionContext, ResolveRequestKind};

    let mut reader = CountingReader::with_files(&[
        "/repo/src/0.vue",
        "/repo/src/1.vue",
        "/repo/src/2.vue",
        "/repo/src/3.vue",
        "/repo/src/4.vue",
        "/repo/src/5.vue",
        "/repo/src/6.vue",
        "/repo/src/7.vue",
        "/repo/node_modules/vue/dist/vue.esm.js",
    ]);
    reader.add_file(
        "/repo/node_modules/vue/package.json",
        r#"{"module":"dist/vue.esm.js"}"#,
    );

    let engine = Engine::new();
    {
        use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
        use crate::resolver::IdeProjectCompilerOptions;
        let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
            root: "/repo".to_string(),
            rank: ProjectRank::Inferred,
            tsconfig_path: None,
            root_files: vec![],
            extensions: vec![],
            workspace_root: "/repo".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new("/repo")),
        }]);
        *engine.project_graph.write() = graph;
        engine.rebuild_and_publish();
    }

    let ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    for i in 0..8 {
        let result = engine.resolve_import(&reader, &format!("/repo/src/{i}.vue"), "vue", ctx);
        assert!(
            result.is_some(),
            "resolution should succeed for importer {i}"
        );
    }

    let manifest_path = "/repo/node_modules/vue/package.json";
    let manifest_reads = reader.read_file_calls_for(manifest_path);

    assert_eq!(
        manifest_reads, 1,
        "manifest at {manifest_path} should be read once and then served from the workspace manifest cache"
    );
}

#[test]
fn resolve_import_reuses_lazy_resolution_cache_for_same_importer_and_specifier() {
    use crate::engine::Engine;
    use crate::types::{ResolutionContext, ResolveRequestKind};

    let mut reader =
        CountingReader::with_files(&["/repo/src/App.vue", "/repo/node_modules/pkg/dist/index.js"]);
    reader.add_file(
        "/repo/node_modules/pkg/package.json",
        r#"{"module":"dist/index.js"}"#,
    );

    let engine = Engine::new();
    {
        use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
        use crate::resolver::IdeProjectCompilerOptions;
        let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
            root: "/repo".to_string(),
            rank: ProjectRank::Inferred,
            tsconfig_path: None,
            root_files: vec![],
            extensions: vec![],
            workspace_root: "/repo".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new("/repo")),
        }]);
        *engine.project_graph.write() = graph;
        engine.rebuild_and_publish();
    }

    let ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let first = engine
        .resolve_import(&reader, "/repo/src/App.vue", "pkg", ctx)
        .expect("first resolution should succeed");
    let after_first_exists = reader.file_exists_calls();
    let after_first_reads = reader.read_file_calls();
    let after_first_provenance = engine.vfs_provenance.snapshot();
    assert_eq!(
        after_first_provenance.import_resolution_cache_hit_count, 0,
        "cold resolution should not report a lazy import cache hit",
    );
    assert_eq!(
        after_first_provenance.import_resolution_cache_miss_count, 1,
        "cold resolution should report a single lazy import cache miss",
    );

    let second = engine
        .resolve_import(&reader, "/repo/src/App.vue", "pkg", ctx)
        .expect("warm resolution should succeed");
    let after_second_provenance = engine.vfs_provenance.snapshot();

    assert_eq!(second, first, "warm cache hit should reuse the same result");
    assert_eq!(
        reader.file_exists_calls(),
        after_first_exists,
        "warm resolution should not rerun file-existence probes",
    );
    assert_eq!(
        reader.read_file_calls(),
        after_first_reads,
        "warm resolution should not reread manifests or source files",
    );
    assert_eq!(
        after_second_provenance.import_resolution_cache_hit_count, 1,
        "second resolution should come from the lazy import cache",
    );
    assert_eq!(
        after_second_provenance.import_resolution_cache_miss_count, 1,
        "warm resolution should not add another cache miss",
    );
}

/// Resolving `#imports` through the workspace manifest API should not re-read
/// the same package.json for every importer.
#[test]
fn package_imports_reread_per_importer() {
    use crate::engine::Engine;
    use crate::types::{ResolutionContext, ResolveRequestKind};

    let mut reader = CountingReader::with_files(&[
        "/repo/src/a.vue",
        "/repo/src/b.vue",
        "/repo/src/c.vue",
        "/repo/src/d.vue",
        "/repo/src/utils.ts",
    ]);
    reader.add_file(
        "/repo/package.json",
        r##"{"imports": {"#utils": "./src/utils.ts"}}"##,
    );

    let engine = Engine::new();
    {
        use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
        use crate::resolver::IdeProjectCompilerOptions;
        let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
            root: "/repo".to_string(),
            rank: ProjectRank::Inferred,
            tsconfig_path: None,
            root_files: vec![],
            extensions: vec![],
            workspace_root: "/repo".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new("/repo")),
        }]);
        *engine.project_graph.write() = graph;
        engine.rebuild_and_publish();
    }

    let ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    for suffix in &["a", "b", "c", "d"] {
        let result =
            engine.resolve_import(&reader, &format!("/repo/src/{suffix}.vue"), "#utils", ctx);
        assert!(
            result.is_some(),
            "resolution should succeed for importer {suffix}"
        );
    }

    let manifest_path = "/repo/package.json";
    let manifest_reads = reader.read_file_calls_for(manifest_path);

    assert_eq!(
        manifest_reads, 1,
        "manifest at {manifest_path} should be read once and then served from the workspace manifest cache"
    );
}

// ── Test 1: Pure path transforms work without ownership ──

#[test]
fn node_modules_missing_ancestor_manifests_do_not_trigger_reads() {
    use crate::engine::Engine;
    use crate::types::{ResolutionContext, ResolveRequestKind};

    let mut reader = CountingReader::with_files(&[
        "/repo/src/components/App.vue",
        "/repo/node_modules/vue/dist/index.d.ts",
    ]);
    reader.add_file(
        "/repo/node_modules/vue/package.json",
        r#"{"types":"dist/index.d.ts"}"#,
    );

    let engine = Engine::new();
    {
        use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
        use crate::resolver::IdeProjectCompilerOptions;
        let graph = ProjectGraph::from_configs(vec![VfsProjectConfig {
            root: "/repo".to_string(),
            rank: ProjectRank::Inferred,
            tsconfig_path: None,
            root_files: vec![],
            extensions: vec![],
            workspace_root: "/repo".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new("/repo")),
        }]);
        *engine.project_graph.write() = graph;
        engine.rebuild_and_publish();
    }

    let ctx = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::TypeImport,
    };

    let result = engine.resolve_import(&reader, "/repo/src/components/App.vue", "vue", ctx);
    assert!(result.is_some(), "package import should resolve");

    assert_eq!(
        reader.read_file_calls_for("/repo/src/components/node_modules/vue/package.json"),
        0,
        "missing nearest node_modules manifest should be skipped by existence facts"
    );
    assert_eq!(
        reader.read_file_calls_for("/repo/src/node_modules/vue/package.json"),
        0,
        "missing intermediate node_modules manifest should be skipped by existence facts"
    );
    assert_eq!(
        reader.read_file_calls_for("/repo/node_modules/vue/package.json"),
        1,
        "real package manifest should still be read exactly once"
    );
}

#[test]
fn provider_id_for_source_vue_without_ownership() {
    let resolver = NativeProjectResolver::new(vec![]);
    assert_eq!(
        resolver.provider_id_for_source("/foo.vue"),
        Some("/foo.vue.verter.ts".to_string()),
        "Vue file should get the reserved .verter.ts API suffix even without project ownership"
    );
}

#[test]
fn provider_id_for_source_non_vue_without_ownership() {
    let resolver = NativeProjectResolver::new(vec![]);
    assert_eq!(
        resolver.provider_id_for_source("/foo.ts"),
        Some("/foo.ts".to_string()),
        "Non-Vue file should return as-is even without project ownership"
    );
}

#[test]
fn provider_id_for_source_custom_ext_without_ownership() {
    let resolver = NativeProjectResolver::new(vec![]);
    assert_eq!(
        resolver.provider_id_for_source("/foo.custom"),
        Some("/foo.custom".to_string()),
        "Custom extension should return as-is even without project ownership"
    );
}

#[test]
fn provider_ide_id_for_source_vue_tsx_without_ownership() {
    let resolver = NativeProjectResolver::new(vec![]);
    assert_eq!(
        resolver.provider_ide_id_for_source("/foo.vue", false),
        Some("/foo.vue.tsx".to_string()),
        "Vue file should get .tsx suffix without ownership"
    );
}

#[test]
fn provider_ide_id_for_source_vue_jsx_without_ownership() {
    let resolver = NativeProjectResolver::new(vec![]);
    assert_eq!(
        resolver.provider_ide_id_for_source("/foo.vue", true),
        Some("/foo.vue.jsx".to_string()),
        "Vue file with is_jsx should get .jsx suffix without ownership"
    );
}

#[test]
fn provider_ide_id_for_source_svelte_jsx_without_ownership() {
    let resolver = NativeProjectResolver::new(vec![]);
    assert_eq!(
        resolver.provider_ide_id_for_source("/foo.svelte", true),
        Some("/foo.svelte.jsx".to_string()),
        "JavaScript Svelte files should get the descriptor-owned .jsx suffix"
    );
}

#[test]
fn provider_ide_id_for_source_non_vue_returns_none() {
    let resolver = NativeProjectResolver::new(vec![]);
    assert_eq!(
        resolver.provider_ide_id_for_source("/foo.ts", false),
        None,
        "Non-Vue file should still return None for IDE path"
    );
}

#[test]
fn type_import_package_with_pnpm_realpath_prefers_types_entry() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/vue-router/package.json",
        "/workspace/node_modules/vue-router/index.cjs",
        "/workspace/node_modules/vue-router/dist/vue-router.js",
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/package.json",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/index.cjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.js",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
    ]);
    let package_json = r#"{
        "name": "vue-router",
        "main": "index.cjs",
        "module": "dist/vue-router.js",
        "types": "dist/vue-router.d.ts",
        "exports": {
            ".": {
                "types": "./dist/vue-router.d.ts",
                "node": {
                    "import": "./vue-router.node.mjs",
                    "require": "./index.cjs"
                },
                "import": "./dist/vue-router.js",
                "require": "./index.cjs"
            }
        }
    }"#;
    reader.add_file(
        "/workspace/node_modules/vue-router/package.json",
        package_json,
    );
    reader.add_file(
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/package.json",
        package_json,
    );
    reader.add_file(
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts",
        "export interface RouterLinkProps { to: string }",
    );
    reader.add_file(
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
        "export interface RouterLinkProps { to: string }",
    );
    reader.add_file(
        "/workspace/node_modules/vue-router/dist/vue-router.js",
        "export const runtimeOnly = true",
    );
    reader.add_file(
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.js",
        "export const runtimeOnly = true",
    );
    reader.add_file(
        "/workspace/node_modules/vue-router/index.cjs",
        "module.exports = {}",
    );
    reader.add_file(
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/index.cjs",
        "module.exports = {}",
    );
    reader.add_realpath(
        "/workspace/node_modules/vue-router/package.json",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/package.json",
    );
    reader.add_realpath(
        "/workspace/node_modules/vue-router/index.cjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/index.cjs",
    );
    reader.add_realpath(
        "/workspace/node_modules/vue-router/dist/vue-router.js",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.js",
    );
    reader.add_realpath(
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
    );

    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.vue".to_string(),
                specifier: "vue-router".to_string(),
                kind: ResolveRequestKind::TypeImport,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect("TypeImport should resolve through the pnpm realpath");

    assert_eq!(
        result.source_id,
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
        "TypeImport should still prefer the package types entry when package files realpath into pnpm store locations",
    );
}

#[test]
fn type_import_package_with_nested_node_conditions_and_pnpm_realpath_prefers_types_entry() {
    let resolver = ProjectResolver::new(vec![project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    )]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/vue-router/package.json",
        "/workspace/node_modules/vue-router/index.cjs",
        "/workspace/node_modules/vue-router/dist/vue-router.cjs",
        "/workspace/node_modules/vue-router/dist/vue-router.prod.cjs",
        "/workspace/node_modules/vue-router/dist/vue-router.js",
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts",
        "/workspace/node_modules/vue-router/vue-router.node.mjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/package.json",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/index.cjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.cjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.prod.cjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.js",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/vue-router.node.mjs",
    ]);
    let package_json = r#"{
        "name": "vue-router",
        "main": "index.cjs",
        "module": "dist/vue-router.js",
        "types": "dist/vue-router.d.ts",
        "exports": {
            ".": {
                "types": "./dist/vue-router.d.ts",
                "node": {
                    "import": {
                        "production": "./vue-router.node.mjs",
                        "development": "./vue-router.node.mjs",
                        "default": "./vue-router.node.mjs"
                    },
                    "require": {
                        "production": "./dist/vue-router.prod.cjs",
                        "development": "./dist/vue-router.cjs",
                        "default": "./index.cjs"
                    }
                },
                "import": "./dist/vue-router.js",
                "require": "./index.cjs"
            }
        }
    }"#;
    reader.add_file(
        "/workspace/node_modules/vue-router/package.json",
        package_json,
    );
    reader.add_file(
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/package.json",
        package_json,
    );
    for path in [
        "/workspace/node_modules/vue-router/index.cjs",
        "/workspace/node_modules/vue-router/dist/vue-router.cjs",
        "/workspace/node_modules/vue-router/dist/vue-router.prod.cjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/index.cjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.cjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.prod.cjs",
    ] {
        reader.add_file(path, "module.exports = {}");
    }
    for path in [
        "/workspace/node_modules/vue-router/dist/vue-router.js",
        "/workspace/node_modules/vue-router/vue-router.node.mjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.js",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/vue-router.node.mjs",
    ] {
        reader.add_file(path, "export const runtimeOnly = true");
    }
    for path in [
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
    ] {
        reader.add_file(path, "export interface RouterLinkProps { to: string }");
    }
    for (path, realpath) in [
        (
            "/workspace/node_modules/vue-router/package.json",
            "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/package.json",
        ),
        (
            "/workspace/node_modules/vue-router/index.cjs",
            "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/index.cjs",
        ),
        (
            "/workspace/node_modules/vue-router/dist/vue-router.cjs",
            "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.cjs",
        ),
        (
            "/workspace/node_modules/vue-router/dist/vue-router.prod.cjs",
            "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.prod.cjs",
        ),
        (
            "/workspace/node_modules/vue-router/dist/vue-router.js",
            "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.js",
        ),
        (
            "/workspace/node_modules/vue-router/dist/vue-router.d.ts",
            "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
        ),
        (
            "/workspace/node_modules/vue-router/vue-router.node.mjs",
            "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/vue-router.node.mjs",
        ),
    ] {
        reader.add_realpath(path, realpath);
    }

    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.vue".to_string(),
                specifier: "vue-router".to_string(),
                kind: ResolveRequestKind::TypeImport,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect("TypeImport should resolve through the pnpm realpath");

    assert_eq!(
        result.source_id,
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
        "TypeImport should still prefer the package types entry with nested node/import/require conditions",
    );
}

#[test]
fn tsconfig_path_to_package_dir_respects_type_import_context() {
    let mut project = project(
        "/workspace",
        "/workspace",
        Some("/workspace/tsconfig.json"),
        ProjectMembership::MatchAll,
    );
    project.compiler_options = IdeProjectCompilerOptions {
        base_url: Some("/workspace".to_string()),
        paths: vec![(
            "vue-router".to_string(),
            vec![
                "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router"
                    .to_string(),
            ],
        )],
        ..Default::default()
    };
    let resolver = ProjectResolver::new(vec![project]);
    let mut reader = TestReader::with_files(&[
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/package.json",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/index.cjs",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.js",
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
    ]);
    reader.add_file(
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/package.json",
        r#"{
            "name": "vue-router",
            "main": "index.cjs",
            "module": "dist/vue-router.js",
            "types": "dist/vue-router.d.ts",
            "exports": {
                ".": {
                    "types": "./dist/vue-router.d.ts",
                    "import": "./dist/vue-router.js",
                    "require": "./index.cjs"
                }
            }
        }"#,
    );
    reader.add_file(
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/index.cjs",
        "module.exports = {}",
    );
    reader.add_file(
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.js",
        "export const runtimeOnly = true",
    );
    reader.add_file(
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
        "export interface RouterLinkProps { to: string }",
    );

    let result = resolver
        .resolve_with_reader(
            &reader,
            &ResolveRequest {
                importer_id: "/workspace/src/App.vue".to_string(),
                specifier: "vue-router".to_string(),
                kind: ResolveRequestKind::TypeImport,
                phase: ResolvePhase::CodegenBlocker,
            },
        )
        .expect("TypeImport should resolve through tsconfig paths");

    assert_eq!(
        result.source_id,
        "/workspace/node_modules/.pnpm/vue-router@5.0.3/node_modules/vue-router/dist/vue-router.d.ts",
        "TypeImport through tsconfig paths should honor declaration/package types instead of probing runtime index.cjs",
    );
}

// ── Ancestor Walk Boundary Tests ──

#[test]
fn ancestor_dirs_stops_at_workspace_root_boundary() {
    let dirs = ancestor_dirs("/workspace/packages/app/src/file.ts", Some("/workspace"));
    assert!(
        dirs.contains(&"/workspace/packages/app/src".to_string()),
        "should include directories within workspace"
    );
    assert!(
        dirs.contains(&"/workspace/packages/app".to_string()),
        "should include parent directories within workspace"
    );
    assert!(
        dirs.contains(&"/workspace".to_string()),
        "should include the workspace root itself"
    );
    assert!(
        !dirs.iter().any(|d| d == "/"),
        "must NOT traverse above workspace root to filesystem root"
    );
}

#[test]
fn ancestor_dirs_from_dir_stops_at_workspace_root_boundary() {
    let dirs = ancestor_dirs_from_dir("/workspace/packages/app/src", Some("/workspace"));
    assert!(
        dirs.contains(&"/workspace/packages/app/src".to_string()),
        "should include the start directory itself"
    );
    assert!(
        dirs.contains(&"/workspace".to_string()),
        "should include the workspace root"
    );
    assert!(
        !dirs.iter().any(|d| d == "/"),
        "must NOT traverse above workspace root"
    );
}

#[test]
fn ancestor_dirs_unbounded_traverses_all_ancestors() {
    // Without a boundary, ancestor_dirs walks up to the empty-string termination
    // (parent_dir("/workspace") returns "" which terminates the loop).
    let dirs = ancestor_dirs("/a/b/c/d/file.ts", None);
    assert_eq!(
        dirs,
        vec![
            "/a/b/c/d".to_string(),
            "/a/b/c".to_string(),
            "/a/b".to_string(),
            "/a".to_string(),
        ],
        "unbounded traversal should visit all ancestor directories"
    );
}

#[test]
fn ancestor_dirs_from_dir_unbounded_traverses_all_ancestors() {
    let dirs = ancestor_dirs_from_dir("/a/b/c", None);
    assert_eq!(
        dirs,
        vec!["/a/b/c".to_string(), "/a/b".to_string(), "/a".to_string(),],
        "unbounded traversal should visit start dir and all ancestors"
    );
}

#[test]
fn owned_resolution_stops_node_modules_walk_at_workspace_root() {
    // Monorepo: workspace_root=/workspace, project_root=/workspace/packages/app
    // package lives in /workspace/node_modules (hoisted), NOT /workspace/packages/app/node_modules
    let mut configured = project(
        "/workspace/packages/app",
        "/workspace",
        Some("/workspace/packages/app/tsconfig.json"),
        ProjectMembership::MatchAll,
    );
    configured.compiler_options = IdeProjectCompilerOptions::default();

    let resolver = ProjectResolver::new(vec![configured]);

    let mut reader = CountingReader::with_files(&[]);
    reader.add_file(
        "/workspace/node_modules/lodash/package.json",
        r#"{ "name": "lodash", "main": "lodash.js", "types": "lodash.d.ts" }"#,
    );
    reader.add_file(
        "/workspace/node_modules/lodash/lodash.d.ts",
        "export function get(): void;",
    );

    // Resolution should find hoisted package under workspace_root/node_modules
    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/workspace/packages/app/src/main.ts".to_string(),
            specifier: "lodash".to_string(),
            kind: ResolveRequestKind::TypeImport,
            phase: ResolvePhase::CodegenBlocker,
        },
    );

    assert!(
        result.is_some(),
        "should resolve hoisted package from workspace_root/node_modules"
    );
    assert_eq!(
        result.unwrap().source_id,
        "/workspace/node_modules/lodash/lodash.d.ts",
    );

    // Verify no probes above workspace_root
    assert_eq!(
        reader.read_file_calls_for("/package.json"),
        0,
        "must NOT probe /package.json above workspace_root"
    );
    assert_eq!(
        reader.read_file_calls_for("/node_modules/lodash/package.json"),
        0,
        "must NOT probe /node_modules/ above workspace_root"
    );
}

#[test]
fn package_imports_obey_workspace_root_boundary() {
    let mut configured = project(
        "/workspace/packages/app",
        "/workspace",
        Some("/workspace/packages/app/tsconfig.json"),
        ProjectMembership::MatchAll,
    );
    configured.compiler_options = IdeProjectCompilerOptions::default();

    let resolver = ProjectResolver::new(vec![configured]);

    let mut reader = CountingReader::with_files(&[]);
    reader.add_file(
        "/workspace/packages/app/package.json",
        r##"{ "name": "@myapp/app", "imports": { "#utils": "./src/utils/index.ts" } }"##,
    );
    reader.add_file(
        "/workspace/packages/app/src/utils/index.ts",
        "export function util() {}",
    );

    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/workspace/packages/app/src/main.ts".to_string(),
            specifier: "#utils".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::CodegenBlocker,
        },
    );

    assert!(
        result.is_some(),
        "should resolve #imports from owning package.json"
    );

    // Verify no probes above workspace_root
    assert_eq!(
        reader.read_file_calls_for("/package.json"),
        0,
        "must NOT probe /package.json above workspace_root for #imports"
    );
}

#[test]
fn unowned_resolution_remains_unbounded() {
    // When no project owner exists, resolution should not be bounded
    let resolver = ProjectResolver::new(vec![]);

    let mut reader = CountingReader::with_files(&[]);
    reader.add_file(
        "/deep/nested/project/node_modules/foo/package.json",
        r#"{ "name": "foo", "main": "index.js" }"#,
    );
    reader.add_file(
        "/deep/nested/project/node_modules/foo/index.js",
        "module.exports = {}",
    );

    let result = resolver.resolve_with_reader(
        &reader,
        &ResolveRequest {
            importer_id: "/deep/nested/project/src/file.ts".to_string(),
            specifier: "foo".to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::CodegenBlocker,
        },
    );

    assert!(
        result.is_some(),
        "unowned resolution should still find packages (unbounded walk)"
    );
}

// ── normalize_canonical_id / collapse_path — delegation to canonical owner ──

#[test]
fn normalize_canonical_id_strips_trailing_slash_via_owner() {
    // FIX 4 regression: delegating to the canonical owner strips a strippable
    // trailing slash (the old impl left `c:/x/y/`).
    assert_eq!(normalize_canonical_id("c:/x/y/"), "c:/x/y");
    // UNC: `//?/UNC/` stripped before `//?/`.
    assert_eq!(normalize_canonical_id("//?/UNC/s/sh/f"), "//s/sh/f");
    // drive lowering + backslash still applied.
    assert_eq!(normalize_canonical_id("D:\\x\\y"), "d:/x/y");
    // roots preserved (not stripped).
    assert_eq!(normalize_canonical_id("c:/"), "c:/");
    assert_eq!(normalize_canonical_id("/"), "/");
}

#[test]
fn collapse_path_still_collapses_dot_and_dotdot() {
    // FIX 4: collapse_path keeps its `.`/`..` segment-collapse semantics even
    // though its front-half normalization now delegates to the owner.
    assert_eq!(collapse_path("c:/a/./b/../c"), "c:/a/c");
    assert_eq!(collapse_path("/a/b/../c"), "/a/c");
    assert_eq!(collapse_path("/a/./b/"), "/a/b");
}

#[test]
fn collapse_path_preserves_unc_host_prefix() {
    // Now that the URI/owner layer produces `//server/share/...` for UNC files,
    // collapse_path must NOT flatten the `//` host prefix to a single `/` — that
    // would give the same UNC file two canonical IDs. Pre-fix `//server/share`
    // collapsed to `/server/share`; this asserts the UNC prefix survives the
    // `.`/`..` collapse.
    assert_eq!(
        collapse_path("//server/share/proj/src/../Foo.vue"),
        "//server/share/proj/Foo.vue"
    );
    assert_eq!(collapse_path("//server/share/a/./b"), "//server/share/a/b");
    assert_eq!(collapse_path("//server/share"), "//server/share");
    // NEGATIVE: must not degrade to a single-slash absolute path.
    assert_ne!(collapse_path("//server/share/x"), "/server/share/x");

    // `..` must NOT escape the UNC share root (`//host/share` is immutable, like
    // `/` or a drive root). `..` directly under the share is a no-op, NOT a pop
    // of the share/host segment.
    assert_eq!(
        collapse_path("//server/share/../App.vue"),
        "//server/share/App.vue"
    );
    assert_eq!(collapse_path("//server/share/.."), "//server/share");
    assert_eq!(
        collapse_path("//server/share/a/../../b"),
        "//server/share/b"
    );
    // NEGATIVE: the share segment must survive — never `//server/App.vue`.
    assert_ne!(
        collapse_path("//server/share/../App.vue"),
        "//server/App.vue"
    );
    assert_ne!(collapse_path("//server/share/.."), "//server");
}

#[test]
fn join_paths_preserves_unc_host_prefix() {
    // The flow codexB cited: a relative import resolved against a UNC base must
    // keep the `//` host prefix (join_paths routes through collapse_path).
    assert_eq!(
        join_paths("//server/share/proj/src", "./Foo.vue"),
        "//server/share/proj/src/Foo.vue"
    );
    assert_eq!(
        join_paths("//server/share/proj/src", "../Bar.vue"),
        "//server/share/proj/Bar.vue"
    );
    assert_ne!(
        join_paths("//server/share/proj/src", "./Foo.vue"),
        "/server/share/proj/src/Foo.vue"
    );
}

// ── Project-reference cycle termination ──

/// Builds a package project rooted at `/workspace/packages/{name}` with its
/// tsconfig at `/workspace/packages/{name}/tsconfig.json`, referencing the
/// given tsconfig paths.
fn referencing_project(name: &str, references: &[&str]) -> IdeProjectConfig {
    let root = format!("/workspace/packages/{name}");
    let mut config = project(
        &root,
        "/workspace",
        Some(&format!("{root}/tsconfig.json")),
        ProjectMembership::MatchAll,
    );
    config.references = references.iter().map(|r| (*r).to_string()).collect();
    config
}

/// A project that resolves the given bare specifier to `src/index.ts` under
/// its own root via tsconfig `paths` + `baseUrl`.
fn resolving_project(name: &str, specifier: &str) -> IdeProjectConfig {
    let mut config = referencing_project(name, &[]);
    config.compiler_options = IdeProjectCompilerOptions {
        base_url: Some(format!("/workspace/packages/{name}/src")),
        paths: vec![(specifier.to_string(), vec!["index".to_string()])],
        ..Default::default()
    };
    config
}

fn resolve_bare(
    resolver: &ProjectResolver,
    reader: &TestReader,
    importer_id: &str,
    specifier: &str,
) -> Option<ResolveResult> {
    resolver.resolve_with_reader(
        reader,
        &ResolveRequest {
            importer_id: importer_id.to_string(),
            specifier: specifier.to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::ProviderGraph,
        },
    )
}

#[test]
fn cyclic_project_references_terminate_without_overflow() {
    // Two-cycle: A references B's tsconfig, B references A's. A specifier
    // that no branch resolves must return None instead of recursing across
    // the reference cycle until the stack overflows.
    let a = referencing_project("a", &["/workspace/packages/b/tsconfig.json"]);
    let b = referencing_project("b", &["/workspace/packages/a/tsconfig.json"]);
    let resolver = ProjectResolver::new(vec![a, b]);
    let reader = TestReader::default();

    let resolved = resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/a/src/App.ts",
        "missing-lib",
    );
    assert!(
        resolved.is_none(),
        "unresolvable specifier across a reference cycle must be None"
    );

    // Control: the same shape WITHOUT the back-edge resolves through the
    // single reference — cycle termination must not disable reference
    // resolution itself.
    let a = referencing_project("a", &["/workspace/packages/b/tsconfig.json"]);
    let b = resolving_project("b", "shared");
    let resolver = ProjectResolver::new(vec![a, b]);
    let reader = TestReader::with_files(&["/workspace/packages/b/src/index.ts"]);

    let resolved = resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/a/src/App.ts",
        "shared",
    )
    .expect("non-cyclic single reference must still resolve");
    assert_eq!(resolved.source_id, "/workspace/packages/b/src/index.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::ProjectReference);
}

#[test]
fn n_cycle_project_references_terminate() {
    // Three-cycle: A → B → C → A. Must terminate with None, not overflow.
    let a = referencing_project("a", &["/workspace/packages/b/tsconfig.json"]);
    let b = referencing_project("b", &["/workspace/packages/c/tsconfig.json"]);
    let c = referencing_project("c", &["/workspace/packages/a/tsconfig.json"]);
    let resolver = ProjectResolver::new(vec![a, b, c]);
    let reader = TestReader::default();

    let resolved = resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/a/src/App.ts",
        "missing-lib",
    );
    assert!(
        resolved.is_none(),
        "unresolvable specifier across a 3-cycle must be None"
    );
}

#[test]
fn deep_acyclic_project_reference_chain_still_resolves() {
    // Acyclic chain A → B → C → D where only D resolves the specifier. The
    // terminal project must still be reached: cycle/stack guards must not
    // block legitimately deep transitive reference chains, and declared
    // first-match-wins ordering must hold.
    let a = referencing_project("a", &["/workspace/packages/b/tsconfig.json"]);
    let b = referencing_project("b", &["/workspace/packages/c/tsconfig.json"]);
    let c = referencing_project("c", &["/workspace/packages/d/tsconfig.json"]);
    let d = resolving_project("d", "deep-lib");
    let resolver = ProjectResolver::new(vec![a, b, c, d]);
    let reader = TestReader::with_files(&["/workspace/packages/d/src/index.ts"]);

    let resolved = resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/a/src/App.ts",
        "deep-lib",
    )
    .expect("deep acyclic reference chain must resolve through the terminal project");
    assert_eq!(resolved.source_id, "/workspace/packages/d/src/index.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::ProjectReference);
    assert_eq!(
        resolved.owner_tsconfig_path.as_deref(),
        Some("/workspace/packages/d/tsconfig.json")
    );
}

#[test]
fn cyclic_branch_skipped_but_sibling_reference_resolves() {
    // Importer project A references [B, C] in declared order. B references
    // back to A (a cycle); C resolves the specifier. The cyclic B branch is
    // skipped without poisoning the walk — the later sibling C must still
    // resolve.
    let a = referencing_project(
        "a",
        &[
            "/workspace/packages/b/tsconfig.json",
            "/workspace/packages/c/tsconfig.json",
        ],
    );
    let b = referencing_project("b", &["/workspace/packages/a/tsconfig.json"]);
    let c = resolving_project("c", "sib");
    let resolver = ProjectResolver::new(vec![a, b, c]);
    let reader = TestReader::with_files(&["/workspace/packages/c/src/index.ts"]);

    let resolved = resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/a/src/App.ts",
        "sib",
    )
    .expect("sibling reference after a cyclic branch must still resolve");
    assert_eq!(resolved.source_id, "/workspace/packages/c/src/index.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::ProjectReference);

    // NEGATIVE: a specifier nothing resolves still terminates as None even
    // with the cyclic branch present.
    assert!(resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/a/src/App.ts",
        "missing-lib",
    )
    .is_none());
}

/// Builds `count` chained projects `{prefix}0 → {prefix}1 → …`, each
/// referencing the next one's tsconfig (acyclic by construction). The LAST
/// project resolves `specifier` via its own tsconfig `paths` + `baseUrl`.
fn chained_projects(prefix: &str, count: usize, specifier: &str) -> Vec<IdeProjectConfig> {
    (0..count)
        .map(|i| {
            let name = format!("{prefix}{i}");
            if i + 1 == count {
                resolving_project(&name, specifier)
            } else {
                let next = format!("/workspace/packages/{prefix}{}/tsconfig.json", i + 1);
                referencing_project(&name, &[next.as_str()])
            }
        })
        .collect()
}

#[test]
fn diamond_project_references_resolve_through_both_arms() {
    // Diamond whose arms share one long chain: A references [B, C]; B reaches
    // the shared chain head l1 through a 10-project prefix (p1..p10), C
    // references l1 directly; the chain l1 → … → l250 ends in a reference to
    // R, the only project resolving the specifier.
    //
    // The B arm runs first and walks the shared chain, but its longer prefix
    // exhausts the depth fuse before reaching R (1 + 10 + 250 > 256), so it
    // pushes and pops l1..l245 on the way out. The C arm then re-enters the
    // SAME chain on a shorter path (1 + 250 <= 256) and resolves through R.
    // This discriminates push-on-descend/pop-on-return: if the active-set pop
    // (or the depth restore) on branch return were missing, the C arm would
    // see l1 as still active (or run on a drained budget), skip the chain,
    // and return None instead of Some.
    let mut projects = vec![
        referencing_project(
            "a",
            &[
                "/workspace/packages/b/tsconfig.json",
                "/workspace/packages/c/tsconfig.json",
            ],
        ),
        referencing_project("b", &["/workspace/packages/p1/tsconfig.json"]),
        referencing_project("c", &["/workspace/packages/l1/tsconfig.json"]),
    ];
    for i in 1..=10usize {
        let next = if i < 10 {
            format!("/workspace/packages/p{}/tsconfig.json", i + 1)
        } else {
            "/workspace/packages/l1/tsconfig.json".to_string()
        };
        projects.push(referencing_project(&format!("p{i}"), &[next.as_str()]));
    }
    for i in 1..=250usize {
        let next = if i < 250 {
            format!("/workspace/packages/l{}/tsconfig.json", i + 1)
        } else {
            "/workspace/packages/r/tsconfig.json".to_string()
        };
        projects.push(referencing_project(&format!("l{i}"), &[next.as_str()]));
    }
    projects.push(resolving_project("r", "diamond-lib"));
    let resolver = ProjectResolver::new(projects);
    let reader = TestReader::with_files(&["/workspace/packages/r/src/index.ts"]);

    let resolved = resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/a/src/App.ts",
        "diamond-lib",
    )
    .expect("second diamond arm must re-enter the shared chain the first arm popped");
    assert_eq!(resolved.source_id, "/workspace/packages/r/src/index.ts");
    assert_eq!(resolved.resolution_kind, ResolutionKind::ProjectReference);

    // NEGATIVE: a specifier nothing resolves still terminates as None across
    // the same diamond instead of hanging or overflowing.
    assert!(resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/a/src/App.ts",
        "missing-lib",
    )
    .is_none());
}

#[test]
fn project_reference_depth_budget_bounds_deep_chain() {
    // Over-budget side: an ACYCLIC chain of 300 linked projects whose LAST
    // project is the only resolver. No cycle exists, so the active-set guard
    // never fires — only the depth fuse bounds this walk. The terminal
    // project sits beyond PROJECT_REFERENCE_DEPTH_LIMIT, so the fuse must
    // stop the descent and return None rather than walk (or overflow into)
    // arbitrarily deep reference chains.
    let projects = chained_projects("deep", 300, "over-lib");
    let resolver = ProjectResolver::new(projects);
    let reader = TestReader::with_files(&["/workspace/packages/deep299/src/index.ts"]);

    let resolved = resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/deep0/src/App.ts",
        "over-lib",
    );
    // NEGATIVE: the beyond-budget terminal resolver must NOT be reached.
    assert!(
        resolved.is_none(),
        "resolver beyond the depth budget must not be reached: {resolved:?}"
    );

    // Under-budget side: the same shape at 10 projects — comfortably inside
    // the fuse. The budget must not cut off legitimate sub-budget chains.
    let projects = chained_projects("short", 10, "under-lib");
    let resolver = ProjectResolver::new(projects);
    let reader = TestReader::with_files(&["/workspace/packages/short9/src/index.ts"]);

    let resolved = resolve_bare(
        &resolver,
        &reader,
        "/workspace/packages/short0/src/App.ts",
        "under-lib",
    )
    .expect("sub-budget acyclic chain must still resolve through its terminal project");
    assert_eq!(
        resolved.source_id,
        "/workspace/packages/short9/src/index.ts"
    );
    assert_eq!(resolved.resolution_kind, ResolutionKind::ProjectReference);
}
