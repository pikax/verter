use std::collections::HashSet;
use std::sync::Arc;

use dashmap::DashMap;
use verter_host::{FileKind, Hash16, UpsertRequest, VerterHost};

/// Reader that resolves content from the host first, then falls back to disk.
pub struct HostFsProjectResolverReader<'a> {
    host: &'a VerterHost,
}

impl<'a> HostFsProjectResolverReader<'a> {
    pub fn new(host: &'a VerterHost) -> Self {
        Self { host }
    }
}

pub fn normalize_fs_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if let Some(stripped) = normalized.strip_prefix("//?/UNC/") {
        return format!("//{stripped}");
    }
    normalized
        .strip_prefix("//?/")
        .unwrap_or(normalized.as_str())
        .to_string()
}

impl verter_vfs::WorkspaceAccess for HostFsProjectResolverReader<'_> {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        ensure_source_loaded_into_host(self.host, canonical_id);
        self.host.get_source(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.host.get_source(canonical_id).is_some()
            || self.host.workspace().file_exists(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        if self.host.get_source(canonical_id).is_some() {
            return Some(normalize_fs_path(canonical_id));
        }
        self.host.workspace().realpath(canonical_id)
    }
}

/// Single filesystem ingress for source content reads.
///
/// If the source is already in the host, returns `true` immediately.
/// Otherwise reads from disk, upserts into the host, and returns `true`
/// on success. Returns `false` if the file cannot be read.
///
/// All source content reads should go through this function to ensure
/// files are loaded into the host exactly once via a single code path.
pub fn ensure_source_loaded_into_host(host: &VerterHost, canonical_id: &str) -> bool {
    if host.get_source(canonical_id).is_some() {
        return true;
    }
    let Some(source) = host.workspace().read_file(canonical_id) else {
        return false;
    };
    host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source,
        file_kind: file_kind_for_canonical_id(canonical_id),
        aliases: Vec::new(),
    })
    .is_ok()
}

/// Cached hydration entry. Only complete hydrations are cached.
#[derive(Debug, Clone)]
struct HydrationCacheEntry {
    /// Semantic hash of the file at hydration time.
    source_hash: Hash16,
    /// Resolver generation at hydration time.
    resolver_generation: u64,
}

/// Cache for compile blocker hydrations. Keyed by canonical ID.
/// Thread-safe via DashMap. Only stores entries for **complete** hydrations
/// (all specifiers resolved). Invalidated by semantic hash mismatch.
pub struct HydrationCache {
    entries: DashMap<String, HydrationCacheEntry>,
}

impl Default for HydrationCache {
    fn default() -> Self {
        Self::new()
    }
}

impl HydrationCache {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Check if a file's hydration is still valid (hash + generation match).
    fn is_valid(&self, canonical_id: &str, current_hash: Hash16, resolver_generation: u64) -> bool {
        self.entries
            .get(canonical_id)
            .map(|entry| {
                entry.source_hash == current_hash
                    && entry.resolver_generation == resolver_generation
            })
            .unwrap_or(false)
    }

    /// Record a successful complete hydration.
    fn insert(&self, canonical_id: &str, source_hash: Hash16, resolver_generation: u64) {
        self.entries.insert(
            canonical_id.to_string(),
            HydrationCacheEntry {
                source_hash,
                resolver_generation,
            },
        );
    }

    /// Remove cache entry for a file (e.g. on file removal).
    pub fn remove(&self, canonical_id: &str) {
        self.entries.remove(canonical_id);
    }
}

/// Outcome of a pre-snapshot blocker hydration.
pub struct HydrationOutcome {
    /// `true` when all blocker specifiers were resolved.
    /// `false` when bare/alias specifiers were deferred.
    pub complete: bool,
}

