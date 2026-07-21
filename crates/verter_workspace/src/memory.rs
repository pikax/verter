use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::audit_sink::{SinkHandle, VfsAuditLayer, VfsAuditSink, VfsReadEvent};
use crate::changes::{ChangeResult, WorkspaceChange};
use crate::engine::Engine;
use crate::path_matches_prefix;
use crate::project_graph::{ProjectGraph, VfsProjectConfig};
use crate::types::{ExactResolution, ExactResolutionResult};

thread_local! {
    static LAST_READ_FILE_TRACE_DETAIL: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

fn set_last_read_file_trace_detail(canonical_id: &str, detail: impl Into<String>) {
    LAST_READ_FILE_TRACE_DETAIL.with(|last| {
        *last.borrow_mut() = Some((canonical_id.to_string(), detail.into()));
    });
}

fn take_last_read_file_trace_detail(canonical_id: &str) -> Option<String> {
    LAST_READ_FILE_TRACE_DETAIL.with(|last| {
        let mut last = last.borrow_mut();
        match last.as_ref() {
            Some((seen_canonical, _)) if seen_canonical == canonical_id => {
                last.take().map(|(_, detail)| detail)
            }
            _ => None,
        }
    })
}

fn mark_parent_dir_dirty(engine: &Engine, canonical_id: &str) {
    if let Some((parent, _)) = canonical_id.rsplit_once('/') {
        engine.dir_index.write().mark_dirty(parent);
    }
}

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

    /// Remove all snapshot entries under a directory prefix. Returns the removed IDs.
    pub fn remove_under(&mut self, prefix: &str) -> Vec<String> {
        let removed: Vec<String> = self
            .entries
            .keys()
            .filter(|path| path_matches_prefix(path, prefix))
            .cloned()
            .collect();
        for canonical_id in &removed {
            self.entries.remove(canonical_id);
        }
        removed
    }

    /// Check if a file exists in the snapshot.
    pub fn contains(&self, canonical_id: &str) -> bool {
        self.entries.contains_key(canonical_id)
    }

    /// Number of files in the snapshot.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Approximate bytes retained by snapshot content and canonical IDs.
    pub fn approx_bytes(&self) -> u64 {
        self.entries
            .iter()
            .map(|(canonical_id, source)| canonical_id.len() as u64 + source.len() as u64)
            .sum()
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
    /// Initial workspace-default extension list. Merged with
    /// `probe_extensions()` at engine construction; further merged on
    /// `set_default_resolve_extensions`. `None` means engine defaults
    /// (probe_extensions only).
    pub default_resolve_extensions: Option<Vec<String>>,
}

/// Memory-only workspace. All files must be injected — no disk fallback.
/// Used by playground, WASM, and tests.
pub struct MemoryWorkspace {
    pub(crate) engine: Engine,
    /// Registered VFS audit sinks.
    pub(crate) sinks: parking_lot::RwLock<Vec<(SinkHandle, Arc<dyn VfsAuditSink>)>>,
    pub(crate) next_sink_id: AtomicU64,
}

impl std::fmt::Debug for MemoryWorkspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryWorkspace")
            .field("engine", &self.engine)
            .finish_non_exhaustive()
    }
}

impl MemoryWorkspace {
    pub fn new(options: MemoryOptions) -> Self {
        let engine = Engine::new();
        if let Some(ref exts) = options.default_resolve_extensions {
            engine.set_default_resolve_extensions(exts.clone());
        }
        Self {
            engine,
            sinks: parking_lot::RwLock::new(Vec::new()),
            next_sink_id: AtomicU64::new(1),
        }
    }

    /// Fan out a `VfsReadEvent` to every registered sink.
    fn emit_vfs_read(
        &self,
        canonical_id: &str,
        layer: VfsAuditLayer,
        cache_hit: bool,
        bytes_read: u64,
        read_ns: Option<u64>,
    ) {
        let registered = self.sinks.read();
        if registered.is_empty() {
            return;
        }
        let event = VfsReadEvent {
            canonical_id: Arc::from(canonical_id),
            layer,
            cache_hit,
            bytes_read,
            read_ns,
            request_id: verter_scheduler::request_context::current_request_id(),
            thread_id: std::thread::current().id(),
        };
        for (_, sink) in registered.iter() {
            sink.on_vfs_read(&event);
        }
    }

