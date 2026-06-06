use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::{ArcSwap, ArcSwapOption};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use verter_scheduler::invalidation::Hash16;

use crate::ambient_lib::AmbientLibsByProject;
use crate::changes::{ChangeResult, WorkspaceChange};
use crate::dir_index::DirIndex;
use crate::env_hash::EnvHashInputs;
use crate::exact_resolution::{DependencySnapshotView, EdgeStore};
use crate::memory::MemorySnapshot;
use crate::module_resolution::{ConditionSet, ModuleResolutionMode};
use crate::overlay::OverlayStore;
use crate::package_index::PackageIndex;
use crate::project_graph::ProjectGraph;
use crate::published_state::{ProjectEnvHashArray, PublishedRoot};
use crate::resolver::IdeProjectConfig;
use crate::traits::WorkspaceResourceSnapshot;
use crate::types::{
    ExactResolution, ExactResolutionResult, ResolvePhase, ResolveRequestKind, ResolveResult,
    VfsProvenance,
};
use crate::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};

/// Path-segment marker used by the package classification helpers to
/// detect node_modules-rooted paths.
const NODE_MODULES_SEGMENT: &str = "/node_modules/";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LazyResolutionCacheKey {
    importer_id: String,
    specifier: String,
    phase: ResolvePhase,
    kind: ResolveRequestKind,
}

#[derive(Debug, Clone)]
struct LazyResolutionCacheEntry {
    result: Option<ResolveResult>,
    content_generation: u64,
    snapshot_generation: SnapshotGeneration,
}

/// Shared internal engine used by both `FilesystemWorkspace` and `MemoryWorkspace`.
///
/// All fields are wrapped in `RwLock` for interior mutability so that
/// `WorkspaceAccess` (which takes `&self`) can read and write state.
///
/// # Published state
///
/// The `published_state` field is the primary source of truth for ownership
/// and resolution. It starts as `None` before first publish. After the first
/// call to `publish_snapshot()`, it is always `Some`.
///
/// `set_project_graph()` and `configure_resolver()` both write to
/// `project_graph` and then call `rebuild_and_publish()` which atomically
/// publishes to `published_state`.
///
/// # Lock ordering
///
/// To prevent deadlocks, locks must be acquired in this order:
///
/// 1. `overlay` (read or write)
/// 2. `snapshot` (read or write)
/// 3. `edges` (read or write)
/// 4. `project_graph` (read only — write is rare)
/// 5. `package_index` (read or write)
/// 6. `dir_index` (read or write)
///
/// `published_state` uses lock-free `ArcSwap` — no ordering constraints.
pub(crate) struct Engine {
    pub(crate) overlay: RwLock<OverlayStore>,
    pub(crate) snapshot: RwLock<MemorySnapshot>,
    pub(crate) edges: RwLock<EdgeStore>,
    lazy_resolution_cache: RwLock<FxHashMap<LazyResolutionCacheKey, LazyResolutionCacheEntry>>,
    pub(crate) content_generation: AtomicU64,
    /// Project graph — the write-side store. Callers update this via
    /// `set_project_graph()` / `configure_resolver()`, then
    /// `rebuild_and_publish()` atomically derives and publishes a
    /// `WorkspaceSnapshot` + `ProjectResolver` to `published_state`.
    pub(crate) project_graph: RwLock<ProjectGraph>,
    #[allow(dead_code)]
    pub(crate) package_index: RwLock<PackageIndex>,
    pub(crate) dir_index: RwLock<DirIndex>,
    pub(crate) vfs_provenance: VfsProvenance,

    /// Atomic published workspace state — primary source of truth for
    /// ownership and resolution.
    ///
    /// Always `Some` after `Engine::new()` — the constructor eagerly publishes
    /// an empty bootstrap snapshot (`ownership_ready: false`). After
    /// `background_init` builds the full project graph, a real snapshot with
    /// `ownership_ready: true` is published.
    pub(crate) published_state: ArcSwapOption<PublishedRoot>,

    /// Per-project ambient TypeScript lib registry.
    ///
    /// Lock-free `ArcSwap` so reads (file shadowing checks, symbol lookup,
    /// dep-fact validation) never block on concurrent registrations. Concrete
    /// workspaces mutate via CAS in `register_ambient_lib`.
    pub(crate) ambient_libs: ArcSwap<AmbientLibsByProject>,

    /// Extension list used for reverse-dep stem stripping. Initialised to
    /// the merged static `probe_extensions()` + initial host config, sorted
    /// longest-first. `ArcSwap` so `set_default_resolve_extensions` does
    /// not stall reverse queries on the hot path.
    pub(crate) default_resolve_extensions: ArcSwap<Vec<String>>,
}

