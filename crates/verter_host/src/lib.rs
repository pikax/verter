//! # verter_host — In-memory virtual file host for Vue SFC compilation
//!
//! Manages the lifecycle of Vue Single File Components in a stateful,
//! in-memory store. Each `.vue` file (or non-SFC dependency) is parsed,
//! hashed, cached, and compiled on demand. The host is the primary API
//! surface consumed by both the Vite bundler plugin (via `verter_napi`)
//! and the browser playground (via `verter_wasm`).
//!
//! ## Dependencies
//!
//! - **`verter_core`** — SFC tokenizer, parser, and template/script/style codegen
//! - **`verter_analysis`** — static analysis (imports, bindings, macros, style analysis)
//!
//! ## Key types
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`VerterHost`] | Main entry point — owns the file store and compile cache |
//! | [`HostConfig`] | Per-host configuration (dev mode, error policy, analysis level) |
//! | [`CompileProfile`] | Per-compilation variant (production, SSR, HMR strategy, etc.) |
//! | [`HostUpdateResult`] | Result of [`VerterHost::upsert`] — lists changed/removed virtual nodes |
//! | [`VirtualFileResponse`] | Result of [`VerterHost::get_virtual_file`] — compiled code + metadata |
//! | [`ResolvedId`] | Result of [`VerterHost::resolve`] — canonical + virtual IDs |
//!
//! ## Caching
//!
//! Each file stores per-profile compile slots keyed by a hash of the
//! [`CompileProfile`]. Slots are invalidated when the file's semantic hash
//! changes, and evicted LRU when the per-file profile cap is exceeded.
//! Smart dependency invalidation (tiered: Tier 1 full, Tier 2 export-level,
//! Tier 3 cross-file type resolution) minimizes unnecessary recompilation.
//!
//! ## Internal modules
//!
//! - [`cache`] — virtual node diffing, compile slot invalidation, LRU eviction
//! - [`compile`] — external source merging, main module assembly
//! - [`deps`] — dependency tracking, tiered smart invalidation
//! - [`hash`] — xxh3-based content hashing, profile hashing, semantic hashing
//! - [`id`] — canonical ID normalization, virtual ID rendering, import resolution
//! - [`parse`] — SFC tokenization → [`ParseSnapshot`](types::ParseSnapshot), non-SFC hashing
//! - [`shared`] — feature-gated `RwLock`/`RefCell` abstraction
//! - [`upsert`] — change detection, result building, export signature diffing

mod cache;
mod compile;
pub mod cross_file;
mod deps;
mod hash;
mod host_manage;
mod host_resolve;
mod host_upsert;
mod id;
mod parse;
mod shared;
pub(crate) mod source_map_remap;
pub mod template_convert;
mod types;
mod upsert;

pub use types::*;

// Re-export for the LSP: standalone @verter/types .d.ts content.
pub use verter_core::VERTER_TYPES_STANDALONE_DTS;

// Re-export CompileTarget so downstream crates (LSP, MCP, FFI) can use it
// without adding verter_core as a direct dependency.
pub use verter_core::compile::CompileTarget;

use std::collections::BTreeSet;

use id::canonicalize_id;
pub use id::resolve_external;
use rustc_hash::FxHashMap;
use shared::{default_shared, read_lock, write_lock, Shared};
use verter_analysis::project_resolver::NativeProjectResolver;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

/// Central file store and compile cache for Vue SFC compilation.
///
/// `VerterHost` owns all tracked files, their parse snapshots, and per-profile
/// compile slots. It is designed to be long-lived (one per Vite dev server or
/// WASM session) and provides the full upsert-resolve-load lifecycle:
///
/// 1. [`upsert`](Self::upsert) — parse and store a file, returning change info
/// 2. [`resolve`](Self::resolve) — map a raw import ID to canonical + virtual IDs
/// 3. [`get_virtual_file`](Self::get_virtual_file) — compile on demand (or cache hit) and return code
///
/// Internal state is protected by `RwLock` for thread-safe concurrent access.
pub struct VerterHost {
    pub(crate) config: HostConfig,
    /// VFS workspace providing file reads, import resolution, and edge recording.
    /// Only available on native targets (not WASM).
    /// Wrapped in RwLock so the workspace can be swapped (e.g., LSP wiring).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) workspace: parking_lot::RwLock<Arc<dyn verter_vfs::WorkspaceAccess>>,
    pub(crate) files: Shared<FxHashMap<String, FileEntry>>,
    pub(crate) alias_to_canonical: Shared<FxHashMap<String, String>>,
    pub(crate) reverse_dependencies: Shared<FxHashMap<String, BTreeSet<String>>>,
    pub(crate) tick: std::sync::atomic::AtomicU64,
    /// Last computed cross-file prop constness overrides.
    /// Used to detect changes on re-computation (Phase 7 invalidation).
    pub(crate) last_const_prop_overrides:
        Shared<rustc_hash::FxHashMap<String, rustc_hash::FxHashSet<String>>>,
    pub(crate) project_resolver: Shared<Option<NativeProjectResolver>>,
    #[cfg(feature = "host_metrics")]
    pub(crate) metrics: HostMetrics,
}

