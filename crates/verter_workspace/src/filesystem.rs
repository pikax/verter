use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::audit_sink::{SinkHandle, VfsAuditLayer, VfsAuditSink, VfsReadEvent};
use crate::changes::{ChangeResult, WorkspaceChange};
use crate::engine::Engine;
use crate::project_graph::{ProjectGraph, VfsProjectConfig};
use crate::types::{ExactResolution, ExactResolutionResult};

// Kept for the per-read detail string surface — callers
// (filesystem_tests, frontier_tests) consume it directly. The broader
// `component_meta_trace_*` span tree is replaced by the `VfsAuditSink`
// registry below.
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

#[cfg(test)]
#[allow(dead_code)]
fn vfs_read_file_missing_result_detail(canonical_id: &str, indexed_negative: bool) -> String {
    if indexed_negative {
        format!(
            "path={} layer=dir_index cache=negative bytes=0",
            canonical_id
        )
    } else {
        format!("path={} layer=missing cache=miss bytes=0", canonical_id)
    }
}

/// Fan out a `VfsReadEvent` to every registered sink. Reads the
/// active `request_id` from the scheduler's TLS so a session-side
/// sink can filter events by request without the workspace having to
/// know about sessions.
fn emit_vfs_read_event(
    sinks: &parking_lot::RwLock<Vec<(SinkHandle, Arc<dyn VfsAuditSink>)>>,
    canonical_id: &str,
    layer: VfsAuditLayer,
    cache_hit: bool,
    bytes_read: u64,
    read_ns: Option<u64>,
) {
    let registered = sinks.read();
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
/// 1. Override (overlay) â€” active editor content
/// 2. Snapshot (cache) â€” previously read content
/// 3. Disk â€” fallback for cache misses (Filesystem mode only)
pub struct FilesystemWorkspace {
    pub(crate) options: FilesystemOptions,
    pub(crate) engine: Engine,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) native_fs: crate::native_fs::NativeFs,
    /// Registered VFS audit sinks. Sessions register one sink per
    /// audited request and receive fan-out for every VFS read.
    pub(crate) sinks: parking_lot::RwLock<Vec<(SinkHandle, Arc<dyn VfsAuditSink>)>>,
    /// Monotonic id generator for sink handles.
    pub(crate) next_sink_id: AtomicU64,
}

impl std::fmt::Debug for FilesystemWorkspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilesystemWorkspace")
            .field("options", &self.options)
            .field("engine", &self.engine)
            .finish_non_exhaustive()
    }
}

impl FilesystemWorkspace {
    pub fn new(options: FilesystemOptions) -> Self {
        Self {
            options,
            engine: Engine::new(),
            #[cfg(not(target_arch = "wasm32"))]
            native_fs: crate::native_fs::NativeFs::new(),
            sinks: parking_lot::RwLock::new(Vec::new()),
            next_sink_id: AtomicU64::new(1),
        }
    }

    /// Access the workspace options.
    pub fn options(&self) -> &FilesystemOptions {
        &self.options
    }

    pub fn vfs_provenance_snapshot(&self) -> crate::types::VfsProvenanceSnapshot {
        self.engine.vfs_provenance.snapshot()
    }