/// Cache-aware wrapper around `hydrate_vue_compile_blockers`.
///
/// Checks the hydration cache first. If the file's semantic hash and resolver
/// generation match the cached entry, hydration is skipped. On success,
/// inserts a new cache entry.
pub fn hydrate_cached(
    cache: &HydrationCache,
    host: &VerterHost,
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_vfs::WorkspaceAccess,
    canonical_id: &str,
    resolver_generation: u64,
) {
    let Some(current_hash) = host.get_semantic_hash(canonical_id) else {
        return;
    };
    if cache.is_valid(canonical_id, current_hash, resolver_generation) {
        return;
    }
    hydrate_vue_compile_blockers(host, resolver, reader, canonical_id);
    cache.insert(canonical_id, current_hash, resolver_generation);
}

/// Pre-snapshot blocker hydration: resolves only relative/absolute specifiers.
///
/// For each compile blocker specifier:
/// - **Relative/absolute** (`./`, `../`, `/`): probe disk with `expand_relative_candidates()`
/// - **Bare/alias**: skip (deferred to real resolver post-snapshot)
///
/// Returns `HydrationOutcome { complete: false }` when any specifiers were skipped.
pub fn hydrate_vue_compile_blockers_pre_snapshot(
    host: &VerterHost,
    canonical_id: &str,
) -> HydrationOutcome {
    let mut complete = true;
    let mut pending = vec![canonical_id.to_string()];
    let mut seen = HashSet::new();

    while let Some(source_id) = pending.pop() {
        if !seen.insert(source_id.clone()) {
            continue;
        }

        if !ensure_source_loaded_into_host(host, &source_id) {
            continue;
        }

        if source_id.ends_with(".vue") {
            if let Some(blockers) = host.get_compile_blockers(&source_id) {
                for request in blockers.external_source_requests {
                    if !is_relative_or_absolute(&request.specifier) {
                        complete = false;
                        continue;
                    }
                    if let Some(loaded_id) =
                        probe_and_load_relative(host, &source_id, &request.specifier)
                    {
                        pending.push(loaded_id);
                    }
                }

                for dep in blockers.macro_type_deps.iter() {
                    if !is_relative_or_absolute(&dep.import_source) {
                        complete = false;
                        continue;
                    }
                    if let Some(loaded_id) =
                        probe_and_load_relative(host, &source_id, &dep.import_source)
                    {
                        pending.push(loaded_id);
                    }
                }
            }
        }
    }

    HydrationOutcome { complete }
}

/// Check if a specifier is relative (`./`, `../`) or absolute (`/`).
fn is_relative_or_absolute(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/')
}

/// Probe disk for a relative specifier using the host's resolve extensions.
/// Returns the canonical ID of the first candidate found on disk.
fn probe_and_load_relative(
    host: &VerterHost,
    owner_canonical: &str,
    specifier: &str,
) -> Option<String> {
    let candidates = host.expand_relative_candidates(owner_canonical, specifier);
    for candidate in candidates {
        let normalized = normalize_fs_path(&candidate);
        if std::path::Path::new(&normalized).is_file()
            && ensure_source_loaded_into_host(host, &candidate)
        {
            return Some(candidate);
        }
    }
    None
}

pub fn hydrate_vue_compile_blockers(
    host: &VerterHost,
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_vfs::WorkspaceAccess,
    canonical_id: &str,
) {
    let mut pending = vec![canonical_id.to_string()];
    let mut seen = HashSet::new();

    while let Some(source_id) = pending.pop() {
        if !seen.insert(source_id.clone()) {
            continue;
        }

        if !load_resolved_file_into_host(host, reader, &source_id) {
            continue;
        }

        if source_id.ends_with(".vue") {
            if let Some(blockers) = host.get_compile_blockers(&source_id) {
                for request in blockers.external_source_requests {
                    if let Some(loaded_id) = resolve_and_load_blocker(
                        host,
                        resolver,
                        reader,
                        &source_id,
                        &request.specifier,
                        crate::project_resolver::ResolveRequestKind::SfcSrcAttr,
                    ) {
                        pending.push(loaded_id);
                    }
                }

                for dep in blockers.macro_type_deps.iter() {
                    if let Some(loaded_id) = resolve_and_load_blocker(
                        host,
                        resolver,
                        reader,
                        &source_id,
                        &dep.import_source,
                        crate::project_resolver::ResolveRequestKind::TypeImport,
                    ) {
                        pending.push(loaded_id);
                    }
                }
            }
        }

        let Some(analysis) = host.get_analysis(&source_id) else {
            continue;
        };
        for (specifier, dep_id) in collect_resolved_codegen_dependencies(
            resolver,
            reader,
            &source_id,
            &analysis.module_references,
        ) {
            if dep_id == source_id {
                continue;
            }
            if track_loaded_dependency(host, reader, &source_id, &specifier, &dep_id) {
                pending.push(dep_id);
            }
        }
    }
}

