use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::changes::{ChangeResult, WorkspaceChange};
use crate::engine::Engine;
use crate::project_graph::{ProjectGraph, VfsProjectConfig};
use crate::types::{ExactResolution, ExactResolutionResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComponentMetaTraceEvent {
    Start,
    End,
    Point,
}

impl ComponentMetaTraceEvent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
            Self::Point => "point",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ComponentMetaTraceContext {
    trace_id: u64,
    span_id: u64,
}

struct ComponentMetaTraceLine<'a> {
    trace_id: u64,
    span_id: u64,
    parent_span_id: Option<u64>,
    depth: usize,
    name: &'a str,
    detail: &'a str,
}

thread_local! {
    static COMPONENT_META_TRACE_STACK: RefCell<Vec<ComponentMetaTraceContext>> = const { RefCell::new(Vec::new()) };
}

fn component_meta_trace_output_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn component_meta_trace_next_span_id() -> u64 {
    static NEXT_SPAN_ID: AtomicU64 = AtomicU64::new(1 << 48);
    NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed)
}

fn component_meta_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();

    *ENABLED.get_or_init(|| {
        std::env::var_os("VERTER_COMPONENT_META_TRACE").is_some()
            || std::env::var_os("VERTER_META_TRACE").is_some()
    })
}

fn component_meta_trace_output_path() -> Option<&'static std::path::PathBuf> {
    static PATH: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();

    PATH.get_or_init(|| {
        std::env::var_os("VERTER_COMPONENT_META_TRACE_PATH")
            .or_else(|| std::env::var_os("VERTER_META_TRACE_PATH"))
            .map(std::path::PathBuf::from)
    })
    .as_ref()
}

fn format_component_meta_trace_line(
    event: ComponentMetaTraceEvent,
    line: ComponentMetaTraceLine<'_>,
    duration: Option<Duration>,
) -> String {
    let parent = line
        .parent_span_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "-".to_string());
    let mut line = format!(
        "[verter-meta-trace] event={} trace={} span={} parent={} request={} subrequest={} caller={} depth={} thread={:?} name={:?} detail={:?}",
        event.as_str(),
        line.trace_id,
        line.span_id,
        parent,
        line.trace_id,
        line.span_id,
        parent,
        line.depth,
        std::thread::current().id(),
        line.name,
        line.detail,
    );
    if let Some(duration) = duration {
        line.push_str(&format!(" dur_ms={:.3}", duration.as_secs_f64() * 1000.0));
    }
    line
}

fn component_meta_trace_write_line(line: &str) {
    use std::io::Write;

    let _lock = component_meta_trace_output_lock().lock();
    if let Some(path) = component_meta_trace_output_path() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
            return;
        }
    }

    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
    let _ = stderr.flush();
}

struct ComponentMetaTraceGuardState {
    trace_id: u64,
    span_id: u64,
    parent_span_id: Option<u64>,
    depth: usize,
    name: &'static str,
    detail: String,
    started: Instant,
}

struct ComponentMetaTraceGuard {
    state: Option<ComponentMetaTraceGuardState>,
}

impl ComponentMetaTraceGuard {
    fn noop() -> Self {
        Self { state: None }
    }
}

impl Drop for ComponentMetaTraceGuard {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };

        COMPONENT_META_TRACE_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let popped = stack.pop();
            debug_assert_eq!(popped.map(|ctx| ctx.span_id), Some(state.span_id));
        });

        component_meta_trace_write_line(&format_component_meta_trace_line(
            ComponentMetaTraceEvent::End,
            ComponentMetaTraceLine {
                trace_id: state.trace_id,
                span_id: state.span_id,
                parent_span_id: state.parent_span_id,
                depth: state.depth,
                name: state.name,
                detail: &state.detail,
            },
            Some(state.started.elapsed()),
        ));
    }
}