impl Engine {
    pub(crate) fn new() -> Self {
        let initial_extensions: Vec<String> = Self::merge_extensions(&[]);
        let engine = Self {
            overlay: RwLock::new(OverlayStore::new()),
            snapshot: RwLock::new(MemorySnapshot::new()),
            edges: RwLock::new(EdgeStore::new()),
            lazy_resolution_cache: RwLock::new(FxHashMap::default()),
            content_generation: AtomicU64::new(1),
            project_graph: RwLock::new(ProjectGraph::new()),
            package_index: RwLock::new(PackageIndex::new()),
            dir_index: RwLock::new(DirIndex::new()),
            vfs_provenance: VfsProvenance::default(),
            published_state: ArcSwapOption::new(None),
            ambient_libs: ArcSwap::from_pointee(AmbientLibsByProject::default()),
            default_resolve_extensions: ArcSwap::from_pointee(initial_extensions),
        };
        // Publish an initial snapshot from the empty project graph so that
        // `published_state` is always `Some`. This ensures basic relative
        // path resolution works immediately, before any `set_project_graph()`
        // or `configure_resolver()` call populates real project configs.
        engine.rebuild_and_publish();
        engine
    }

    /// Merge `host_resolve_extensions` with the workspace's static
    /// `probe_extensions()` list, dedupe, and sort by descending length
    /// then ascending lex. Used by [`Engine::new`] and
    /// [`Engine::set_default_resolve_extensions`] (single source of truth).
    fn merge_extensions(host_resolve_extensions: &[String]) -> Vec<String> {
        let mut merged: BTreeSet<String> = crate::resolver::probe_extensions()
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        for ext in host_resolve_extensions {
            merged.insert(ext.clone());
        }
        let mut sorted: Vec<String> = merged.into_iter().collect();
        sorted.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        sorted
    }

    /// Replace the workspace's reverse-dep extension list (additive: merges
    /// with `probe_extensions()` and sorts longest-first at set-time per F4).
    /// Lock-free swap; does not stall reverse queries.
    pub(crate) fn set_default_resolve_extensions(&self, host_resolve_extensions: Vec<String>) {
        let sorted = Self::merge_extensions(&host_resolve_extensions);
        self.default_resolve_extensions.store(Arc::new(sorted));
    }

    /// Publish a workspace snapshot atomically.
    ///
    /// After this call, all readers loading from `published_state` see the
    /// new snapshot. One store, one generation.
    pub(crate) fn publish_snapshot(&self, root: PublishedRoot) {
        self.published_state.store(Some(Arc::new(root)));
    }

    pub(crate) fn current_content_generation(&self) -> u64 {
        self.content_generation.load(Ordering::Relaxed)
    }