pub fn file_kind_for_canonical_id(canonical_id: &str) -> FileKind {
    if canonical_id.ends_with(".vue") {
        FileKind::VueSfc
    } else {
        FileKind::NonSfc
    }
}

fn load_resolved_file_into_host(
    host: &VerterHost,
    reader: &dyn verter_vfs::WorkspaceAccess,
    canonical_id: &str,
) -> bool {
    // Try the ingress (real filesystem) first, then fall back to the reader
    // (which may provide in-memory content in tests or from other sources).
    if ensure_source_loaded_into_host(host, canonical_id) {
        return true;
    }
    let Some(source) = reader.read_file(canonical_id) else {
        return false;
    };
    host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source,
        file_kind: file_kind_for_canonical_id(canonical_id),
        aliases: Vec::new(),
    })
    .is_ok()
}

fn track_loaded_dependency(
    host: &VerterHost,
    reader: &dyn verter_vfs::WorkspaceAccess,
    owner_id: &str,
    specifier: &str,
    dep_id: &str,
) -> bool {
    if owner_id == dep_id {
        return false;
    }

    if !load_resolved_file_into_host(host, reader, dep_id) {
        return false;
    }

    host.set_import_dependencies(
        owner_id,
        vec![verter_host::DependencyResolution {
            specifier: specifier.to_string(),
            resolved_canonical_id: Some(dep_id.to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    true
}

fn resolve_and_load_blocker(
    host: &VerterHost,
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_vfs::WorkspaceAccess,
    owner_id: &str,
    specifier: &str,
    kind: crate::project_resolver::ResolveRequestKind,
) -> Option<String> {
    let resolved = resolver.resolve_with_reader(
        reader,
        &crate::project_resolver::ResolveRequest {
            importer_id: owner_id.to_string(),
            specifier: specifier.to_string(),
            kind,
            phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
        },
    )?;

    let dep_id = resolved.source_id;
    track_loaded_dependency(host, reader, owner_id, specifier, &dep_id).then_some(dep_id)
}

fn analyzed_module_reference_request_kind(
    reference: &verter_analysis::AnalyzedModuleReference,
) -> crate::project_resolver::ResolveRequestKind {
    if reference.is_type_only {
        crate::project_resolver::ResolveRequestKind::TypeImport
    } else if reference.semantics == verter_analysis::ModuleReferenceSemantics::Require {
        crate::project_resolver::ResolveRequestKind::RequireCall
    } else {
        crate::project_resolver::ResolveRequestKind::EsmImport
    }
}

/// Returns `(specifier, resolved_canonical_id)` pairs.
fn collect_resolved_codegen_dependencies(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_vfs::WorkspaceAccess,
    importer_id: &str,
    module_references: &[verter_analysis::AnalyzedModuleReference],
) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();

    for reference in module_references {
        let kind = analyzed_module_reference_request_kind(reference);
        match reference.analyzability {
            verter_analysis::ModuleReferenceAnalyzability::Exact => {
                if let Some(specifier) = &reference.literal_specifier {
                    if let Some(result) = resolver.resolve_with_reader(
                        reader,
                        &crate::project_resolver::ResolveRequest {
                            importer_id: importer_id.to_string(),
                            specifier: specifier.clone(),
                            kind,
                            phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
                        },
                    ) {
                        if seen.insert(result.source_id.clone()) {
                            resolved.push((specifier.clone(), result.source_id));
                        }
                    }
                }
            }
            verter_analysis::ModuleReferenceAnalyzability::FiniteSet => {
                for specifier in &reference.finite_specifiers {
                    if let Some(result) = resolver.resolve_with_reader(
                        reader,
                        &crate::project_resolver::ResolveRequest {
                            importer_id: importer_id.to_string(),
                            specifier: specifier.clone(),
                            kind,
                            phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
                        },
                    ) {
                        if seen.insert(result.source_id.clone()) {
                            resolved.push((specifier.clone(), result.source_id));
                        }
                    }
                }
            }
            verter_analysis::ModuleReferenceAnalyzability::UnknownDynamic => {}
        }
    }

    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use verter_host::{CompileErrorPolicy, CompileProfile, HostConfig, HostError};

    #[derive(Default)]
    struct TestResolverReader {
        files: HashSet<String>,
        texts: HashMap<String, Arc<str>>,
    }

    impl TestResolverReader {
        fn with_texts(entries: &[(&str, &str)]) -> Self {
            let mut reader = Self::default();
            for (path, text) in entries {
                let normalized = path.replace('\\', "/");
                reader.files.insert(normalized.clone());
                reader.texts.insert(normalized, Arc::<str>::from(*text));
            }
            reader
        }
    }

    impl verter_vfs::WorkspaceAccess for TestResolverReader {
        fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
            self.texts.get(&canonical_id.replace('\\', "/")).cloned()
        }

        fn file_exists(&self, canonical_id: &str) -> bool {
            self.files.contains(&canonical_id.replace('\\', "/"))
        }

        fn realpath(&self, canonical_id: &str) -> Option<String> {
            let normalized = canonical_id.replace('\\', "/");
            self.file_exists(&normalized).then_some(normalized)
        }
    }

    fn strict_host() -> VerterHost {
        VerterHost::new_standalone(HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        })
    }

    fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(id.to_string()),
                input_id: id.to_string(),
                source: Arc::from(source),
                file_kind: FileKind::VueSfc,
                aliases: Vec::new(),
            })
            .unwrap();
    }

    #[test]
    fn hydrate_vue_compile_blockers_loads_external_and_transitive_type_deps() {
        let host = strict_host();
        let source = "<template src=\"@/partials/panel.html\"></template>\n<script setup lang=\"ts\">\nimport type { Props } from '@/types'\nconst props = defineProps<Props>()\n</script>";
        upsert_vue(&host, "/workspace/src/App.vue", source);
        let ide_profile = CompileProfile {
            target: verter_host::CompileTarget::BUNDLER | verter_host::CompileTarget::TSX,
            ..CompileProfile::default()
        };

        let initial = host.ensure_compiled("/workspace/src/App.vue", &CompileProfile::default());
        assert!(
            matches!(initial, Err(HostError::CompileError { .. })),
            "compile should fail before blocker hydration"
        );

        let mut project = crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        );
        project.compiler_options = crate::project_resolver::IdeProjectCompilerOptions {
            base_url: Some("/workspace".to_string()),
            paths: vec![("@/*".to_string(), vec!["src/*".to_string()])],
        };
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![project]);
        let reader = TestResolverReader::with_texts(&[
            (
                "/workspace/src/partials/panel.html",
                "<div>{{ props.msg }}</div>",
            ),
            (
                "/workspace/src/types.ts",
                "import type { Nested } from '@/nested'\nexport interface Props { msg: Nested }",
            ),
            ("/workspace/src/nested.ts", "export type Nested = string"),
        ]);

        hydrate_vue_compile_blockers(&host, &resolver, &reader, "/workspace/src/App.vue");

        assert!(
            host.get_source("/workspace/src/partials/panel.html")
                .is_some(),
            "external template source should be loaded into the host"
        );
        assert!(
            host.get_source("/workspace/src/types.ts").is_some(),
            "macro type dependency should be loaded into the host"
        );
        assert!(
            host.get_source("/workspace/src/nested.ts").is_some(),
            "transitive type dependency should be loaded into the host"
        );

        host.ensure_compiled("/workspace/src/App.vue", &ide_profile)
            .expect("compile should succeed once codegen blockers are hydrated");
        assert!(
            host.get_ide("/workspace/src/App.vue", &ide_profile)
                .is_some(),
            "hydrated compile should restore IDE output"
        );
    }

    #[test]
    fn collect_resolved_codegen_dependencies_uses_codegen_blocker_phase() {
        let mut project = crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        );
        project.compiler_options = crate::project_resolver::IdeProjectCompilerOptions {
            base_url: Some("/workspace/src".to_string()),
            paths: vec![("@/*".to_string(), vec!["*".to_string()])],
        };
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![project]);
        let reader = TestResolverReader::with_texts(&[
            ("/workspace/src/types.ts", "export interface Props {}"),
            ("/workspace/src/nested.ts", "export type Nested = string"),
        ]);
        let refs = vec![
            verter_analysis::AnalyzedModuleReference {
                syntax: verter_analysis::ModuleReferenceSyntax::StaticImport,
                semantics: verter_analysis::ModuleReferenceSemantics::Import,
                is_type_only: true,
                raw_text: "'@/types'".to_string(),
                literal_specifier: Some("@/types".to_string()),
                finite_specifiers: Vec::new(),
                static_prefix: None,
                analyzability: verter_analysis::ModuleReferenceAnalyzability::Exact,
                span: verter_span::Span::new(0, 8),
                expr_span: verter_span::Span::new(0, 8),
            },
            verter_analysis::AnalyzedModuleReference {
                syntax: verter_analysis::ModuleReferenceSyntax::StaticImport,
                semantics: verter_analysis::ModuleReferenceSemantics::Import,
                is_type_only: false,
                raw_text: "'./nested'".to_string(),
                literal_specifier: Some("./nested".to_string()),
                finite_specifiers: Vec::new(),
                static_prefix: None,
                analyzability: verter_analysis::ModuleReferenceAnalyzability::Exact,
                span: verter_span::Span::new(9, 19),
                expr_span: verter_span::Span::new(9, 19),
            },
        ];

        let resolved = collect_resolved_codegen_dependencies(
            &resolver,
            &reader,
            "/workspace/src/App.vue",
            &refs,
        );

        assert_eq!(
            resolved,
            vec![
                ("@/types".to_string(), "/workspace/src/types.ts".to_string()),
                (
                    "./nested".to_string(),
                    "/workspace/src/nested.ts".to_string()
                )
            ]
        );
    }

    #[test]
    fn ensure_source_loaded_into_host_loads_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("utils.ts");
        std::fs::write(&file_path, "export const x = 1;").unwrap();

        let canonical_id = normalize_fs_path(&file_path.to_string_lossy());
        let host = VerterHost::new_standalone(HostConfig::default());

        // Not in host yet
        assert!(
            host.get_source(&canonical_id).is_none(),
            "file should not be in host before ingress"
        );

        // Ingress loads it
        assert!(
            ensure_source_loaded_into_host(&host, &canonical_id),
            "ingress should succeed for file on disk"
        );
        assert!(
            host.get_source(&canonical_id).is_some(),
            "file should be in host after ingress"
        );
        assert_eq!(
            host.get_source(&canonical_id).unwrap().as_ref(),
            "export const x = 1;",
            "ingress should preserve file content"
        );
    }

    #[test]
    fn ensure_source_loaded_into_host_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("App.vue");
        std::fs::write(&file_path, "<template><div>hi</div></template>").unwrap();

        let canonical_id = normalize_fs_path(&file_path.to_string_lossy());
        let host = VerterHost::new_standalone(HostConfig::default());

        assert!(ensure_source_loaded_into_host(&host, &canonical_id));

        // Mutate file on disk — second call should NOT re-read since host already has it
        std::fs::write(&file_path, "<template><div>changed</div></template>").unwrap();
        assert!(ensure_source_loaded_into_host(&host, &canonical_id));
        assert_eq!(
            host.get_source(&canonical_id).unwrap().as_ref(),
            "<template><div>hi</div></template>",
            "second ingress call must not overwrite existing host content"
        );
    }

    #[test]
    fn ensure_source_loaded_into_host_returns_false_for_missing_file() {
        let host = VerterHost::new_standalone(HostConfig::default());
        assert!(
            !ensure_source_loaded_into_host(&host, "/nonexistent/file.ts"),
            "ingress should return false for missing files"
        );
        assert!(
            host.get_source("/nonexistent/file.ts").is_none(),
            "missing file should not appear in host"
        );
    }

    #[test]
    fn hydration_cache_skips_when_hash_and_generation_match() {
        let cache = HydrationCache::new();
        let hash: Hash16 = [1; 16];
        cache.insert("/src/App.vue", hash, 1);
        assert!(
            cache.is_valid("/src/App.vue", hash, 1),
            "cache hit: same hash + generation"
        );
    }

    #[test]
    fn hydration_cache_invalidates_on_hash_mismatch() {
        let cache = HydrationCache::new();
        cache.insert("/src/App.vue", [1; 16], 1);
        assert!(
            !cache.is_valid("/src/App.vue", [2; 16], 1),
            "cache miss: different hash"
        );
    }

    #[test]
    fn hydration_cache_invalidates_on_generation_mismatch() {
        let cache = HydrationCache::new();
        cache.insert("/src/App.vue", [1; 16], 1);
        assert!(
            !cache.is_valid("/src/App.vue", [1; 16], 2),
            "cache miss: different generation"
        );
    }

    #[test]
    fn hydration_cache_returns_false_for_unknown_file() {
        let cache = HydrationCache::new();
        assert!(
            !cache.is_valid("/src/Unknown.vue", [0; 16], 0),
            "cache miss: never inserted"
        );
    }

    #[test]
    fn hydration_cache_remove_clears_entry() {
        let cache = HydrationCache::new();
        cache.insert("/src/App.vue", [1; 16], 1);
        cache.remove("/src/App.vue");
        assert!(
            !cache.is_valid("/src/App.vue", [1; 16], 1),
            "cache miss after removal"
        );
    }

    #[test]
    fn pre_snapshot_hydration_skips_bare_specifiers() {
        let host = strict_host();
        let source = "<script setup lang=\"ts\">\nimport type { Props } from 'some-pkg'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props }}</div></template>";
        upsert_vue(&host, "/workspace/src/App.vue", source);
        let outcome = hydrate_vue_compile_blockers_pre_snapshot(&host, "/workspace/src/App.vue");
        assert!(
            !outcome.complete,
            "bare specifier should make outcome incomplete"
        );
    }

    #[test]
    fn pre_snapshot_hydration_resolves_relative_specifiers() {
        let dir = tempfile::tempdir().unwrap();
        let types_path = dir.path().join("types.ts");
        std::fs::write(&types_path, "export interface Props { msg: string }").unwrap();

        let vue_path = dir.path().join("App.vue");
        let vue_source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>".to_string();
        std::fs::write(&vue_path, &vue_source).unwrap();

        let host = strict_host();
        let canonical_vue = normalize_fs_path(&vue_path.to_string_lossy());
        let canonical_types = normalize_fs_path(&types_path.to_string_lossy());
        upsert_vue(&host, &canonical_vue, &vue_source);

        let outcome = hydrate_vue_compile_blockers_pre_snapshot(&host, &canonical_vue);
        assert!(
            outcome.complete,
            "relative specifier should be fully resolved"
        );
        assert!(
            host.get_source(&canonical_types).is_some(),
            "relative dependency should be loaded into host"
        );
    }

    #[test]
    fn reader_read_file_routes_through_ingress() {
        use verter_vfs::WorkspaceAccess;
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("types.ts");
        std::fs::write(&file_path, "export type T = string;").unwrap();

        let canonical_id = normalize_fs_path(&file_path.to_string_lossy());
        let host = VerterHost::new_standalone(HostConfig::default());

        assert!(host.get_source(&canonical_id).is_none());

        let reader = HostFsProjectResolverReader::new(&host);
        let content = reader.read_file(&canonical_id);
        assert!(content.is_some(), "reader should return content from disk");
        assert_eq!(content.unwrap().as_ref(), "export type T = string;");

        // After read_file, file should be in the host (ingress side effect)
        assert!(
            host.get_source(&canonical_id).is_some(),
            "read_file must load file into host via ingress"
        );
    }
}