fn component_meta_trace_scope(
    name: &'static str,
    detail: impl Into<String>,
) -> ComponentMetaTraceGuard {
    if !component_meta_trace_enabled() {
        return ComponentMetaTraceGuard { state: None };
    }

    let detail = detail.into();
    let span_id = component_meta_trace_next_span_id();
    let (trace_id, parent_span_id, depth) = COMPONENT_META_TRACE_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let parent = stack.last().copied();
        let trace_id = parent.map(|ctx| ctx.trace_id).unwrap_or(span_id);
        let depth = stack.len();
        stack.push(ComponentMetaTraceContext { trace_id, span_id });
        (trace_id, parent.map(|ctx| ctx.span_id), depth)
    });

    component_meta_trace_write_line(&format_component_meta_trace_line(
        ComponentMetaTraceEvent::Start,
        ComponentMetaTraceLine {
            trace_id,
            span_id,
            parent_span_id,
            depth,
            name,
            detail: &detail,
        },
        None,
    ));

    ComponentMetaTraceGuard {
        state: Some(ComponentMetaTraceGuardState {
            trace_id,
            span_id,
            parent_span_id,
            depth,
            name,
            detail,
            started: Instant::now(),
        }),
    }
}

fn component_meta_trace_event(name: &'static str, detail: impl Into<String>) {
    if !component_meta_trace_enabled() {
        return;
    }

    let detail = detail.into();
    let span_id = component_meta_trace_next_span_id();
    let (trace_id, parent_span_id, depth) = COMPONENT_META_TRACE_STACK.with(|stack| {
        let stack = stack.borrow();
        let parent = stack.last().copied();
        let trace_id = parent.map(|ctx| ctx.trace_id).unwrap_or(span_id);
        (trace_id, parent.map(|ctx| ctx.span_id), stack.len())
    });

    component_meta_trace_write_line(&format_component_meta_trace_line(
        ComponentMetaTraceEvent::Point,
        ComponentMetaTraceLine {
            trace_id,
            span_id,
            parent_span_id,
            depth,
            name,
            detail: &detail,
        },
        None,
    ));
}

macro_rules! component_meta_trace_scope {
    ($name:expr, $detail:expr $(,)?) => {{
        if component_meta_trace_enabled() {
            crate::filesystem::component_meta_trace_scope($name, $detail)
        } else {
            ComponentMetaTraceGuard::noop()
        }
    }};
}

macro_rules! component_meta_trace_event {
    ($name:expr, $detail:expr $(,)?) => {{
        if component_meta_trace_enabled() {
            crate::filesystem::component_meta_trace_event($name, $detail);
        }
    }};
}

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
        self.engine.snapshot.write().inject(canonical_id, source);
        self.engine.bump_content_generation();
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

impl crate::traits::WorkspaceAccess for FilesystemWorkspace {
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        let _trace = component_meta_trace_scope!("vfs_read_file", format!("path={canonical_id}"));
        // 1. Overlay
        if let Some(content) = self.engine.overlay.read().get(canonical_id) {
            component_meta_trace_event!(
                "vfs_read_file_result",
                format!(
                    "path={} layer=overlay cache=hit bytes={}",
                    canonical_id,
                    content.len(),
                ),
            );
            return Some(content);
        }
        // 2. Snapshot cache
        if let Some(content) = self.engine.snapshot.read().read(canonical_id) {
            component_meta_trace_event!(
                "vfs_read_file_result",
                format!(
                    "path={} layer=snapshot cache=hit bytes={}",
                    canonical_id,
                    content.len(),
                ),
            );
            return Some(content);
        }
        // 3. Disk fallback â€” read and cache in snapshot
        #[cfg(not(target_arch = "wasm32"))]
        {
            let indexed_exists = self.ensure_parent_dir_indexed(canonical_id);
            let _disk_trace =
                component_meta_trace_scope!("vfs_read_file_disk", format!("path={canonical_id}"));
            if matches!(indexed_exists, Some(false)) {
                component_meta_trace_event!(
                    "vfs_read_file_disk_result",
                    format!("path={} found=false bytes=0", canonical_id),
                );
                component_meta_trace_event!(
                    "vfs_read_file_result",
                    vfs_read_file_missing_result_detail(canonical_id, true),
                );
                return None;
            }
            if let Some(content) = self.native_fs.read_file(canonical_id) {
                self.engine
                    .snapshot
                    .write()
                    .inject(canonical_id.to_string(), content.clone());
                component_meta_trace_event!(
                    "vfs_read_file_disk_result",
                    format!("path={} found=true bytes={}", canonical_id, content.len(),),
                );
                component_meta_trace_event!(
                    "vfs_read_file_result",
                    format!(
                        "path={} layer=disk cache=miss bytes={}",
                        canonical_id,
                        content.len(),
                    ),
                );
                return Some(content);
            }
            component_meta_trace_event!(
                "vfs_read_file_disk_result",
                format!("path={} found=false bytes=0", canonical_id),
            );
            self.engine
                .vfs_provenance
                .native_fs_read_file_miss_count
                .fetch_add(1, Ordering::Relaxed);
        }
        component_meta_trace_event!(
            "vfs_read_file_result",
            vfs_read_file_missing_result_detail(canonical_id, false),
        );
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