    pub(crate) fn bump_content_generation(&self) -> u64 {
        self.clear_lazy_resolution_cache();
        self.content_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn clear_lazy_resolution_cache(&self) {
        self.lazy_resolution_cache.write().clear();
    }

    /// Load the current published state (lock-free).
    ///
    /// Always returns `Some` after `Engine::new()`. Check
    /// `ownership_ready` to distinguish bootstrap from real snapshots.
    pub(crate) fn load_published(&self) -> Option<Arc<PublishedRoot>> {
        self.published_state.load_full()
    }

    pub(crate) fn resource_snapshot(&self) -> WorkspaceResourceSnapshot {
        let overlay = self.overlay.read();
        let snapshot = self.snapshot.read();
        let edges = self.edges.read();
        let package_index = self.package_index.read();
        let published = self.load_published();

        WorkspaceResourceSnapshot {
            overlay_entries: overlay.len(),
            overlay_bytes: overlay.approx_bytes(),
            snapshot_entries: snapshot.len(),
            snapshot_bytes: snapshot.approx_bytes(),
            edge_file_count: edges.file_count(),
            reverse_dep_bucket_count: edges.reverse_dep_bucket_count(),
            package_manifest_count: package_index.found_count(),
            published_project_count: published
                .as_ref()
                .map(|root| root.snapshot.projects.len())
                .unwrap_or(0),
        }
    }

    /// Build and publish a snapshot from the current project graph.
    ///
    /// Derives a `WorkspaceSnapshot` + `ProjectResolver` from the current
    /// `project_graph` and atomically publishes them to `published_state`.
    /// Called by `set_project_graph()` and `configure_resolver()`.
    ///
    /// **Env-hash composition (project-scoped env-hash API).** Computes per-project
    /// `[parse, resolve, type_, lib]` env-hash arrays and project-identity
    /// hashes ONCE here, before publication, so the published snapshot
    /// carries its env-hash tables atomically. Producer reads from the
    /// project graph's `compiler_options` and the engine-level resolve
    /// extensions; consumers look up tables on the published snapshot.
    pub(crate) fn rebuild_and_publish(&self) {
        let graph = self.project_graph.read();
        let resolver = graph.to_project_resolver();

        // Build a WorkspaceSnapshot from the graph's projects
        let projects: Vec<_> = graph
            .iter()
            .enumerate()
            .map(|(i, config)| {
                crate::snapshot_builder::ownership_project_from_vfs_config(
                    config,
                    crate::workspace_snapshot::ProjectId(i as u32),
                )
            })
            .collect();

        let generation = SnapshotGeneration(graph.generation());

        drop(graph);

        let env_inputs_resolve_extensions = self.default_resolve_extensions.load_full();

        let (env_hashes_by_project, project_identity_hashes) =
            compose_env_hash_tables(&projects, &env_inputs_resolve_extensions);

        let snapshot = WorkspaceSnapshot {
            projects,
            resolver,
            generation,
        };

        self.clear_lazy_resolution_cache();
        self.published_state
            .store(Some(Arc::new(PublishedRoot::with_env_hash_tables(
                Arc::new(snapshot),
                env_hashes_by_project,
                project_identity_hashes,
            ))));
    }

    pub(crate) fn read_package_manifest(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
        canonical_id: &str,
    ) -> Option<crate::types::PackageManifest> {
        use crate::package_index::ManifestEntry;

        let canonical_id = crate::resolver::normalize_canonical_id(canonical_id);
        {
            let cache = self.package_index.read();
            match cache.get_cached(&canonical_id) {
                Some(ManifestEntry::Found(manifest)) => return Some((**manifest).clone()),
                Some(ManifestEntry::NotFound) => return None, // negative cache hit
                None => {}                                    // cache miss — proceed to read
            }
        }

        match reader.read_file(&canonical_id) {
            Some(source) => {
                let mut cache = self.package_index.write();
                Some(cache.get_or_parse(&canonical_id, &source).clone())
            }
            None => {
                // Cache the negative result so repeated probes are free.
                let mut cache = self.package_index.write();
                cache.insert_not_found(&canonical_id);
                None
            }
        }
    }

    pub(crate) fn invalidate_package_manifest(&self, canonical_id: &str) {
        let canonical_id = crate::resolver::normalize_canonical_id(canonical_id);
        if canonical_id.ends_with("/package.json") {
            self.package_index.write().invalidate(&canonical_id);
        }
    }

    fn mark_parent_dir_dirty(&self, canonical_id: &str) {
        if let Some((parent, _)) = canonical_id.rsplit_once('/') {
            self.dir_index.write().mark_dirty(parent);
        }
    }

    /// Apply a batch of workspace changes.
    pub(crate) fn apply_changes(&self, changes: Vec<WorkspaceChange>) -> ChangeResult {
        let mut result = ChangeResult::default();
        let mut content_changed = false;

        for change in changes {
            match change {
                WorkspaceChange::OverlaySet {
                    canonical_id,
                    source,
                } => {
                    self.invalidate_package_manifest(&canonical_id);
                    self.overlay.write().set(canonical_id.clone(), source);
                    result.invalidated_files.push(canonical_id);
                    content_changed = true;
                }
                WorkspaceChange::OverlayClear { canonical_id } => {
                    self.invalidate_package_manifest(&canonical_id);
                    if self.overlay.write().clear(&canonical_id) {
                        result.invalidated_files.push(canonical_id);
                        content_changed = true;
                    }
                }
                WorkspaceChange::FileChanged {
                    canonical_id,
                    source,
                } => {
                    self.invalidate_package_manifest(&canonical_id);
                    self.mark_parent_dir_dirty(&canonical_id);
                    if !self.overlay.read().has_overlay(&canonical_id) {
                        if let Some(content) = source {
                            self.snapshot.write().inject(canonical_id.clone(), content);
                        } else {
                            self.snapshot.write().remove(&canonical_id);
                        }
                        result.invalidated_files.push(canonical_id);
                        content_changed = true;
                    }
                }
                WorkspaceChange::FileDeleted { canonical_id } => {
                    self.invalidate_package_manifest(&canonical_id);
                    self.mark_parent_dir_dirty(&canonical_id);
                    self.edges.write().remove_file(&canonical_id);
                    self.snapshot.write().remove(&canonical_id);
                    result.invalidated_files.push(canonical_id);
                    content_changed = true;
                }
                WorkspaceChange::DirectoryTreeDirty { prefix } => {
                    self.package_index.write().invalidate_under(&prefix);
                    self.dir_index.write().mark_dirty_under(&prefix);
                    self.clear_lazy_resolution_cache();
                }
                WorkspaceChange::ConfigChanged { canonical_id: _ } => {
                    self.clear_lazy_resolution_cache();
                    result.graph_rebuilt = true;
                    result.generation = Some(self.project_graph.read().generation() + 1);
                }
            }
        }

        if content_changed {
            self.bump_content_generation();
        }

        result
    }

    /// Set exact resolutions for a file.
    pub(crate) fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult {
        self.edges
            .write()
            .replace_exact_resolutions(canonical_id, resolutions)
    }

    /// Replace owner's transitive-semantic dep set. Always fires; closes F15.
    pub(crate) fn replace_semantic_transitive(&self, canonical_id: &str, deps: BTreeSet<String>) {
        self.edges
            .write()
            .replace_semantic_transitive(canonical_id, deps);
    }

    /// Inspection — clone of an owner's dependency snapshot.
    #[allow(dead_code)]
    pub(crate) fn dependency_snapshot(&self, canonical_id: &str) -> Option<DependencySnapshotView> {
        self.edges.read().snapshot(canonical_id)
    }

    /// Add a single ambient-resolved dep (incremental). Routes ambient
    /// dependencies into the dedicated `ambient_resolved` class so they
    /// survive `record_parsed_edges` re-records (closes F1.5).
    pub(crate) fn add_ambient_resolved_dep(&self, canonical_id: &str, virtual_id: &str) -> bool {
        self.edges
            .write()
            .add_ambient_resolved_dep(canonical_id, virtual_id)
    }

    /// Resolve an import using exact resolutions then the project resolver chain.
    ///
    /// Resolution priority:
    /// 1. Exact resolutions (authoritative — no fallthrough on match)
    /// 2. Published snapshot resolver (preferred) or legacy resolver (fallback)
    /// 3. `None` (no heuristic fallback)
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn resolve_import(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
        importer_id: &str,
        specifier: &str,
        ctx: crate::types::ResolutionContext,
    ) -> Option<crate::types::ResolveResult> {
        // 1. Check exact resolutions (authoritative, no fallthrough)
        {
            let edges = self.edges.read();
            if let Some(exact) = edges.get_exact_resolution(importer_id, specifier, ctx) {
                return exact.resolved_canonical_id.as_ref().map(|id| {
                    crate::types::ResolveResult {
                        source_id: id.clone(),
                        provider_id: id.clone(),
                        provider_specifier: specifier.to_string(),
                        provider_target: crate::types::ProviderTarget::SourceFile,
                        resolution_kind: crate::types::ResolutionKind::Bundler,
                        owner_tsconfig_path: None,
                    }
                });
            }
        }

        let published = self.published_state.load_full();
        let content_generation = self.current_content_generation();
        let snapshot_generation = published
            .as_ref()
            .map(|root| root.snapshot.generation)
            .unwrap_or_default();
        let cache_key = LazyResolutionCacheKey {
            importer_id: importer_id.to_string(),
            specifier: specifier.to_string(),
            phase: ctx.phase,
            kind: ctx.kind,
        };
        if let Some(entry) = self
            .lazy_resolution_cache
            .read()
            .get(&cache_key)
            .cloned()
            .filter(|entry| {
                entry.content_generation == content_generation
                    && entry.snapshot_generation == snapshot_generation
            })
        {
            self.vfs_provenance
                .import_resolution_cache_hit_count
                .fetch_add(1, Ordering::Relaxed);
            if let Some(ref result) = entry.result {
                self.edges
                    .write()
                    .add_lazy_resolved_dep(importer_id, &result.source_id);
            }
            return entry.result;
        }
        self.vfs_provenance
            .import_resolution_cache_miss_count
            .fetch_add(1, Ordering::Relaxed);

        // 2. Use published snapshot resolver (None before first publish)
        let result = if let Some(root) = published {
            let resolver = &root.snapshot.resolver;
            let request = crate::types::ResolveRequest {
                importer_id: importer_id.to_string(),
                specifier: specifier.to_string(),
                kind: ctx.kind,
                phase: ctx.phase,
            };
            resolver.resolve_with_reader(reader, &request)
        } else {
            None
        };

        // Cache successful resolution in forward/reverse dep graph
        if let Some(ref result) = result {
            self.edges
                .write()
                .add_lazy_resolved_dep(importer_id, &result.source_id);
        }

        self.lazy_resolution_cache.write().insert(
            cache_key,
            LazyResolutionCacheEntry {
                result: result.clone(),
                content_generation,
                snapshot_generation,
            },
        );

        result
    }

    pub(crate) fn resolve_import_for_project(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
        owner: &crate::types::ProjectOwnership,
        specifier: &str,
        ctx: crate::types::ResolutionContext,
    ) -> Option<crate::types::ResolveResult> {
        let root = self.published_state.load_full()?;
        root.snapshot
            .resolver
            .resolve_for_project_with_reader(reader, owner, specifier, ctx)
    }

    /// Get the owning project for a file.
    pub(crate) fn owner_for_file(
        &self,
        canonical_id: &str,
    ) -> Option<crate::types::ProjectOwnership> {
        let root = self.published_state.load_full()?;
        root.snapshot.single_owner_for_file(canonical_id).map(|id| {
            let project = root.snapshot.project(id);
            crate::types::ProjectOwnership {
                project_root: project.root.as_str().to_string(),
                tsconfig_path: root
                    .snapshot
                    .tsconfig_path(id)
                    .map(|p| p.as_str().to_string()),
            }
        })
    }

    /// Whether `canonical_id` (or its realpath) is claimed by any
    /// registered workspace project.
    ///
    /// This consults the published snapshot's `OwnershipProject`
    /// list. A path is workspace-owned when it sits inside some
    /// project's `root` AND the suffix between that root and the
    /// path contains no further `/node_modules/` segment. The
    /// suffix-check preserves two important corner cases:
    ///
    /// - A workspace package whose root happens to live inside
    ///   `node_modules/` (uncommon but legal in pnpm) IS
    ///   workspace-owned for files under that root.
    /// - A third-party `node_modules/` source under an outer
    ///   project's root is NOT workspace-owned, because the
    ///   suffix between the outer root and the file contains
    ///   `/node_modules/`.
    ///
    /// Pnpm-symlink hops are handled by the caller resolving
    /// `realpath` before invoking this method.
    ///
    /// Returns `false` before the workspace publishes its first
    /// snapshot.
    pub(crate) fn is_workspace_owned(&self, canonical_id: &str) -> bool {
        let Some(root) = self.published_state.load_full() else {
            return false;
        };
        let path = crate::canonical_path::CanonicalPath::new(canonical_id);
        let path_str = path.as_str();
        root.snapshot.projects.iter().any(|project| {
            let project_root = project.root.as_str();
            if !path.starts_with_dir(&project.root) {
                return false;
            }
            // Suffix between project.root and path. Empty suffix
            // (path == project.root) is workspace-owned.
            let Some(suffix) = path_str.strip_prefix(project_root) else {
                return false;
            };
            !suffix.contains(NODE_MODULES_SEGMENT)
        })
    }

    /// Whether `canonical_id` (or its realpath) sits inside a
    /// `node_modules/` directory AND no registered project root
    /// claims the path.
    ///
    /// Discriminating cases:
    /// - third-party `node_modules/lodash/...` → `true`
    /// - workspace package linked via `node_modules/` → `false`
    ///   (the project root claims it via [`is_workspace_owned`])
    /// - workspace source outside `node_modules/` → `false`
    /// - path with no `node_modules/` segment → `false`
    pub(crate) fn is_package_backed(&self, canonical_id: &str) -> bool {
        let path = crate::canonical_path::CanonicalPath::new(canonical_id);
        if !path.as_str().contains(NODE_MODULES_SEGMENT) {
            return false;
        }
        !self.is_workspace_owned(canonical_id)
    }

    /// Compute the preferred alias-based import specifier for a target file.
    pub(crate) fn preferred_specifier(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
        importer_id: &str,
        target_id: &str,
    ) -> Option<String> {
        let root = self.published_state.load_full()?;
        root.snapshot
            .resolver
            .preferred_specifier(reader, importer_id, target_id)
    }

    /// Resolve a parsed-edge import (relative specifier or non-relative
    /// `ExternalSrc`) WITHOUT consulting `exact_resolutions` and WITHOUT
    /// writing `lazy_resolved` side effects. Used exclusively by
    /// [`Engine::record_parsed_edges`] (R5: parsed-edge resolver bypasses
    /// exacts — closes Codex 2 #1).
    ///
    /// The R4 lifecycle requires that parsed-edge resolution is parser-
    /// driven, not bundler-driven: bundler-injected exacts dampen unresolved
    /// stems via the active-stem model but do NOT reclassify the relative as
    /// resolved. [`Engine::resolve_import`] reads `exact_resolutions` first;
    /// using it from `record_parsed_edges` would silently promote stale
    /// exact targets into `parsed_resolved` after a re-upsert.
    pub(crate) fn resolve_parsed_edge(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
        importer_id: &str,
        specifier: &str,
        ctx: crate::types::ResolutionContext,
    ) -> Option<crate::types::ResolveResult> {
        // No exact_resolutions read.

        let published = self.published_state.load_full();
        let content_generation = self.current_content_generation();
        let snapshot_generation = published
            .as_ref()
            .map(|root| root.snapshot.generation)
            .unwrap_or_default();
        let cache_key = LazyResolutionCacheKey {
            importer_id: importer_id.to_string(),
            specifier: specifier.to_string(),
            phase: ctx.phase,
            kind: ctx.kind,
        };
        if let Some(entry) = self
            .lazy_resolution_cache
            .read()
            .get(&cache_key)
            .cloned()
            .filter(|entry| {
                entry.content_generation == content_generation
                    && entry.snapshot_generation == snapshot_generation
            })
        {
            // Read-only cache hit. NO add_lazy_resolved_dep call (R5).
            return entry.result;
        }

        let result = if let Some(root) = published {
            let resolver = &root.snapshot.resolver;
            let request = crate::types::ResolveRequest {
                importer_id: importer_id.to_string(),
                specifier: specifier.to_string(),
                kind: ctx.kind,
                phase: ctx.phase,
            };
            resolver.resolve_with_reader(reader, &request)
        } else {
            None
        };

        // Cache the resolution (perf), but NOT as a lazy_resolved dep.
        self.lazy_resolution_cache.write().insert(
            cache_key,
            LazyResolutionCacheEntry {
                content_generation,
                snapshot_generation,
                result: result.clone(),
            },
        );

        result
    }

    /// Record parsed edges, eagerly resolving relative/src edges via the
    /// parsed-edge resolver (R5 bypasses `exact_resolutions`).
    pub(crate) fn record_parsed_edges(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
    ) {
        let mut parsed_resolved: BTreeSet<String> = BTreeSet::new();
        let mut bare_specifiers: Vec<(String, ResolveRequestKind)> = Vec::new();
        let mut unresolved_pairs: Vec<((String, ResolveRequestKind), String)> = Vec::new();

        for edge in edges {
            match edge {
                crate::types::ParsedEdge::Relative { specifier, kind } => {
                    let ctx = crate::types::ResolutionContext {
                        phase: crate::types::ResolvePhase::CodegenBlocker,
                        kind: *kind,
                    };
                    if let Some(result) =
                        self.resolve_parsed_edge(reader, canonical_id, specifier, ctx)
                    {
                        parsed_resolved.insert(result.source_id);
                    } else if specifier.starts_with('.') {
                        let normalized =
                            crate::relative_path::normalize_relative_specifier(specifier);
                        let stem = crate::relative_path::join_relative(canonical_id, &normalized);
                        unresolved_pairs.push(((normalized, *kind), stem));
                    }
                }
                crate::types::ParsedEdge::ExternalSrc {
                    specifier,
                    resolved_path,
                } => {
                    if let Some(path) = resolved_path {
                        parsed_resolved.insert(path.clone());
                    } else {
                        let ctx = crate::types::ResolutionContext {
                            phase: crate::types::ResolvePhase::CodegenBlocker,
                            kind: crate::types::ResolveRequestKind::SfcSrcAttr,
                        };
                        if let Some(result) =
                            self.resolve_parsed_edge(reader, canonical_id, specifier, ctx)
                        {
                            parsed_resolved.insert(result.source_id);
                        }
                    }
                }
                crate::types::ParsedEdge::Bare { specifier, kind } => {
                    bare_specifiers.push((specifier.clone(), *kind));
                }
            }
        }

        // Per R4 lifecycle: replace_parsed_edges CLEARS exact_resolved +
        // exact_resolutions + lazy_resolved + semantic_transitive.
        // ambient_resolved survives. Bundler must re-call
        // set_import_dependencies after every upsert.
        self.edges.write().replace_parsed_edges(
            canonical_id,
            parsed_resolved,
            unresolved_pairs,
            bare_specifiers,
        );
    }

    /// Query reverse deps (files that import this file). Strips the
    /// configured longest-suffix-first extension list and consults BOTH
    /// the canonical and stem reverse axes.
    pub(crate) fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        // Lock-free read of the configured extension list (already sorted
        // longest-first at set-time).
        let exts = self.default_resolve_extensions.load();
        let stripped = crate::relative_path::strip_extension_first(canonical_id, &exts);
        self.edges
            .read()
            .reverse_deps_for_target(canonical_id, stripped)
    }

