use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    static RESOLUTION_DIRECTORY_OBSERVATIONS: RefCell<Vec<ResolutionDirectoryObservation>> = const { RefCell::new(Vec::new()) };
}

/// A recorded directory-membership observation.
///
/// Only the `Unstable` arm exists on `wasm32`: that target has no directory
/// enumeration at all (`FilesystemWorkspace::read_dir` and the `NativeFs`
/// boundary behind it are native-only), so no enumerable outcome can ever be
/// observed there and the frozen reader always reports the observation set as
/// incomplete.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolutionDirectoryValue {
    #[cfg(not(target_arch = "wasm32"))]
    Entries(Vec<crate::error::DirEntry>),
    #[cfg(not(target_arch = "wasm32"))]
    NotFound,
    /// The path exists but is not a directory (e.g. an index-file probe
    /// under a carrier FILE an alias maps onto). A deterministic, stable
    /// observation — the typed path-probe seam classifies the same errno as
    /// `Absent` — never unstable I/O.
    #[cfg(not(target_arch = "wasm32"))]
    NotADirectory,
    Unstable,
}

/// Whether a directory enumeration failed because the path is a FILE — the
/// stable "not enumerable" outcome, distinct from genuinely unstable I/O.
#[cfg(not(target_arch = "wasm32"))]
fn vfs_error_is_not_a_directory(error: &crate::error::VfsError) -> bool {
    matches!(
        error,
        crate::error::VfsError::Io(io) if io.kind() == std::io::ErrorKind::NotADirectory
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolutionDirectoryObservation {
    canonical: String,
    value: ResolutionDirectoryValue,
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

#[cfg(not(target_arch = "wasm32"))]
fn record_resolution_directory_observation(canonical_id: &str, value: ResolutionDirectoryValue) {
    RESOLUTION_DIRECTORY_OBSERVATIONS.with(|observations| {
        observations
            .borrow_mut()
            .push(ResolutionDirectoryObservation {
                canonical: verter_semantic::resolver_core::normalize_canonical_id(canonical_id),
                value,
            });
    });
}

fn take_resolution_directory_evidence() -> Vec<ResolutionDirectoryObservation> {
    RESOLUTION_DIRECTORY_OBSERVATIONS
        .with(|observations| std::mem::take(&mut *observations.borrow_mut()))
}

fn take_resolution_directory_observations() -> Vec<String> {
    take_resolution_directory_evidence()
        .into_iter()
        .map(|observation| observation.canonical)
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn record_resolution_directory_result(
    canonical_id: &str,
    result: &Result<Vec<crate::error::DirEntry>, crate::error::VfsError>,
) {
    let value = match result {
        Ok(entries) => {
            let mut entries = entries.clone();
            entries.sort();
            ResolutionDirectoryValue::Entries(entries)
        }
        Err(crate::error::VfsError::NotFound(_)) => ResolutionDirectoryValue::NotFound,
        Err(error) if vfs_error_is_not_a_directory(error) => {
            ResolutionDirectoryValue::NotADirectory
        }
        Err(_) => ResolutionDirectoryValue::Unstable,
    };
    record_resolution_directory_observation(canonical_id, value);
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
        Self::new_with_input_resolution_budgets(
            options,
            verter_semantic::resolver_core::InputResolutionBudgets::default(),
        )
    }

    /// Construct with a complete tightening-only semantic budget policy.
    pub fn new_with_input_resolution_budgets(
        options: FilesystemOptions,
        budgets: verter_semantic::resolver_core::InputResolutionBudgets,
    ) -> Self {
        Self {
            options,
            engine: Engine::new_with_input_resolution_budgets(budgets),
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

        // A dirty parent about to be rescanned may have had a path component
        // relinked or removed, so any realpath memo entry under it is suspect.
        // Evicting here (in addition to the explicit `apply_changes` wiring)
        // closes the gap for any dir-index dirty source whose refresh consumes
        // the dirtiness — a refresh must never clear the dirty mark while stale
        // realpath entries survive.
        if was_dirty {
            self.native_fs.invalidate_realpath_under(parent);
        }

        self.engine
            .vfs_provenance
            .native_fs_read_dir_count
            .fetch_add(1, Ordering::Relaxed);
        match self.native_fs.read_dir(parent) {
            Ok(mut entries) => {
                entries.sort();
                record_resolution_directory_observation(
                    parent,
                    ResolutionDirectoryValue::Entries(entries.clone()),
                );
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
                record_resolution_directory_observation(parent, ResolutionDirectoryValue::NotFound);
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
            Err(error) if vfs_error_is_not_a_directory(&error) => {
                // The "parent" is a FILE (an index-file probe under a carrier
                // file): a stable observation — nothing exists under it.
                record_resolution_directory_observation(
                    parent,
                    ResolutionDirectoryValue::NotADirectory,
                );
                self.engine.dir_index.write().refresh(parent, Vec::new());
                self.engine
                    .vfs_provenance
                    .dir_index_refresh_count
                    .fetch_add(1, Ordering::Relaxed);
                Some(false)
            }
            Err(_) => {
                record_resolution_directory_observation(parent, ResolutionDirectoryValue::Unstable);
                None
            }
        }
    }

    /// Inject a file directly into the snapshot cache.
    pub fn inject_file(&self, canonical_id: String, source: Arc<str>) {
        self.engine.mutate_content_for(
            &canonical_id,
            false,
            Some(verter_semantic::resolver_core::PathProbe::File),
            crate::engine::BaseRealpathTransition::Unknown,
            || {
                self.engine.invalidate_package_manifest(&canonical_id);
                self.engine
                    .snapshot
                    .write()
                    .inject(canonical_id.clone(), source);
                ((), true)
            },
        );
    }

    /// Apply a batch of workspace changes.
    ///
    /// This is the authoritative external-change channel (watcher batches land
    /// here). Each change carries a dir-index dirty signal that
    /// `Engine::apply_changes` fires; the realpath memo lives on `NativeFs`
    /// (owned here, NOT on the shared `Engine` that also backs
    /// `MemoryWorkspace`), so `FilesystemWorkspace` drives its eviction
    /// explicitly from the same signals before delegating: `FileChanged` /
    /// `FileDeleted` evict the changed path, `DirectoryTreeDirty` evicts the
    /// subtree.
    pub fn apply_changes(&self, changes: Vec<WorkspaceChange>) -> ChangeResult {
        self.engine
            .apply_changes_with_preflight(changes, |changes| {
                // The realpath memo lives on `NativeFs`, which exists only on
                // native targets; `wasm32` has no memo to evict.
                let _ = changes;
                #[cfg(not(target_arch = "wasm32"))]
                for change in changes {
                    match change {
                        WorkspaceChange::FileChanged { canonical_id, .. }
                        | WorkspaceChange::FileDeleted { canonical_id } => {
                            self.native_fs.invalidate_realpath_under(canonical_id);
                        }
                        WorkspaceChange::DirectoryTreeDirty { prefix } => {
                            self.native_fs.invalidate_realpath_under(prefix);
                        }
                        WorkspaceChange::OverlaySet { .. }
                        | WorkspaceChange::OverlayClear { .. }
                        | WorkspaceChange::ConfigChanged { .. } => {}
                    }
                }
            })
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
        self.engine.set_configured_resolver_projects(None);
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

    /// TEST-ONLY: subscribe to snapshot publications. See
    /// [`crate::engine::Engine::subscribe_published`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn subscribe_published(&self) -> std::sync::mpsc::Receiver<u64> {
        self.engine.subscribe_published()
    }

    /// Add an explicit project to the graph and rebuild the resolver.
    pub fn add_explicit_project(&self, config: VfsProjectConfig) {
        self.engine.set_configured_resolver_projects(None);
        let mut graph = self.engine.project_graph.write();
        let mut projects: Vec<VfsProjectConfig> = graph.iter().cloned().collect();
        projects.push(config);
        *graph = ProjectGraph::from_configs(projects);
        drop(graph);
        self.engine.rebuild_and_publish();
    }

    /// Resolve against an immutable, request-local snapshot of every
    /// filesystem observation made by the resolver.
    ///
    /// A discovery pass records live VFS/OS observations but is necessarily
    /// ReturnOnly. After the observation set is revalidated, the Engine reruns
    /// the request against a frozen reader. Only that second pass may admit a
    /// durable resolution product. A new observation in the second pass marks
    /// the snapshot incomplete and forces another bounded discovery attempt.
    fn resolve_import_at_published_snapshot(
        &self,
        published: &Arc<crate::published_state::PublishedRoot>,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        let mut input_ledger =
            crate::resolver::InputResolutionLedger::new(self.engine.input_resolution_budgets);

        loop {
            let recorder = FilesystemResolutionRecorder::new(self, Arc::clone(published));
            let discovery = self
                .engine
                .resolve_import_outcome_for_published_in_operation(
                    &recorder,
                    self.resolution_evidence_source(),
                    importer_id,
                    specifier,
                    ctx,
                    crate::engine::ResolutionOperation::pinned(
                        published,
                        &mut input_ledger,
                        &|| true,
                    ),
                );
            if matches!(
                discovery.non_admission_reason(),
                Some(
                    verter_audit::NonAdmissionReason::ResolutionViewSuperseded
                        | verter_audit::NonAdmissionReason::BudgetExceeded
                )
            ) {
                return discovery;
            }
            input_ledger.discard_staged_loaded_inputs();

            let frozen = recorder.freeze();
            if !frozen.revalidate() {
                if input_ledger.charge_outer_restart(self).is_err() {
                    return crate::resolution_currency::ResolutionOutcome::refused(
                        None,
                        verter_audit::NonAdmissionReason::BudgetExceeded,
                    );
                }
                continue;
            }
            let final_valid = std::cell::Cell::new(true);
            let outcome = self
                .engine
                .resolve_import_outcome_for_published_in_operation(
                    &frozen,
                    self.resolution_evidence_source(),
                    importer_id,
                    specifier,
                    ctx,
                    crate::engine::ResolutionOperation::pinned(
                        published,
                        &mut input_ledger,
                        &|| {
                            let valid = frozen.complete() && frozen.revalidate();
                            final_valid.set(valid);
                            valid
                        },
                    ),
                );
            if final_valid.get() {
                return outcome;
            }
            if input_ledger.charge_outer_restart(self).is_err() {
                return crate::resolution_currency::ResolutionOutcome::refused(
                    None,
                    verter_audit::NonAdmissionReason::BudgetExceeded,
                );
            }
        }
    }

    fn resolve_import_with_overlay_snapshot(
        &self,
        overlay: &crate::resolution_currency::ResolutionOverlaySnapshot,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        let Some(published) = self.load_published() else {
            return crate::resolution_currency::ResolutionOutcome::refused(
                None,
                verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance,
            );
        };
        let mut input_ledger =
            crate::resolver::InputResolutionLedger::new(self.engine.input_resolution_budgets);

        loop {
            let recorder = FilesystemResolutionRecorder::new(self, Arc::clone(&published));
            let discovery = {
                let reader =
                    crate::resolution_currency::OverlaySnapshotReader::new(&recorder, overlay);
                self.engine
                    .resolve_import_outcome_for_published_in_operation(
                        &reader,
                        self.resolution_evidence_source(),
                        importer_id,
                        specifier,
                        ctx,
                        crate::engine::ResolutionOperation::pinned(
                            &published,
                            &mut input_ledger,
                            &|| true,
                        ),
                    )
            };
            if matches!(
                discovery.non_admission_reason(),
                Some(
                    verter_audit::NonAdmissionReason::ResolutionViewSuperseded
                        | verter_audit::NonAdmissionReason::BudgetExceeded
                )
            ) {
                return discovery;
            }
            input_ledger.discard_staged_loaded_inputs();

            let frozen = recorder.freeze();
            if !frozen.revalidate() {
                if input_ledger.charge_outer_restart(self).is_err() {
                    return crate::resolution_currency::ResolutionOutcome::refused(
                        None,
                        verter_audit::NonAdmissionReason::BudgetExceeded,
                    );
                }
                continue;
            }
            let final_valid = std::cell::Cell::new(true);
            let outcome = {
                let reader =
                    crate::resolution_currency::OverlaySnapshotReader::new(&frozen, overlay);
                self.engine
                    .resolve_import_outcome_for_published_in_operation(
                        &reader,
                        self.resolution_evidence_source(),
                        importer_id,
                        specifier,
                        ctx,
                        crate::engine::ResolutionOperation::pinned(
                            &published,
                            &mut input_ledger,
                            &|| {
                                let valid = frozen.complete() && frozen.revalidate();
                                final_valid.set(valid);
                                valid
                            },
                        ),
                    )
            };
            if final_valid.get() {
                return outcome;
            }
            if input_ledger.charge_outer_restart(self).is_err() {
                return crate::resolution_currency::ResolutionOutcome::refused(
                    None,
                    verter_audit::NonAdmissionReason::BudgetExceeded,
                );
            }
        }
    }

    /// Record a resolution-derived parsed-edge batch against an immutable,
    /// request-local snapshot of every filesystem observation the recording
    /// makes.
    ///
    /// The live filesystem reader is never resolution-event-bridge complete,
    /// so every relative edge resolved through it refuses admission and the
    /// whole batch is dropped. A discovery pass records the live VFS/OS
    /// observations and publishes nothing; once that observation set
    /// revalidates, the SAME recording reruns against the frozen reader —
    /// the only reader whose parsed-edge resolutions the Engine admits.
    ///
    /// `None` when no root is published yet (nothing durable may be recorded
    /// against an unpublished workspace) or when the observation snapshot
    /// never stabilised within the bounded attempt budget.
    fn record_parsed_edges_with_frozen_evidence<T>(
        &self,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
        publish: impl Fn(
            &dyn crate::traits::WorkspaceRead,
            &mut crate::resolver::InputResolutionLedger,
            &dyn Fn() -> bool,
        ) -> Option<T>,
    ) -> Option<T> {
        crate::probe_scope!(RECORD_EDGES_FROZEN);
        let published = self.load_published()?;
        let mut input_ledger =
            crate::resolver::InputResolutionLedger::new(self.engine.input_resolution_budgets);

        loop {
            let recorder = FilesystemResolutionRecorder::new(self, Arc::clone(&published));
            self.engine.observe_parsed_edge_evidence_in_operation(
                &recorder,
                canonical_id,
                edges,
                &mut input_ledger,
            );
            input_ledger.discard_staged_loaded_inputs();

            let frozen = recorder.freeze();
            if !frozen.revalidate() {
                if input_ledger.charge_outer_restart(self).is_err() {
                    break;
                }
                continue;
            }
            let final_valid = std::cell::Cell::new(true);
            if let Some(product) = publish(&frozen, &mut input_ledger, &|| {
                let valid = frozen.complete() && frozen.revalidate();
                final_valid.set(valid);
                valid
            }) {
                return Some(product);
            }
            if final_valid.get() || input_ledger.charge_outer_restart(self).is_err() {
                break;
            }
        }
        None
    }

    fn record_parsed_edges_many_with_frozen_evidence(
        &self,
        records: &[(String, Vec<crate::types::ParsedEdge>)],
    ) -> Option<()> {
        crate::probe_scope!(RECORD_EDGES_FROZEN);
        if records.is_empty() {
            return Some(());
        }
        let published = self.load_published()?;
        let mut input_ledgers: Vec<_> = records
            .iter()
            .map(|_| {
                crate::resolver::InputResolutionLedger::new(self.engine.input_resolution_budgets)
            })
            .collect();

        loop {
            let recorder = FilesystemResolutionRecorder::new(self, Arc::clone(&published));
            for ((canonical_id, edges), ledger) in records.iter().zip(input_ledgers.iter_mut()) {
                self.engine.observe_parsed_edge_evidence_in_operation(
                    &recorder,
                    canonical_id,
                    edges,
                    ledger,
                );
                ledger.discard_staged_loaded_inputs();
            }

            let frozen = recorder.freeze();
            if !frozen.revalidate() {
                if input_ledgers
                    .iter_mut()
                    .any(|ledger| ledger.charge_outer_restart(self).is_err())
                {
                    break;
                }
                continue;
            }

            let final_valid = std::cell::Cell::new(true);
            let committed = self.engine.record_parsed_edges_many_in_operation(
                &frozen,
                records,
                &mut input_ledgers,
                &|| {
                    let valid = frozen.complete() && frozen.revalidate();
                    final_valid.set(valid);
                    valid
                },
            );
            if committed {
                return Some(());
            }
            if final_valid.get()
                || input_ledgers
                    .iter_mut()
                    .any(|ledger| ledger.charge_outer_restart(self).is_err())
            {
                break;
            }
        }
        None
    }
}

struct FilesystemResolutionObservations {
    files: parking_lot::Mutex<HashMap<String, Option<Arc<str>>>>,
    probes: parking_lot::Mutex<HashMap<String, verter_semantic::resolver_core::PathProbe>>,
    realpaths: parking_lot::Mutex<HashMap<String, Option<String>>>,
    manifests: parking_lot::Mutex<HashMap<String, Option<crate::types::PackageManifest>>>,
    directories: parking_lot::Mutex<HashMap<String, ResolutionDirectoryValue>>,
    consistent: AtomicBool,
}

impl Default for FilesystemResolutionObservations {
    fn default() -> Self {
        Self {
            files: parking_lot::Mutex::new(HashMap::new()),
            probes: parking_lot::Mutex::new(HashMap::new()),
            realpaths: parking_lot::Mutex::new(HashMap::new()),
            manifests: parking_lot::Mutex::new(HashMap::new()),
            directories: parking_lot::Mutex::new(HashMap::new()),
            consistent: AtomicBool::new(true),
        }
    }
}

impl FilesystemResolutionObservations {
    fn record<T: PartialEq>(
        &self,
        map: &parking_lot::Mutex<HashMap<String, T>>,
        canonical_id: &str,
        value: T,
    ) {
        let mut map = map.lock();
        let key = observation_key(canonical_id);
        if map.get(&key).is_some_and(|previous| previous != &value) {
            self.consistent.store(false, Ordering::Release);
        }
        map.insert(key, value);
    }

    fn record_manifest(&self, canonical_id: &str, value: Option<crate::types::PackageManifest>) {
        let mut manifests = self.manifests.lock();
        let key = observation_key(canonical_id);
        if manifests
            .get(&key)
            .is_some_and(|previous| !manifests_equal(previous.as_ref(), value.as_ref()))
        {
            self.consistent.store(false, Ordering::Release);
        }
        manifests.insert(key, value);
    }

    fn absorb_directory_evidence(&self, observations: Vec<ResolutionDirectoryObservation>) {
        for observation in observations {
            if observation.value == ResolutionDirectoryValue::Unstable {
                self.consistent.store(false, Ordering::Release);
            }
            self.record(&self.directories, &observation.canonical, observation.value);
        }
    }
}

/// Backend-internal memos a live read has PROVEN stale, collected so one
/// repair call drops exactly them.
///
/// Kept as sets rather than repaired inline because the directory-index
/// repair takes a write lock: a freeze that contradicted forty probes under
/// one parent must take it once, not forty times.
#[derive(Debug, Default)]
struct StaleResolutionMemos {
    directories: std::collections::BTreeSet<String>,
    realpath_prefixes: std::collections::BTreeSet<String>,
    manifests: std::collections::BTreeSet<String>,
}

struct FilesystemResolutionRecorder<'a> {
    workspace: &'a FilesystemWorkspace,
    published: Arc<crate::published_state::PublishedRoot>,
    observations: FilesystemResolutionObservations,
}

impl<'a> FilesystemResolutionRecorder<'a> {
    fn new(
        workspace: &'a FilesystemWorkspace,
        published: Arc<crate::published_state::PublishedRoot>,
    ) -> Self {
        Self {
            workspace,
            published,
            observations: FilesystemResolutionObservations::default(),
        }
    }

    /// Seal the observation set into the immutable evidence the admitted
    /// replay reads, re-reading every observed path type, file/manifest value,
    /// realpath and directory membership INDEPENDENTLY from the live
    /// filesystem.
    ///
    /// Discovery is allowed to read through the ordinary workspace caches —
    /// it only has to identify WHICH observations the resolver makes. The
    /// frozen evidence may not: a directory index or realpath memo that went
    /// stale (a path that appeared or vanished without an event reaching the
    /// bridge) would otherwise be contradicted by [`Self::revalidate`]'s
    /// independent re-read on every single attempt, so the request could never
    /// admit again.
    ///
    /// A re-read that CONFLICTS with the recorded observation is exactly
    /// "state newer than the captured root". It enters through the ordinary
    /// invalidation protocol — the affected directory index and realpath memo
    /// are marked dirty — so the next attempt's discovery observes the live
    /// truth, and this snapshot is marked incomplete so the current attempt
    /// retries instead of admitting a half-refreshed evidence set.
    fn freeze(self) -> FrozenFilesystemResolutionReader<'a> {
        self.observations
            .absorb_directory_evidence(take_resolution_directory_evidence());
        let workspace = self.workspace;
        let mut complete = self.observations.consistent.into_inner();
        // Memos whose cached answer demonstrably disagrees with the live
        // filesystem, so the retry reads current state instead of replaying
        // the same stale answer.
        let mut stale = StaleResolutionMemos::default();

        let mut files = self.observations.files.into_inner();
        for (canonical, recorded) in &mut files {
            match workspace.independent_file_bytes(canonical) {
                Ok(live) if live == *recorded => {}
                Ok(live) => {
                    *recorded = live;
                    complete = false;
                }
                Err(()) => complete = false,
            }
        }

        let mut probes = self.observations.probes.into_inner();
        for (canonical, recorded) in &mut probes {
            let live = workspace.independent_probe_path(canonical);
            if live == *recorded {
                continue;
            }
            *recorded = live;
            complete = false;
            if let Some((parent, _)) = split_parent_basename(canonical) {
                stale.directories.insert(parent.to_owned());
                stale.realpath_prefixes.insert(parent.to_owned());
            }
        }

        let mut realpaths = self.observations.realpaths.into_inner();
        for (canonical, recorded) in &mut realpaths {
            match workspace.independent_realpath(canonical) {
                Ok(live) if live == *recorded => {}
                Ok(live) => {
                    *recorded = live;
                    complete = false;
                    stale.realpath_prefixes.insert(canonical.clone());
                }
                Err(()) => complete = false,
            }
        }

        let mut manifests = self.observations.manifests.into_inner();
        for (canonical, recorded) in &mut manifests {
            match workspace.independent_manifest(canonical) {
                Ok(live) if manifests_equal(live.as_ref(), recorded.as_ref()) => {}
                Ok(live) => {
                    *recorded = live;
                    complete = false;
                    stale.manifests.insert(canonical.clone());
                }
                Err(()) => complete = false,
            }
        }

        let mut directories = self.observations.directories.into_inner();
        for (canonical, recorded) in &mut directories {
            let live = workspace.independent_directory(canonical);
            if live == ResolutionDirectoryValue::Unstable {
                complete = false;
            }
            if live == *recorded {
                continue;
            }
            *recorded = live;
            complete = false;
            stale.directories.insert(canonical.clone());
        }

        workspace.repair_resolution_memos(&stale);

        FrozenFilesystemResolutionReader {
            workspace,
            published: self.published,
            content_generation: workspace.engine.current_content_generation(),
            files,
            probes,
            realpaths,
            manifests,
            directories,
            complete: AtomicBool::new(complete),
        }
    }
}

fn observation_key(canonical_id: &str) -> String {
    verter_semantic::resolver_core::normalize_canonical_id(canonical_id)
}

impl FilesystemWorkspace {
    /// Count one live evidence syscall. The counter is the cost model's
    /// measurement rail: warm reuse on this backend is zero-syscall WITHIN a
    /// content generation and O(distinct witness path-canonicals) live reads
    /// per generation, so a change that turns it into O(reuses) is visible
    /// as a number rather than as a regression nobody notices.
    #[cfg(not(target_arch = "wasm32"))]
    fn count_live_evidence_read(&self) {
        self.engine
            .vfs_provenance
            .resolution_evidence_live_read_count
            .fetch_add(1, Ordering::Relaxed);
    }

    fn independent_file_bytes(&self, canonical_id: &str) -> Result<Option<Arc<str>>, ()> {
        if let Some(source) = self.engine.overlay.read().get(canonical_id) {
            return Ok(Some(source));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.count_live_evidence_read();
            self.native_fs.read_file_live(canonical_id).map_err(|_| ())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = canonical_id;
            Err(())
        }
    }

    fn independent_probe_path(
        &self,
        canonical_id: &str,
    ) -> verter_semantic::resolver_core::PathProbe {
        if self.engine.overlay.read().has_overlay(canonical_id) {
            return verter_semantic::resolver_core::PathProbe::File;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.count_live_evidence_read();
            self.native_fs.probe_path(canonical_id)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = canonical_id;
            verter_semantic::resolver_core::PathProbe::Unknown
        }
    }

    fn independent_realpath(&self, canonical_id: &str) -> Result<Option<String>, ()> {
        if self.engine.overlay.read().has_overlay(canonical_id) {
            return Ok(Some(observation_key(canonical_id)));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.count_live_evidence_read();
            self.native_fs.realpath_live(canonical_id).map_err(|_| ())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = canonical_id;
            Err(())
        }
    }

    fn independent_manifest(
        &self,
        canonical_id: &str,
    ) -> Result<Option<crate::types::PackageManifest>, ()> {
        self.independent_file_bytes(canonical_id).map(|source| {
            source.map(|source| crate::package_index::parse_package_json(source.as_ref()))
        })
    }

    /// **This backend's live evidence read** — the implementation behind
    /// [`crate::traits::WorkspaceRead::observe_resolution_evidence`].
    ///
    /// It goes through the `independent_*` rail, which is the whole point:
    /// the ordinary reads answer a typed probe out of a CLEAN parent
    /// directory index without a syscall, a realpath out of the `NativeFs`
    /// memo, a manifest out of the package cache, and bytes out of the shared
    /// file snapshot — all four refreshed only by an event. When no event
    /// exists (a package installed into an unwatched `node_modules`), reading
    /// through them can only ever confirm them. The overlay is the one thing
    /// consulted, because an open buffer's content is authoritative state
    /// rather than a copy of disk.
    ///
    /// Cost, per canonical: one `metadata` for a path that is absent, or
    /// inaccessible, or unclassifiable; plus one `canonicalize` for a present
    /// one; plus one `read` for a present manifest. Every non-present probe
    /// short-circuits the other two — the same errno the metadata call just
    /// reported would come back from both, so skipping them is an identical
    /// answer for fewer syscalls, not an approximation.
    ///
    /// `Absent`, `Inaccessible` and `Unknown` are all observed VALUES, and all
    /// three are returned as such. `Inaccessible` in particular is a
    /// first-class outcome this codebase already acts on — an observed
    /// `Inaccessible` forces
    /// [`verter_audit::NonAdmissionReason::ResolutionInaccessiblePath`] — so
    /// dropping it here would leave the candidate's `PathProbe` fact frozen at
    /// its last readable value, its signature validating forever, and the
    /// canonical pinned in the pending ledger, never stamped: a positive
    /// candidate whose target became unreadable would be served warm for the
    /// process's lifetime. Revoked macOS Full Disk Access, a root-owned bind
    /// mount under `node_modules`, `ELOOP`/`EIO`, and a Windows sharing
    /// violation all land here.
    ///
    /// `None` is reserved for "this source genuinely cannot observe the
    /// canonical at all": unstable I/O behind a present path. A read that did
    /// not happen must certify nothing.
    #[cfg(not(target_arch = "wasm32"))]
    fn observe_live_evidence(
        &self,
        canonical_id: &str,
    ) -> Option<crate::resolution_currency::LiveResolutionObservation> {
        use crate::resolution_currency::{
            is_package_manifest_path, manifest_fingerprint_of, LiveResolutionObservation,
        };
        use verter_semantic::resolver_core::PathProbe;

        let key = observation_key(canonical_id);
        let is_manifest = is_package_manifest_path(&key);
        let probe = self.independent_probe_path(canonical_id);
        match probe {
            PathProbe::File | PathProbe::Directory => {}
            PathProbe::Absent | PathProbe::Inaccessible | PathProbe::Unknown => {
                return Some(LiveResolutionObservation {
                    probe,
                    realpath: None,
                    manifest: is_manifest.then_some(None),
                });
            }
        }
        let realpath = self
            .independent_realpath(canonical_id)
            .ok()?
            .map(|path| verter_semantic::resolver_core::normalize_canonical_id(&path));
        let manifest = if is_manifest {
            Some(
                self.independent_manifest(canonical_id)
                    .ok()?
                    .as_ref()
                    .map(manifest_fingerprint_of),
            )
        } else {
            None
        };
        Some(LiveResolutionObservation {
            probe,
            realpath,
            manifest,
        })
    }

    /// **This backend's memo repair** — the one place a stale directory index
    /// entry or realpath memo entry is dropped.
    ///
    /// Called only where a live read has already CONTRADICTED a recorded
    /// value: by `FilesystemResolutionRecorder::freeze` when its independent
    /// re-read disagrees with a discovery observation, and by the evidence
    /// hook when it disagrees with the world's recorded baseline. One
    /// invalidation rail, driven from two places, so a repair can never be
    /// performed by one path and skipped by the other.
    fn repair_resolution_memos(&self, stale: &StaleResolutionMemos) {
        if !stale.directories.is_empty() {
            let mut dir_index = self.engine.dir_index.write();
            for directory in &stale.directories {
                dir_index.mark_dirty(directory);
            }
        }
        for manifest in &stale.manifests {
            // BOTH layers, or the repair does not repair. The parsed manifest
            // is derived from bytes the shared file snapshot is holding
            // read-through, so dropping only the parse re-parses the same
            // stale bytes and the next discovery pass observes the same stale
            // manifest the live read just contradicted — a two-pass bridge
            // that can never converge, which surfaces as a resolution refused
            // for retry exhaustion rather than as a stale answer.
            //
            // The overlay is untouched: it wins over the snapshot in every
            // read, and its content is authoritative rather than cached.
            self.engine.invalidate_package_manifest(manifest);
            self.engine.snapshot.write().remove(manifest);
        }
        #[cfg(not(target_arch = "wasm32"))]
        for prefix in &stale.realpath_prefixes {
            self.native_fs.invalidate_realpath_under(prefix);
        }
    }

    /// Read `canonical_id` live and repair the memos that the live value
    /// proves stale — exactly the memos, and only on disagreement.
    #[cfg(not(target_arch = "wasm32"))]
    fn observe_live_evidence_and_repair(
        &self,
        canonical_id: &str,
        recorded: Option<&crate::resolution_currency::RecordedResolutionBaseline>,
    ) -> Option<crate::resolution_currency::LiveResolutionObservation> {
        let live = self.observe_live_evidence(canonical_id)?;
        // No belief to contradict — a first observation repairs nothing,
        // because nothing is evidence that anything cached is wrong.
        let disagreement = recorded
            .map(|recorded| recorded.disagreements(&live))
            .unwrap_or_default();
        if !disagreement.any() {
            return Some(live);
        }
        let mut stale = StaleResolutionMemos::default();
        if disagreement.probe {
            // A path that appeared or vanished invalidates its parent's
            // membership; both its own realpath and anything resolving
            // THROUGH it are stale.
            if let Some((parent, _)) = split_parent_basename(canonical_id) {
                stale.directories.insert(parent.to_owned());
            }
            stale
                .realpath_prefixes
                .insert(observation_key(canonical_id));
        }
        if disagreement.realpath {
            // A symlink can retarget with the typed probe UNCHANGED (`File`
            // before, `File` after), so this limb is not implied by the one
            // above: without it the memo keeps answering with the old target.
            stale
                .realpath_prefixes
                .insert(observation_key(canonical_id));
        }
        if disagreement.manifest {
            stale.manifests.insert(observation_key(canonical_id));
        }
        self.repair_resolution_memos(&stale);
        Some(live)
    }

    /// This backend's declared evidence capability, stated at every Engine
    /// resolution entry it calls.
    ///
    /// `Uncovered`: no watcher covers `node_modules`, so a package appearing
    /// there reaches this backend with no event of any kind. On `wasm32`
    /// there is no live filesystem behind the caches at all — no
    /// `independent_*` read can succeed — so the honest answer is `Inert`, and
    /// nothing is re-observed or stamped rather than a fabricated `Unknown`
    /// being folded over every recorded baseline.
    fn resolution_evidence_source(
        &self,
    ) -> crate::resolution_currency::ResolutionEvidenceSource<'_> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::resolution_currency::ResolutionEvidenceSource::Uncovered(self)
        }
        #[cfg(target_arch = "wasm32")]
        {
            crate::resolution_currency::ResolutionEvidenceSource::Inert
        }
    }

    fn independent_directory(&self, canonical_id: &str) -> ResolutionDirectoryValue {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.native_fs.read_dir(canonical_id) {
                Ok(mut entries) => {
                    entries.sort();
                    ResolutionDirectoryValue::Entries(entries)
                }
                Err(crate::error::VfsError::NotFound(_)) => ResolutionDirectoryValue::NotFound,
                Err(error) if vfs_error_is_not_a_directory(&error) => {
                    ResolutionDirectoryValue::NotADirectory
                }
                Err(_) => ResolutionDirectoryValue::Unstable,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = canonical_id;
            ResolutionDirectoryValue::Unstable
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl crate::resolution_currency::LiveResolutionEvidence for FilesystemWorkspace {
    fn observe_live_resolution_evidence(
        &self,
        canonical_id: &str,
        recorded: Option<&crate::resolution_currency::RecordedResolutionBaseline>,
    ) -> Option<crate::resolution_currency::LiveResolutionObservation> {
        self.observe_live_evidence_and_repair(canonical_id, recorded)
    }
}

impl crate::traits::WorkspaceRead for FilesystemResolutionRecorder<'_> {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        let result = crate::traits::WorkspaceRead::read_file(self.workspace, canonical_id);
        self.observations
            .record(&self.observations.files, canonical_id, result.clone());
        result
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        matches!(
            self.probe_path(canonical_id),
            verter_semantic::resolver_core::PathProbe::File
                | verter_semantic::resolver_core::PathProbe::Directory
        )
    }

    fn probe_path(&self, canonical_id: &str) -> verter_semantic::resolver_core::PathProbe {
        let result = crate::traits::WorkspaceRead::probe_path(self.workspace, canonical_id);
        self.observations
            .record(&self.observations.probes, canonical_id, result);
        result
    }

    fn resolution_event_bridge_complete(&self) -> bool {
        // The DISCOVERY pass reads live disk through mutable caches, so it
        // can never admit; only the frozen replay below can. It is NOT
        // request-local, though: it may READ the shared candidate slot, so
        // a warm owner edge answers without touching the filesystem at
        // all. Suppressing the read here is what made every resolution on
        // this backend cold.
        false
    }

    fn take_resolution_directory_observations(&self) -> Vec<String> {
        let observations = take_resolution_directory_evidence();
        let canonicals = observations
            .iter()
            .map(|observation| observation.canonical.clone())
            .collect();
        self.observations.absorb_directory_evidence(observations);
        canonicals
    }

    fn resolution_population(&self) -> verter_semantic::resolver_core::ResolutionPopulation {
        crate::traits::WorkspaceRead::resolution_population(self.workspace)
    }

    fn capture_resolution_world(
        &self,
    ) -> Option<Arc<crate::resolution_currency::CapturedResolutionWorld>> {
        crate::traits::WorkspaceRead::capture_resolution_world(self.workspace)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        let result = crate::traits::WorkspaceRead::realpath(self.workspace, canonical_id);
        self.observations
            .record(&self.observations.realpaths, canonical_id, result.clone());
        result
    }

    fn read_package_manifest(&self, canonical_id: &str) -> Option<crate::types::PackageManifest> {
        let result =
            crate::traits::WorkspaceRead::read_package_manifest(self.workspace, canonical_id);
        self.observations
            .record_manifest(canonical_id, result.clone());
        result
    }

    fn preflight_resolution_inputs_bounded(
        &self,
        keys: &[verter_semantic::resolver_core::InputKey],
        basis: verter_semantic::resolver_core::ResolutionBasis,
    ) -> Result<
        crate::resolver::ResolutionInputReservationBatch,
        verter_semantic::resolver_core::AttemptFailure,
    > {
        let reservation = crate::traits::WorkspaceRead::preflight_resolution_inputs_bounded(
            self.workspace,
            keys,
            basis,
        )?;
        for entry in reservation.entries() {
            match entry {
                crate::resolver::ResolutionInputReservation::PathProbe {
                    key: verter_semantic::resolver_core::InputKey::PathProbe { path },
                    value,
                    ..
                } => self
                    .observations
                    .record(&self.observations.probes, path, *value),
                crate::resolver::ResolutionInputReservation::RealPath {
                    key: verter_semantic::resolver_core::InputKey::RealPath { path },
                    value,
                    ..
                } => self
                    .observations
                    .record(&self.observations.realpaths, path, value.clone()),
                _ => {}
            }
        }
        Ok(reservation)
    }

    fn load_preflighted_resolution_inputs(
        &self,
        reservation: &crate::resolver::ResolutionInputReservationBatch,
    ) -> Result<
        crate::resolver::LoadedResolutionInputBatch,
        verter_semantic::resolver_core::AttemptFailure,
    > {
        let loaded = crate::traits::WorkspaceRead::load_preflighted_resolution_inputs(
            self.workspace,
            reservation,
        )?;
        for entry in loaded.entries() {
            if let crate::resolver::LoadedResolutionInput::PackageManifest {
                manifest_path,
                value,
                ..
            } = entry
            {
                self.observations
                    .record_manifest(manifest_path, value.as_deref().cloned());
            }
        }
        Ok(loaded)
    }

    fn classify_file(&self, canonical_id: &str) -> verter_language::FileLanguage {
        crate::traits::WorkspaceRead::classify_file(self.workspace, canonical_id)
    }

    fn is_workspace_owned(&self, canonical_id: &str) -> bool {
        let resolved = self.realpath(canonical_id);
        self.workspace
            .engine
            .is_workspace_owned(resolved.as_deref().unwrap_or(canonical_id))
    }

    fn is_package_backed(&self, canonical_id: &str) -> bool {
        let resolved = self.realpath(canonical_id);
        self.workspace
            .engine
            .is_package_backed(resolved.as_deref().unwrap_or(canonical_id))
    }

    fn content_generation(&self) -> u64 {
        self.workspace.engine.current_content_generation()
    }

    fn resolution_fact_generation(&self) -> u64 {
        self.workspace.engine.current_resolution_fact_generation()
    }

    fn last_content_transition_generation(&self, canonical_id: &str) -> u64 {
        self.workspace
            .engine
            .last_content_transition_generation(canonical_id)
    }

    fn published_root(&self) -> Option<Arc<crate::published_state::PublishedRoot>> {
        Some(Arc::clone(&self.published))
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        crate::traits::WorkspaceRead::reverse_deps_for(self.workspace, canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        crate::traits::WorkspaceRead::forward_deps_for(self.workspace, canonical_id)
    }

    fn dependency_snapshot(&self, canonical_id: &str) -> Option<crate::DependencySnapshotView> {
        crate::traits::WorkspaceRead::dependency_snapshot(self.workspace, canonical_id)
    }

    fn read_dir(&self, dir: &str) -> Result<Vec<crate::error::DirEntry>, crate::error::VfsError> {
        crate::traits::WorkspaceRead::read_dir(self.workspace, dir)
    }

    fn walk(
        &self,
        root: &str,
        filter_dir: &dyn Fn(&str) -> bool,
        filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, crate::error::VfsError> {
        self.observations.consistent.store(false, Ordering::Release);
        crate::traits::WorkspaceRead::walk(self.workspace, root, filter_dir, filter_file)
    }

    fn is_dir(&self, path: &str) -> bool {
        crate::traits::WorkspaceRead::is_dir(self.workspace, path)
    }
}

struct FrozenFilesystemResolutionReader<'a> {
    workspace: &'a FilesystemWorkspace,
    published: Arc<crate::published_state::PublishedRoot>,
    content_generation: u64,
    files: HashMap<String, Option<Arc<str>>>,
    probes: HashMap<String, verter_semantic::resolver_core::PathProbe>,
    realpaths: HashMap<String, Option<String>>,
    manifests: HashMap<String, Option<crate::types::PackageManifest>>,
    directories: HashMap<String, ResolutionDirectoryValue>,
    complete: AtomicBool,
}

impl FrozenFilesystemResolutionReader<'_> {
    fn mark_incomplete(&self) {
        self.complete.store(false, Ordering::Release);
    }

    fn complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }

    fn revalidate(&self) -> bool {
        if !self.complete() {
            return false;
        }
        if self.workspace.engine.current_content_generation() != self.content_generation {
            return false;
        }
        let Some(current) = self.workspace.engine.load_published() else {
            return false;
        };
        if !Arc::ptr_eq(&current, &self.published) {
            return false;
        }
        self.files.iter().all(|(canonical, expected)| {
            self.workspace
                .independent_file_bytes(canonical)
                .is_ok_and(|actual| actual == *expected)
        }) && self.probes.iter().all(|(canonical, expected)| {
            self.workspace.independent_probe_path(canonical) == *expected
        }) && self.realpaths.iter().all(|(canonical, expected)| {
            self.workspace
                .independent_realpath(canonical)
                .is_ok_and(|actual| actual == *expected)
        }) && self.manifests.iter().all(|(canonical, expected)| {
            self.workspace
                .independent_manifest(canonical)
                .is_ok_and(|actual| manifests_equal(actual.as_ref(), expected.as_ref()))
        }) && self.directories.iter().all(|(canonical, expected)| {
            self.workspace.independent_directory(canonical) == *expected
        })
    }
}

fn manifests_equal(
    left: Option<&crate::types::PackageManifest>,
    right: Option<&crate::types::PackageManifest>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.name == right.name
                && left.version == right.version
                && left.main == right.main
                && left.module == right.module
                && left.types == right.types
                && left.typings == right.typings
                && left.exports == right.exports
                && left.imports == right.imports
                && left.raw == right.raw
        }
        _ => false,
    }
}

