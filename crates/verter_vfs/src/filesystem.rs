use std::sync::Arc;

use crate::changes::{ChangeResult, WorkspaceChange};
use crate::engine::Engine;
use crate::project_graph::{ProjectGraph, VfsProjectConfig};
use crate::types::{ExactResolution, ExactResolutionResult};

/// Options for creating a `FilesystemWorkspace`.
#[derive(Debug, Clone, Default)]
pub struct FilesystemOptions {
    /// Workspace root directories.
    pub roots: Vec<String>,
    /// Whether to eagerly preload files at startup.
    pub eager_preload: bool,
}

/// Filesystem-backed workspace with overlay support.
///
/// File reads follow the three-layer priority:
/// 1. Override (overlay) — active editor content
/// 2. Snapshot (cache) — previously read content
/// 3. Disk — fallback for cache misses (Filesystem mode only)
#[derive(Debug)]
pub struct FilesystemWorkspace {
    pub(crate) options: FilesystemOptions,
    pub(crate) engine: Engine,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) native_fs: crate::native_fs::NativeFs,
}

impl FilesystemWorkspace {
    pub fn new(options: FilesystemOptions) -> Self {
        Self {
            options,
            engine: Engine::new(),
            #[cfg(not(target_arch = "wasm32"))]
            native_fs: crate::native_fs::NativeFs::new(),
        }
    }

    /// Access the workspace options.
    pub fn options(&self) -> &FilesystemOptions {
        &self.options
    }

    /// Inject a file directly into the snapshot cache.
    pub fn inject_file(&self, canonical_id: String, source: Arc<str>) {
        self.engine.snapshot.write().inject(canonical_id, source);
    }

    /// Apply a batch of workspace changes.
    pub fn apply_changes(&self, changes: Vec<WorkspaceChange>) -> ChangeResult {
        self.engine.apply_changes(changes)
    }

    /// Set exact resolutions for a file.
    pub fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult {
        self.engine.set_exact_resolutions(canonical_id, resolutions)
    }

    /// Set the project graph and rebuild the resolver.
    pub fn set_project_graph(&self, graph: ProjectGraph) {
        *self.engine.project_graph.write() = graph;
        self.engine.rebuild_resolver();
    }

    /// Add an explicit project to the graph and rebuild the resolver.
    pub fn add_explicit_project(&self, config: VfsProjectConfig) {
        let mut graph = self.engine.project_graph.write();
        let mut projects: Vec<VfsProjectConfig> = graph.iter().cloned().collect();
        projects.push(config);
        *graph = ProjectGraph::from_configs(projects);
        drop(graph);
        self.engine.rebuild_resolver();
    }
}

// ── WorkspaceAccess implementation ──

impl crate::traits::WorkspaceAccess for FilesystemWorkspace {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        // 1. Overlay
        if let Some(content) = self.engine.overlay.read().get(canonical_id) {
            return Some(content);
        }
        // 2. Snapshot cache
        if let Some(content) = self.engine.snapshot.read().read(canonical_id) {
            return Some(content);
        }
        // 3. Disk fallback — read and cache in snapshot
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(content) = self.native_fs.read_file(canonical_id) {
                self.engine
                    .snapshot
                    .write()
                    .inject(canonical_id.to_string(), content.clone());
                return Some(content);
            }
        }
        None
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        if self.engine.overlay.read().has_overlay(canonical_id) {
            return true;
        }
        if self.engine.snapshot.read().contains(canonical_id) {
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.native_fs.file_exists(canonical_id) {
                return true;
            }
        }
        false
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        // If in overlay or snapshot, return as-is
        if self.engine.overlay.read().has_overlay(canonical_id)
            || self.engine.snapshot.read().contains(canonical_id)
        {
            return Some(canonical_id.to_string());
        }
        // Disk fallback
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.native_fs.realpath(canonical_id)
        }
        #[cfg(target_arch = "wasm32")]
        None
    }

    fn classify_file(&self, canonical_id: &str) -> crate::types::FileKind {
        if canonical_id.ends_with(".vue") {
            crate::types::FileKind::VueSfc
        } else {
            crate::types::FileKind::NonSfc
        }
    }

    fn resolve_import(
        &self,
        importer_id: &str,
        specifier: &str,
        kind: crate::types::ResolveRequestKind,
    ) -> Option<crate::types::ResolveResult> {
        self.engine
            .resolve_import(self, importer_id, specifier, kind)
    }

    fn owner_for_file(&self, canonical_id: &str) -> Option<crate::types::ProjectOwnership> {
        self.engine.owner_for_file(canonical_id)
    }

    fn record_parsed_edges(&self, canonical_id: &str, edges: &[crate::types::ParsedEdge]) {
        self.engine.record_parsed_edges(self, canonical_id, edges);
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.engine.reverse_deps_for(canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.engine.forward_deps_for(canonical_id)
    }
}

// ── ProjectResolverReader implementation ──

impl crate::resolver::ProjectResolverReader for FilesystemWorkspace {
    fn read_text(&self, canonical_id: &str) -> Option<Arc<str>> {
        crate::traits::WorkspaceAccess::read_file(self, canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        crate::traits::WorkspaceAccess::file_exists(self, canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        crate::traits::WorkspaceAccess::realpath(self, canonical_id)
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "filesystem_tests.rs"]
mod tests;
