use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwapOption;
use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use crate::changes::{ChangeResult, WorkspaceChange};
use crate::dir_index::DirIndex;
use crate::exact_resolution::EdgeStore;
use crate::memory::MemorySnapshot;
use crate::overlay::OverlayStore;
use crate::package_index::PackageIndex;
use crate::project_graph::ProjectGraph;
use crate::published_state::PublishedRoot;
use crate::traits::WorkspaceResourceSnapshot;
use crate::types::{
    ExactResolution, ExactResolutionResult, ResolvePhase, ResolveRequestKind, ResolveResult,
    VfsProvenance,
};
use crate::workspace_snapshot::{SnapshotGeneration, WorkspaceSnapshot};

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
}

impl Engine {
    pub(crate) fn new() -> Self {
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
        };
        // Publish an initial snapshot from the empty project graph so that
        // `published_state` is always `Some`. This ensures basic relative
        // path resolution works immediately, before any `set_project_graph()`
        // or `configure_resolver()` call populates real project configs.
        engine.rebuild_and_publish();
        engine
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

        let snapshot = WorkspaceSnapshot {
            projects,
            resolver,
            generation,
        };

        self.clear_lazy_resolution_cache();
        self.published_state
            .store(Some(Arc::new(PublishedRoot::new_vfs_only(Arc::new(
                snapshot,
            )))));
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
            .set_exact_resolutions(canonical_id, resolutions)
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
                    .add_lazily_resolved_dep(importer_id, &result.source_id);
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
                .add_lazily_resolved_dep(importer_id, &result.source_id);
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

    /// Record parsed edges, eagerly resolving relative/src edges via the resolver.
    pub(crate) fn record_parsed_edges(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
    ) {
        let mut eagerly_resolved = Vec::new();
        let mut bare_specifiers = Vec::new();

        for edge in edges {
            match edge {
                crate::types::ParsedEdge::Relative { specifier, kind } => {
                    let ctx = crate::types::ResolutionContext {
                        phase: crate::types::ResolvePhase::CodegenBlocker,
                        kind: *kind,
                    };
                    if let Some(result) = self.resolve_import(reader, canonical_id, specifier, ctx)
                    {
                        eagerly_resolved.push(result.source_id);
                    }
                }
                crate::types::ParsedEdge::ExternalSrc {
                    specifier,
                    resolved_path,
                } => {
                    if let Some(path) = resolved_path {
                        eagerly_resolved.push(path.clone());
                    } else {
                        let ctx = crate::types::ResolutionContext {
                            phase: crate::types::ResolvePhase::CodegenBlocker,
                            kind: crate::types::ResolveRequestKind::SfcSrcAttr,
                        };
                        if let Some(result) =
                            self.resolve_import(reader, canonical_id, specifier, ctx)
                        {
                            eagerly_resolved.push(result.source_id);
                        }
                    }
                }
                crate::types::ParsedEdge::Bare { specifier, kind } => {
                    bare_specifiers.push((specifier.clone(), *kind));
                }
            }
        }

        self.edges
            .write()
            .record_parsed_edges(canonical_id, eagerly_resolved, bare_specifiers);
    }

    /// Query reverse deps (files that import this file).
    pub(crate) fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.edges.read().reverse_deps(canonical_id)
    }

    /// Query forward deps (files this file imports).
    pub(crate) fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.edges.read().forward_deps(canonical_id)
    }
}

// Debug implementation that doesn't require Debug on RwLock contents
impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}