    fn classify_file(&self, canonical_id: &str) -> crate::types::FileKind {
        if canonical_id.ends_with(".vue") {
            crate::types::FileKind::VueSfc
        } else {
            crate::types::FileKind::NonSfc
        }
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

    fn content_generation(&self) -> u64 {
        self.engine.current_content_generation()
    }

    fn vfs_provenance_snapshot(&self) -> crate::types::VfsProvenanceSnapshot {
        FilesystemWorkspace::vfs_provenance_snapshot(self)
    }

    fn reset_vfs_provenance(&self) {
        FilesystemWorkspace::reset_vfs_provenance(self);
    }

    fn resource_snapshot(&self) -> crate::traits::WorkspaceResourceSnapshot {
        self.engine.resource_snapshot()
    }

    fn preferred_specifier(&self, importer_id: &str, target_id: &str) -> Option<String> {
        self.engine
            .preferred_specifier(self, importer_id, target_id)
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

    fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        self.engine.invalidate_package_manifest(canonical_id);
        self.engine
            .overlay
            .write()
            .set(canonical_id.to_string(), source);
        self.engine.bump_content_generation();
    }

    fn notify_close(&self, canonical_id: &str) {
        self.engine.invalidate_package_manifest(canonical_id);
        self.engine.overlay.write().clear(canonical_id);
        // Invalidate snapshot so next read falls through to disk,
        // picking up any saves made while the overlay was active.
        self.engine.snapshot.write().remove(canonical_id);
        self.engine.bump_content_generation();
    }

    fn notify_delete(&self, canonical_id: &str) {
        self.engine.invalidate_package_manifest(canonical_id);
        if let Some((parent, _)) = split_parent_basename(canonical_id) {
            self.engine.dir_index.write().mark_dirty(parent);
        }
        self.engine.overlay.write().clear(canonical_id);
        self.engine.snapshot.write().remove(canonical_id);
        self.engine.edges.write().remove_file(canonical_id);
        self.engine.bump_content_generation();
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

    // â”€â”€ Directory and mutation operations (delegate to NativeFs) â”€â”€

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
        self.engine.bump_content_generation();
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
        self.engine.bump_content_generation();
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
        self.engine.bump_content_generation();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn is_dir(&self, path: &str) -> bool {
        self.native_fs.is_dir(path)
    }
}

fn split_parent_basename(canonical_id: &str) -> Option<(&str, &str)> {
    let (parent, basename) = canonical_id.rsplit_once('/')?;
    if parent.is_empty() || basename.is_empty() {
        return None;
    }
    Some((parent, basename))
}

fn mark_parent_dir_dirty(engine: &crate::engine::Engine, canonical_id: &str) {
    if let Some((parent, _)) = split_parent_basename(canonical_id) {
        engine.dir_index.write().mark_dirty(parent);
    }
}

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