    pub fn reset_vfs_provenance(&self) {
        self.engine.vfs_provenance.reset();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_parent_dir_indexed(&self, canonical_id: &str) -> Option<bool> {
        let (parent, _basename) = split_parent_basename(canonical_id)?;
        let was_dirty = {
            let dir_index = self.engine.dir_index.read();
            match dir_index.lookup(canonical_id) {
                crate::dir_index::DirIndexLookup::Hit(exists) => {
                    self.engine
                        .vfs_provenance
                        .dir_index_hit_count
                        .fetch_add(1, Ordering::Relaxed);
                    return Some(exists);
                }
                crate::dir_index::DirIndexLookup::Dirty => true,
                crate::dir_index::DirIndexLookup::Unindexed => false,
            }
        };

        self.engine
            .vfs_provenance
            .native_fs_read_dir_count
            .fetch_add(1, Ordering::Relaxed);
        match self.native_fs.read_dir(parent) {
            Ok(entries) => {
                let basenames = entries
                    .into_iter()
                    .filter(|entry| !entry.is_dir)
                    .filter_map(|entry| basename_from_path(&entry.path))
                    .collect();
                self.engine.dir_index.write().refresh(parent, basenames);
                self.engine
                    .vfs_provenance
                    .dir_index_refresh_count
                    .fetch_add(1, Ordering::Relaxed);
                if was_dirty {
                    self.engine
                        .vfs_provenance
                        .dir_index_dirty_rescan_count
                        .fetch_add(1, Ordering::Relaxed);
                }
                self.engine.dir_index.read().file_exists(canonical_id)
            }
            Err(crate::error::VfsError::NotFound(_)) => {
                self.engine.dir_index.write().refresh(parent, Vec::new());
                self.engine
                    .vfs_provenance
                    .dir_index_refresh_count
                    .fetch_add(1, Ordering::Relaxed);
                if was_dirty {
                    self.engine
                        .vfs_provenance
                        .dir_index_dirty_rescan_count
                        .fetch_add(1, Ordering::Relaxed);
                }
                Some(false)
            }
            Err(_) => None,
        }
    }

    /// Inject a file directly into the snapshot cache.
    pub fn inject_file(&self, canonical_id: String, source: Arc<str>) {
        self.engine.invalidate_package_manifest(&canonical_id);
        self.engine
            .snapshot
            .write()
            .inject(canonical_id.clone(), source);
        // Per-canonical content transition — same recording chokepoint
        // as every other per-canonical mutator, so artifact-only
        // freshness gates observe the injection.
        self.engine.bump_content_generation_for(&canonical_id);
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

    /// Publish a workspace snapshot with optional consumer extension.
    ///
    /// After this call, all readers see the new snapshot (lock-free).
    pub fn publish_snapshot(&self, root: crate::published_state::PublishedRoot) {
        self.engine.publish_snapshot(root);
    }

    /// Load the current published state (lock-free).
    ///
    /// Always returns `Some` after construction. Check `ownership_ready`
    /// to distinguish bootstrap from real snapshots.
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

// â”€â”€ WorkspaceAccess implementation â”€â”€

// split into `WorkspaceRead` (read-only)
// and `WorkspaceAccess` (mutators) per the trait hierarchy.
impl crate::traits::WorkspaceRead for FilesystemWorkspace {
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
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

        // 1. Overlay
        if let Some(content) = self.engine.overlay.read().get(canonical_id) {
            set_last_read_file_trace_detail(canonical_id, "layer=overlay cache=hit");
            emit_vfs_read_event(
                &self.sinks,
                canonical_id,
                VfsAuditLayer::Overlay,
                true,
                content.len() as u64,
                read_ns(started),
            );
            return Some(content);
        }
        // 2. Snapshot cache
        if let Some(content) = self.engine.snapshot.read().read(canonical_id) {
            set_last_read_file_trace_detail(canonical_id, "layer=snapshot cache=hit");
            emit_vfs_read_event(
                &self.sinks,
                canonical_id,
                VfsAuditLayer::Snapshot,
                true,
                content.len() as u64,
                read_ns(started),
            );
            return Some(content);
        }
        // 3. Disk fallback — read and cache in snapshot
        #[cfg(not(target_arch = "wasm32"))]
        {
            let indexed_exists = self.ensure_parent_dir_indexed(canonical_id);
            if matches!(indexed_exists, Some(false)) {
                set_last_read_file_trace_detail(canonical_id, "layer=dir_index cache=negative");
                emit_vfs_read_event(
                    &self.sinks,
                    canonical_id,
                    VfsAuditLayer::DirIndexNegative,
                    false,
                    0,
                    read_ns(started),
                );
                return None;
            }
            if let Some(content) = self.native_fs.read_file(canonical_id) {
                self.engine
                    .snapshot
                    .write()
                    .inject(canonical_id.to_string(), content.clone());
                set_last_read_file_trace_detail(canonical_id, "layer=disk cache=miss");
                emit_vfs_read_event(
                    &self.sinks,
                    canonical_id,
                    VfsAuditLayer::Disk,
                    false,
                    content.len() as u64,
                    read_ns(started),
                );
                return Some(content);
            }
            self.engine
                .vfs_provenance
                .native_fs_read_file_miss_count
                .fetch_add(1, Ordering::Relaxed);
        }
        set_last_read_file_trace_detail(canonical_id, "layer=missing cache=miss");
        emit_vfs_read_event(
            &self.sinks,
            canonical_id,
            VfsAuditLayer::Missing,
            false,
            0,
            read_ns(started),
        );
        None
    }

    fn take_last_read_file_trace_detail(&self, canonical_id: &str) -> Option<String> {
        take_last_read_file_trace_detail(canonical_id)
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
            if let Some(exists) = self.ensure_parent_dir_indexed(canonical_id) {
                return exists;
            }
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

    fn read_package_manifest(&self, canonical_id: &str) -> Option<crate::types::PackageManifest> {
        self.engine.read_package_manifest(self, canonical_id)
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
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

    fn owner_for_file(&self, canonical_id: &str) -> Option<crate::types::ProjectOwnership> {
        self.engine.owner_for_file(canonical_id)
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

    fn vfs_provenance_snapshot(&self) -> crate::types::VfsProvenanceSnapshot {
        FilesystemWorkspace::vfs_provenance_snapshot(self)
    }

    fn resource_snapshot(&self) -> crate::traits::WorkspaceResourceSnapshot {
        self.engine.resource_snapshot()
    }

    fn preferred_specifier(&self, importer_id: &str, target_id: &str) -> Option<String> {
        self.engine
            .preferred_specifier(self, importer_id, target_id)
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.engine.reverse_deps_for(canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.engine.forward_deps_for(canonical_id)
    }

    fn known_canonicals(&self) -> Vec<String> {
        // Overlay (open buffers) + snapshot (injected/loaded) content. Disk
        // files that have never been read are NOT enumerated — they are not yet
        // program members and an ambient declarer among them is reached via the
        // normal config-driven load, not this membership probe.
        self.engine.known_canonicals()
    }

    fn dependency_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<crate::exact_resolution::DependencySnapshotView> {
        self.engine.dependency_snapshot(canonical_id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_dir(&self, dir: &str) -> Result<Vec<crate::error::DirEntry>, crate::error::VfsError> {
        self.native_fs.read_dir(dir)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn walk(
        &self,
        root: &str,
        filter_dir: &dyn Fn(&str) -> bool,
        filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, crate::error::VfsError> {
        self.native_fs.walk(root, filter_dir, filter_file)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn is_dir(&self, path: &str) -> bool {
        self.native_fs.is_dir(path)
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

impl crate::traits::WorkspaceAccess for FilesystemWorkspace {
    fn reset_vfs_provenance(&self) {
        FilesystemWorkspace::reset_vfs_provenance(self);
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
        // Invalidate snapshot so next read falls through to disk,
        // picking up any saves made while the overlay was active.
        self.engine.snapshot.write().remove(canonical_id);
        self.engine.bump_content_generation_for(canonical_id);
    }

    fn notify_delete(&self, canonical_id: &str) {
        self.engine.invalidate_package_manifest(canonical_id);
        if let Some((parent, _)) = split_parent_basename(canonical_id) {
            self.engine.dir_index.write().mark_dirty(parent);
        }
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

    // ── Directory and mutation operations (delegate to NativeFs) ──

    #[cfg(not(target_arch = "wasm32"))]
    fn write_file(&self, path: &str, content: &str) -> Result<(), crate::error::VfsError> {
        self.native_fs.write_file(path, content)?;
        self.engine.invalidate_package_manifest(path);
        mark_parent_dir_dirty(&self.engine, path);
        self.engine.edges.write().remove_file(path);
        // Inject into snapshot so subsequent reads see the new content
        self.engine
            .snapshot
            .write()
            .inject(path.to_string(), Arc::from(content));
        self.engine.bump_content_generation_for(path);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn create_dir_all(&self, path: &str) -> Result<(), crate::error::VfsError> {
        self.native_fs.create_dir_all(path)?;
        {
            let mut dir_index = self.engine.dir_index.write();
            dir_index.mark_dirty(path);
        }
        mark_parent_dir_dirty(&self.engine, path);
        self.engine.bump_content_generation();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn delete_file(&self, path: &str) -> Result<(), crate::error::VfsError> {
        self.native_fs.delete_file(path)?;
        self.engine.invalidate_package_manifest(path);
        mark_parent_dir_dirty(&self.engine, path);
        self.engine.snapshot.write().remove(path);
        self.engine.edges.write().remove_file(path);
        self.engine.bump_content_generation_for(path);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn delete_dir_all(&self, path: &str) -> Result<(), crate::error::VfsError> {
        self.native_fs.delete_dir_all(path)?;
        self.engine.package_index.write().invalidate_under(path);
        {
            let mut dir_index = self.engine.dir_index.write();
            dir_index.mark_dirty_under(path);
        }
        mark_parent_dir_dirty(&self.engine, path);
        self.engine.snapshot.write().remove_under(path);
        self.engine.edges.write().remove_under(path);
        // A recursive disk delete transitions EVERY canonical under
        // `path` — including ones the snapshot cache never saw (the
        // filesystem engine reads through to disk). Record the SUBTREE
        // so a delete→recreate of any member never serves a retained
        // pre-delete artifact as fresh.
        self.engine.record_subtree_content_transition(path);
        self.engine.bump_content_generation();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), crate::error::VfsError> {
        self.native_fs.copy_file(src, dst)?;
        self.engine.invalidate_package_manifest(dst);
        mark_parent_dir_dirty(&self.engine, dst);
        self.engine.snapshot.write().remove(dst);
        self.engine.edges.write().remove_file(dst);
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
        // re-records.
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

fn split_parent_basename(canonical_id: &str) -> Option<(&str, &str)> {
    let (parent, basename) = canonical_id.rsplit_once('/')?;
    if parent.is_empty() || basename.is_empty() {
        return None;
    }
    Some((parent, basename))
}

#[cfg(not(target_arch = "wasm32"))]
fn mark_parent_dir_dirty(engine: &crate::engine::Engine, canonical_id: &str) {
    if let Some((parent, _)) = split_parent_basename(canonical_id) {
        engine.dir_index.write().mark_dirty(parent);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn basename_from_path(path: &str) -> Option<String> {
    path.rsplit('/')
        .next()
        .filter(|basename| !basename.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "filesystem_tests.rs"]
mod tests;
