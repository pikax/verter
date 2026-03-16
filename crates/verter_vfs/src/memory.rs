use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::changes::{ChangeResult, WorkspaceChange};
use crate::engine::Engine;
use crate::project_graph::{ProjectGraph, VfsProjectConfig};
use crate::types::{ExactResolution, ExactResolutionResult};

/// In-memory file content cache used by MemoryWorkspace and as the
/// snapshot layer in FilesystemWorkspace.
///
/// Files are stored as `Arc<str>` for cheap cloning.
#[derive(Debug, Default)]
pub struct MemorySnapshot {
    entries: FxHashMap<String, Arc<str>>,
}

impl MemorySnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read file content from the snapshot.
    pub fn read(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.entries.get(canonical_id).cloned()
    }

    /// Inject or update file content. Returns `true` if the content actually changed.
    pub fn inject(&mut self, canonical_id: String, source: Arc<str>) -> bool {
        match self.entries.get(&canonical_id) {
            Some(existing) if Arc::ptr_eq(existing, &source) || **existing == *source => false,
            _ => {
                self.entries.insert(canonical_id, source);
                true
            }
        }
    }

    /// Remove a file from the snapshot. Returns `true` if the file existed.
    pub fn remove(&mut self, canonical_id: &str) -> bool {
        self.entries.remove(canonical_id).is_some()
    }

    /// Check if a file exists in the snapshot.
    pub fn contains(&self, canonical_id: &str) -> bool {
        self.entries.contains_key(canonical_id)
    }

    /// Number of files in the snapshot.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the snapshot is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over all canonical IDs in the snapshot.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(|s| s.as_str())
    }
}

/// Options for creating a `MemoryWorkspace`.
#[derive(Debug, Clone, Default)]
pub struct MemoryOptions {
    /// Logical workspace roots (for project graph construction).
    pub roots: Vec<String>,
}

/// Memory-only workspace. All files must be injected — no disk fallback.
/// Used by playground, WASM, and tests.
#[derive(Debug)]
pub struct MemoryWorkspace {
    pub(crate) engine: Engine,
}

impl MemoryWorkspace {
    pub fn new(_options: MemoryOptions) -> Self {
        Self {
            engine: Engine::new(),
        }
    }

    /// Inject a file into the snapshot.
    pub fn inject_file(&self, canonical_id: String, source: Arc<str>) {
        self.engine.snapshot.write().inject(canonical_id, source);
    }

    /// Remove a file from the snapshot.
    pub fn remove_file(&self, canonical_id: &str) {
        self.engine.snapshot.write().remove(canonical_id);
        self.engine.edges.write().remove_file(canonical_id);
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

impl crate::traits::WorkspaceAccess for MemoryWorkspace {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        // 1. Check overlay
        if let Some(content) = self.engine.overlay.read().get(canonical_id) {
            return Some(content);
        }
        // 2. Check snapshot (no disk fallback in memory mode)
        self.engine.snapshot.read().read(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.engine.overlay.read().has_overlay(canonical_id)
            || self.engine.snapshot.read().contains(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        if self.file_exists(canonical_id) {
            Some(canonical_id.to_string())
        } else {
            None
        }
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

    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<crate::types::ExactResolution>,
    ) -> crate::types::ExactResolutionResult {
        self.engine.set_exact_resolutions(canonical_id, resolutions)
    }

    fn configure_resolver(&self, projects: Vec<crate::resolver::IdeProjectConfig>) {
        let vfs_configs: Vec<crate::project_graph::VfsProjectConfig> = projects
            .into_iter()
            .map(|p| crate::project_graph::VfsProjectConfig {
                root: p.root.clone(),
                rank: crate::project_graph::ProjectRank::Explicit,
                tsconfig_path: p.tsconfig_path.clone(),
                root_files: vec![],
                extensions: vec![".vue".to_string(), ".ts".to_string(), ".tsx".to_string()],
                workspace_root: p.workspace_root.clone(),
                workspace_aliases: p.workspace_aliases.clone(),
                compiler_options: p.compiler_options.clone(),
                references: p.references.clone(),
                membership: p.membership.clone(),
            })
            .collect();
        let graph = crate::project_graph::ProjectGraph::from_configs(vfs_configs);
        *self.engine.project_graph.write() = graph;
        self.engine.rebuild_resolver();
    }
}

// ── ProjectResolverReader implementation ──

impl crate::resolver::ProjectResolverReader for MemoryWorkspace {
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
#[path = "memory_tests.rs"]
mod tests;