    /// Query forward deps (files this file imports).
    pub(crate) fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.edges.read().forward_deps(canonical_id)
    }

    /// Enumerate every canonical the workspace currently holds content for:
    /// open/upserted overlay buffers UNIONED with injected/published snapshot
    /// content. These are the program members an ambient `declare module`
    /// declarer can live in (program-completeness for external module
    /// augmentation, where the declarer may be a root `.d.ts` that nothing
    /// imports). Deduplicated; order is unspecified.
    pub(crate) fn known_canonicals(&self) -> Vec<String> {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        seen.extend(self.overlay.read().ids().map(str::to_owned));
        seen.extend(self.snapshot.read().ids().map(str::to_owned));
        seen.into_iter().collect()
    }

    // ── Ambient lib registration ──

    /// Register an ambient lib via the CAS loop (`ambient_lib::cas_register`).
    ///
    /// Resolves `spec.project_id` against the published snapshot to compute a
    /// `ProjectStableKey`. Honors A5 user-wins shadowing by querying
    /// `WorkspaceAccess::file_exists` for non-ambient collisions. Bumps
    /// `content_generation` on actual content change so dep validators
    /// invalidate downstream caches.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn register_ambient_lib(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
        spec: crate::ambient_lib::AmbientLibSpec,
    ) -> Result<(), crate::ambient_lib::AmbientLibError> {
        use crate::ambient_lib::{
            cas_register, compute_ambient_hash16, normalize_canonical_id, AmbientLibError,
        };

        let published = self.load_published().ok_or(AmbientLibError::NotPublished)?;
        if published.snapshot.projects.is_empty() {
            return Err(AmbientLibError::NotPublished);
        }
        let stable_key = match spec.project_id {
            Some(pid) => published
                .snapshot
                .projects
                .iter()
                .find(|p| p.id == pid)
                .map(|p| crate::project_key::ProjectStableKey::from_project(p, &p.workspace_root))
                .ok_or(AmbientLibError::UnknownOrAmbiguousProject)?,
            None if published.snapshot.projects.len() == 1 => {
                let p = &published.snapshot.projects[0];
                crate::project_key::ProjectStableKey::from_project(p, &p.workspace_root)
            }
            None => return Err(AmbientLibError::UnknownOrAmbiguousProject),
        };

        let canonical = normalize_canonical_id(&spec.canonical_id);

        // A5: shadowing check — a real user file at this canonical_id wins.
        if reader.file_exists(canonical.as_ref()) {
            return Err(AmbientLibError::NonAmbientCollision(canonical));
        }

        // A6 eager step: cheap shallow parse for top-level export names.
        let top_level_exports: Arc<[Arc<str>]> = {
            let names = crate::ambient_parse::parse_top_level_exports(
                canonical.as_ref(),
                spec.source.as_ref(),
            )
            .map_err(AmbientLibError::ParseFailure)?;
            names.into_boxed_slice().into()
        };

        let content_hash = compute_ambient_hash16(spec.source.as_bytes());
        let changed = cas_register(
            &self.ambient_libs,
            stable_key,
            canonical,
            Arc::clone(&spec.source),
            content_hash,
            top_level_exports,
        );
        if changed {
            self.bump_content_generation();
        }
        Ok(())
    }

    /// Unregister an ambient lib by `(stable_key, canonical_id)`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn unregister_ambient_lib(
        &self,
        stable_key: crate::project_key::ProjectStableKey,
        canonical_id: &str,
    ) -> Result<(), crate::ambient_lib::AmbientLibError> {
        use crate::ambient_lib::{cas_unregister, normalize_canonical_id};

        let canonical = normalize_canonical_id(canonical_id);
        let removed = cas_unregister(&self.ambient_libs, stable_key, canonical);
        if removed {
            self.bump_content_generation();
        }
        Ok(())
    }

    /// Read an ambient lib's source. A5: returns `None` when a non-ambient
    /// user file exists at the canonical_id (shadowing).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn read_ambient_lib(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
        stable_key: crate::project_key::ProjectStableKey,
        canonical_id: &str,
    ) -> Option<Arc<str>> {
        let canonical = crate::ambient_lib::normalize_canonical_id(canonical_id);
        if reader.file_exists(canonical.as_ref()) {
            return None;
        }
        let ambient = self.ambient_libs.load_full();
        ambient
            .by_project
            .get(&stable_key)?
            .libs
            .get(canonical.as_ref())
            .map(|entry| Arc::clone(&entry.source))
    }

    /// O(1) symbol → `(stable_key, canonical, lib_order)` lookup against the
    /// project's registered ambient libs (A2). Returns the first lib (by
    /// `lib_order`) that exposes the symbol.
    pub(crate) fn lookup_ambient_symbol(
        &self,
        consumer_project: crate::project_key::ProjectStableKey,
        symbol: &str,
    ) -> Option<crate::ambient_lib::AmbientSymbolHit> {
        let ambient = self.ambient_libs.load_full();
        let p = ambient.by_project.get(&consumer_project)?;
        let candidates = p.symbol_index.get(symbol)?;
        let (canonical_id, lib_order) = candidates.first()?.clone();
        let virtual_id = crate::ambient_lib::ambient_virtual_canonical_id(
            consumer_project,
            canonical_id.as_ref(),
        );
        Some(crate::ambient_lib::AmbientSymbolHit {
            project: consumer_project,
            canonical_id,
            virtual_id,
            lib_order,
        })
    }

    /// Resolve a `ProjectId` to its stable key against the published snapshot.
    pub(crate) fn project_stable_key(
        &self,
        project_id: crate::workspace_snapshot::ProjectId,
    ) -> Option<crate::project_key::ProjectStableKey> {
        let published = self.load_published()?;
        published
            .snapshot
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .map(|p| crate::project_key::ProjectStableKey::from_project(p, &p.workspace_root))
    }

    /// Lock-free read of the ambient lib registry — used by validators.
    pub(crate) fn ambient_libs_view(&self) -> Arc<crate::ambient_lib::AmbientLibsByProject> {
        self.ambient_libs.load_full()
    }
}

