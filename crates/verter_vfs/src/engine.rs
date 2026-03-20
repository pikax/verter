use std::sync::Arc;

use arc_swap::ArcSwapOption;
use parking_lot::RwLock;

use crate::changes::{ChangeResult, WorkspaceChange};
use crate::exact_resolution::EdgeStore;
use crate::memory::MemorySnapshot;
use crate::overlay::OverlayStore;
use crate::package_index::PackageIndex;
use crate::project_graph::ProjectGraph;
use crate::published_state::PublishedRoot;
use crate::types::{ExactResolution, ExactResolutionResult};
use crate::workspace_snapshot::{SnapshotGeneration, WorkspaceSnapshot};

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
///
/// `published_state` uses lock-free `ArcSwap` — no ordering constraints.
pub(crate) struct Engine {
    pub(crate) overlay: RwLock<OverlayStore>,
    pub(crate) snapshot: RwLock<MemorySnapshot>,
    pub(crate) edges: RwLock<EdgeStore>,
    /// Project graph — the write-side store. Callers update this via
    /// `set_project_graph()` / `configure_resolver()`, then
    /// `rebuild_and_publish()` atomically derives and publishes a
    /// `WorkspaceSnapshot` + `ProjectResolver` to `published_state`.
    pub(crate) project_graph: RwLock<ProjectGraph>,
    #[allow(dead_code)]
    pub(crate) package_index: RwLock<PackageIndex>,

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
            project_graph: RwLock::new(ProjectGraph::new()),
            package_index: RwLock::new(PackageIndex::new()),
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

    /// Load the current published state (lock-free).
    ///
    /// Always returns `Some` after `Engine::new()`. Check
    /// `ownership_ready` to distinguish bootstrap from real snapshots.
    pub(crate) fn load_published(&self) -> Option<Arc<PublishedRoot>> {
        self.published_state.load_full()
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
        let canonical_id = crate::resolver::normalize_canonical_id(canonical_id);
        {
            let cache = self.package_index.read();
            if let Some(manifest) = cache.get_cached(&canonical_id) {
                return Some(manifest.clone());
            }
        }

        let source = reader.read_file(&canonical_id)?;
        let mut cache = self.package_index.write();
        Some(cache.get_or_parse(&canonical_id, &source).clone())
    }

    pub(crate) fn invalidate_package_manifest(&self, canonical_id: &str) {
        let canonical_id = crate::resolver::normalize_canonical_id(canonical_id);
        if canonical_id.ends_with("/package.json") {
            self.package_index.write().invalidate(&canonical_id);
        }
    }

    /// Apply a batch of workspace changes.
    pub(crate) fn apply_changes(&self, changes: Vec<WorkspaceChange>) -> ChangeResult {
        let mut result = ChangeResult::default();

        for change in changes {
            match change {
                WorkspaceChange::OverlaySet {
                    canonical_id,
                    source,
                } => {
                    self.invalidate_package_manifest(&canonical_id);
                    self.overlay.write().set(canonical_id.clone(), source);
                    result.invalidated_files.push(canonical_id);
                }
                WorkspaceChange::OverlayClear { canonical_id } => {
                    self.invalidate_package_manifest(&canonical_id);
                    if self.overlay.write().clear(&canonical_id) {
                        result.invalidated_files.push(canonical_id);
                    }
                }
                WorkspaceChange::FileChanged {
                    canonical_id,
                    source,
                } => {
                    self.invalidate_package_manifest(&canonical_id);
                    if !self.overlay.read().has_overlay(&canonical_id) {
                        if let Some(content) = source {
                            self.snapshot.write().inject(canonical_id.clone(), content);
                        } else {
                            self.snapshot.write().remove(&canonical_id);
                        }
                        result.invalidated_files.push(canonical_id);
                    }
                }
                WorkspaceChange::FileDeleted { canonical_id } => {
                    self.invalidate_package_manifest(&canonical_id);
                    self.edges.write().remove_file(&canonical_id);
                    self.snapshot.write().remove(&canonical_id);
                    result.invalidated_files.push(canonical_id);
                }
                WorkspaceChange::ConfigChanged { canonical_id: _ } => {
                    result.graph_rebuilt = true;
                    result.generation = Some(self.project_graph.read().generation() + 1);
                }
            }
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

        // 2. Use published snapshot resolver (None before first publish)
        let result = if let Some(root) = self.published_state.load_full() {
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

        result
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
