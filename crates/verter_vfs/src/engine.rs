use parking_lot::RwLock;

use crate::changes::{ChangeResult, WorkspaceChange};
use crate::exact_resolution::EdgeStore;
use crate::memory::MemorySnapshot;
use crate::overlay::OverlayStore;
use crate::package_index::PackageIndex;
use crate::project_graph::ProjectGraph;
use crate::resolver::ProjectResolver;
use crate::types::{ExactResolution, ExactResolutionResult};

/// Shared internal engine used by both `FilesystemWorkspace` and `MemoryWorkspace`.
///
/// All fields are wrapped in `RwLock` for interior mutability so that
/// `WorkspaceAccess` (which takes `&self`) can read and write state.
///
/// # Lock ordering
///
/// To prevent deadlocks, locks must be acquired in this order:
///
/// 1. `overlay` (read or write)
/// 2. `snapshot` (read or write)
/// 3. `edges` (read or write)
/// 4. `project_graph` (read only — write is rare, done in `set_project_graph`)
/// 5. `resolver` (read or write)
/// 6. `package_index` (read or write)
///
/// **Never** acquire a higher-numbered lock while holding a lower-numbered lock's
/// write guard. Read guards are fine to hold across acquisitions (parking_lot
/// RwLock is reentrant for reads). The `rebuild_resolver()` method explicitly
/// drops the `project_graph` read guard before acquiring the `resolver` write
/// guard (see the `drop(graph)` call).
pub(crate) struct Engine {
    pub(crate) overlay: RwLock<OverlayStore>,
    pub(crate) snapshot: RwLock<MemorySnapshot>,
    pub(crate) edges: RwLock<EdgeStore>,
    pub(crate) project_graph: RwLock<ProjectGraph>,
    #[allow(dead_code)] // Reserved for future node_modules resolution
    pub(crate) package_index: RwLock<PackageIndex>,
    pub(crate) resolver: RwLock<Option<ProjectResolver>>,
}

impl Engine {
    pub(crate) fn new() -> Self {
        Self {
            overlay: RwLock::new(OverlayStore::new()),
            snapshot: RwLock::new(MemorySnapshot::new()),
            edges: RwLock::new(EdgeStore::new()),
            project_graph: RwLock::new(ProjectGraph::new()),
            package_index: RwLock::new(PackageIndex::new()),
            resolver: RwLock::new(None),
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
                    self.overlay.write().set(canonical_id.clone(), source);
                    result.invalidated_files.push(canonical_id);
                }
                WorkspaceChange::OverlayClear { canonical_id } => {
                    if self.overlay.write().clear(&canonical_id) {
                        result.invalidated_files.push(canonical_id);
                    }
                }
                WorkspaceChange::FileChanged {
                    canonical_id,
                    source,
                } => {
                    // Skip if overlay is active (overlay takes precedence)
                    if !self.overlay.read().has_overlay(&canonical_id) {
                        // If source was provided, inject into snapshot
                        if let Some(content) = source {
                            self.snapshot.write().inject(canonical_id.clone(), content);
                        } else {
                            // No source means "re-read from disk" — invalidate snapshot
                            self.snapshot.write().remove(&canonical_id);
                        }
                        result.invalidated_files.push(canonical_id);
                    }
                }
                WorkspaceChange::FileDeleted { canonical_id } => {
                    self.edges.write().remove_file(&canonical_id);
                    self.snapshot.write().remove(&canonical_id);
                    result.invalidated_files.push(canonical_id);
                }
                WorkspaceChange::ConfigChanged { canonical_id: _ } => {
                    // Full project graph rebuild — Phase 2+ will implement config parsing
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

    /// Rebuild the project resolver from the current project graph.
    pub(crate) fn rebuild_resolver(&self) {
        let graph = self.project_graph.read();
        let resolver = graph.to_project_resolver();
        drop(graph);
        *self.resolver.write() = Some(resolver);
    }

    /// Resolve an import using exact resolutions then the project resolver chain.
    ///
    /// Resolution priority:
    /// 1. Exact resolutions (authoritative — no fallthrough on match)
    /// 2. Project resolver (tsconfig paths, workspace aliases, etc.)
    /// 3. `None` (no heuristic fallback)
    ///
    /// When the project resolver succeeds for a bare import, the result is
    /// cached in `lazily_resolved_deps` so the forward/reverse dep graph
    /// includes it.
    pub(crate) fn resolve_import(
        &self,
        reader: &dyn crate::resolver::ProjectResolverReader,
        importer_id: &str,
        specifier: &str,
        kind: crate::types::ResolveRequestKind,
    ) -> Option<crate::types::ResolveResult> {
        // 1. Check exact resolutions (authoritative, no fallthrough)
        {
            let edges = self.edges.read();
            if let Some(exact) = edges.get_exact_resolution(importer_id, specifier) {
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

        // 2. Project resolver
        let result = {
            let resolver_guard = self.resolver.read();
            if let Some(resolver) = resolver_guard.as_ref() {
                let request = crate::types::ResolveRequest {
                    importer_id: importer_id.to_string(),
                    specifier: specifier.to_string(),
                    kind,
                    phase: crate::types::ResolvePhase::CodegenBlocker,
                };
                resolver.resolve_with_reader(reader, &request)
            } else {
                None
            }
        };

        // Cache successful resolution in the forward/reverse dep graph
        // so that bare imports appear in dependency tracking.
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
        self.project_graph.read().owner_for_file(canonical_id)
    }

    /// Compute the preferred alias-based import specifier for a target file.
    pub(crate) fn preferred_specifier(
        &self,
        reader: &dyn crate::resolver::ProjectResolverReader,
        importer_id: &str,
        target_id: &str,
    ) -> Option<String> {
        let resolver_guard = self.resolver.read();
        let resolver = resolver_guard.as_ref()?;
        resolver.preferred_specifier(reader, importer_id, target_id)
    }

    /// Record parsed edges, eagerly resolving relative/src edges via the resolver.
    ///
    /// `reader` is the workspace implementing `ProjectResolverReader`, used by
    /// `resolve_import` internally.
    ///
    /// For `ExternalSrc` edges: if a pre-resolved path is provided, it is used
    /// directly. Otherwise, the specifier is resolved through the project
    /// resolver (supporting alias-based `<script src>` / `<template src>`).
    pub(crate) fn record_parsed_edges(
        &self,
        reader: &dyn crate::resolver::ProjectResolverReader,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
    ) {
        let mut eagerly_resolved = Vec::new();
        let mut bare_specifiers = Vec::new();

        for edge in edges {
            match edge {
                crate::types::ParsedEdge::Relative { specifier, kind } => {
                    if let Some(result) =
                        self.resolve_import(reader, canonical_id, specifier, *kind)
                    {
                        eagerly_resolved.push(result.source_id);
                    }
                }
                crate::types::ParsedEdge::ExternalSrc {
                    specifier,
                    resolved_path,
                } => {
                    if let Some(path) = resolved_path {
                        // Pre-resolved path provided — use directly
                        eagerly_resolved.push(path.clone());
                    } else {
                        // No pre-resolved path — resolve through project resolver
                        // (supports alias-based src attributes like <script src="@/setup.ts">)
                        if let Some(result) = self.resolve_import(
                            reader,
                            canonical_id,
                            specifier,
                            crate::types::ResolveRequestKind::SfcSrcAttr,
                        ) {
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