// Debug implementation that doesn't require Debug on RwLock contents
impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

// ── Env-hash composition for published snapshots (project-scoped env-hash API) ──

/// Stable parser-flag identifiers mixed into every project's
/// `parse_env_hash` regardless of tsconfig. Today the parser feature
/// surface is a single fixed identifier; new flags extend this slice in
/// declaration order without breaking determinism.
const WORKSPACE_PARSER_FLAGS: &[&str] = &["verter-parser-v1"];

/// Workspace-level ambient corpus fingerprint mixed into every project's
/// `lib_env_hash`. Replaced with the real ambient-registry fingerprint
/// once the substrate carries one; for now the constant ensures the
/// composed lib hash is deterministic and distinguishable from
/// "no lib data".
const WORKSPACE_AMBIENT_FINGERPRINT: u64 = 0xC0DE_BABE_0000_0001;

/// Workspace-default `exports`/`imports` condition set mixed into every
/// project's `resolve_env_hash`. This is the resolve-domain default the
/// composer feeds until the published project payload carries the project's
/// own `moduleResolution` / conditions (the full per-project resolution
/// matrix is a U0 `resolver_core` concern — see
/// `docs/arch/native-typeinfo-parity-u2-reducers.md` →
/// `U0.RESOLVER_CORE_FOUNDATIONS`). The default mirrors the TS provider-graph
/// condition order so the composed `resolve_env_hash` is deterministic.
fn workspace_default_export_conditions() -> ConditionSet {
    ConditionSet::new(["types", "import", "default"])
}