impl crate::traits::WorkspaceRead for FrozenFilesystemResolutionReader<'_> {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        match self.files.get(&observation_key(canonical_id)) {
            Some(result) => result.clone(),
            None => {
                self.mark_incomplete();
                None
            }
        }
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        matches!(
            self.probe_path(canonical_id),
            verter_semantic::resolver_core::PathProbe::File
                | verter_semantic::resolver_core::PathProbe::Directory
        )
    }

    fn probe_path(&self, canonical_id: &str) -> verter_semantic::resolver_core::PathProbe {
        self.probes
            .get(&observation_key(canonical_id))
            .copied()
            .unwrap_or_else(|| {
                self.mark_incomplete();
                verter_semantic::resolver_core::PathProbe::Unknown
            })
    }

    fn resolution_event_bridge_complete(&self) -> bool {
        self.complete() && self.revalidate()
    }

    fn resolution_population(&self) -> verter_semantic::resolver_core::ResolutionPopulation {
        crate::traits::WorkspaceRead::resolution_population(self.workspace)
    }

    fn capture_resolution_world(
        &self,
    ) -> Option<Arc<crate::resolution_currency::CapturedResolutionWorld>> {
        crate::traits::WorkspaceRead::capture_resolution_world(self.workspace)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        match self.realpaths.get(&observation_key(canonical_id)) {
            Some(result) => result.clone(),
            None => {
                self.mark_incomplete();
                None
            }
        }
    }

    fn read_package_manifest(&self, canonical_id: &str) -> Option<crate::types::PackageManifest> {
        match self.manifests.get(&observation_key(canonical_id)) {
            Some(result) => result.clone(),
            None => {
                self.mark_incomplete();
                None
            }
        }
    }

    fn preflight_resolution_inputs_bounded(
        &self,
        keys: &[verter_semantic::resolver_core::InputKey],
        basis: verter_semantic::resolver_core::ResolutionBasis,
    ) -> Result<
        crate::resolver::ResolutionInputReservationBatch,
        verter_semantic::resolver_core::AttemptFailure,
    > {
        crate::resolver::preflight_supported_resolution_inputs(
            keys,
            basis,
            |path| Ok((self.probe_path(path), Vec::new())),
            |path| Ok((self.realpath(path), Vec::new())),
            |manifest_path, key| match self.manifests.get(&observation_key(manifest_path)) {
                Some(value) => Ok((
                    value.is_some(),
                    value
                        .as_ref()
                        .and_then(|manifest| manifest.raw.as_ref())
                        .map_or(0, |raw| raw.len() as u64),
                    Vec::new(),
                )),
                None => {
                    self.mark_incomplete();
                    Err(
                        verter_semantic::resolver_core::AttemptFailure::InputLoadUnavailable {
                            key: Box::new(key.clone()),
                        },
                    )
                }
            },
        )
    }

    fn load_preflighted_resolution_inputs(
        &self,
        reservation: &crate::resolver::ResolutionInputReservationBatch,
    ) -> Result<
        crate::resolver::LoadedResolutionInputBatch,
        verter_semantic::resolver_core::AttemptFailure,
    > {
        crate::resolver::load_supported_resolution_inputs(
            reservation,
            |manifest_path, expected_present, _reserved_raw_bytes, key| {
                match self.manifests.get(&observation_key(manifest_path)) {
                    Some(value) if value.is_some() == expected_present => Ok(value.clone()),
                    Some(_) => Err(
                        verter_semantic::resolver_core::AttemptFailure::InputLoadIntegrity {
                            unresolved: vec![key.clone()],
                            reason: verter_semantic::resolver_core::InputLoadIntegrityReason::IncompleteBoundedCapture,
                        },
                    ),
                    None => {
                        self.mark_incomplete();
                        Err(
                            verter_semantic::resolver_core::AttemptFailure::InputLoadUnavailable {
                                key: Box::new(key.clone()),
                            },
                        )
                    }
                }
            },
        )
    }

    fn commit_loaded_resolution_inputs(&self, entries: &[crate::resolver::LoadedResolutionInput]) {
        crate::traits::WorkspaceRead::commit_loaded_resolution_inputs(self.workspace, entries);
    }

    fn classify_file(&self, canonical_id: &str) -> verter_language::FileLanguage {
        crate::traits::WorkspaceRead::classify_file(self.workspace, canonical_id)
    }

    fn is_workspace_owned(&self, canonical_id: &str) -> bool {
        let resolved = self.realpath(canonical_id);
        self.workspace
            .engine
            .is_workspace_owned(resolved.as_deref().unwrap_or(canonical_id))
    }

    fn is_package_backed(&self, canonical_id: &str) -> bool {
        let resolved = self.realpath(canonical_id);
        self.workspace
            .engine
            .is_package_backed(resolved.as_deref().unwrap_or(canonical_id))
    }

    fn content_generation(&self) -> u64 {
        self.content_generation
    }

    fn resolution_fact_generation(&self) -> u64 {
        crate::traits::WorkspaceRead::resolution_fact_generation(self.workspace)
    }

    fn last_content_transition_generation(&self, canonical_id: &str) -> u64 {
        self.workspace
            .engine
            .last_content_transition_generation(canonical_id)
    }

    fn published_root(&self) -> Option<Arc<crate::published_state::PublishedRoot>> {
        Some(Arc::clone(&self.published))
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        crate::traits::WorkspaceRead::reverse_deps_for(self.workspace, canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        crate::traits::WorkspaceRead::forward_deps_for(self.workspace, canonical_id)
    }

    fn dependency_snapshot(&self, canonical_id: &str) -> Option<crate::DependencySnapshotView> {
        crate::traits::WorkspaceRead::dependency_snapshot(self.workspace, canonical_id)
    }

    fn read_dir(&self, dir: &str) -> Result<Vec<crate::error::DirEntry>, crate::error::VfsError> {
        match self.directories.get(&observation_key(dir)) {
            #[cfg(not(target_arch = "wasm32"))]
            Some(ResolutionDirectoryValue::Entries(entries)) => Ok(entries.clone()),
            #[cfg(not(target_arch = "wasm32"))]
            Some(ResolutionDirectoryValue::NotFound) => {
                Err(crate::error::VfsError::NotFound(observation_key(dir)))
            }
            #[cfg(not(target_arch = "wasm32"))]
            Some(ResolutionDirectoryValue::NotADirectory) => Err(crate::error::VfsError::Io(
                std::io::Error::from(std::io::ErrorKind::NotADirectory),
            )),
            Some(ResolutionDirectoryValue::Unstable) | None => {
                self.mark_incomplete();
                Err(crate::error::VfsError::UnsupportedOperation(
                    "incomplete frozen resolution directory",
                ))
            }
        }
    }

    fn walk(
        &self,
        _root: &str,
        _filter_dir: &dyn Fn(&str) -> bool,
        _filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, crate::error::VfsError> {
        self.mark_incomplete();
        Err(crate::error::VfsError::UnsupportedOperation(
            "walk is not a frozen resolution observation",
        ))
    }

    fn is_dir(&self, path: &str) -> bool {
        matches!(
            self.probe_path(path),
            verter_semantic::resolver_core::PathProbe::Directory
        )
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

    fn probe_path(&self, canonical_id: &str) -> verter_semantic::resolver_core::PathProbe {
        if self.engine.overlay.read().has_overlay(canonical_id)
            || self.engine.snapshot.read().contains(canonical_id)
        {
            return verter_semantic::resolver_core::PathProbe::File;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // A complete clean directory index can answer stable absence
            // without a syscall. A positive still goes through metadata so a
            // directory is not laundered into File.
            if self.ensure_parent_dir_indexed(canonical_id) == Some(false) {
                return verter_semantic::resolver_core::PathProbe::Absent;
            }
            self.native_fs.probe_path(canonical_id)
        }
        #[cfg(target_arch = "wasm32")]
        {
            verter_semantic::resolver_core::PathProbe::Absent
        }
    }

    fn resolution_event_bridge_complete(&self) -> bool {
        false
    }

    fn take_resolution_directory_observations(&self) -> Vec<String> {
        take_resolution_directory_observations()
    }

    fn resolution_population(&self) -> verter_semantic::resolver_core::ResolutionPopulation {
        self.engine.default_resolution_population()
    }

    fn capture_resolution_world(
        &self,
    ) -> Option<Arc<crate::resolution_currency::CapturedResolutionWorld>> {
        self.engine
            .capture_published_resolution_world(self.engine.default_resolution_population())
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

    fn preflight_resolution_inputs_bounded(
        &self,
        keys: &[verter_semantic::resolver_core::InputKey],
        basis: verter_semantic::resolver_core::ResolutionBasis,
    ) -> Result<
        crate::resolver::ResolutionInputReservationBatch,
        verter_semantic::resolver_core::AttemptFailure,
    > {
        crate::resolver::preflight_supported_resolution_inputs(
            keys,
            basis,
            |path| {
                let value = if self.engine.overlay.read().has_overlay(path)
                    || self.engine.snapshot.read().contains(path)
                {
                    verter_semantic::resolver_core::PathProbe::File
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.native_fs.probe_path_live(path).map_err(|_| {
                            verter_semantic::resolver_core::AttemptFailure::TransientInputLoadFailure {
                                key: Box::new(verter_semantic::resolver_core::InputKey::PathProbe {
                                    path: path.into(),
                                }),
                            }
                        })?
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        verter_semantic::resolver_core::PathProbe::Absent
                    }
                };
                Ok((value, Vec::new()))
            },
            |path| {
                let value = if self.engine.overlay.read().has_overlay(path)
                    || self.engine.snapshot.read().contains(path)
                {
                    Some(path.to_string())
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.native_fs.realpath_live(path).map_err(|_| {
                            verter_semantic::resolver_core::AttemptFailure::TransientInputLoadFailure {
                                key: Box::new(verter_semantic::resolver_core::InputKey::RealPath {
                                    path: path.into(),
                                }),
                            }
                        })?
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        None
                    }
                };
                Ok((value, Vec::new()))
            },
            |manifest_path, key| {
                let _ = take_resolution_directory_observations();
                let in_memory = self
                    .engine
                    .overlay
                    .read()
                    .get(manifest_path)
                    .or_else(|| self.engine.snapshot.read().read(manifest_path));
                #[cfg(target_arch = "wasm32")]
                let length = {
                    let _ = key;
                    in_memory.map(|source| source.len() as u64)
                };
                #[cfg(not(target_arch = "wasm32"))]
                let length = if let Some(source) = in_memory {
                    Some(source.len() as u64)
                } else {
                    self.native_fs.file_len_live(manifest_path).map_err(|_| {
                        verter_semantic::resolver_core::AttemptFailure::TransientInputLoadFailure {
                            key: Box::new(key.clone()),
                        }
                    })?
                };
                Ok((length.is_some(), length.unwrap_or(0), Vec::new()))
            },
        )
    }

    fn load_preflighted_resolution_inputs(
        &self,
        reservation: &crate::resolver::ResolutionInputReservationBatch,
    ) -> Result<
        crate::resolver::LoadedResolutionInputBatch,
        verter_semantic::resolver_core::AttemptFailure,
    > {
        crate::resolver::load_supported_resolution_inputs(
            reservation,
            |manifest_path, expected_present, reserved_raw_bytes, key| {
                let in_memory = self
                    .engine
                    .overlay
                    .read()
                    .get(manifest_path)
                    .or_else(|| self.engine.snapshot.read().read(manifest_path));
                let source = if let Some(source) = in_memory {
                    if source.len() as u64 > reserved_raw_bytes {
                        return Err(
                            verter_semantic::resolver_core::AttemptFailure::InputLoadIntegrity {
                                unresolved: vec![key.clone()],
                                reason: verter_semantic::resolver_core::InputLoadIntegrityReason::ActualOverReservation,
                            },
                        );
                    }
                    Some(source)
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        match self
                            .native_fs
                            .read_file_bounded_live(manifest_path, reserved_raw_bytes)
                            .map_err(|_| {
                                verter_semantic::resolver_core::AttemptFailure::TransientInputLoadFailure {
                                    key: Box::new(key.clone()),
                                }
                            })? {
                            crate::native_fs::BoundedFileRead::Missing => None,
                            crate::native_fs::BoundedFileRead::Exceeded => {
                                return Err(
                                    verter_semantic::resolver_core::AttemptFailure::InputLoadIntegrity {
                                        unresolved: vec![key.clone()],
                                        reason: verter_semantic::resolver_core::InputLoadIntegrityReason::ActualOverReservation,
                                    },
                                );
                            }
                            crate::native_fs::BoundedFileRead::Complete(source) => Some(source),
                        }
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        None
                    }
                };
                if source.is_some() != expected_present {
                    return Err(
                        verter_semantic::resolver_core::AttemptFailure::InputLoadIntegrity {
                            unresolved: vec![key.clone()],
                            reason: verter_semantic::resolver_core::InputLoadIntegrityReason::IncompleteBoundedCapture,
                        },
                    );
                }
                Ok(source.map(|source| crate::package_index::parse_package_json(&source)))
            },
        )
    }

    fn commit_loaded_resolution_inputs(&self, entries: &[crate::resolver::LoadedResolutionInput]) {
        for entry in entries {
            let crate::resolver::LoadedResolutionInput::PackageManifest {
                value,
                manifest_path,
                ..
            } = entry
            else {
                continue;
            };
            if let Some(raw) = value.as_ref().and_then(|manifest| manifest.raw.as_deref()) {
                if !self.engine.overlay.read().has_overlay(manifest_path)
                    && !self.engine.snapshot.read().contains(manifest_path)
                {
                    self.engine
                        .snapshot
                        .write()
                        .inject(manifest_path.clone(), Arc::from(raw));
                }
            }
        }
        let mut cache = self.engine.package_index.write();
        for entry in entries {
            if let crate::resolver::LoadedResolutionInput::PackageManifest {
                value,
                manifest_path,
                ..
            } = entry
            {
                match value.as_ref().and_then(|manifest| manifest.raw.as_deref()) {
                    Some(raw) => {
                        let _ = cache.get_or_parse(manifest_path, raw);
                    }
                    None => cache.insert_not_found(manifest_path),
                }
            }
        }
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn resolve_import(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> Option<verter_semantic::resolver_core::ResolveResult> {
        self.engine.resolve_import_with_evidence(
            self,
            self.resolution_evidence_source(),
            importer_id,
            specifier,
            ctx,
        )
    }

    fn resolve_import_outcome(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        let Some(published) = self.load_published() else {
            return crate::resolution_currency::ResolutionOutcome::refused(
                None,
                verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance,
            );
        };
        self.resolve_import_at_published_snapshot(&published, importer_id, specifier, ctx)
    }

    fn resolve_import_outcome_with_overlay(
        &self,
        overlay: &crate::resolution_currency::ResolutionOverlaySnapshot,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        self.resolve_import_with_overlay_snapshot(overlay, importer_id, specifier, ctx)
    }

    fn resolve_import_at_published(
        &self,
        published: &Arc<crate::published_state::PublishedRoot>,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        self.resolve_import_at_published_snapshot(published, importer_id, specifier, ctx)
    }

    fn resolve_import_for_project(
        &self,
        owner: &verter_semantic::resolver_core::ProjectOwnership,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> Option<verter_semantic::resolver_core::ResolveResult> {
        self.engine
            .resolve_import_for_project(self, owner, specifier, ctx)
    }

    fn resolve_import_for_project_outcome(
        &self,
        owner: &verter_semantic::resolver_core::ProjectOwnership,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        self.engine
            .resolve_import_for_project_outcome(self, owner, specifier, ctx)
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

    fn strict_self_root_generation(&self) -> Option<u64> {
        Some(self.engine.current_strict_self_root_generation())
    }

    fn strict_self_root_authority_id(&self) -> Option<u64> {
        Some(self.engine.strict_self_root_authority_id())
    }

    fn strict_self_root_transition_active(&self) -> bool {
        self.engine.strict_self_root_transition_active()
    }

    fn resolution_fact_generation(&self) -> u64 {
        self.engine.current_resolution_fact_generation()
    }

    fn last_content_transition_generation(&self, canonical_id: &str) -> u64 {
        self.engine.last_content_transition_generation(canonical_id)
    }

    fn record_content_transition(&self, canonical_id: &str) {
        self.engine.record_content_transition(canonical_id);
    }

    fn vfs_provenance_snapshot(&self) -> crate::types::VfsProvenanceSnapshot {
        FilesystemWorkspace::vfs_provenance_snapshot(self)
    }

    fn resource_snapshot(&self) -> crate::traits::WorkspaceResourceSnapshot {
        self.engine.resource_snapshot()
    }

    fn preferred_specifier(&self, importer_id: &str, target_id: &str) -> Option<String> {
        self.engine.preferred_specifier(
            self,
            self.resolution_evidence_source(),
            importer_id,
            target_id,
        )
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
        let result = self.native_fs.read_dir(dir);
        record_resolution_directory_result(dir, &result);
        result
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
        stable_key: verter_semantic::resolver_core::ProjectStableKey,
        canonical_id: &str,
    ) -> Option<Arc<str>> {
        self.engine.read_ambient_lib(self, stable_key, canonical_id)
    }

    fn project_stable_key(
        &self,
        project_id: crate::workspace_snapshot::ProjectId,
    ) -> Option<verter_semantic::resolver_core::ProjectStableKey> {
        self.engine.project_stable_key(project_id)
    }

    fn lookup_ambient_symbol(
        &self,
        consumer_project: verter_semantic::resolver_core::ProjectStableKey,
        symbol: &str,
    ) -> Option<verter_semantic::resolver_core::AmbientSymbolHit> {
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
    fn begin_strict_self_root_transition(&self) {
        self.engine.begin_strict_self_root_transition();
    }

    fn end_strict_self_root_transition(&self) {
        self.engine.end_strict_self_root_transition();
    }

    fn publish_owner_resolution_set(
        &self,
        owner_canonical: &str,
    ) -> Option<crate::fact_cache::FactVersionRef> {
        self.engine.publish_owner_resolution_set(
            owner_canonical,
            crate::traits::WorkspaceRead::resolution_population(self),
        )
    }

    fn source_env_generation(&self) -> Option<u64> {
        Some(self.engine.current_source_env_generation())
    }

    fn reset_vfs_provenance(&self) {
        FilesystemWorkspace::reset_vfs_provenance(self);
    }

    fn record_parsed_edges(&self, canonical_id: &str, edges: &[crate::types::ParsedEdge]) {
        self.record_parsed_edges_with_frozen_evidence(
            canonical_id,
            edges,
            |reader, ledger, valid| {
                self.engine
                    .record_parsed_edges_in_operation(reader, canonical_id, edges, ledger, valid)
                    .then_some(())
            },
        );
    }

    fn record_parsed_edges_many(&self, records: &[(String, Vec<crate::types::ParsedEdge>)]) {
        if self
            .record_parsed_edges_many_with_frozen_evidence(records)
            .is_none()
        {
            for (canonical_id, edges) in records {
                self.record_parsed_edges_with_frozen_evidence(
                    canonical_id,
                    edges,
                    |reader, ledger, valid| {
                        self.engine
                            .record_parsed_edges_in_operation(
                                reader,
                                canonical_id,
                                edges,
                                ledger,
                                valid,
                            )
                            .then_some(())
                    },
                );
            }
        }
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
        self.record_parsed_edges_with_frozen_evidence(
            canonical_id,
            edges,
            |reader, ledger, valid| {
                self.engine
                    .record_parsed_edges_with_exact_resolutions_in_operation(
                        reader,
                        canonical_id,
                        edges,
                        resolutions.clone(),
                        ledger,
                        valid,
                    )
            },
        )
        .unwrap_or_default()
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
        self.engine.mutate_overlay_upsert(canonical_id, || {
            let changed = self
                .engine
                .overlay
                .write()
                .set(canonical_id.to_string(), source);
            if changed {
                self.engine.invalidate_package_manifest(canonical_id);
            }
            ((), changed)
        });
    }

    fn notify_upsert_many(&self, records: &[(String, Arc<str>)]) {
        self.engine.mutate_overlay_upsert_many(records);
    }

    fn notify_close(&self, canonical_id: &str) {
        self.engine.mutate_overlay_close(canonical_id, || {
            self.engine.invalidate_package_manifest(canonical_id);
            let changed = self.engine.overlay.write().clear(canonical_id);
            // Invalidate snapshot so next read falls through to disk,
            // picking up any saves made while the overlay was active.
            self.engine.snapshot.write().remove(canonical_id);
            ((), changed)
        });
    }

    fn notify_delete(&self, canonical_id: &str) {
        self.engine.mutate_overlay_close(canonical_id, || {
            let changed = self.engine.overlay.write().clear(canonical_id);
            ((), changed)
        });
        self.engine.mutate_content_for(
            canonical_id,
            true,
            Some(verter_semantic::resolver_core::PathProbe::Absent),
            crate::engine::BaseRealpathTransition::Known(None),
            || {
                self.engine.invalidate_package_manifest(canonical_id);
                if let Some((parent, _)) = split_parent_basename(canonical_id) {
                    self.engine.dir_index.write().mark_dirty(parent);
                }
                #[cfg(not(target_arch = "wasm32"))]
                self.native_fs.invalidate_realpath_under(canonical_id);
                self.engine.snapshot.write().remove(canonical_id);
                ((), true)
            },
        );
    }

    fn configure_resolver(&self, projects: Vec<verter_semantic::resolver_core::IdeProjectConfig>) {
        self.engine
            .set_configured_resolver_projects(Some(projects.clone()));
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
        self.engine.mutate_content_for(
            path,
            true,
            Some(verter_semantic::resolver_core::PathProbe::File),
            crate::engine::BaseRealpathTransition::Unknown,
            || {
                if let Err(error) = self.native_fs.write_file(path, content) {
                    return (Err(error), false);
                }
                self.engine.invalidate_package_manifest(path);
                mark_parent_dir_dirty(&self.engine, path);
                // Inject into snapshot so subsequent reads see the new content.
                self.engine
                    .snapshot
                    .write()
                    .inject(path.to_string(), Arc::from(content));
                (Ok(()), true)
            },
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn create_dir_all(&self, path: &str) -> Result<(), crate::error::VfsError> {
        self.engine.mutate_content_for(
            path,
            false,
            Some(verter_semantic::resolver_core::PathProbe::Directory),
            crate::engine::BaseRealpathTransition::Unknown,
            || {
                if let Err(error) = self.native_fs.create_dir_all(path) {
                    return (Err(error), false);
                }
                {
                    let mut dir_index = self.engine.dir_index.write();
                    dir_index.mark_dirty(path);
                }
                mark_parent_dir_dirty(&self.engine, path);
                (Ok(()), true)
            },
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn delete_file(&self, path: &str) -> Result<(), crate::error::VfsError> {
        self.engine.mutate_content_for(
            path,
            true,
            Some(verter_semantic::resolver_core::PathProbe::Absent),
            crate::engine::BaseRealpathTransition::Known(None),
            || {
                if let Err(error) = self.native_fs.delete_file(path) {
                    return (Err(error), false);
                }
                self.engine.invalidate_package_manifest(path);
                mark_parent_dir_dirty(&self.engine, path);
                self.engine.snapshot.write().remove(path);
                (Ok(()), true)
            },
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn delete_dir_all(&self, path: &str) -> Result<(), crate::error::VfsError> {
        self.engine.mutate_content_subtree(path, true, || {
            if let Err(error) = self.native_fs.delete_dir_all(path) {
                return (Err(error), false);
            }
            self.engine.package_index.write().invalidate_under(path);
            {
                let mut dir_index = self.engine.dir_index.write();
                dir_index.mark_dirty_under(path);
            }
            mark_parent_dir_dirty(&self.engine, path);
            self.engine.snapshot.write().remove_under(path);
            // A recursive disk delete transitions EVERY canonical under
            // `path` — including ones the snapshot cache never saw.
            (Ok(()), true)
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), crate::error::VfsError> {
        self.engine.mutate_content_for(
            dst,
            true,
            Some(verter_semantic::resolver_core::PathProbe::File),
            crate::engine::BaseRealpathTransition::Unknown,
            || {
                if let Err(error) = self.native_fs.copy_file(src, dst) {
                    return (Err(error), false);
                }
                self.engine.invalidate_package_manifest(dst);
                mark_parent_dir_dirty(&self.engine, dst);
                self.engine.snapshot.write().remove(dst);
                (Ok(()), true)
            },
        )
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
        stable_key: verter_semantic::resolver_core::ProjectStableKey,
        canonical_id: &str,
    ) -> Result<(), crate::ambient_lib::AmbientLibError> {
        self.engine.unregister_ambient_lib(stable_key, canonical_id)
    }

    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str) {
        // Route ambient deps through the dedicated
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

#[cfg(all(test, feature = "external-corpus"))]
#[cfg(not(target_arch = "wasm32"))]
#[path = "filesystem_external_corpus_tests.rs"]
mod external_corpus_tests;