// Manual Debug impl because Arc<dyn WorkspaceAccess> doesn't implement Debug.
impl std::fmt::Debug for VerterHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerterHost")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl VerterHost {
    /// Create a new host backed by the given workspace.
    ///
    /// The workspace provides file reads, import resolution, and edge recording
    /// through the [`WorkspaceAccess`](verter_vfs::WorkspaceAccess) trait.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(config: HostConfig, workspace: Arc<dyn verter_vfs::WorkspaceAccess>) -> Self {
        Self {
            config,
            workspace: parking_lot::RwLock::new(workspace),
            files: default_shared(FxHashMap::default()),
            alias_to_canonical: default_shared(FxHashMap::default()),
            reverse_dependencies: default_shared(FxHashMap::default()),
            tick: std::sync::atomic::AtomicU64::new(1),
            last_const_prop_overrides: default_shared(rustc_hash::FxHashMap::default()),
            project_resolver: default_shared(None),
            #[cfg(feature = "host_metrics")]
            metrics: HostMetrics::default(),
        }
    }

    /// Create a new host (WASM variant, no workspace).
    #[cfg(target_arch = "wasm32")]
    pub fn new(config: HostConfig) -> Self {
        Self {
            config,
            files: default_shared(FxHashMap::default()),
            alias_to_canonical: default_shared(FxHashMap::default()),
            reverse_dependencies: default_shared(FxHashMap::default()),
            tick: std::sync::atomic::AtomicU64::new(1),
            last_const_prop_overrides: default_shared(rustc_hash::FxHashMap::default()),
            project_resolver: default_shared(None),
            #[cfg(feature = "host_metrics")]
            metrics: HostMetrics::default(),
        }
    }

    /// Create a standalone host with an internal memory workspace.
    ///
    /// For backward compatibility with tests and simple use cases that don't
    /// need an external workspace. Creates a [`MemoryWorkspace`](verter_vfs::MemoryWorkspace)
    /// internally.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_standalone(config: HostConfig) -> Self {
        let workspace = Arc::new(verter_vfs::MemoryWorkspace::new(
            verter_vfs::MemoryOptions::default(),
        ));
        Self::new(config, workspace)
    }

    /// Create a standalone host (WASM variant).
    #[cfg(target_arch = "wasm32")]
    pub fn new_standalone(config: HostConfig) -> Self {
        Self::new(config)
    }

    /// Get a clone of the workspace Arc.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn workspace(&self) -> Arc<dyn verter_vfs::WorkspaceAccess> {
        self.workspace.read().clone()
    }

    /// Swap the workspace backing this host.
    ///
    /// Used by the LSP to wire the host to the same `FilesystemWorkspace`
    /// used for direct mutation access.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_workspace(&self, workspace: Arc<dyn verter_vfs::WorkspaceAccess>) {
        *self.workspace.write() = workspace;
    }

    /// Clone the workspace Arc for internal use.
    /// Acquires a short-lived read lock, clones the Arc, and releases.
    /// Do NOT hold the returned Arc across blocking calls.
    #[cfg(not(target_arch = "wasm32"))]
    fn ws(&self) -> Arc<dyn verter_vfs::WorkspaceAccess> {
        self.workspace.read().clone()
    }

    /// Resolve an import through the workspace (VFS).
    ///
    /// Uses the workspace's resolution chain: exact resolutions (authoritative)
    /// then project resolver then no fallthrough.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn resolve_import_via_workspace(
        &self,
        parent_canonical_id: &str,
        import_source: &str,
    ) -> Option<String> {
        self.ws()
            .resolve_import(
                parent_canonical_id,
                import_source,
                verter_vfs::ResolveRequestKind::EsmImport,
            )
            .map(|r| r.source_id)
    }

    /// Release all cached data (files, aliases, dependency graph).
    ///
    /// After calling `close()` the host is empty but still usable (you could
    /// upsert files again). The primary purpose is to allow the Rust allocator
    /// to free the backing memory so that NAPI-RS-backed hosts don't keep the
    /// Node.js process alive waiting for GC finalisation.
    pub fn close(&self) {
        write_lock(&self.files).clear();
        write_lock(&self.alias_to_canonical).clear();
        write_lock(&self.reverse_dependencies).clear();
        write_lock(&self.last_const_prop_overrides).clear();
        *write_lock(&self.project_resolver) = None;
    }

    /// Configure project-scoped path alias resolution.
    ///
    /// Accepts a list of [`IdeProjectConfig`] describing tsconfig paths,
    /// workspace aliases, and project references. The host uses these to
    /// resolve aliased import specifiers (e.g. `@/components/Foo.vue`,
    /// `#imports`) without relying on external caller-provided resolutions.
    ///
    /// Pass an empty slice to clear the resolver.
    pub fn configure_projects(
        &self,
        projects: Vec<verter_analysis::project_resolver::IdeProjectConfig>,
    ) {
        let resolver = if projects.is_empty() {
            None
        } else {
            Some(NativeProjectResolver::new(projects.clone()))
        };
        *write_lock(&self.project_resolver) = resolver;

        // Sync to workspace so the VFS resolver stays in sync.
        #[cfg(not(target_arch = "wasm32"))]
        self.ws().configure_resolver(projects);
    }

    #[cfg(feature = "host_metrics")]
    pub fn metrics_snapshot(&self) -> HostMetricsSnapshot {
        use std::collections::BTreeMap;
        use std::sync::atomic::Ordering::Relaxed;
        let upserts = self.metrics.upserts.load(Relaxed);
        let compile_requests = self.metrics.compile_requests.load(Relaxed);
        let compile_cache_hits = self.metrics.compile_cache_hits.load(Relaxed);
        let slice_hash_time_us_total = self.metrics.slice_hash_time_us_total.load(Relaxed);
        let compile_time_us_total = self.metrics.compile_time_us_total.load(Relaxed);

        let compile_time_us_total_by_profile: BTreeMap<u64, u64> = self
            .metrics
            .compile_time_us_total_by_profile
            .lock()
            .expect("metrics lock poisoned")
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        let compile_count_by_profile: BTreeMap<u64, u64> = self
            .metrics
            .compile_count_by_profile
            .lock()
            .expect("metrics lock poisoned")
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();

        HostMetricsSnapshot {
            upserts,
            compile_requests,
            compile_cache_hits,
            compile_cache_hit_rate: if compile_requests == 0 {
                0.0
            } else {
                compile_cache_hits as f64 / compile_requests as f64
            },
            virtual_loads: self.metrics.virtual_loads.load(Relaxed),
            resolves: self.metrics.resolves.load(Relaxed),
            style_override_calls: self.metrics.style_override_calls.load(Relaxed),
            slice_hash_time_us_total,
            avg_slice_hash_time_us: if upserts == 0 {
                0.0
            } else {
                slice_hash_time_us_total as f64 / upserts as f64
            },
            compile_time_us_total,
            compile_time_us_total_by_profile,
            compile_count_by_profile,
        }
    }

    /// Resolve an alias to its canonical ID, or normalize the ID if no alias exists.
    pub(crate) fn resolve_alias_or_canonical(&self, id: &str) -> String {
        let normalized = canonicalize_id(id);
        let alias_map = read_lock(&self.alias_to_canonical);
        alias_map
            .get(normalized.as_ref())
            .cloned()
            .unwrap_or_else(|| normalized.into_owned())
    }

    /// Sync the alias-to-canonical map: remove stale aliases, insert current ones.
    pub(crate) fn update_alias_map(
        &self,
        canonical_id: &str,
        old_aliases: &BTreeSet<String>,
        new_aliases: &BTreeSet<String>,
    ) {
        let mut alias_map = write_lock(&self.alias_to_canonical);
        for old_alias in old_aliases {
            if !new_aliases.contains(old_alias) {
                alias_map.remove(old_alias);
            }
        }
        for alias in new_aliases {
            alias_map.insert(alias.clone(), canonical_id.to_string());
        }
    }

    /// Sync the reverse dependency graph: remove stale edges, insert current ones.
    pub(crate) fn update_reverse_deps(
        &self,
        canonical_id: &str,
        old_deps: &BTreeSet<String>,
        new_deps: &BTreeSet<String>,
    ) {
        let mut rev = write_lock(&self.reverse_dependencies);
        for dep in old_deps {
            if !new_deps.contains(dep) {
                if let Some(owners) = rev.get_mut(dep) {
                    owners.remove(canonical_id);
                    if owners.is_empty() {
                        rev.remove(dep);
                    }
                }
            }
        }
        for dep in new_deps {
            rev.entry(dep.clone())
                .or_default()
                .insert(canonical_id.to_string());
        }
    }

    /// Smart invalidation: when a dependency changes, only invalidate dependent
    /// SFCs whose macro-consumed types were actually affected.
    pub(crate) fn smart_invalidate_dependents(
        &self,
        dependency_id: &str,
        old_export_signatures: &[verter_analysis::ExportSignature],
        new_export_signatures: &[verter_analysis::ExportSignature],
    ) {
        // Native path: read reverse deps from workspace (authoritative source),
        // then merge with the legacy reverse_dependencies map for backward
        // compatibility (standalone hosts, tests without exact resolutions).
        #[cfg(not(target_arch = "wasm32"))]
        {
            let ws = self.ws();
            let mut owners: BTreeSet<String> =
                ws.reverse_deps_for(dependency_id).into_iter().collect();
            // Also check extensionless variant
            if let Some(stem) = deps::strip_configured_extension(
                dependency_id,
                &self.config.resolve_extensions,
                None,
            ) {
                for o in ws.reverse_deps_for(stem) {
                    owners.insert(o);
                }
            }

            // Merge with legacy reverse_dependencies (captures deps from
            // update_reverse_deps that the workspace may not have).
            {
                let rev = shared::read_lock(&self.reverse_dependencies);
                if let Some(legacy) = rev.get(dependency_id) {
                    for o in legacy {
                        owners.insert(o.clone());
                    }
                }
                if let Some(stem) = deps::strip_configured_extension(
                    dependency_id,
                    &self.config.resolve_extensions,
                    None,
                ) {
                    if let Some(more) = rev.get(stem) {
                        for o in more {
                            owners.insert(o.clone());
                        }
                    }
                }
            }

            deps::smart_invalidate_dependents_with_owners(
                &self.files,
                owners,
                &self.project_resolver,
                &self.config,
                dependency_id,
                old_export_signatures,
                new_export_signatures,
            );
            return;
        }

        // WASM fallback: use legacy reverse_dependencies map.
        #[allow(unreachable_code)]
        deps::smart_invalidate_dependents(
            &self.files,
            &self.reverse_dependencies,
            &self.project_resolver,
            &self.config,
            dependency_id,
            old_export_signatures,
            new_export_signatures,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use rustc_hash::FxHashMap;

    use super::cache;
    use super::deps::{import_resolves_to_dep, strip_configured_extension};
    use super::id::canonicalize_id;
    use super::parse::parse_vue_snapshot;
    use super::shared::{read_lock, write_lock};
    use super::upsert::{
        build_upsert_result, compute_changed_exports, compute_upsert_changes, UpsertChangeResult,
        UpsertResultData,
    };
    use super::*;
    use verter_analysis::AnalysisScope;
    use verter_vfs::WorkspaceAccess;

    fn profile_dev() -> CompileProfile {
        CompileProfile {
            is_production: false,
            hmr_strategy: HmrStrategy::Vite,
            ..CompileProfile::default()
        }
    }

    fn upsert_vue(host: &VerterHost, id: &str, src: &str) -> HostUpdateResult {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap()
    }

    #[test]
    fn get_source_returns_source_for_canonical_and_alias() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let source = "<template><div>hello</div></template>";

        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Comp.vue".to_string(),
            source: Arc::from(source),
            file_kind: FileKind::VueSfc,
            aliases: vec!["AliasComp.vue".to_string()],
        })
        .unwrap();

        assert_eq!(host.get_source("Comp.vue").as_deref(), Some(source));
        assert_eq!(host.get_source("AliasComp.vue").as_deref(), Some(source));
        assert_eq!(host.get_source("Missing.vue"), None);
    }

    #[test]
    fn host_internal_diagnostic_spans_remain_byte_offsets() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let source = "<template>\n  😀<div>\n</template>\n";

        let result = upsert_vue(&host, "Comp.vue", source);
        let expected_div_start = source.find("<div>").unwrap() as u32;

        let matches_byte_span = result.diagnostics.diagnostics.iter().any(|d| {
            d.code.contains("XMissingEndTag") && d.span.map(|s| s.start) == Some(expected_div_start)
        });

        assert!(
            matches_byte_span,
            "expected byte span {} in XMissingEndTag diagnostics, got: {:?}",
            expected_div_start,
            result
                .diagnostics
                .diagnostics
                .iter()
                .map(|d| (
                    d.code.clone(),
                    d.span.map(|s| s.start),
                    d.span.map(|s| s.end)
                ))
                .collect::<Vec<_>>()
        );
    }

    fn file_entry_from_snapshot(canonical_id: &str, src: &str, snap: &ParseSnapshot) -> FileEntry {
        FileEntry {
            canonical_id: canonical_id.to_string(),
            file_kind: FileKind::VueSfc,
            source: Arc::from(src),
            whole_hash: snap.whole_hash,
            semantic_hash: snap.semantic_hash,
            slices: snap.slices.clone(),
            descriptor: snap.descriptor.clone(),
            meta: snap.meta.clone(),
            aliases: BTreeSet::new(),
            dependencies: BTreeSet::new(),
            dependency_resolutions: FxHashMap::default(),
            external_requests: Vec::new(),
            src_blocks: Vec::new(),
            parse_diagnostics: DiagnosticsSnapshot::default(),
            script_analysis: verter_analysis::ScriptAnalysisSnapshot::default(),
            export_signatures: Vec::new(),
            style_analyses: Arc::new(Vec::new()),
            template_analysis: None,
            arc_script_cache: ScriptAnalysisArcs::default(),
            resolved_type_hashes: FxHashMap::default(),
            style_overrides: FxHashMap::default(),
            content_overrides: FxHashMap::default(),
            compile_slots: FxHashMap::default(),
            latest_diagnostics: FxHashMap::default(),
            diagnostics_generation: 0,
            generation: 1,
            cached_parse: None,
            cached_tsc_extract: None,
        }
    }

    /// @ai-generated - AnalysisLevel::Essential runs script analysis but not style
    #[test]
    fn analysis_level_essential_runs_script_not_style() {
        let host = VerterHost::new_standalone(HostConfig {
            analysis_level: AnalysisLevel::Essential,
            ..HostConfig::default()
        });
        let src = "<script setup>\nimport { ref } from 'vue'\nconst n = ref(1)\n</script>\n<template><div>{{n}}</div></template>\n<style scoped>.a { color: red }</style>";
        let _ = upsert_vue(&host, "Comp.vue", src);

        let files = read_lock(&host.files);
        let entry = files.get("Comp.vue").unwrap();
        assert!(
            !entry.script_analysis.imports.is_empty(),
            "script analysis should be populated at AnalysisLevel::Essential"
        );
        assert!(
            entry.style_analyses.is_empty(),
            "style analyses should not be populated at AnalysisLevel::Essential"
        );
    }

    /// @ai-generated - AnalysisLevel::None skips all analysis during upsert
    #[test]
    fn analysis_level_none_skips_all_analysis_in_upsert() {
        let host = VerterHost::new_standalone(HostConfig {
            analysis_level: AnalysisLevel::None,
            ..HostConfig::default()
        });
        let src = "<script setup>\nimport { ref } from 'vue'\nconst n = ref(1)\n</script>\n<template><div>{{n}}</div></template>\n<style scoped>.a { color: red }</style>";
        let _ = upsert_vue(&host, "Comp.vue", src);

        let files = read_lock(&host.files);
        let entry = files.get("Comp.vue").unwrap();
        assert!(
            entry.script_analysis.imports.is_empty(),
            "script analysis should not be populated at AnalysisLevel::None"
        );
        assert!(
            entry.style_analyses.is_empty(),
            "style analyses should not be populated at AnalysisLevel::None"
        );
    }

    /// @ai-generated - build_upsert_result: first insert returns all nodes as changed
    #[test]
    fn build_upsert_result_first_insert() {
        let src =
            "<script setup>const n = 1</script><template><div/></template><style>.a{}</style>";
        let (snapshot, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
        let data = UpsertResultData {
            new_meta: snapshot.meta,
            parse_diagnostics: snapshot.parse_diagnostics,
            imports: snapshot.script_analysis.imports,
            module_references: snapshot.script_analysis.module_references,
            external_requests: snapshot.external_requests,
            preprocessor_requests: snapshot.preprocessor_requests,
            export_signatures: snapshot.export_signatures,
        };
        let changes = UpsertChangeResult {
            slice_changes: SliceChanges::default(),
            changed: true,
            semantic_changed: true,
        };
        let result = build_upsert_result(
            "Comp.vue".to_string(),
            data,
            &changes,
            &[], // no prev_nodes
            &FileMeta::default(),
            0.0,
        )
        .unwrap();

        assert!(result.changed);
        assert!(result
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Main));
        assert!(result
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Script));
        assert!(result
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Template));
        assert!(result
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Style { index: 0 }));
        assert!(result.removed_virtual_nodes.is_empty());
        assert_eq!(
            result.changed_virtual_ids.len(),
            result.changed_lsp_ids.len()
        );
    }

    /// @ai-generated - build_upsert_result: no change returns empty
    #[test]
    fn build_upsert_result_no_change() {
        let src = "<script setup>const n = 1</script><template><div/></template>";
        let (snapshot, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
        let data = UpsertResultData {
            new_meta: snapshot.meta,
            parse_diagnostics: snapshot.parse_diagnostics,
            imports: snapshot.script_analysis.imports,
            module_references: snapshot.script_analysis.module_references,
            external_requests: snapshot.external_requests,
            preprocessor_requests: snapshot.preprocessor_requests,
            export_signatures: snapshot.export_signatures,
        };
        let prev = vec![
            VirtualNodeKind::Main,
            VirtualNodeKind::Script,
            VirtualNodeKind::Template,
        ];
        let changes = UpsertChangeResult {
            slice_changes: SliceChanges::default(),
            changed: false,
            semantic_changed: false,
        };
        let result = build_upsert_result(
            "Comp.vue".to_string(),
            data,
            &changes,
            &prev,
            &FileMeta::default(),
            0.0,
        )
        .unwrap();

        assert!(!result.changed);
        assert!(result.changed_virtual_nodes.is_empty());
        assert!(result.removed_virtual_nodes.is_empty());
    }

    #[test]
    fn canonicalize_id_handles_edge_cases() {
        assert_eq!(
            canonicalize_id("C:\\Users\\foo\\Comp.vue"),
            "c:/Users/foo/Comp.vue"
        );
        assert_eq!(canonicalize_id("Comp.vue?vue&type=script"), "Comp.vue");
        assert_eq!(canonicalize_id("Comp.vue._VERTER_.bundle.ts"), "Comp.vue");
        assert_eq!(canonicalize_id("  Comp.vue  "), "Comp.vue");
        assert_eq!(canonicalize_id(""), "");
        assert_eq!(canonicalize_id("   "), "");
    }

    /// @ai-generated - compute_changed_exports: added export detected
    #[test]
    fn compute_changed_exports_added() {
        let old = vec![];
        let new = vec![verter_analysis::ExportSignature {
            name: "MyType".to_string(),
            declaration_hash: [1; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        }];
        let changed = compute_changed_exports(&old, &new);
        assert!(changed.contains("MyType"));
    }

    /// @ai-generated - compute_changed_exports: both empty → empty set
    #[test]
    fn compute_changed_exports_both_empty() {
        let changed = compute_changed_exports(&[], &[]);
        assert!(changed.is_empty());
    }

    /// @ai-generated - compute_changed_exports: hash changed detected
    #[test]
    fn compute_changed_exports_hash_changed() {
        let old = vec![verter_analysis::ExportSignature {
            name: "MyType".to_string(),
            declaration_hash: [1; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        }];
        let new = vec![verter_analysis::ExportSignature {
            name: "MyType".to_string(),
            declaration_hash: [2; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        }];
        let changed = compute_changed_exports(&old, &new);
        assert!(changed.contains("MyType"));
    }

    /// @ai-generated - compute_changed_exports: mixed add + remove + change + unchanged
    #[test]
    fn compute_changed_exports_mixed() {
        let old = vec![
            verter_analysis::ExportSignature {
                name: "Kept".to_string(),
                declaration_hash: [1; 16],
                is_type: true,
                span: Default::default(),
                reexport_source: None,
                reexport_local: None,
                local_span: None,
            },
            verter_analysis::ExportSignature {
                name: "Changed".to_string(),
                declaration_hash: [2; 16],
                is_type: true,
                span: Default::default(),
                reexport_source: None,
                reexport_local: None,
                local_span: None,
            },
            verter_analysis::ExportSignature {
                name: "Removed".to_string(),
                declaration_hash: [3; 16],
                is_type: true,
                span: Default::default(),
                reexport_source: None,
                reexport_local: None,
                local_span: None,
            },
        ];
        let new = vec![
            verter_analysis::ExportSignature {
                name: "Kept".to_string(),
                declaration_hash: [1; 16],
                is_type: true,
                span: Default::default(),
                reexport_source: None,
                reexport_local: None,
                local_span: None,
            },
            verter_analysis::ExportSignature {
                name: "Changed".to_string(),
                declaration_hash: [9; 16],
                is_type: true,
                span: Default::default(),
                reexport_source: None,
                reexport_local: None,
                local_span: None,
            },
            verter_analysis::ExportSignature {
                name: "Added".to_string(),
                declaration_hash: [4; 16],
                is_type: true,
                span: Default::default(),
                reexport_source: None,
                reexport_local: None,
                local_span: None,
            },
        ];
        let changed = compute_changed_exports(&old, &new);
        assert!(!changed.contains("Kept"), "unchanged should not appear");
        assert!(changed.contains("Changed"), "hash-changed should appear");
        assert!(changed.contains("Removed"), "removed should appear");
        assert!(changed.contains("Added"), "added should appear");
        assert_eq!(changed.len(), 3);
    }

    /// @ai-generated - compute_changed_exports: removed export detected
    #[test]
    fn compute_changed_exports_removed() {
        let old = vec![verter_analysis::ExportSignature {
            name: "MyType".to_string(),
            declaration_hash: [1; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        }];
        let new = vec![];
        let changed = compute_changed_exports(&old, &new);
        assert!(changed.contains("MyType"));
    }

    /// @ai-generated - compute_changed_exports: unchanged export not in set
    #[test]
    fn compute_changed_exports_unchanged() {
        let old = vec![verter_analysis::ExportSignature {
            name: "MyType".to_string(),
            declaration_hash: [1; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        }];
        let new = vec![verter_analysis::ExportSignature {
            name: "MyType".to_string(),
            declaration_hash: [1; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        }];
        let changed = compute_changed_exports(&old, &new);
        assert!(changed.is_empty());
    }

    /// @ai-generated - compute_upsert_changes: first insert (no old entry) → changed=true
    #[test]
    fn compute_upsert_changes_first_insert() {
        let (new, _) = parse_vue_snapshot(
            "Comp.vue",
            "<script setup>const n = 1</script><template><div/></template>",
            AnalysisScope::LSP,
        );
        let result = compute_upsert_changes(None, &new);
        assert!(result.changed, "first insert should be changed");
        assert!(
            result.semantic_changed,
            "first insert should be semantic_changed"
        );
        assert!(
            !result.slice_changes.script_changed,
            "no old entry means no diff"
        );
    }

    /// @ai-generated - compute_upsert_changes: identical content → not changed
    #[test]
    fn compute_upsert_changes_identical_content() {
        let src = "<script setup>const n = 1</script><template><div/></template>";
        let (old_snap, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
        let (new_snap, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
        let old_entry = file_entry_from_snapshot("Comp.vue", src, &old_snap);
        let result = compute_upsert_changes(Some(&old_entry), &new_snap);
        assert!(!result.changed, "identical content should not be changed");
        assert!(!result.semantic_changed);
    }

    /// @ai-generated - compute_upsert_changes: script-only change detected
    #[test]
    fn compute_upsert_changes_script_change() {
        let src1 = "<script setup>const n = 1</script><template><div/></template>";
        let src2 = "<script setup>const n = 2</script><template><div/></template>";
        let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
        let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
        let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
        let result = compute_upsert_changes(Some(&old_entry), &new_snap);
        assert!(result.changed);
        assert!(result.slice_changes.script_changed);
        assert!(!result.slice_changes.template_changed);
    }

    /// @ai-generated - compute_upsert_changes: structure change (style added)
    #[test]
    fn compute_upsert_changes_structure_change() {
        let src1 = "<script setup>const n = 1</script><template><div/></template>";
        let src2 =
            "<script setup>const n = 1</script><template><div/></template><style>.a{}</style>";
        let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
        let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
        let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
        let result = compute_upsert_changes(Some(&old_entry), &new_snap);
        assert!(result.changed);
        assert!(result.slice_changes.structure_changed);
    }

    /// @ai-generated - compute_upsert_changes: template-only change detected
    #[test]
    fn compute_upsert_changes_template_change() {
        let src1 = "<script setup>const n = 1</script><template><div/></template>";
        let src2 = "<script setup>const n = 1</script><template><section/></template>";
        let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
        let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
        let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
        let result = compute_upsert_changes(Some(&old_entry), &new_snap);
        assert!(result.changed);
        assert!(!result.slice_changes.script_changed);
        assert!(result.slice_changes.template_changed);
    }

    /// @ai-generated — Style-only change is detected by is_style_only()
    #[test]
    fn compute_upsert_changes_style_only() {
        let src1 =
            "<script setup>const n = 1</script><template><div/></template><style>.a{}</style>";
        let src2 =
            "<script setup>const n = 1</script><template><div/></template><style>.b{}</style>";
        let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
        let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
        let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
        let result = compute_upsert_changes(Some(&old_entry), &new_snap);
        assert!(result.changed);
        assert!(
            result.slice_changes.is_style_only(),
            "only style content changed"
        );
        assert!(!result.slice_changes.script_changed);
        assert!(!result.slice_changes.template_changed);
        assert!(!result.slice_changes.structure_changed);
        assert_eq!(result.slice_changes.style_indices_changed, vec![0]);
    }

    /// @ai-generated — Script + style change is NOT style-only
    #[test]
    fn compute_upsert_changes_script_and_style_not_style_only() {
        let src1 =
            "<script setup>const n = 1</script><template><div/></template><style>.a{}</style>";
        let src2 =
            "<script setup>const n = 2</script><template><div/></template><style>.b{}</style>";
        let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
        let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
        let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
        let result = compute_upsert_changes(Some(&old_entry), &new_snap);
        assert!(result.changed);
        assert!(!result.slice_changes.is_style_only(), "script also changed");
    }

    /// @ai-generated - Custom resolve_extensions config is respected
    #[test]
    fn custom_resolve_extensions_config() {
        let host = VerterHost::new_standalone(HostConfig {
            resolve_extensions: vec![".ts".to_string(), ".js".to_string()],
            ..HostConfig::default()
        });

        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
        );

        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType { foo: string }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Change MyType — should still invalidate with custom extensions
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType { foo: string; bar: number }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "custom resolve_extensions should still match .ts"
        );
    }

    /// @ai-generated - File kind change from VueSfc to NonSfc produces correct node list
    #[test]
    fn file_kind_change_vue_to_nonsfc() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // First upsert as VueSfc
        let _ = upsert_vue(
            &host,
            "Comp.vue",
            "<script setup>const n = 1</script><template><div/></template>",
        );
        let nodes_before = host.list_virtual_files("Comp.vue");
        assert!(nodes_before.contains(&VirtualNodeKind::Script));
        assert!(nodes_before.contains(&VirtualNodeKind::Template));

        // Re-upsert as NonSfc
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "Comp.vue".to_string(),
                source: Arc::from("export default {}"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let nodes_after = host.list_virtual_files("Comp.vue");
        assert_eq!(nodes_after, vec![VirtualNodeKind::Main]);
    }

    /// @ai-generated - generation field increments on each upsert
    #[test]
    fn generation_counter_increments_on_upsert() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
        let _ = upsert_vue(&host, "Comp.vue", src1);

        let gen1 = {
            let files = read_lock(&host.files);
            files.get("Comp.vue").unwrap().generation
        };
        assert_eq!(gen1, 1);

        let src2 = "<script setup>const n = 2</script><template><div>{{n}}</div></template>";
        let _ = upsert_vue(&host, "Comp.vue", src2);

        let gen2 = {
            let files = read_lock(&host.files);
            files.get("Comp.vue").unwrap().generation
        };
        assert_eq!(gen2, 2);
    }

    /// @ai-generated - import_resolves_to_dep: non-relative in dependency set
    #[test]
    fn import_resolves_to_dep_non_relative_in_deps() {
        let mut deps = BTreeSet::new();
        deps.insert("lodash".to_string());
        let entry = FileEntry {
            canonical_id: "/src/A.vue".to_string(),
            file_kind: FileKind::VueSfc,
            source: Arc::from(""),
            whole_hash: [0; 16],
            semantic_hash: [0; 16],
            slices: SliceHashes::default(),
            descriptor: DescriptorMin::default(),
            meta: FileMeta::default(),
            aliases: BTreeSet::new(),
            dependencies: deps,
            dependency_resolutions: FxHashMap::default(),
            external_requests: Vec::new(),
            src_blocks: Vec::new(),
            parse_diagnostics: DiagnosticsSnapshot::default(),
            script_analysis: verter_analysis::ScriptAnalysisSnapshot::default(),
            export_signatures: Vec::new(),
            style_analyses: Arc::new(Vec::new()),
            template_analysis: None,
            arc_script_cache: ScriptAnalysisArcs::default(),
            resolved_type_hashes: FxHashMap::default(),
            style_overrides: FxHashMap::default(),
            content_overrides: FxHashMap::default(),
            compile_slots: FxHashMap::default(),
            latest_diagnostics: FxHashMap::default(),
            diagnostics_generation: 0,
            generation: 0,
            cached_parse: None,
            cached_tsc_extract: None,
        };
        let exts = vec![".ts".to_string()];
        assert!(import_resolves_to_dep(&entry, "lodash", "lodash", &exts));
        assert!(!import_resolves_to_dep(
            &entry,
            "lodash",
            "underscore",
            &exts
        ));
    }

    /// @ai-generated - import_resolves_to_dep: non-relative not in deps → false
    #[test]
    fn import_resolves_to_dep_non_relative_not_in_deps() {
        let entry = FileEntry {
            canonical_id: "/src/A.vue".to_string(),
            file_kind: FileKind::VueSfc,
            source: Arc::from(""),
            whole_hash: [0; 16],
            semantic_hash: [0; 16],
            slices: SliceHashes::default(),
            descriptor: DescriptorMin::default(),
            meta: FileMeta::default(),
            aliases: BTreeSet::new(),
            dependencies: BTreeSet::new(),
            dependency_resolutions: FxHashMap::default(),
            external_requests: Vec::new(),
            src_blocks: Vec::new(),
            parse_diagnostics: DiagnosticsSnapshot::default(),
            script_analysis: verter_analysis::ScriptAnalysisSnapshot::default(),
            export_signatures: Vec::new(),
            style_analyses: Arc::new(Vec::new()),
            template_analysis: None,
            arc_script_cache: ScriptAnalysisArcs::default(),
            resolved_type_hashes: FxHashMap::default(),
            style_overrides: FxHashMap::default(),
            content_overrides: FxHashMap::default(),
            compile_slots: FxHashMap::default(),
            latest_diagnostics: FxHashMap::default(),
            diagnostics_generation: 0,
            generation: 0,
            cached_parse: None,
            cached_tsc_extract: None,
        };
        let exts = vec![".ts".to_string()];
        assert!(!import_resolves_to_dep(&entry, "lodash", "lodash", &exts));
    }

    /// @ai-generated - import_resolves_to_dep: relative import exact match
    #[test]
    fn import_resolves_to_dep_relative_exact() {
        let entry = FileEntry {
            canonical_id: "/src/A.vue".to_string(),
            file_kind: FileKind::VueSfc,
            source: Arc::from(""),
            whole_hash: [0; 16],
            semantic_hash: [0; 16],
            slices: SliceHashes::default(),
            descriptor: DescriptorMin::default(),
            meta: FileMeta::default(),
            aliases: BTreeSet::new(),
            dependencies: BTreeSet::new(),
            dependency_resolutions: FxHashMap::default(),
            external_requests: Vec::new(),
            src_blocks: Vec::new(),
            parse_diagnostics: DiagnosticsSnapshot::default(),
            script_analysis: verter_analysis::ScriptAnalysisSnapshot::default(),
            export_signatures: Vec::new(),
            style_analyses: Arc::new(Vec::new()),
            template_analysis: None,
            arc_script_cache: ScriptAnalysisArcs::default(),
            resolved_type_hashes: FxHashMap::default(),
            style_overrides: FxHashMap::default(),
            content_overrides: FxHashMap::default(),
            compile_slots: FxHashMap::default(),
            latest_diagnostics: FxHashMap::default(),
            diagnostics_generation: 0,
            generation: 0,
            cached_parse: None,
            cached_tsc_extract: None,
        };
        let exts = vec![".ts".to_string(), ".js".to_string()];
        assert!(import_resolves_to_dep(&entry, "./B", "/src/B", &exts));
        assert!(!import_resolves_to_dep(&entry, "./B", "/other/B", &exts));
    }

    /// @ai-generated - import_resolves_to_dep: relative import with extension strip
    #[test]
    fn import_resolves_to_dep_relative_extension_strip() {
        let entry = FileEntry {
            canonical_id: "/src/A.vue".to_string(),
            file_kind: FileKind::VueSfc,
            source: Arc::from(""),
            whole_hash: [0; 16],
            semantic_hash: [0; 16],
            slices: SliceHashes::default(),
            descriptor: DescriptorMin::default(),
            meta: FileMeta {
                script_lang: Some("ts".to_string()),
                ..FileMeta::default()
            },
            aliases: BTreeSet::new(),
            dependencies: BTreeSet::new(),
            dependency_resolutions: FxHashMap::default(),
            external_requests: Vec::new(),
            src_blocks: Vec::new(),
            parse_diagnostics: DiagnosticsSnapshot::default(),
            script_analysis: verter_analysis::ScriptAnalysisSnapshot::default(),
            export_signatures: Vec::new(),
            style_analyses: Arc::new(Vec::new()),
            template_analysis: None,
            arc_script_cache: ScriptAnalysisArcs::default(),
            resolved_type_hashes: FxHashMap::default(),
            style_overrides: FxHashMap::default(),
            content_overrides: FxHashMap::default(),
            compile_slots: FxHashMap::default(),
            latest_diagnostics: FxHashMap::default(),
            diagnostics_generation: 0,
            generation: 0,
            cached_parse: None,
            cached_tsc_extract: None,
        };
        let exts = vec![".ts".to_string(), ".js".to_string()];
        // ./types resolves to /src/types, dep is /src/types.ts
        // Extension strip on /src/types.ts → /src/types → match
        assert!(import_resolves_to_dep(
            &entry,
            "./types",
            "/src/types.ts",
            &exts
        ));
    }

    /// @ai-generated - invalidate_nodes removes last_good_outputs for targeted nodes
    #[test]
    fn invalidate_nodes_removes_last_good() {
        use cache::invalidate_nodes;

        let mut slots = FxHashMap::default();
        let mut outputs = FxHashMap::default();
        outputs.insert(
            VirtualNodeKind::Main,
            CachedVirtualFile {
                code: Arc::from("main code"),
                source_map: None,
                lang: Some("js".to_string()),
                meta: VirtualMeta::default(),
            },
        );
        outputs.insert(
            VirtualNodeKind::Template,
            CachedVirtualFile {
                code: Arc::from("template code"),
                source_map: None,
                lang: Some("tsx".to_string()),
                meta: VirtualMeta::default(),
            },
        );
        let last_good = Some(outputs.clone());
        slots.insert(
            42u64,
            CompileSlot {
                semantic_hash: [0; 16],
                style_override_hash: 0,
                content_override_hash: 0,
                outputs,
                diagnostics: DiagnosticsSnapshot::default(),
                last_good_outputs: last_good,
                last_access_tick: 1,
                tsx: None,
                template_analysis: None,
            },
        );

        invalidate_nodes(
            &mut slots,
            &[VirtualNodeKind::Main, VirtualNodeKind::Template],
        );

        let slot = slots.get(&42).unwrap();
        assert!(!slot.outputs.contains_key(&VirtualNodeKind::Main));
        assert!(!slot.outputs.contains_key(&VirtualNodeKind::Template));
        // last_good_outputs also cleared for these nodes
        let last_good = slot.last_good_outputs.as_ref().unwrap();
        assert!(!last_good.contains_key(&VirtualNodeKind::Main));
        assert!(!last_good.contains_key(&VirtualNodeKind::Template));
    }

    #[test]
    fn profile_cap_evicts_oldest_profiles() {
        let host = VerterHost::new_standalone(HostConfig {
            max_profiles_per_file: 2,
            ..HostConfig::default()
        });
        let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
        let _ = upsert_vue(&host, "Comp.vue", src);

        let p1 = CompileProfile {
            hmr_strategy: HmrStrategy::Vite,
            ..CompileProfile::default()
        };
        let p2 = CompileProfile {
            hmr_strategy: HmrStrategy::Webpack,
            ..CompileProfile::default()
        };
        let p3 = CompileProfile {
            is_production: true,
            ..CompileProfile::default()
        };

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: p1.clone(),
            })
            .unwrap();

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: p2.clone(),
            })
            .unwrap();

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: p3.clone(),
            })
            .unwrap();

        let result = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: p3,
            })
            .unwrap();
        assert!(!result.code.is_empty());
    }

    /// @ai-generated - Relative imports auto-register in dependency graph
    #[test]
    fn relative_imports_auto_register_deps() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport type { MyType } from './types'\n</script>\n<template><div/></template>",
        );

        // Check that ./types was resolved and added to dependencies
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.dependencies.contains("/src/types"),
            "relative import should auto-register as dependency, got: {:?}",
            comp.dependencies
        );
    }

    /// @ai-generated - set_import_dependencies adds to reverse dep graph
    #[test]
    fn set_import_dependencies_adds_to_reverse_deps() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let _ = upsert_vue(
            &host,
            "Comp.vue",
            "<script setup lang=\"ts\">\nimport { helper } from '@/utils'\n</script>\n<template><div/></template>",
        );

        // Caller resolves @/utils → /src/utils.ts
        host.set_import_dependencies(
            "Comp.vue",
            vec![DependencyResolution {
                specifier: "@/utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        // Check that reverse dependency was added
        let rev = read_lock(&host.reverse_dependencies);
        let owners = rev.get("/src/utils.ts");
        assert!(
            owners.is_some() && owners.unwrap().contains("Comp.vue"),
            "reverse dep should be registered"
        );
    }

    /// @ai-generated - set_import_dependencies: subsequent dep upsert triggers invalidation
    #[test]
    fn set_import_dependencies_subsequent_upsert_invalidates() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport { helper } from '@/utils'\n</script>\n<template><div/></template>",
        );

        // Resolve @/utils → /src/utils.ts
        host.set_import_dependencies(
            "/src/Comp.vue",
            vec![DependencyResolution {
                specifier: "@/utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        // Upsert the dependency
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/utils.ts".to_string(),
                source: Arc::from("export function helper() { return 1 }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        // Compile Comp.vue to populate cache
        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Change the dependency
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/utils.ts".to_string(),
                source: Arc::from("export function helper() { return 2 }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        // Comp.vue should be invalidated because it has a runtime import from this dep
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared when runtime dep changes"
        );
    }

    #[test]
    fn invalidate_dependents_of_works_when_dependency_was_never_loaded() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport { helper } from './tempUtil'\n</script>\n<template><div>{{ helper }}</div></template>",
        );

        host.set_import_dependencies(
            "/src/Comp.vue",
            vec![DependencyResolution {
                specifier: "./tempUtil".to_string(),
                resolved_canonical_id: Some("/src/tempUtil.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        host.invalidate_dependents_of("/src/tempUtil.ts");

        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared even when the deleted dependency was never loaded"
        );
    }

    /// @ai-generated - Dep file with no export signatures → full invalidation (Tier 1 fallback)
    #[test]
    fn smart_invalidation_no_signatures_full_invalidation() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // Manually set up a dependency relationship without export signatures
        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<template src=\"./tpl.html\"></template><script setup>const n = 1</script>",
        );
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/tpl.html".to_string(),
                source: Arc::from("<div>A</div>"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        // Compile Comp.vue
        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Re-upsert non-SFC dependency (tpl.html has no TS exports → empty signatures)
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/tpl.html".to_string(),
                source: Arc::from("<section>B</section>"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        // Comp.vue should be invalidated (full invalidation since no export signatures)
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared for Tier 1 fallback"
        );
    }

    /// @ai-generated - Dep file unchanged export → SFC NOT invalidated
    #[test]
    fn smart_invalidation_unchanged_export_no_invalidation() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // Upsert SFC that imports MyType
        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
        );

        // Upsert dependency with MyType and OtherType
        let _ = host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from(
                "export interface MyType { a: string }\nexport interface OtherType { x: number }",
            ),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

        // Compile Comp.vue
        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Change only OtherType (not used by Comp.vue's macros)
        let _ = host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { a: string }\nexport interface OtherType { x: number; y: string }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

        // Comp.vue should still have a cache hit (MyType didn't change)
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        // Compile slots should NOT be empty — the smart invalidation should have skipped it
        assert!(
            !comp.compile_slots.is_empty(),
            "compile slots should not be cleared when only unused exports changed"
        );
    }

    /// @ai-generated - strip_configured_extension: empty extensions → None
    #[test]
    fn strip_configured_extension_empty_extensions() {
        assert_eq!(strip_configured_extension("/src/types.ts", &[], None), None);
        assert_eq!(
            strip_configured_extension("/src/types.ts", &[], Some("ts")),
            None
        );
    }

    /// @ai-generated - strip_configured_extension with script_lang prioritizes matching extensions
    #[test]
    fn strip_configured_extension_prioritizes_script_lang() {
        let extensions = vec![
            ".ts".to_string(),
            ".tsx".to_string(),
            ".js".to_string(),
            ".jsx".to_string(),
        ];
        // With lang="ts", .ts is tried first (and matches)
        assert_eq!(
            strip_configured_extension("/src/types.ts", &extensions, Some("ts")),
            Some("/src/types")
        );
        // With lang="js", .js would be tried first but .ts is also in the list
        assert_eq!(
            strip_configured_extension("/src/types.ts", &extensions, Some("js")),
            Some("/src/types")
        );
        // No lang — falls through to full list
        assert_eq!(
            strip_configured_extension("/src/types.ts", &extensions, None),
            Some("/src/types")
        );
        // Extension not in config → None
        assert_eq!(
            strip_configured_extension("/src/types.vue", &extensions, None),
            None
        );
    }

    /// @ai-generated - Tier 3: comment added to dep type → NO invalidation
    /// (Tier 2 WOULD invalidate since export text hash changes, but Tier 3 saves it)
    #[test]
    fn tier3_comment_added_no_invalidation() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
        );

        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType { foo: string }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Add a comment — Tier 2 export text hash changes, but Tier 3 resolved shape is the same
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("/** Updated docs */\nexport interface MyType { foo: string }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            !comp.compile_slots.is_empty(),
            "compile slots should NOT be cleared when only comments changed (Tier 3 saves)"
        );
    }

    /// @ai-generated - Tier 3: property added to dep type → invalidation
    #[test]
    fn tier3_property_added_invalidates() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
        );

        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType { foo: string }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Add a property to MyType
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType { foo: string; bar: number }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        // Tier 3: resolved type shape changed → invalidation
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared when resolved type shape changed (prop added)"
        );
    }

    /// @ai-generated - Tier 3: property type changed → invalidation
    #[test]
    fn tier3_property_type_changed_invalidates() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
        );

        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType { foo: string }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Change foo's type from string to number
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType { foo: number }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared when prop type changed"
        );
    }

    /// @ai-generated - Tier 3: resolved_type_hashes are stored for future comparisons
    #[test]
    fn tier3_stores_resolved_type_hashes() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
        );

        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType { foo: string }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Trigger smart invalidation by re-upserting dep with whitespace change
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType {\n  foo: string;\n}"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        // Check that resolved_type_hashes were stored
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        let key = ("/src/types.ts".to_string(), "MyType".to_string());
        assert!(
            comp.resolved_type_hashes.contains_key(&key),
            "resolved_type_hashes should store hash for (dep_id, type_name)"
        );
    }

    /// @ai-generated - Tier 3: unrelated type changed in same file → NO invalidation
    #[test]
    fn tier3_unrelated_type_change_no_invalidation() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
        );

        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from(
                    "export interface MyType { foo: string }\nexport interface Other { x: number }",
                ),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Only change Other (not used by SFC macros)
        let _ = host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from(
                "export interface MyType { foo: string }\nexport interface Other { x: number; y: boolean }",
            ),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

        // MyType unchanged → Tier 2 already skips (export hash matches)
        // Tier 3 not even needed here since Tier 2 already handles it
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            !comp.compile_slots.is_empty(),
            "compile slots should NOT be cleared when only unrelated types changed"
        );
    }

    /// @ai-generated - Tier 3: whitespace-only change to dep type → NO invalidation
    #[test]
    fn tier3_whitespace_only_change_no_invalidation() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // SFC imports MyType and uses it in defineProps
        let _ = upsert_vue(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
        );

        // Initial dep with MyType
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType { foo: string; bar: number }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        // Compile to populate cache
        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/src/Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        // Change MyType with only whitespace differences (same prop shape)
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface MyType {\n  foo: string;\n  bar: number;\n}"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        // Tier 3: resolved type shape is identical → no invalidation
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            !comp.compile_slots.is_empty(),
            "compile slots should NOT be cleared when resolved type shape is unchanged (Tier 3)"
        );
    }

    /// @ai-generated - update_alias_map: removes old aliases, adds new ones
    #[test]
    fn update_alias_map_removes_old_adds_new() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let old_aliases: BTreeSet<String> = ["old-alias".to_string()].into();
        let new_aliases: BTreeSet<String> =
            ["new-alias".to_string(), "Comp.vue".to_string()].into();

        // Pre-populate old alias
        {
            let mut map = write_lock(&host.alias_to_canonical);
            map.insert("old-alias".to_string(), "Comp.vue".to_string());
        }

        host.update_alias_map("Comp.vue", &old_aliases, &new_aliases);

        let map = read_lock(&host.alias_to_canonical);
        assert!(
            !map.contains_key("old-alias"),
            "old alias should be removed"
        );
        assert_eq!(map.get("new-alias"), Some(&"Comp.vue".to_string()));
        assert_eq!(map.get("Comp.vue"), Some(&"Comp.vue".to_string()));
    }

    /// @ai-generated - update_reverse_deps: keeps shared deps when another owner exists
    #[test]
    fn update_reverse_deps_keeps_shared_dep() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // shared-dep.ts is owned by both Comp.vue and Other.vue
        {
            let mut rev = write_lock(&host.reverse_dependencies);
            let owners = rev.entry("shared-dep.ts".to_string()).or_default();
            owners.insert("Comp.vue".to_string());
            owners.insert("Other.vue".to_string());
        }

        // Comp.vue drops shared-dep.ts
        let old_deps: BTreeSet<String> = ["shared-dep.ts".to_string()].into();
        let new_deps: BTreeSet<String> = BTreeSet::new();
        host.update_reverse_deps("Comp.vue", &old_deps, &new_deps);

        let rev = read_lock(&host.reverse_dependencies);
        // shared-dep.ts should still exist because Other.vue still depends on it
        let owners = rev.get("shared-dep.ts").unwrap();
        assert!(!owners.contains("Comp.vue"));
        assert!(owners.contains("Other.vue"));
    }

    /// @ai-generated - update_reverse_deps: removes stale deps, adds new ones
    #[test]
    fn update_reverse_deps_removes_stale_adds_new() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let old_deps: BTreeSet<String> = ["old-dep.ts".to_string()].into();
        let new_deps: BTreeSet<String> = ["new-dep.ts".to_string()].into();

        // Pre-populate old reverse dep
        {
            let mut rev = write_lock(&host.reverse_dependencies);
            rev.entry("old-dep.ts".to_string())
                .or_default()
                .insert("Comp.vue".to_string());
        }

        host.update_reverse_deps("Comp.vue", &old_deps, &new_deps);

        let rev = read_lock(&host.reverse_dependencies);
        assert!(
            !rev.contains_key("old-dep.ts"),
            "old dependency should be removed"
        );
        let new_owners = rev.get("new-dep.ts").unwrap();
        assert!(new_owners.contains("Comp.vue"));
    }

    /// @ai-generated - FileMeta::virtual_nodes: empty meta produces only Main
    #[test]
    fn virtual_nodes_empty() {
        let meta = FileMeta::default();
        let nodes = meta.virtual_nodes();
        assert_eq!(nodes, vec![VirtualNodeKind::Main]);
    }

    /// @ai-generated - FileMeta::virtual_nodes: full SFC produces all node kinds
    #[test]
    fn virtual_nodes_full_sfc() {
        let meta = FileMeta {
            has_script: true,
            has_template: true,
            script_lang: Some("ts".to_string()),
            style_langs: vec![None, Some("scss".to_string())],
            custom_types: vec!["i18n".to_string()],
            custom_langs: vec![None],
            ..FileMeta::default()
        };
        let nodes = meta.virtual_nodes();
        assert_eq!(
            nodes,
            vec![
                VirtualNodeKind::Main,
                VirtualNodeKind::Script,
                VirtualNodeKind::Template,
                VirtualNodeKind::Style { index: 0 },
                VirtualNodeKind::Style { index: 1 },
                VirtualNodeKind::Custom { index: 0 },
            ]
        );
    }

    // ── E2E: Style override with source map remapping ──

    /// Build a source map JSON from (dst_line, dst_col, src_line, src_col) tuples.
    fn build_test_source_map(original: &str, mappings: &[(u32, u32, u32, u32)]) -> String {
        use sourcemap::SourceMapBuilder;

        let mut builder = SourceMapBuilder::new(Some("output.css"));
        let src_id = builder.add_source("input.sass");
        builder.set_source_contents(src_id, Some(original));

        for &(dst_line, dst_col, src_line, src_col) in mappings {
            builder.add_raw(
                dst_line,
                dst_col,
                src_line,
                src_col,
                Some(src_id),
                None,
                false,
            );
        }

        let sm = builder.into_sourcemap();
        let mut buf = Vec::new();
        sm.to_writer(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    /// E2E Test: Multiple style blocks — CSS block unaffected, preprocessed block remapped.
    ///
    /// Verifies that when a Vue SFC has both `<style>` (CSS) and a preprocessed
    /// `<style lang="sass">` block, applying a style override with source map:
    /// - Does NOT alter the plain CSS block's analysis spans
    /// - DOES remap the preprocessed block's analysis spans to original SFC positions
    #[test]
    fn style_override_remaps_preprocessed_block_preserves_css_block() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // SFC with two style blocks: plain CSS (index 0) and "sass" (index 1)
        let sfc = concat!(
            "<template><div class=\"used\">hello</div></template>\n",
            "<style>\n",
            ".used { color: red; }\n",
            "</style>\n",
            "<style lang=\"sass\">\n",
            ".header\n",
            "  font-size: 16px\n",
            "</style>\n",
        );

        upsert_vue(&host, "multi.vue", sfc);

        // Get original analysis before override
        let analysis_before = host.get_analysis("multi.vue").unwrap();
        assert_eq!(
            analysis_before.styles.len(),
            2,
            "should have 2 style blocks"
        );

        let css_block_before = &analysis_before.styles[0];
        let css_classes_before = css_block_before.css.as_ref().unwrap().classes.clone();

        // The Sass block (index 1) initially has no CSS analysis
        // because `build_preprocessor_style_analysis` is used for non-CSS langs
        let sass_block_before = &analysis_before.styles[1];
        let _sass_css_before = sass_block_before.css.as_ref();

        // Simulate transpilation: "Sass" → CSS
        let compiled_css = ".header { font-size: 16px; }\n";

        // The content_offset points right after the `>` of `<style lang="sass">`,
        // which is the `\n` before `.header`. So the actual content from the
        // preprocessor's perspective is `\n.header\n  font-size: 16px\n`.
        // In this content, `.header` is on line 1 (line 0 is the empty `\n`).
        let original_content = "\n.header\n  font-size: 16px\n";
        let sm_json = build_test_source_map(
            original_content,
            &[
                (0, 0, 1, 0), // .header in compiled (line 0) → original line 1, col 0
            ],
        );

        // Apply the style override for index 1 (the sass block)
        let profile = CompileProfile {
            source_map: true,
            target: CompileTarget::BUNDLER | CompileTarget::TSX,
            ..CompileProfile::default()
        };
        let result = host.apply_style_overrides(StyleOverrideRequest {
            canonical_id: "multi.vue".to_string(),
            compile_profile: profile,
            overrides: vec![StyleOverrideEntry {
                index: 1,
                code: Arc::from(compiled_css),
                source_map: Some(Arc::from(sm_json)),
            }],
        });
        assert!(result.is_ok(), "apply_style_overrides should succeed");

        // Get analysis after override
        let analysis_after = host.get_analysis("multi.vue").unwrap();
        assert_eq!(
            analysis_after.styles.len(),
            2,
            "should still have 2 style blocks"
        );

        // CSS block (index 0) should be UNCHANGED
        let css_block_after = &analysis_after.styles[0];
        let css_classes_after = css_block_after.css.as_ref().unwrap().classes.clone();
        assert_eq!(
            css_classes_before.len(),
            css_classes_after.len(),
            "CSS block class count should be unchanged"
        );
        for (before, after) in css_classes_before.iter().zip(css_classes_after.iter()) {
            assert_eq!(
                before.name, after.name,
                "CSS block class names should match"
            );
            assert_eq!(
                before.span.start, after.span.start,
                "CSS block class spans should be unchanged"
            );
        }

        // Sass block (index 1) should now have CSS analysis from the compiled CSS
        let sass_block_after = &analysis_after.styles[1];
        let sass_css_after = sass_block_after.css.as_ref();
        assert!(
            sass_css_after.is_some(),
            "sass block should now have CSS analysis after override"
        );

        let sass_selectors = &sass_css_after.unwrap().selectors;
        assert!(
            !sass_selectors.is_empty(),
            "should have at least one selector"
        );

        // The .header selector span should point to the original sass content in the SFC
        let header_sel = sass_selectors.iter().find(|s| s.text == ".header");
        assert!(header_sel.is_some(), ".header selector should exist");
        let header_sel = header_sel.unwrap();

        // .header is one byte into the style content (after the leading newline),
        // so the stored span is SFC-absolute content_offset + 1.
        assert_eq!(
            header_sel.span.start,
            sass_block_after.content_offset + 1,
            ".header should be stored as an SFC-absolute span"
        );

        // content_offset should point right after `>` of `<style lang="sass">`
        // (the `\n` before `.header`, NOT at `.header` itself)
        let tag_end = sfc.find("<style lang=\"sass\">").unwrap() + "<style lang=\"sass\">".len();
        assert_eq!(
            sass_block_after.content_offset as usize, tag_end,
            "content_offset should point right after the style tag"
        );

        // Double-check the stored absolute span points directly at ".header".
        let sfc_absolute = header_sel.span.start;
        assert_eq!(
            &sfc[sfc_absolute as usize..sfc_absolute as usize + 7],
            ".header",
            "stored selector span should point to '.header' in the original SFC"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // apply_block_overrides
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - apply_block_overrides: template override produces compile-ready source
    #[test]
    fn apply_block_overrides_template() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let sfc = "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
        let _ = upsert_vue(&host, "test.vue", sfc);

        let profile = CompileProfile::default();
        let result = host.apply_block_overrides(BlockOverrideRequest {
            canonical_id: "test.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Template,
                index: 0,
                code: Arc::from("<div>hello</div>"),
                source_map: None,
            }],
        });
        assert!(result.is_ok(), "apply_block_overrides should succeed");
        let result = result.unwrap();
        assert!(result.changed, "should report changed");

        // Verify the source was updated — the template should now contain native HTML
        let source = host.get_source("test.vue");
        assert!(source.is_some(), "source should exist");
        let source = source.unwrap();
        assert!(
            source.contains("<div>hello</div>"),
            "synthetic source should contain preprocessed HTML, got: {}",
            source
        );
        assert!(
            !source.contains("lang=\"pug\""),
            "synthetic source should NOT contain lang=\"pug\""
        );

        // Verify the file can be compiled (get_virtual_file succeeds)
        let vf = host.get_virtual_file(VirtualQuery {
            raw_id: Some("test.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile,
        });
        assert!(
            vf.is_ok(),
            "should be able to compile template after block override"
        );
    }

    /// @ai-generated - apply_block_overrides: no change if same override applied twice
    #[test]
    fn apply_block_overrides_no_change_if_same_hash() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let sfc = "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
        let _ = upsert_vue(&host, "test.vue", sfc);

        let profile = CompileProfile::default();
        let overrides = vec![BlockOverrideEntry {
            block_type: PreprocessorBlockType::Template,
            index: 0,
            code: Arc::from("<div>hello</div>"),
            source_map: None,
        }];

        // First apply
        let r1 = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: "test.vue".to_string(),
                compile_profile: profile.clone(),
                overrides: overrides.clone(),
            })
            .unwrap();
        assert!(r1.changed, "first apply should report changed");

        // Second apply with same content — should report no change
        let r2 = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: "test.vue".to_string(),
                compile_profile: profile,
                overrides,
            })
            .unwrap();
        assert!(
            !r2.changed,
            "second apply with same hash should report no change"
        );
    }

    /// @ai-generated - apply_block_overrides: style overrides delegated to existing mechanism
    #[test]
    fn apply_block_overrides_style_delegated() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let sfc = "<template><div>hello</div></template>\n<script setup>const x = 1</script>\n<style lang=\"scss\">.a { .b { color: red } }</style>";
        let _ = upsert_vue(&host, "test.vue", sfc);

        let profile = CompileProfile {
            source_map: true,
            ..CompileProfile::default()
        };
        let result = host.apply_block_overrides(BlockOverrideRequest {
            canonical_id: "test.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Style,
                index: 0,
                code: Arc::from(".a .b { color: red }"),
                source_map: None,
            }],
        });
        assert!(
            result.is_ok(),
            "apply_block_overrides with style should succeed"
        );

        // Verify the style virtual file serves the overridden CSS
        let vf = host.get_virtual_file(VirtualQuery {
            raw_id: Some("test.vue?vue&type=style&index=0&lang.css".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile,
        });
        assert!(vf.is_ok(), "should be able to get style virtual file");
        let vf = vf.unwrap();
        assert!(
            vf.code.contains(".a .b"),
            "style output should contain overridden CSS, got: {}",
            &vf.code[..vf.code.len().min(200)]
        );
    }

    /// After style preprocessing, virtual IDs should use lang.css
    /// instead of the original preprocessor lang (e.g. lang.sass).
    /// Without this, Vite would try to re-preprocess compiled CSS
    /// as SASS indented syntax, causing build failures.
    #[test]
    fn style_override_changes_virtual_id_lang_to_css() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let sfc = "<template><div>hello</div></template>\n<style lang=\"sass\">.a\n  color: red\n</style>";
        let upsert = upsert_vue(&host, "test.vue", sfc);
        // Before override, the URL should have the original lang
        assert!(
            upsert
                .changed_virtual_ids
                .iter()
                .any(|id| id.contains("lang.sass")),
            "before override, should have lang.sass in virtual IDs: {:?}",
            upsert.changed_virtual_ids
        );

        let profile = CompileProfile::default();
        let _ = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: "test.vue".to_string(),
                compile_profile: profile.clone(),
                overrides: vec![BlockOverrideEntry {
                    block_type: PreprocessorBlockType::Style,
                    index: 0,
                    code: Arc::from(".a { color: red; }"),
                    source_map: None,
                }],
            })
            .expect("apply_block_overrides should succeed");

        // The main module assembly should use lang.css after override
        let main = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("test.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile,
            })
            .expect("should get main virtual file");
        assert!(
            main.code.contains("lang.css"),
            "main module should import style with lang.css, got:\n{}",
            main.code
        );
        assert!(
            !main.code.contains("lang.sass"),
            "main module should NOT import style with lang.sass, got:\n{}",
            main.code
        );
    }

    /// @ai-generated - upsert returns preprocessor_requests for pug template
    #[test]
    fn upsert_returns_preprocessor_requests() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let sfc = "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
        let result = upsert_vue(&host, "test.vue", sfc);
        assert!(
            !result.preprocessor_requests.is_empty(),
            "should have preprocessor requests for pug template"
        );
        let req = &result.preprocessor_requests[0];
        assert_eq!(req.block_type, PreprocessorBlockType::Template);
        assert_eq!(req.lang, "pug");
        assert!(req.content.contains("div hello"));
    }

    /// Verify that the host's `Shared<T>` RwLock has writer-preferring semantics.
    ///
    /// When a writer is waiting, new readers should queue behind it. This prevents
    /// writer starvation where continuous readers indefinitely delay write operations.
    ///
    /// With `parking_lot::RwLock` (writer-preferring): once the writer calls write(),
    /// new read() calls block until the writer is done → reader_cycles stays low (~16,
    /// equal to the number of currently-holding readers that finish their cycle).
    ///
    /// A reader-preferring lock would allow hundreds+ of reader cycles while the
    /// writer waits, causing the upsert latency issues seen in production.
    #[test]
    fn writer_starvation_under_continuous_read_pressure() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::time::{Duration, Instant};

        let data = Arc::new(default_shared(0u64));
        let stop = Arc::new(AtomicBool::new(false));
        let writer_waiting = Arc::new(AtomicBool::new(false));
        let reader_cycles_during_wait = Arc::new(AtomicU64::new(0));

        // Spawn 16 reader threads that hold the lock for ~5ms each, back-to-back.
        let mut reader_handles = Vec::new();
        for _ in 0..16 {
            let data = Arc::clone(&data);
            let stop = Arc::clone(&stop);
            let ww = Arc::clone(&writer_waiting);
            let cycles = Arc::clone(&reader_cycles_during_wait);
            reader_handles.push(std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let guard = read_lock(&data);
                    // Count read cycles that occur while the writer is waiting.
                    // With writer-preferring locks: readers block → very low count.
                    if ww.load(Ordering::Relaxed) {
                        cycles.fetch_add(1, Ordering::Relaxed);
                    }
                    // Hold the read lock for ~5ms (busy wait)
                    let hold_start = Instant::now();
                    while hold_start.elapsed() < Duration::from_millis(5) {
                        std::hint::spin_loop();
                    }
                    drop(guard);
                }
            }));
        }

        // Let readers saturate
        std::thread::sleep(Duration::from_millis(100));

        // Signal that writer is about to request the lock
        writer_waiting.store(true, Ordering::SeqCst);

        // Acquire write lock — measures how many reader cycles happen while waiting
        let data_w = Arc::clone(&data);
        let start = Instant::now();
        let mut guard = write_lock(&data_w);
        *guard = 42;
        let write_latency = start.elapsed();
        drop(guard);

        // Stop readers
        stop.store(true, Ordering::Relaxed);
        for h in reader_handles {
            let _ = h.join();
        }

        let reader_cycles = reader_cycles_during_wait.load(Ordering::SeqCst);

        // With writer-preferring lock, once the writer calls write(), new readers
        // are blocked. Only the ~16 currently-holding readers finish their 5ms cycle.
        // Threshold: 50 (generous: 16 holding + possible re-acquires before visibility).
        assert!(
            reader_cycles <= 50,
            "writer-preferring lock should block new readers while writer waits — \
             got {reader_cycles} reader cycles during writer wait (latency: {write_latency:?})"
        );
        // Writer should complete in ~5ms (max reader hold time), not seconds.
        assert!(
            write_latency < Duration::from_millis(500),
            "writer should acquire lock quickly with writer-preferring semantics — \
             took {write_latency:?}"
        );
    }

    #[test]
    fn close_clears_all_caches() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // Upsert a file to populate caches
        upsert_vue(&host, "test.vue", "<template><div>hello</div></template>");

        // Verify the host has data
        assert!(
            !read_lock(&host.files).is_empty(),
            "host should have files before close"
        );

        // Close and verify everything is cleared
        host.close();

        assert!(
            read_lock(&host.files).is_empty(),
            "files should be empty after close"
        );
        assert!(
            read_lock(&host.alias_to_canonical).is_empty(),
            "alias_to_canonical should be empty after close"
        );
        assert!(
            read_lock(&host.reverse_dependencies).is_empty(),
            "reverse_dependencies should be empty after close"
        );
        assert!(
            read_lock(&host.last_const_prop_overrides).is_empty(),
            "last_const_prop_overrides should be empty after close"
        );
    }

    #[test]
    fn close_allows_reuse() {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert_vue(&host, "test.vue", "<template><div>hello</div></template>");
        host.close();

        // Host should still be usable after close
        upsert_vue(
            &host,
            "test2.vue",
            "<template><span>world</span></template>",
        );
        assert!(
            read_lock(&host.files).contains_key("test2.vue"),
            "host should accept new files after close"
        );
        assert!(
            !read_lock(&host.files).contains_key("test.vue"),
            "previously closed files should not reappear"
        );
    }

    // ── Project resolver tests ───────────────────────────────────────

    fn make_project_config(
        root: &str,
        paths: Vec<(&str, Vec<&str>)>,
    ) -> verter_analysis::project_resolver::IdeProjectConfig {
        let mut config = verter_analysis::project_resolver::IdeProjectConfig::new(
            root.to_string(),
            root.to_string(),
            None,
        );
        config.compiler_options.paths = paths
            .into_iter()
            .map(|(pat, targets)| {
                (
                    pat.to_string(),
                    targets.iter().map(|t| t.to_string()).collect(),
                )
            })
            .collect();
        config
    }

    fn upsert_non_sfc(host: &VerterHost, id: &str, src: &str) {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    }

    #[test]
    fn configure_projects_exact_alias() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // Configure project with exact path mapping
        host.configure_projects(vec![make_project_config(
            "/project",
            vec![("#imports", vec!["./types/imports.d.ts"])],
        )]);

        // Upsert target file
        upsert_non_sfc(
            &host,
            "/project/types/imports.d.ts",
            "export type Foo = string;",
        );

        // Upsert parent file that imports via the alias
        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport type { Foo } from '#imports'\n</script>\n<template><div/></template>",
        );

        // Resolve the aliased import
        let resolved = host.resolve_import("/project/src/App.vue", "#imports");
        assert_eq!(
            resolved.as_deref(),
            Some("/project/types/imports.d.ts"),
            "exact alias #imports should resolve via project resolver"
        );
    }

    #[test]
    fn configure_projects_wildcard_alias() {
        let host = VerterHost::new_standalone(HostConfig::default());

        host.configure_projects(vec![make_project_config(
            "/project",
            vec![("@/*", vec!["./src/*"])],
        )]);

        upsert_vue(
            &host,
            "/project/src/components/Child.vue",
            "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div>{{ msg }}</div></template>",
        );

        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><Child msg=\"hi\" /></template>",
        );

        let resolved = host.resolve_import("/project/src/App.vue", "@/components/Child.vue");
        assert_eq!(
            resolved.as_deref(),
            Some("/project/src/components/Child.vue"),
            "wildcard alias @/* should resolve via project resolver"
        );
    }

    #[test]
    fn configure_projects_multi_project() {
        let host = VerterHost::new_standalone(HostConfig::default());

        host.configure_projects(vec![
            make_project_config("/workspace/app", vec![("@app/*", vec!["./src/*"])]),
            make_project_config("/workspace/lib", vec![("@lib/*", vec!["./src/*"])]),
        ]);

        upsert_non_sfc(
            &host,
            "/workspace/app/src/utils.ts",
            "export const foo = 1;",
        );
        upsert_non_sfc(
            &host,
            "/workspace/lib/src/utils.ts",
            "export const bar = 2;",
        );
        upsert_vue(
            &host,
            "/workspace/app/src/App.vue",
            "<script setup>\nimport { foo } from '@app/utils'\n</script>\n<template><div/></template>",
        );
        upsert_vue(
            &host,
            "/workspace/lib/src/Lib.vue",
            "<script setup>\nimport { bar } from '@lib/utils'\n</script>\n<template><div/></template>",
        );

        let app_resolved = host.resolve_import("/workspace/app/src/App.vue", "@app/utils");
        assert_eq!(
            app_resolved.as_deref(),
            Some("/workspace/app/src/utils.ts"),
            "@app/* should resolve to app project"
        );

        let lib_resolved = host.resolve_import("/workspace/lib/src/Lib.vue", "@lib/utils");
        assert_eq!(
            lib_resolved.as_deref(),
            Some("/workspace/lib/src/utils.ts"),
            "@lib/* should resolve to lib project"
        );
    }

    #[test]
    fn set_import_dependencies_overrides_project_resolver() {
        let host = VerterHost::new_standalone(HostConfig::default());

        host.configure_projects(vec![make_project_config(
            "/project",
            vec![("@/*", vec!["./src/*"])],
        )]);

        // Two possible targets
        upsert_non_sfc(&host, "/project/src/utils.ts", "export const a = 1;");
        upsert_non_sfc(&host, "/project/custom/utils.ts", "export const b = 2;");

        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport { b } from '@/utils'\n</script>\n<template><div/></template>",
        );

        // Project resolver would map @/utils → /project/src/utils.ts
        // But structured deps should override to custom/utils.ts
        host.set_import_dependencies(
            "/project/src/App.vue",
            vec![DependencyResolution {
                specifier: "@/utils".to_string(),
                resolved_canonical_id: Some("/project/custom/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let resolved = host.resolve_import("/project/src/App.vue", "@/utils");
        assert_eq!(
            resolved.as_deref(),
            Some("/project/custom/utils.ts"),
            "structured deps should override project resolver"
        );
    }

    #[test]
    fn configure_projects_empty_clears() {
        let host = VerterHost::new_standalone(HostConfig::default());

        host.configure_projects(vec![make_project_config(
            "/project",
            vec![("#imports", vec!["./types/imports.d.ts"])],
        )]);

        upsert_non_sfc(
            &host,
            "/project/types/imports.d.ts",
            "export type Foo = string;",
        );
        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport type { Foo } from '#imports'\n</script>\n<template><div/></template>",
        );

        // Should resolve before clearing
        assert!(
            host.resolve_import("/project/src/App.vue", "#imports")
                .is_some(),
            "should resolve before clearing"
        );

        // Clear resolver
        host.configure_projects(vec![]);

        // Should no longer resolve via project resolver
        let resolved = host.resolve_import("/project/src/App.vue", "#imports");
        assert!(
            resolved.is_none(),
            "should not resolve after clearing projects, got: {:?}",
            resolved
        );
    }

    #[test]
    fn configure_projects_fallthrough_unloaded() {
        let host = VerterHost::new_standalone(HostConfig::default());

        host.configure_projects(vec![make_project_config(
            "/project",
            vec![("@/*", vec!["./src/*"])],
        )]);

        // DON'T upsert the target file — it's not loaded
        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><div/></template>",
        );

        // Should gracefully return None (no panic)
        let resolved = host.resolve_import("/project/src/App.vue", "@/components/Child.vue");
        assert!(
            resolved.is_none(),
            "should fall through when resolved file not in host"
        );
    }

    #[test]
    fn cross_file_optimization_with_project_resolver() {
        let host = VerterHost::new_standalone(HostConfig::default());

        host.configure_projects(vec![make_project_config(
            "/project",
            vec![("@/*", vec!["./src/*"])],
        )]);

        upsert_vue(
            &host,
            "/project/src/components/Child.vue",
            "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div>{{ msg }}</div></template>",
        );

        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><Child msg=\"hello\" /></template>",
        );

        // Compile both to generate template analysis
        host.get_virtual_file(VirtualQuery {
            raw_id: Some("/project/src/components/Child.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: CompileProfile::default(),
        })
        .unwrap();
        host.get_virtual_file(VirtualQuery {
            raw_id: Some("/project/src/App.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: CompileProfile::default(),
        })
        .unwrap();

        let result = host.compute_cross_file_optimizations();
        let child_consts = result
            .const_prop_overrides
            .get("/project/src/components/Child.vue");
        assert!(
            child_consts.is_some(),
            "cross-file optimization should resolve aliased import via project resolver. Overrides: {:?}",
            result.const_prop_overrides
        );
        assert!(child_consts.unwrap().contains("msg"), "msg should be const");
    }

    // ── Workspace integration tests (Phase 4) ──

    #[test]
    fn new_with_workspace_stores_workspace_ref() {
        let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
            verter_vfs::MemoryOptions::default(),
        ));
        let host = VerterHost::new(HostConfig::default(), ws.clone());

        // Positive: workspace reference is accessible
        let ws_ref = host.workspace();
        assert!(!ws_ref.file_exists("nonexistent.vue"));

        // Inject a file via workspace and verify it's visible
        ws.inject_file("test.vue".to_string(), Arc::from("<template></template>"));
        assert!(ws_ref.file_exists("test.vue"));
    }

    #[test]
    fn new_standalone_creates_host_with_memory_workspace() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // Positive: host is functional
        let ws = host.workspace();
        assert!(!ws.file_exists("anything.vue"));

        // Positive: host can still upsert and compile normally
        upsert_vue(&host, "App.vue", "<template><div>hi</div></template>");
        let source = host.get_source("App.vue");
        assert!(source.is_some(), "upsert should work on standalone host");
    }

    #[test]
    fn workspace_accessor_returns_same_arc() {
        let ws: Arc<dyn verter_vfs::WorkspaceAccess> = Arc::new(verter_vfs::MemoryWorkspace::new(
            verter_vfs::MemoryOptions::default(),
        ));
        let host = VerterHost::new(HostConfig::default(), ws.clone());

        // Positive: workspace() returns the same Arc we passed in
        let ws_ref = host.workspace();
        // Compare trait object pointer identity via data pointer
        let ptr1 = Arc::as_ptr(&ws) as *const () as usize;
        let ptr2 = Arc::as_ptr(&ws_ref) as *const () as usize;
        assert_eq!(ptr1, ptr2, "workspace() should return the same Arc");
    }

    #[test]
    fn resolve_import_via_workspace_uses_exact_resolutions() {
        let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
            verter_vfs::MemoryOptions::default(),
        ));

        // Set up exact resolutions on the workspace
        ws.set_exact_resolutions(
            "/src/App.vue",
            vec![verter_vfs::ExactResolution {
                specifier: "./Child.vue".to_string(),
                resolved_canonical_id: Some("/src/Child.vue".to_string()),
                possible_canonical_ids: vec!["/src/Child.vue".to_string()],
            }],
        );

        let host = VerterHost::new(HostConfig::default(), ws);

        // Positive: exact resolution works
        let resolved = host.resolve_import_via_workspace("/src/App.vue", "./Child.vue");
        assert_eq!(
            resolved.as_deref(),
            Some("/src/Child.vue"),
            "should resolve through workspace exact resolutions"
        );

        // Negative: non-existent specifier returns None
        let not_found = host.resolve_import_via_workspace("/src/App.vue", "./Missing.vue");
        assert!(
            not_found.is_none(),
            "should return None for unresolved imports"
        );
    }

    #[test]
    fn resolve_import_via_workspace_returns_none_for_no_resolution() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // Negative: standalone workspace has no resolutions
        let result = host.resolve_import_via_workspace("/src/App.vue", "./anything");
        assert!(
            result.is_none(),
            "standalone workspace should not resolve arbitrary imports"
        );
    }

    #[test]
    fn host_debug_impl_works_with_workspace() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let debug_str = format!("{:?}", host);
        assert!(
            debug_str.contains("VerterHost"),
            "Debug should produce VerterHost output"
        );
        assert!(
            !debug_str.contains("workspace"),
            "Debug should not expose workspace internals"
        );
    }

    // ── VFS authoritative host runtime tests ──

    #[test]
    fn set_workspace_swaps_resolution_source() {
        // Start with a standalone host (MemoryWorkspace).
        let host = VerterHost::new_standalone(HostConfig::default());

        // Upsert two files: a parent and a dependency.
        upsert_vue(
            &host,
            "/src/App.vue",
            "<script setup>\nimport Btn from './Btn.vue'\n</script>\n<template><Btn/></template>",
        );
        upsert_vue(
            &host,
            "/src/Btn.vue",
            "<template><button>click</button></template>",
        );

        // Build a NEW workspace that has exact resolution wired.
        let new_ws = Arc::new(verter_vfs::MemoryWorkspace::new(
            verter_vfs::MemoryOptions::default(),
        ));
        new_ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from("<script setup>\nimport Btn from './Btn.vue'\n</script>\n<template><Btn/></template>"),
        );
        new_ws.inject_file(
            "/src/Btn.vue".to_string(),
            Arc::from("<template><button>click</button></template>"),
        );
        // Set exact resolution: ./Btn.vue -> /src/Btn.vue
        new_ws.set_exact_resolutions(
            "/src/App.vue",
            vec![verter_vfs::ExactResolution {
                specifier: "./Btn.vue".to_string(),
                resolved_canonical_id: Some("/src/Btn.vue".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        // Swap the workspace.
        host.set_workspace(new_ws.clone() as Arc<dyn verter_vfs::WorkspaceAccess>);

        // Positive: resolve_import_via_workspace should use the new workspace.
        let result = host.resolve_import_via_workspace("/src/App.vue", "./Btn.vue");
        assert_eq!(
            result.as_deref(),
            Some("/src/Btn.vue"),
            "set_workspace should make the new workspace's exact resolutions available"
        );

        // Negative: the old standalone workspace should no longer be used.
        // (The new workspace doesn't have an arbitrary specifier.)
        let no_result = host.resolve_import_via_workspace("/src/App.vue", "./NotExist.vue");
        assert!(
            no_result.is_none(),
            "specifiers not in the new workspace should not resolve"
        );
    }

    #[test]
    fn configure_projects_syncs_to_workspace() {
        use verter_analysis::project_resolver::IdeProjectConfig;

        let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
            verter_vfs::MemoryOptions::default(),
        ));
        let host = VerterHost::new(HostConfig::default(), ws.clone());

        // Inject files into the workspace.
        ws.inject_file(
            "/my-project/src/App.vue".to_string(),
            Arc::from("<script setup>\nimport Foo from '@/Foo.vue'\n</script>\n<template><Foo/></template>"),
        );
        ws.inject_file(
            "/my-project/src/Foo.vue".to_string(),
            Arc::from("<template><div>Foo</div></template>"),
        );

        // Configure project with a path alias: @ -> /my-project/src
        let mut project = IdeProjectConfig::new(
            "/my-project".to_string(),
            "/my-project".to_string(),
            Some("/my-project/tsconfig.json".to_string()),
        );
        project.compiler_options.paths =
            vec![("@/*".to_string(), vec!["/my-project/src/*".to_string()])];
        host.configure_projects(vec![project]);

        // Positive: workspace should now resolve @/Foo.vue via the synced resolver.
        let result = ws.resolve_import(
            "/my-project/src/App.vue",
            "@/Foo.vue",
            verter_vfs::ResolveRequestKind::EsmImport,
        );
        assert!(
            result.is_some(),
            "configure_projects should sync resolver to workspace"
        );
        assert_eq!(
            result.unwrap().source_id,
            "/my-project/src/Foo.vue",
            "workspace resolver should resolve @/Foo.vue"
        );

        // Negative: non-matching alias should not resolve.
        let no_result = ws.resolve_import(
            "/my-project/src/App.vue",
            "~/Bar.vue",
            verter_vfs::ResolveRequestKind::EsmImport,
        );
        assert!(
            no_result.is_none(),
            "non-matching alias should not resolve via workspace"
        );
    }

    #[test]
    fn set_import_dependencies_syncs_exact_resolutions_to_workspace() {
        let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
            verter_vfs::MemoryOptions::default(),
        ));
        let host = VerterHost::new(HostConfig::default(), ws.clone());

        // Upsert the parent file.
        upsert_vue(
            &host,
            "/src/App.vue",
            "<script setup>\nimport Btn from '@comp/Btn.vue'\n</script>\n<template><Btn/></template>",
        );
        // Upsert the dependency.
        upsert_vue(
            &host,
            "/src/components/Btn.vue",
            "<template><button/></template>",
        );

        // Set import dependencies (simulating bundler/LSP resolution).
        host.set_import_dependencies(
            "/src/App.vue",
            vec![DependencyResolution {
                specifier: "@comp/Btn.vue".to_string(),
                resolved_canonical_id: Some("/src/components/Btn.vue".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        // Positive: workspace should now have exact resolution for this specifier.
        let result = ws.resolve_import(
            "/src/App.vue",
            "@comp/Btn.vue",
            verter_vfs::ResolveRequestKind::EsmImport,
        );
        assert!(
            result.is_some(),
            "set_import_dependencies should sync exact resolutions to workspace"
        );
        assert_eq!(
            result.unwrap().source_id,
            "/src/components/Btn.vue",
            "workspace exact resolution should match the provided dependency"
        );

        // Negative: other specifiers should not resolve.
        let no_result = ws.resolve_import(
            "/src/App.vue",
            "@comp/Other.vue",
            verter_vfs::ResolveRequestKind::EsmImport,
        );
        assert!(
            no_result.is_none(),
            "specifiers not in set_import_dependencies should not resolve"
        );
    }

    #[test]
    fn workspace_resolution_is_phase_0_primary() {
        let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
            verter_vfs::MemoryOptions::default(),
        ));
        let host = VerterHost::new(HostConfig::default(), ws.clone());

        // Inject files.
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                "<script setup>\nimport T from './types'\n</script>\n<template><div/></template>",
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from("export type Foo = string;"),
        );

        // Upsert both into the host.
        upsert_vue(
            &host,
            "/src/App.vue",
            "<script setup>\nimport T from './types'\n</script>\n<template><div/></template>",
        );
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export type Foo = string;"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

        // Set exact resolution on workspace ONLY (not on host's dependency_resolutions).
        ws.set_exact_resolutions(
            "/src/App.vue",
            vec![verter_vfs::ExactResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        // Positive: workspace Phase 0 should resolve ./types -> /src/types.ts
        // even though the host's legacy Phase 1 has no dependency_resolutions for it.
        let result = host.resolve_import_via_workspace("/src/App.vue", "./types");
        assert_eq!(
            result.as_deref(),
            Some("/src/types.ts"),
            "workspace Phase 0 should be primary resolution source"
        );

        // Negative: random specifiers still don't resolve.
        let no_result = host.resolve_import_via_workspace("/src/App.vue", "./nonexistent");
        assert!(
            no_result.is_none(),
            "unresolvable specifiers should still return None"
        );
    }

    #[test]
    fn smart_invalidation_reads_workspace_reverse_deps() {
        let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
            verter_vfs::MemoryOptions::default(),
        ));

        // Inject files into the workspace.
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from("export interface Props { name: string }"),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from("<script setup lang=\"ts\">\nimport type { Props } from './types'\ndefineProps<Props>()\n</script>\n<template><div/></template>"),
        );

        let host = VerterHost::new(HostConfig::default(), ws.clone());

        // Upsert both files.
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface Props { name: string }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
        upsert_vue(
            &host,
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport type { Props } from './types'\ndefineProps<Props>()\n</script>\n<template><div/></template>",
        );

        // Simulate the bundler/LSP resolution flow: after upsert, the caller
        // provides resolved import dependencies. set_import_dependencies now
        // syncs exact resolutions to the workspace.
        host.set_import_dependencies(
            "/src/App.vue",
            vec![DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        // Positive: workspace should now have /src/App.vue as a reverse dep
        // of /src/types.ts via exact_resolved_deps.
        let rev_deps = ws.reverse_deps_for("/src/types.ts");
        assert!(
            rev_deps.contains(&"/src/App.vue".to_string()),
            "workspace reverse deps should include App.vue after set_import_dependencies (got: {:?})",
            rev_deps,
        );

        // Negative: non-imported files should NOT appear as reverse deps.
        let other_rev = ws.reverse_deps_for("/src/something-else.ts");
        assert!(
            !other_rev.contains(&"/src/App.vue".to_string()),
            "unrelated files should not appear in reverse deps"
        );
    }
}