/// Compose per-project `[parse, resolve, type_, lib]` env-hash arrays
/// and project-identity hashes from the published `OwnershipProject`s.
///
/// Producer-side composition: reads `compiler_options` directly from each
/// `Configured` project payload; for `Fallback` projects, uses
/// `IdeProjectConfig::new(root, workspace_root, None)` so the resulting
/// `project_identity` distinguishes fallback identities (a fallback
/// project at root `/A` is not the same identity as a configured project
/// at root `/A`).
///
/// The `resolve_extensions` slice flows through `EnvHashInputs` so
/// extension-priority changes invalidate every project's
/// `resolve_env_hash` (single producer site — engine reads
/// `default_resolve_extensions` once before iterating projects).
pub(crate) fn compose_env_hash_tables(
    projects: &[OwnershipProject],
    resolve_extensions: &[String],
) -> (
    FxHashMap<ProjectId, ProjectEnvHashArray>,
    FxHashMap<ProjectId, Hash16>,
) {
    let extensions_refs: Vec<&str> = resolve_extensions.iter().map(String::as_str).collect();
    let export_conditions = workspace_default_export_conditions();
    let inputs = EnvHashInputs {
        parser_flags: WORKSPACE_PARSER_FLAGS,
        resolve_extensions: &extensions_refs,
        type_strict: false,
        type_no_implicit_any: false,
        lib_names: &[],
        type_roots: &[],
        module_resolution_mode: ModuleResolutionMode::default(),
        export_conditions: &export_conditions,
        ambient_corpus_fingerprint: WORKSPACE_AMBIENT_FINGERPRINT,
    };

    let mut env_hashes_by_project: FxHashMap<ProjectId, ProjectEnvHashArray> = FxHashMap::default();
    env_hashes_by_project.reserve(projects.len());
    let mut project_identity_hashes: FxHashMap<ProjectId, Hash16> = FxHashMap::default();
    project_identity_hashes.reserve(projects.len());

    for project in projects {
        let config = ide_project_config_from_ownership(project);
        let arr: ProjectEnvHashArray = [
            config.parse_env_hash(&inputs),
            config.resolve_env_hash(&inputs),
            config.type_env_hash(&inputs),
            config.lib_env_hash(&inputs),
        ];
        env_hashes_by_project.insert(project.id, arr);
        project_identity_hashes.insert(project.id, config.project_identity());
    }

    (env_hashes_by_project, project_identity_hashes)
}