    /// Inject a file into the snapshot.
    pub fn inject_file(&self, canonical_id: String, source: Arc<str>) {
        self.engine.invalidate_package_manifest(&canonical_id);
        self.engine
            .snapshot
            .write()
            .inject(canonical_id.clone(), source);
        self.engine.bump_content_generation_for(&canonical_id);
    }

    /// Remove a file from the snapshot.
    pub fn remove_file(&self, canonical_id: &str) {
        self.engine.invalidate_package_manifest(canonical_id);
        self.engine.snapshot.write().remove(canonical_id);
        self.engine.edges.write().remove_file(canonical_id);
        self.engine.bump_content_generation_for(canonical_id);
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
        self.engine.rebuild_and_publish();
    }

    /// Load the current published state (lock-free).
    ///
    /// Always returns `Some` after construction — `Engine::new()`
    /// publishes an empty bootstrap snapshot eagerly. Mirrors
    /// `FilesystemWorkspace::load_published` so per-workspace consumers
    /// (tests, env-hash readers) need not branch on workspace flavour.
    pub fn load_published(&self) -> Option<std::sync::Arc<crate::published_state::PublishedRoot>> {
        self.engine.load_published()
    }

    /// Add an explicit project to the graph and rebuild the resolver.
    pub fn add_explicit_project(&self, config: VfsProjectConfig) {
        let mut graph = self.engine.project_graph.write();
        let mut projects: Vec<VfsProjectConfig> = graph.iter().cloned().collect();
        projects.push(config);
        *graph = ProjectGraph::from_configs(projects);
        drop(graph);
        self.engine.rebuild_and_publish();
    }
}

// ── WorkspaceRead / WorkspaceAccess implementations ──
// read-only methods live on the
// `WorkspaceRead` impl; mutators stay on `WorkspaceAccess`.

impl crate::traits::WorkspaceRead for MemoryWorkspace {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        // Per-file `read_ns` capture is gated on the active request's
        // `audit_timing_capture` flag — when `false`, the zero-cost
        // path skips the `Instant::now()` calls entirely.
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        let started = if timing_on {
            Some(std::time::Instant::now())
        } else {
            None
        };
        let read_ns = |started: Option<std::time::Instant>| -> Option<u64> {
            started.map(|t| t.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64)
        };

