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

    /// Compute the preferred alias-based import specifier for a target file.
    ///
    /// Returns the shortest tsconfig-path or workspace-alias specifier that
    /// round-trips correctly. Returns `None` if no alias matches.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preferred_specifier(&self, importer_id: &str, target_id: &str) -> Option<String> {
        self.ws().preferred_specifier(importer_id, target_id)
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

    /// Set the host's internal project resolver for compilation (Phase 2 fallback).
    ///
    /// Does NOT sync to the workspace — workspace resolver comes from
    /// `set_project_graph()` on the `FilesystemWorkspace`. Use this when
    /// the workspace resolver is populated separately (e.g., by the LSP's
    /// `background_init` which calls `set_project_graph()` directly).
    pub fn set_internal_resolver(
        &self,
        projects: Vec<verter_analysis::project_resolver::IdeProjectConfig>,
    ) {
        let resolver = if projects.is_empty() {
            None
        } else {
            Some(NativeProjectResolver::new(projects))
        };
        *write_lock(&self.project_resolver) = resolver;
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
#[path = "lib_tests.rs"]
mod lib_tests;