/// Project the `IdeProjectConfig` shape from an `OwnershipProject` for
/// env-hash composition. Configured projects carry their own
/// compiler_options / references / workspace_aliases / membership;
/// fallback projects produce an empty config rooted at their own root /
/// workspace root.
fn ide_project_config_from_ownership(project: &OwnershipProject) -> IdeProjectConfig {
    match &project.payload {
        ProjectPayload::Configured {
            tsconfig_path,
            compiler_options,
            references,
            workspace_aliases,
            membership,
        } => {
            let mut config = IdeProjectConfig::new(
                project.root.as_str().to_string(),
                project.workspace_root.as_str().to_string(),
                Some(tsconfig_path.as_str().to_string()),
            );
            config.compiler_options = compiler_options.clone();
            config.references = references.iter().map(|r| r.as_str().to_string()).collect();
            config.workspace_aliases = workspace_aliases.clone();
            config.membership = crate::snapshot_builder::spec_to_membership(&membership.spec);
            config
        }
        ProjectPayload::Fallback { .. } => IdeProjectConfig::new(
            project.root.as_str().to_string(),
            project.workspace_root.as_str().to_string(),
            None,
        ),
    }
}

/// Workspace-default env-hash array used when a canonical has no owning
/// project. Composed from the engine's resolve-extension list mixed with
/// a stable "no project" identity so the default is non-zero and changes
/// when the workspace-level extension list changes.
pub(crate) fn workspace_default_env_hash_array_for_engine(engine: &Engine) -> ProjectEnvHashArray {
    let extensions = engine.default_resolve_extensions.load_full();
    let extensions_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
    let export_conditions = workspace_default_export_conditions();
    let inputs = EnvHashInputs {
        parser_flags: WORKSPACE_PARSER_FLAGS,
        resolve_extensions: &extensions_refs,
        type_strict: false,
        type_no_implicit_any: false,
        lib_names: &[],
        type_roots: &[],
        module_resolution_mode: ModuleResolutionMode::default(),
        export_conditions: &export_conditions,
        ambient_corpus_fingerprint: WORKSPACE_AMBIENT_FINGERPRINT,
    };
    let config = IdeProjectConfig::new(String::new(), String::new(), None);
    [
        config.parse_env_hash(&inputs),
        config.resolve_env_hash(&inputs),
        config.type_env_hash(&inputs),
        config.lib_env_hash(&inputs),
    ]
}

/// Workspace-default project-identity hash for canonicals with no owning
/// project. See [`workspace_default_env_hash_array_for_engine`] for the
/// rationale on producing a deterministic non-zero default that depends
/// on workspace configuration rather than collapsing to all-zero.
pub(crate) fn workspace_default_project_identity_hash_for_engine(_engine: &Engine) -> Hash16 {
    // Default project identity carries an empty (workspace_root, root,
    // tsconfig) tuple — distinguishes "no owning project" from any
    // published project (which always has a non-empty workspace_root /
    // root) without colliding across workspaces.
    let config = IdeProjectConfig::new(String::new(), String::new(), None);
    config.project_identity()
}