        // 1. Check overlay
        if let Some(content) = self.engine.overlay.read().get(canonical_id) {
            set_last_read_file_trace_detail(canonical_id, "layer=overlay cache=hit");
            self.emit_vfs_read(
                canonical_id,
                VfsAuditLayer::Overlay,
                true,
                content.len() as u64,
                read_ns(started),
            );
            return Some(content);
        }
        // 2. Check snapshot (no disk fallback in memory mode)
        let content = self.engine.snapshot.read().read(canonical_id);
        match content.as_ref() {
            Some(content) => {
                set_last_read_file_trace_detail(canonical_id, "layer=snapshot cache=hit");
                self.emit_vfs_read(
                    canonical_id,
                    VfsAuditLayer::Snapshot,
                    true,
                    content.len() as u64,
                    read_ns(started),
                );
            }
            None => {
                set_last_read_file_trace_detail(canonical_id, "layer=missing cache=miss");
                self.emit_vfs_read(
                    canonical_id,
                    VfsAuditLayer::Missing,
                    false,
                    0,
                    read_ns(started),
                );
            }
        }
        content
    }

    fn take_last_read_file_trace_detail(&self, canonical_id: &str) -> Option<String> {
        take_last_read_file_trace_detail(canonical_id)
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

    fn read_package_manifest(&self, canonical_id: &str) -> Option<crate::types::PackageManifest> {
        self.engine.read_package_manifest(self, canonical_id)
    }

    fn resolve_import(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: crate::types::ResolutionContext,
    ) -> Option<crate::types::ResolveResult> {
        self.engine
            .resolve_import(self, importer_id, specifier, ctx)
    }

    fn resolve_import_for_project(
        &self,
        owner: &crate::types::ProjectOwnership,
        specifier: &str,
        ctx: crate::types::ResolutionContext,
    ) -> Option<crate::types::ResolveResult> {
        self.engine
            .resolve_import_for_project(self, owner, specifier, ctx)
    }

    fn is_workspace_owned(&self, canonical_id: &str) -> bool {
        let resolved = self.realpath(canonical_id);
        let target = resolved.as_deref().unwrap_or(canonical_id);
        self.engine.is_workspace_owned(target)
    }

    fn is_package_backed(&self, canonical_id: &str) -> bool {
        let resolved = self.realpath(canonical_id);
        let target = resolved.as_deref().unwrap_or(canonical_id);
        self.engine.is_package_backed(target)
    }

    fn content_generation(&self) -> u64 {
        self.engine.current_content_generation()
    }

    fn last_content_transition_generation(&self, canonical_id: &str) -> u64 {
        self.engine.last_content_transition_generation(canonical_id)
    }

    fn record_content_transition(&self, canonical_id: &str) {
        self.engine.record_content_transition(canonical_id);
    }

    fn vfs_provenance_snapshot(&self) -> crate::types::VfsProvenanceSnapshot {
        self.engine.vfs_provenance.snapshot()
    }

    fn resource_snapshot(&self) -> crate::traits::WorkspaceResourceSnapshot {
        self.engine.resource_snapshot()
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.engine.reverse_deps_for(canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.engine.forward_deps_for(canonical_id)
    }

    fn known_canonicals(&self) -> Vec<String> {
        self.engine.known_canonicals()
    }

    fn dependency_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<crate::exact_resolution::DependencySnapshotView> {
        self.engine.dependency_snapshot(canonical_id)
    }

    fn read_dir(&self, dir: &str) -> Result<Vec<crate::error::DirEntry>, crate::error::VfsError> {
        let snapshot = self.engine.snapshot.read();
        let dir_prefix = if dir.ends_with('/') {
            dir.to_string()
        } else {
            format!("{dir}/")
        };

        let mut entries = std::collections::BTreeSet::new();
        for id in snapshot.ids() {
            if let Some(rest) = id.strip_prefix(&dir_prefix) {
                // Direct child: no more slashes, or first segment is a directory
                if let Some(slash_pos) = rest.find('/') {
                    let child_dir = format!("{}{}", dir_prefix, &rest[..slash_pos]);
                    entries.insert(crate::error::DirEntry {
                        path: child_dir,
                        is_dir: true,
                    });
                } else {
                    entries.insert(crate::error::DirEntry {
                        path: id.to_string(),
                        is_dir: false,
                    });
                }
            }
        }

        if entries.is_empty() {
            Err(crate::error::VfsError::NotFound(dir.to_string()))
        } else {
            Ok(entries.into_iter().collect())
        }
    }

    fn walk(
        &self,
        root: &str,
        filter_dir: &dyn Fn(&str) -> bool,
        filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, crate::error::VfsError> {
        let snapshot = self.engine.snapshot.read();
        let root_prefix = if root.ends_with('/') {
            root.to_string()
        } else {
            format!("{root}/")
        };

        let mut result = Vec::new();
        for id in snapshot.ids() {
            if !id.starts_with(&root_prefix) {
                continue;
            }
            // Check all parent directories pass the filter
            let rest = &id[root_prefix.len()..];
            let mut skip = false;
            let mut dir_path = root.to_string();
            for segment in rest.split('/') {
                if rest.ends_with(segment) && !rest.contains('/') {
                    // This is the filename, not a directory
                    break;
                }
                dir_path = format!("{dir_path}/{segment}");
                if !filter_dir(&dir_path) {
                    skip = true;
                    break;
                }
            }
            if skip {
                continue;
            }
            if filter_file(id) {
                result.push(id.to_string());
            }
        }
        Ok(result)
    }

    fn is_dir(&self, path: &str) -> bool {
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        let guard = self.engine.snapshot.read();
        let ids: Vec<&str> = guard.ids().collect();
        ids.iter().any(|id| id.starts_with(&prefix))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_ambient_lib(
        &self,
        stable_key: crate::project_key::ProjectStableKey,
        canonical_id: &str,
    ) -> Option<Arc<str>> {
        self.engine.read_ambient_lib(self, stable_key, canonical_id)
    }

    fn project_stable_key(
        &self,
        project_id: crate::workspace_snapshot::ProjectId,
    ) -> Option<crate::project_key::ProjectStableKey> {
        self.engine.project_stable_key(project_id)
    }

    fn lookup_ambient_symbol(
        &self,
        consumer_project: crate::project_key::ProjectStableKey,
        symbol: &str,
    ) -> Option<crate::ambient_lib::AmbientSymbolHit> {
        self.engine.lookup_ambient_symbol(consumer_project, symbol)
    }

    fn ambient_libs_view(&self) -> Arc<crate::ambient_lib::AmbientLibsByProject> {
        self.engine.ambient_libs_view()
    }

    fn published_root(&self) -> Option<Arc<crate::published_state::PublishedRoot>> {
        self.engine.load_published()
    }
}

impl crate::traits::WorkspaceAccess for MemoryWorkspace {
    fn reset_vfs_provenance(&self) {
        self.engine.vfs_provenance.reset();
    }

    fn record_parsed_edges(&self, canonical_id: &str, edges: &[crate::types::ParsedEdge]) {
        self.engine.record_parsed_edges(self, canonical_id, edges);
    }

    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<crate::types::ExactResolution>,
    ) -> crate::types::ExactResolutionResult {
        self.engine.set_exact_resolutions(canonical_id, resolutions)
    }

    fn record_parsed_edges_with_exact_resolutions(
        &self,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
        resolutions: Vec<crate::types::ExactResolution>,
    ) -> crate::types::ExactResolutionResult {
        self.engine.record_parsed_edges_with_exact_resolutions(
            self,
            canonical_id,
            edges,
            resolutions,
        )
    }

    fn replace_semantic_transitive(
        &self,
        canonical_id: &str,
        deps: std::collections::BTreeSet<String>,
    ) {
        self.engine.replace_semantic_transitive(canonical_id, deps);
    }

    fn set_default_resolve_extensions(&self, host_extensions: Vec<String>) {
        self.engine.set_default_resolve_extensions(host_extensions);
    }

    fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        // R22 contract: byte-identical re-upsert is a TRUE no-op. The
        // overlay `set` returns whether content actually changed; only
        // when it did do we invalidate the package manifest and bump
        // the content generation. Bumping when nothing changed would
        // needlessly clear the lazy-resolution cache and force
        // downstream observers to re-validate.
        let changed = self
            .engine
            .overlay
            .write()
            .set(canonical_id.to_string(), source);
        if changed {
            self.engine.invalidate_package_manifest(canonical_id);
            self.engine.bump_content_generation_for(canonical_id);
        }
    }

    fn notify_close(&self, canonical_id: &str) {
        self.engine.invalidate_package_manifest(canonical_id);
        self.engine.overlay.write().clear(canonical_id);
        self.engine.bump_content_generation_for(canonical_id);
    }

    fn notify_delete(&self, canonical_id: &str) {
        self.engine.invalidate_package_manifest(canonical_id);
        self.engine.overlay.write().clear(canonical_id);
        self.engine.snapshot.write().remove(canonical_id);
        self.engine.edges.write().remove_file(canonical_id);
        self.engine.bump_content_generation_for(canonical_id);
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
        self.engine.rebuild_and_publish();
    }

    // ── Directory mutations (in-memory) ──

    fn write_file(&self, path: &str, content: &str) -> Result<(), crate::error::VfsError> {
        self.engine.invalidate_package_manifest(path);
        mark_parent_dir_dirty(&self.engine, path);
        self.engine.edges.write().remove_file(path);
        self.engine
            .snapshot
            .write()
            .inject(path.to_string(), Arc::from(content));
        self.engine.bump_content_generation_for(path);
        Ok(())
    }

    fn create_dir_all(&self, _path: &str) -> Result<(), crate::error::VfsError> {
        // Directories are implicit in MemoryWorkspace
        Ok(())
    }

    fn delete_file(&self, path: &str) -> Result<(), crate::error::VfsError> {
        self.engine.invalidate_package_manifest(path);
        mark_parent_dir_dirty(&self.engine, path);
        self.engine.snapshot.write().remove(path);
        self.engine.edges.write().remove_file(path);
        self.engine.bump_content_generation_for(path);
        Ok(())
    }

    fn delete_dir_all(&self, path: &str) -> Result<(), crate::error::VfsError> {
        // Boundary-correct, trailing-slash-normalized subtree match
        // (also covers the exact entry): `delete_dir_all("/a/")` and
        // `delete_dir_all("/a")` remove the same id set, and a
        // byte-prefix sibling (`/ab.ts` under `/a`) never matches.
        let mut snapshot = self.engine.snapshot.write();
        let ids_to_remove: Vec<String> = snapshot
            .ids()
            .filter(|id| path_matches_prefix(id, path))
            .map(|id| id.to_string())
            .collect();
        for id in &ids_to_remove {
            self.engine.invalidate_package_manifest(id);
            snapshot.remove(id);
        }
        drop(snapshot);
        let mut edges = self.engine.edges.write();
        for id in &ids_to_remove {
            edges.remove_file(id);
        }
        drop(edges);
        self.engine.dir_index.write().mark_dirty_under(path);
        mark_parent_dir_dirty(&self.engine, path);
        for id in &ids_to_remove {
            self.engine.record_content_transition(id);
        }
        self.engine.bump_content_generation();
        Ok(())
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), crate::error::VfsError> {
        let content = self
            .engine
            .snapshot
            .read()
            .read(src)
            .ok_or_else(|| crate::error::VfsError::NotFound(src.to_string()))?;
        self.engine.invalidate_package_manifest(dst);
        mark_parent_dir_dirty(&self.engine, dst);
        self.engine.edges.write().remove_file(dst);
        self.engine
            .snapshot
            .write()
            .inject(dst.to_string(), content);
        self.engine.bump_content_generation_for(dst);
        Ok(())
    }

    fn register_audit_sink(
        &self,
        sink: Arc<dyn crate::audit_sink::VfsAuditSink>,
    ) -> Result<SinkHandle, crate::audit_sink::AuditSinkError> {
        let handle = SinkHandle(self.next_sink_id.fetch_add(1, Ordering::Relaxed));
        self.sinks.write().push((handle, sink));
        Ok(handle)
    }

    fn deregister_audit_sink(
        &self,
        handle: SinkHandle,
    ) -> Result<(), crate::audit_sink::AuditSinkError> {
        let mut sinks = self.sinks.write();
        let len_before = sinks.len();
        sinks.retain(|(h, _)| *h != handle);
        if sinks.len() < len_before {
            Ok(())
        } else {
            Err(crate::audit_sink::AuditSinkError::HandleNotFound)
        }
    }

    // ── Ambient lib registry ──

    #[cfg(not(target_arch = "wasm32"))]
    fn register_ambient_lib(
        &self,
        spec: crate::ambient_lib::AmbientLibSpec,
    ) -> Result<(), crate::ambient_lib::AmbientLibError> {
        self.engine.register_ambient_lib(self, spec)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn unregister_ambient_lib(
        &self,
        stable_key: crate::project_key::ProjectStableKey,
        canonical_id: &str,
    ) -> Result<(), crate::ambient_lib::AmbientLibError> {
        self.engine.unregister_ambient_lib(stable_key, canonical_id)
    }

    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str) {
        // F1.5 fix: route ambient deps through the dedicated
        // `ambient_resolved` class so they survive `record_parsed_edges`
        // re-records. Previously this routed to `lazy_resolved` which is
        // cleared on every parse re-record.
        self.engine.add_ambient_resolved_dep(consumer, virtual_id);
    }

    // ── Project-scoped env-hash API ──

    fn env_hash_array_for_project(
        &self,
        project_id: crate::workspace_snapshot::ProjectId,
    ) -> Option<crate::published_state::ProjectEnvHashArray> {
        let root = self.engine.load_published()?;
        root.env_hashes_by_project.get(&project_id).copied()
    }

    fn project_identity_hash_for_project(
        &self,
        project_id: crate::workspace_snapshot::ProjectId,
    ) -> Option<verter_scheduler::invalidation::Hash16> {
        let root = self.engine.load_published()?;
        root.project_identity_hashes.get(&project_id).copied()
    }

    fn workspace_default_env_hash_array(&self) -> crate::published_state::ProjectEnvHashArray {
        crate::engine::workspace_default_env_hash_array_for_engine(&self.engine)
    }

    fn workspace_default_project_identity_hash(&self) -> verter_scheduler::invalidation::Hash16 {
        crate::engine::workspace_default_project_identity_hash_for_engine(&self.engine)
    }
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
