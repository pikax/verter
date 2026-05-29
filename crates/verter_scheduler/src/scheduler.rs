//! Main scheduler: per-file node architecture with async staging.
//!
//! The [`Scheduler`] is the central coordination point. Callers submit
//! requests via [`submit_request`](Scheduler::submit_request), which returns
//! a [`CompletionHandle`] that resolves when the target stage is reached.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use dashmap::DashMap;
use parking_lot::Mutex;

use crate::driver::{Submission, SubmissionInbox};
use crate::edges::EdgeManager;
use crate::executor::{DefaultExecutor, StageExecutor};
use crate::job::{
    completion_pair, CompletionHandle, CompletionSender, CompletionState, RequestResult,
};
use crate::node::{AnalysisSnapshot, ArtifactSnapshot, FileNode, SourceSnapshot};
use crate::overlay::OverlayMap;
use crate::queue::{AgingConfig, JobIndex, JobKey, QueueEntry};
use crate::source_loader::{FileKind as SourceFileKind, SourceLoader};
use crate::stage::{Priority, TargetStage, TaskKind};

/// Contention instrumentation counters for the scheduler. Owned by
/// [`Scheduler`]; surfaced via
/// [`Scheduler::counters`](Scheduler::counters) so host-level provenance
/// snapshots (verter_session's `MetaProvenanceSnapshot`) can aggregate
/// them without introducing a cross-crate dependency on `MetaProvenance`.
///
/// All fields are plain `AtomicU64`; reads are `Relaxed`.
#[derive(Default, Debug)]
pub struct SchedulerCounters {
    /// Submissions entering the inbox via `submit_request`.
    pub submit_count: AtomicU64,
    /// Peak inbox depth observed (monotonic increase via `fetch_max`).
    pub inbox_depth_max: AtomicU64,
}

/// Configuration for the scheduler.
#[derive(Clone, Debug)]
pub struct SchedulerConfig {
    /// Number of CPU pool threads (default: num_cpus).
    pub cpu_threads: usize,
    /// Number of I/O pool threads (default: 4).
    pub io_threads: usize,
    /// Aging thresholds for priority promotion.
    pub aging: AgingConfig,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            cpu_threads: num_cpus(),
            #[cfg(target_arch = "wasm32")]
            cpu_threads: 1,
            io_threads: 4,
            aging: AgingConfig::default(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

#[cfg(not(target_arch = "wasm32"))]
fn should_join_driver_thread(
    handle_thread_id: std::thread::ThreadId,
    current_thread_id: std::thread::ThreadId,
) -> bool {
    handle_thread_id != current_thread_id
}

/// A request to the scheduler.
pub struct Request {
    pub file_id: String,
    pub target: TargetStage,
    pub priority: Priority,
    pub source: Option<Arc<str>>,
    pub file_kind: Option<SourceFileKind>,
    /// Optional session-side request context. When present, the
    /// scheduler stores the winner's context on the dedup group,
    /// fires `on_dedup_joiner` callbacks when this request joins an
    /// existing group, and installs the context into worker TLS
    /// around each stage closure so `current_request_id()` returns a
    /// meaningful value while the job runs.
    pub request_context: Option<crate::request_context::OpaqueRequestContext>,
}

/// Batch submission handle.
///
/// Produced by [`Scheduler::submit_batch`]; drained via
/// [`Scheduler::wait_batch`]. Callers submit N independent requests
/// before any waits; the scheduler fans them out onto its Rayon pool.
/// The handle carries one [`CompletionHandle`] per submitted request
/// in submission order so `wait_batch` can surface results in the
/// same order.
pub struct BatchHandle {
    pub(crate) handles: Vec<CompletionHandle<RequestResult>>,
}

/// The main scheduler.
///
/// Manages per-file nodes, a priority queue, and a driver thread that
/// processes submissions and dispatches work to CPU/IO pools.
pub struct Scheduler {
    /// Per-file nodes (concurrent access via DashMap).
    pub(crate) nodes: DashMap<String, Arc<FileNode>>,
    /// Edge manager (reverse index + blocker registry).
    pub(crate) edges: EdgeManager,
    /// Scheduler-owned job index (protected by Mutex, driver-only access).
    pub(crate) job_index: Mutex<JobIndex>,
    /// Lock-free inbox for submissions.
    pub(crate) inbox: SubmissionInbox,
    /// Current resolver snapshot (atomically swappable).
    pub(crate) overlay: Arc<OverlayMap>,
    /// Source loader for file reads.
    pub(crate) source_loader: Arc<dyn SourceLoader>,
    /// Stage executor — provides the actual parse/analysis/compile logic.
    pub(crate) executor: Arc<dyn StageExecutor>,
    /// Configuration (read at construction for pool sizing).
    pub(crate) config: SchedulerConfig,
    /// Dedicated rayon thread pool for CPU-bound work (parse, analysis, compile).
    /// Separate from the global rayon pool so per-scheduler sizing takes effect.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cpu_pool: rayon::ThreadPool,
    /// Bounded I/O pool for file reads. Separate from CPU pool so blocking
    /// disk reads don't starve parse/analyze work.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) io_pool: crate::pool::IoPool,
    /// Tombstones for removed files. Value is a monotonic removal counter.
    /// A `source: Some(...)` submission only clears the tombstone if it was
    /// submitted AFTER the removal (checked via an atomic counter on the
    /// scheduler that is bumped on each removal and stamped on each submission).
    pub tombstones: DashMap<String, u64>,
    /// Per-file generation floor. After remove, the floor is set to the
    /// removed node's generation. A re-added node starts at floor+1, so
    /// stale completions from the old incarnation never match.
    pub generation_floors: DashMap<String, u64>,
    /// Deferred blocker IDs for files whose node was at generation 0 when
    /// `register_resolved_deps` was called. Replayed when the node advances
    /// past generation 0 during Source stage completion.
    pub deferred_blocker_ids: DashMap<String, Vec<String>>,
    /// Monotonic counter bumped on every `remove()`. Submissions carry the
    /// counter value at submission time so the driver can reject pre-remove
    /// submissions even if they carry a source buffer.
    pub(crate) removal_epoch: AtomicU64,
    /// Shutdown flag.
    pub(crate) shutdown: AtomicBool,
    /// Driver thread handle (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) driver_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Contention instrumentation counters surfaced through
    /// [`Self::counters`].
    pub(crate) counters: SchedulerCounters,
}

impl Scheduler {
    /// Create a new scheduler with a driver thread (native).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(config: SchedulerConfig, source_loader: Arc<dyn SourceLoader>) -> Arc<Self> {
        Self::with_executor(config, source_loader, Arc::new(DefaultExecutor))
    }

    /// Create a new scheduler with a custom stage executor and driver thread (native).
    ///
    /// The driver thread holds a `Weak<Scheduler>`, so dropping the last caller
    /// `Arc` allows `Drop` to run (sets shutdown, joins driver, drains pending).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_executor(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        executor: Arc<dyn StageExecutor>,
    ) -> Arc<Self> {
        let cpu_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(config.cpu_threads)
            .thread_name(|i| format!("verter-cpu-{i}"))
            .build()
            .expect("failed to build rayon CPU pool");
        let io_pool = crate::pool::IoPool::new(config.io_threads);

        let scheduler = Arc::new(Self {
            nodes: DashMap::new(),
            edges: EdgeManager::new(),
            job_index: Mutex::new(JobIndex::new(config.aging.clone())),
            inbox: SubmissionInbox::new(),
            overlay: Arc::new(OverlayMap::new()),
            source_loader,
            executor,
            config,
            cpu_pool,
            io_pool,
            tombstones: DashMap::new(),
            generation_floors: DashMap::new(),
            deferred_blocker_ids: DashMap::new(),
            removal_epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            driver_handle: Mutex::new(None),
            counters: SchedulerCounters::default(),
        });

        // Driver holds Weak so it doesn't prevent Drop.
        // Also clones the receiver so it can block on it without upgrading.
        let weak = Arc::downgrade(&scheduler);
        let receiver = scheduler.inbox.receiver.clone();
        let handle = std::thread::Builder::new()
            .name("verter-scheduler".to_string())
            .spawn(move || {
                Self::driver_loop_native(weak, receiver);
            })
            .expect("failed to spawn scheduler driver");

        *scheduler.driver_handle.lock() = Some(handle);
        scheduler
    }

    /// Create a new scheduler for sync/test use (no driver thread).
    /// Use `drive_one()` / `drive_all()` to process manually.
    pub fn new_sync(config: SchedulerConfig, source_loader: Arc<dyn SourceLoader>) -> Arc<Self> {
        Self::new_sync_with_executor(config, source_loader, Arc::new(DefaultExecutor))
    }

    /// Create a sync scheduler with a custom stage executor.
    pub fn new_sync_with_executor(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        executor: Arc<dyn StageExecutor>,
    ) -> Arc<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        let cpu_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(config.cpu_threads)
            .thread_name(|i| format!("verter-cpu-{i}"))
            .build()
            .expect("failed to build rayon CPU pool");
        #[cfg(not(target_arch = "wasm32"))]
        let io_pool = crate::pool::IoPool::new(config.io_threads);

        Arc::new(Self {
            nodes: DashMap::new(),
            edges: EdgeManager::new(),
            job_index: Mutex::new(JobIndex::new(config.aging.clone())),
            inbox: SubmissionInbox::new(),
            overlay: Arc::new(OverlayMap::new()),
            source_loader,
            executor,
            config,
            #[cfg(not(target_arch = "wasm32"))]
            cpu_pool,
            #[cfg(not(target_arch = "wasm32"))]
            io_pool,
            tombstones: DashMap::new(),
            generation_floors: DashMap::new(),
            deferred_blocker_ids: DashMap::new(),
            removal_epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            #[cfg(not(target_arch = "wasm32"))]
            driver_handle: Mutex::new(None),
            counters: SchedulerCounters::default(),
        })
    }

    // ── Request Submission ──

    /// Submit a request. Returns a handle that resolves when the target stage is reached.
    pub fn submit_request(&self, request: Request) -> CompletionHandle<RequestResult> {
        let (handle, sender) = completion_pair();
        let submission = Submission::NewRequest {
            file_id: request.file_id,
            target: request.target,
            priority: request.priority,
            source: request.source,
            file_kind: request.file_kind,
            sender: sender.clone(),
            submitted_epoch: self.removal_epoch.load(Ordering::Acquire),
            request_context: request.request_context,
        };
        match self.inbox.sender.send(submission) {
            Ok(()) => {
                // Record the submission + update the peak inbox depth
                // observed (contention instrumentation).
                self.counters.submit_count.fetch_add(1, Ordering::Relaxed);
                let depth = self.inbox.sender.len() as u64;
                let prev_max = self.counters.inbox_depth_max.load(Ordering::Relaxed);
                if depth > prev_max {
                    let _ = self
                        .counters
                        .inbox_depth_max
                        .fetch_max(depth, Ordering::Relaxed);
                }
                handle
            }
            Err(_) => {
                // Inbox closed (scheduler shutting down)
                sender.send(CompletionState::Shutdown);
                handle
            }
        }
    }

    /// Batch submit. Submits N requests without individual waits so
    /// the scheduler can coalesce drain and fan-out onto its Rayon
    /// pool. Returns a [`BatchHandle`] that carries one completion
    /// handle per request in submission order.
    pub fn submit_batch(&self, requests: Vec<Request>) -> BatchHandle {
        let mut handles = Vec::with_capacity(requests.len());
        for request in requests {
            handles.push(self.submit_request(request));
        }
        BatchHandle { handles }
    }

    /// Wait for a submitted batch to complete. Drains each
    /// [`CompletionHandle`] in submission order. The caller receives
    /// per-request results as they arrive; the scheduler fans out the
    /// work across its configured CPU pool.
    ///
    /// Uses `wait_or_drive` so both native (driver thread) and
    /// single-threaded callers share the same completion semantics.
    pub fn wait_batch(
        self: &Arc<Self>,
        batch: BatchHandle,
    ) -> Vec<crate::job::CompletionState<RequestResult>> {
        batch
            .handles
            .iter()
            .map(|handle| self.wait_or_drive(handle))
            .collect()
    }

    /// Dispatch a batch of non-staged scheduler jobs (per-job-kind
    /// work that does not flow through the
    /// Source → Analysis → Artifact lifecycle). Each job runs as a
    /// closure on the scheduler's CPU pool and the function returns
    /// per-job results in submission order.
    ///
    /// Used by `MetaSession::get_component_meta_batch` to fan out N
    /// independent component-meta queries onto the Rayon pool when
    /// the session is in [`crate::scheduler::SchedulerConfig`]'s
    /// Batch execution mode. Interactive callers continue to use the
    /// single-request synchronous path through
    /// `MetaSession::get_component_meta`.
    ///
    /// Counter side effect: increments `counters.submit_count` by
    /// **exactly one per batch dispatch**, regardless of `jobs.len()`.
    /// One batch dispatch is one scheduler submission; the N individual
    /// jobs share that submission's context. Callers can rely on
    /// `submit_count` being O(1) per `dispatch_meta_jobs` call (zero
    /// when `jobs` is empty).
    ///
    /// On WASM (single-threaded), runs sequentially on the calling
    /// thread — same observable behaviour, no Rayon fan-out.
    pub fn dispatch_meta_jobs<F, R>(
        self: &Arc<Self>,
        jobs: Vec<crate::stage::SchedulerJobKind>,
        executor: F,
    ) -> Vec<R>
    where
        F: Fn(&crate::stage::SchedulerJobKind) -> R + Sync + Send,
        R: Send,
    {
        if jobs.is_empty() {
            return Vec::new();
        }
        // One scheduler submission per batch dispatch, independent of
        // `jobs.len()` — see the batch-API verify-bullet:
        // `submit_count` increases by exactly 1 per batch.
        self.counters
            .submit_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            self.cpu_pool
                .install(|| jobs.par_iter().map(&executor).collect())
        }
        #[cfg(target_arch = "wasm32")]
        {
            jobs.iter().map(&executor).collect()
        }
    }

    /// Access the scheduler's contention instrumentation counters.
    pub fn counters(&self) -> &SchedulerCounters {
        &self.counters
    }

    // ── Sync Fast-Path Reads ──

    /// Get current source snapshot if generation-coherent.
    pub fn try_get_source(&self, id: &str) -> Option<Arc<SourceSnapshot>> {
        self.nodes.get(id)?.current_source()
    }

    /// Get current analysis snapshot if generation-coherent.
    pub fn try_get_analysis(&self, id: &str) -> Option<Arc<AnalysisSnapshot>> {
        self.nodes.get(id)?.current_analysis()
    }

    /// Get current artifact snapshot if generation-coherent.
    pub fn try_get_artifact(&self, id: &str, profile_hash: u64) -> Option<Arc<ArtifactSnapshot>> {
        self.nodes.get(id)?.current_artifact(profile_hash)
    }

    /// Get last-known-good artifact regardless of generation.
    pub fn try_get_last_known_good(
        &self,
        id: &str,
        profile_hash: u64,
    ) -> Option<Arc<ArtifactSnapshot>> {
        self.nodes.get(id)?.last_known_good_artifact(profile_hash)
    }

    /// Check if a node exists for a file.
    pub fn has_node(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    /// List all node IDs.
    pub fn node_ids(&self) -> Vec<String> {
        self.nodes.iter().map(|e| e.key().clone()).collect()
    }

    /// Full reset: stop the driver, drain inbox, remove all nodes, clear
    /// all state. Call `restart_driver()` after to resume processing.
    ///
    /// Provides a true quiesce barrier — no concurrent processing during
    /// the clear phase because the driver thread is joined first.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn reset(&self) {
        // 1. Stop the driver thread.
        self.shutdown.store(true, Ordering::Release);
        let _ = self.inbox.sender.send(Submission::Wake);
        if let Some(handle) = self.driver_handle.lock().take() {
            if should_join_driver_thread(handle.thread().id(), std::thread::current().id()) {
                let _ = handle.join();
            }
        }

        // 2. Drain inbox — driver is stopped, so this is exclusive.
        while let Ok(submission) = self.inbox.receiver.try_recv() {
            if let Submission::NewRequest { sender, .. } = submission {
                sender.send(CompletionState::Shutdown);
            }
        }

        // 3. Remove all nodes, recording generation floors so re-added
        //    files start above any prior incarnation's generation.
        let ids: Vec<String> = self.nodes.iter().map(|e| e.key().clone()).collect();
        for id in &ids {
            if let Some((_, node)) = self.nodes.remove(id) {
                let gen = node.generation();
                self.generation_floors.insert(id.clone(), gen);
                node.pending_requests.signal_shutdown();
            }
        }

        // 4. Clear state except generation_floors (preserved across resets
        //    to prevent cross-incarnation stale completions from matching).
        self.edges.reverse_index.inner.clear();
        self.edges.forward_deps.clear();
        self.edges.blockers.pending.clear();
        self.edges.blockers.waiters.clear();
        self.job_index.lock().clear();
        self.tombstones.clear();
        // generation_floors intentionally NOT cleared — stale worker completions
        // from the old incarnation can still arrive after restart, and floors
        // ensure re-added files start at a generation above any prior use.
        self.deferred_blocker_ids.clear();

        // 5. Drain inbox again — catch any completions that workers sent
        //    between step 2 and now. These are harmless (nodes removed in
        //    step 3, so handle_stage_complete/handle_blocker_resolved will
        //    no-op), but draining keeps the channel clean.
        while let Ok(submission) = self.inbox.receiver.try_recv() {
            if let Submission::NewRequest { sender, .. } = submission {
                sender.send(CompletionState::Shutdown);
            }
        }

        // 6. Unset shutdown so the next driver can run.
        self.shutdown.store(false, Ordering::Release);
    }

    /// Restart the driver thread after a `reset()`. Must be called on
    /// `Arc<Self>` because the driver needs a `Weak` reference.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn restart_driver(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let receiver = self.inbox.receiver.clone();
        let handle = std::thread::Builder::new()
            .name("verter-scheduler".to_string())
            .spawn(move || {
                Self::driver_loop_native(weak, receiver);
            })
            .expect("failed to restart scheduler driver");
        *self.driver_handle.lock() = Some(handle);
    }

    /// Drain and discard all pending inbox submissions. Used by `close()`
    /// to quiesce the scheduler before clearing state, preventing
    /// already-queued submissions from recreating removed nodes.
    pub fn quiesce(&self) {
        while let Ok(submission) = self.inbox.receiver.try_recv() {
            // Signal shutdown to any senders in discarded submissions
            // so their CompletionHandles don't hang.
            if let Submission::NewRequest { sender, .. } = submission {
                sender.send(CompletionState::Shutdown);
            }
        }
    }

    // ── Edge Management ──

    /// Register exact resolutions for a file (from bundler/LSP `set_import_dependencies`).
    ///
    /// Updates the scheduler's forward/reverse edges with the newly resolved
    /// dep IDs and, for any deps that match macro_type_deps, registers blockers
    /// if those deps haven't reached Analysis yet. Also auto-ingests deps
    /// not yet in the scheduler.
    pub fn register_resolved_deps(
        &self,
        file_id: &str,
        resolved_dep_ids: Vec<String>,
        blocker_dep_ids: Vec<String>,
    ) {
        // Ensure the node exists.
        self.nodes
            .entry(file_id.to_string())
            .or_insert_with(|| self.create_node(file_id, None));

        // Update forward/reverse edges.
        let new_deps: std::collections::BTreeSet<String> = resolved_dep_ids.into_iter().collect();
        self.edges.record_forward_deps(file_id, new_deps);

        // Store blocker IDs for replay on the next Source completion.
        if !blocker_dep_ids.is_empty() {
            self.deferred_blocker_ids
                .insert(file_id.to_string(), blocker_dep_ids.clone());
        } else {
            self.deferred_blocker_ids.remove(file_id);
            return;
        }

        // If Source has already completed for the current generation, register
        // blockers immediately. Otherwise they'll be replayed on the next
        // Source completion.
        let node = match self.nodes.get(file_id) {
            Some(n) => n.clone(),
            None => return,
        };
        let generation = node.generation();
        if generation == 0 || node.current_source().is_none() {
            return; // Source hasn't committed yet — deferred replay will handle it
        }

        let inherited_priority = node
            .pending_requests
            .highest_priority_for_generation(generation)
            .unwrap_or(Priority::Background);

        let mut blockers = Vec::new();
        for dep_id in &blocker_dep_ids {
            if self.tombstones.contains_key(dep_id) {
                continue;
            }
            if !self.nodes.contains_key(dep_id) {
                let dep_node = self.create_node(dep_id, None);
                dep_node.bump_generation();
                self.nodes.insert(dep_id.clone(), dep_node);
                let _ = self.inbox.sender.send(Submission::NewRequest {
                    file_id: dep_id.clone(),
                    target: TargetStage::Analysis,
                    priority: std::cmp::min(inherited_priority, Priority::Interactive),
                    source: None,
                    file_kind: None,
                    request_context: None,
                    sender: {
                        let (_, s) = completion_pair::<RequestResult>();
                        s
                    },
                    submitted_epoch: self.removal_epoch.load(Ordering::Acquire),
                });
            }
            let needs_blocker = self
                .nodes
                .get(dep_id)
                .map(|n| n.current_analysis().is_none())
                .unwrap_or(true);
            if needs_blocker {
                blockers.push(crate::edges::BlockerRef {
                    file_id: dep_id.clone(),
                    required_stage: TaskKind::Analysis,
                });
            }
        }
        if !blockers.is_empty() {
            self.edges.blockers.register(
                file_id,
                generation,
                TaskKind::Analysis,
                blockers,
                inherited_priority,
            );
        }
    }

    /// Create a FileNode for a file, respecting the generation floor
    /// from prior incarnations so stale completions never match.
    fn create_node(&self, file_id: &str, file_kind: Option<SourceFileKind>) -> Arc<FileNode> {
        let kind = file_kind.unwrap_or_else(|| self.source_loader.classify(file_id));
        let node = Arc::new(FileNode::new(
            file_id.to_string(),
            crate::node::FileKind::from_source_loader_kind(kind),
        ));
        // Set generation above any prior incarnation's floor.
        if let Some(floor) = self.generation_floors.get(file_id) {
            let floor_val = *floor;
            // Bump generation to floor+1 so the new incarnation starts
            // above all generations the old incarnation ever used.
            while node.generation() <= floor_val {
                node.bump_generation();
            }
        }
        node
    }

    /// Get the scheduler configuration.
    pub fn config(&self) -> &SchedulerConfig {
        &self.config
    }

    /// Commit an externally-produced artifact snapshot.
    ///
    /// Called by the host after `compile_entry()` succeeds. The scheduler
    /// stores the result and signals any pending Artifact request handles.
    /// This allows the host to drive compilation through its existing
    /// pipeline while the scheduler tracks generation coherence.
    pub fn commit_artifact(&self, file_id: &str, profile_hash: u64, snapshot: ArtifactSnapshot) {
        if let Some(node) = self.nodes.get(file_id) {
            let generation = snapshot.generation;
            // Full coherence check: node generation, Source, AND Analysis
            // must all match. Without this, an external compile can publish
            // an artifact before the scheduler's own pipeline has committed
            // the prerequisite stages.
            if node.generation() != generation {
                return;
            }
            if node.current_source().is_none() || node.current_analysis().is_none() {
                return;
            }
            let snap = Arc::new(snapshot);
            node.artifacts.insert(profile_hash, Arc::clone(&snap));
            let result = RequestResult::Artifact(snap);
            node.pending_requests.signal_stage_complete(
                generation,
                &TaskKind::Artifact { profile_hash },
                &result,
            );
        }
    }

    /// Evict the artifact snapshot for `(file_id, profile_hash)` only
    /// when the stored snapshot is no newer than `max_generation`.
    ///
    /// Called by the host when a compile path refuses cache admission
    /// (e.g. an overflowed fact signature) and any prior artifact for
    /// the same `(canonical, profile)` produced at or before the
    /// caller's start-of-compile generation must not remain observable
    /// via `try_get_artifact`. The symmetric counterpart to
    /// [`commit_artifact`](Self::commit_artifact): commit publishes the
    /// snapshot, this evicts it under the generation gate.
    ///
    /// Generation gate. The slow refused compile that started at
    /// generation `N` may reach this call AFTER a fresh successful
    /// compile at generation `N+k` has landed a newer artifact via
    /// `commit_artifact`. Unconditionally removing would clobber the
    /// newer artifact (since `commit_artifact` rejects stale publishes
    /// via the node generation check, the inverse asymmetry would be a
    /// silent data race). The caller passes its captured compile-start
    /// generation as `max_generation`; the eviction proceeds only when
    /// the stored snapshot's `generation <= max_generation`.
    ///
    /// No-op when the node or the per-profile slot is absent, OR when
    /// the stored snapshot is newer than `max_generation`. Does NOT
    /// touch generation, source, or analysis state — only the
    /// `(profile_hash → snapshot)` entry on the artifact map.
    pub fn remove_artifact_if_not_newer_than(
        &self,
        file_id: &str,
        profile_hash: u64,
        max_generation: u64,
    ) {
        if let Some(node) = self.nodes.get(file_id) {
            // Race-free remove-if: `DashMap::remove_if` runs the
            // predicate under the per-shard lock so a concurrent
            // `commit_artifact` cannot land a newer snapshot between
            // the read and the remove.
            node.artifacts
                .remove_if(&profile_hash, |_, snap| snap.generation <= max_generation);
        }
    }

    /// Get the shared overlay map.
    pub fn overlay(&self) -> &Arc<OverlayMap> {
        &self.overlay
    }

    // ── Lifecycle ──

    /// Invalidate a file (bump generation, supersede pending requests).
    pub fn invalidate(&self, id: &str) {
        if let Some(node) = self.nodes.get(id) {
            let new_gen = node.bump_generation();
            node.pending_requests.supersede_old_generations(new_gen);
        }
    }

    /// Remove a file from the scheduler.
    ///
    /// Signals shutdown to pending request handles, removes the node,
    /// cleans up forward/reverse edges, and unblocks any dependents that
    /// were waiting on this file (since the blocker can never resolve).
    pub fn remove(&self, id: &str) {
        // Bump epoch and tombstone with the new value. Any submission stamped
        // with an earlier epoch is rejected as pre-remove.
        let epoch = self.removal_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.tombstones.insert(id.to_string(), epoch);

        // Cancel all queued jobs and clear deferred state for this file.
        self.job_index.lock().cancel_file(id);
        self.deferred_blocker_ids.remove(id);

        if let Some((_, node)) = self.nodes.remove(id) {
            node.pending_requests.signal_shutdown();
            let gen = node.generation();
            // Record floor so a re-added node starts above this generation.
            self.generation_floors.insert(id.to_string(), gen);
            self.edges.remove_file(id);
            self.edges.blockers.remove_file_as_blocked(id, gen);

            // Unblock dependents that were waiting on this file as a blocker.
            // Since the file is deleted, its Analysis can never complete, so
            // dependents are released and their artifacts can proceed (they'll
            // get compile errors if they actually need the deleted type).
            let stranded = self.edges.blockers.remove_file_as_blocker(id);
            for job in stranded {
                if let Some(dep_node) = self.nodes.get(&job.file_id) {
                    self.enqueue_pending_artifacts(&job.file_id, job.generation, &dep_node);
                }
            }
        }
    }

    /// Close a file: clear overlay + pending_source, keep node alive.
    pub fn close_file(&self, id: &str) {
        self.overlay.clear(id);
        if let Some(node) = self.nodes.get(id) {
            let new_gen = node.bump_generation();
            node.pending_source.store(Arc::new(None));
            node.pending_requests.supersede_old_generations(new_gen);

            // Enqueue a Source job at Background priority to reload from disk
            let _ = self.inbox.sender.send(Submission::NewRequest {
                file_id: id.to_string(),
                target: TargetStage::Analysis,
                priority: Priority::Background,
                source: None,
                file_kind: None,
                submitted_epoch: self.removal_epoch.load(Ordering::Acquire),
                request_context: None,
                sender: {
                    let (_, sender) = completion_pair::<RequestResult>();
                    sender
                },
            });
        }
    }

    // ── Driver ──

    /// Driver loop (native). Holds `Weak<Scheduler>` — exits when the last
    /// external Arc is dropped or the shutdown flag is set.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn driver_loop_native(
        weak: std::sync::Weak<Scheduler>,
        receiver: crossbeam_channel::Receiver<Submission>,
    ) {
        let aging_interval = std::time::Duration::from_secs(5);

        loop {
            // Upgrade Weak to Arc — if this fails, the scheduler was dropped.
            let scheduler = match weak.upgrade() {
                Some(s) => s,
                None => break,
            };

            if scheduler.shutdown.load(Ordering::Acquire) {
                // Final drain under the strong ref before exiting.
                scheduler.drain_inbox();
                break;
            }

            // Step 1: Drain all inbox submissions
            scheduler.drain_inbox();

            // Step 2: Dispatch all ready work
            scheduler.dispatch_ready_work();

            // Drop the strong ref before blocking so the caller's Drop can run.
            drop(scheduler);

            // Step 3: Wait for next submission or aging timer.
            // We block on the cloned receiver — no Arc needed.
            match receiver.recv_timeout(aging_interval) {
                Ok(submission) => {
                    // Need the scheduler to process this submission.
                    if let Some(scheduler) = weak.upgrade() {
                        scheduler.process_submission(submission);
                    }
                    // Else: scheduler dropped during recv — loop will exit on next upgrade
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }
    }

    /// Drain all available submissions from the inbox.
    fn drain_inbox(&self) {
        while let Ok(submission) = self.inbox.receiver.try_recv() {
            self.process_submission(submission);
        }
    }

    /// Process a single submission.
    fn process_submission(&self, submission: Submission) {
        match submission {
            Submission::Wake => {}
            Submission::NewRequest {
                file_id,
                target,
                priority,
                source,
                file_kind,
                sender,
                submitted_epoch,
                request_context,
            } => {
                self.handle_new_request(
                    file_id,
                    target,
                    priority,
                    source,
                    file_kind,
                    sender,
                    submitted_epoch,
                    request_context,
                );
            }
            Submission::StageComplete {
                file_id,
                generation,
                task_kind,
            } => {
                self.handle_stage_complete(&file_id, generation, task_kind);
            }
            Submission::BlockerResolved {
                file_id,
                generation,
                completed_stage,
            } => {
                self.handle_blocker_resolved(&file_id, generation, &completed_stage);
            }
        }
    }

    /// Handle a new request submission.
    #[allow(clippy::too_many_arguments)]
    fn handle_new_request(
        &self,
        file_id: String,
        target: TargetStage,
        priority: Priority,
        source: Option<Arc<str>>,
        file_kind: Option<SourceFileKind>,
        sender: CompletionSender<RequestResult>,
        submitted_epoch: u64,
        request_context: Option<crate::request_context::OpaqueRequestContext>,
    ) {
        // Check tombstone. A submission is stale if it was submitted before
        // the removal (submitted_epoch < tombstone_epoch). Only genuinely
        // post-remove submissions (submitted_epoch >= tombstone_epoch AND
        // carrying source) can clear the tombstone.
        if let Some(tombstone_ref) = self.tombstones.get(&file_id) {
            let tombstone_epoch = *tombstone_ref;
            drop(tombstone_ref);
            if submitted_epoch < tombstone_epoch {
                // Submitted before the removal — always stale, even with source.
                sender.send(CompletionState::Failed(
                    crate::job::SchedulerError::FileNotFound {
                        file_id: file_id.clone(),
                    },
                ));
                return;
            }
            // Submitted at or after the removal epoch.
            if source.is_some() {
                // Genuine re-add: clear tombstone and proceed.
                self.tombstones.remove(&file_id);
            } else {
                // Source: None after removal — stale (e.g. close_file reload
                // for a file that was subsequently deleted).
                sender.send(CompletionState::Failed(
                    crate::job::SchedulerError::FileNotFound {
                        file_id: file_id.clone(),
                    },
                ));
                return;
            }
        }

        // Ensure node exists
        let node = self
            .nodes
            .entry(file_id.clone())
            .or_insert_with(|| self.create_node(&file_id, file_kind))
            .clone();

        let generation = if source.is_some() {
            // Source provided: bump generation
            let gen = node.bump_generation();
            // Store source in overlay for SourceLoader access
            if let Some(ref src) = source {
                self.overlay.set(file_id.clone(), Arc::clone(src));
            }
            // Store in pending_source for the Source job
            node.pending_source
                .store(Arc::new(source.map(|s| (gen, s))));
            // Supersede old generation requests
            node.pending_requests.supersede_old_generations(gen);
            gen
        } else {
            let gen = node.generation();
            if gen == 0 {
                // Node was just created, needs a Source job
                node.bump_generation()
            } else {
                gen
            }
        };

        // Check if target is already satisfied
        let already_satisfied = match &target {
            TargetStage::Source => node.current_source().is_some(),
            TargetStage::Analysis => node.current_analysis().is_some(),
            TargetStage::Artifact { profile_hash } => {
                node.current_artifact(*profile_hash).is_some()
            }
        };

        if already_satisfied {
            // Signal immediately
            let result = match &target {
                TargetStage::Source => RequestResult::Source(node.current_source().unwrap()),
                TargetStage::Analysis => RequestResult::Analysis(node.current_analysis().unwrap()),
                TargetStage::Artifact { profile_hash } => {
                    RequestResult::Artifact(node.current_artifact(*profile_hash).unwrap())
                }
            };
            sender.send(CompletionState::Ready(result));
            return;
        }

        // Register pending request. The dedup hook fires inside `register`
        // when this request joins an existing winner's group — the winner's
        // stored context is called back via `on_dedup_joiner`.
        let canonical_id: Arc<str> = Arc::from(file_id.as_str());
        let upgrade = node.pending_requests.register(
            generation,
            target.clone(),
            priority,
            sender,
            request_context,
            &canonical_id,
        );

        // Determine what job to enqueue
        let first_missing = if node.current_source().is_none() {
            TaskKind::Source
        } else if node.current_analysis().is_none() {
            TaskKind::Analysis
        } else {
            target.required_task_kind()
        };

        // Apply priority upgrade if dedup group returned one
        let effective_priority = if let Some(upgraded) = upgrade {
            upgraded
        } else {
            priority
        };

        // If the next job is an Artifact but there are outstanding blockers,
        // do NOT enqueue — the request is registered in pending_requests and
        // handle_blocker_resolved will enqueue it when blockers clear.
        if matches!(first_missing, TaskKind::Artifact { .. })
            && self
                .edges
                .blockers
                .has_pending_blockers(&file_id, generation)
        {
            // Upgrade blocker priority if the new request is higher priority.
            if matches!(first_missing, TaskKind::Artifact { .. }) {
                self.edges.blockers.upgrade_priority(
                    &file_id,
                    generation,
                    TaskKind::Analysis,
                    effective_priority,
                );
            }
            return;
        }

        // Insert job into queue
        let mut job_index = self.job_index.lock();
        job_index.insert(QueueEntry::new(
            JobKey {
                file_id: file_id.clone(),
                generation,
                task_kind: first_missing,
            },
            effective_priority,
            Instant::now(),
            None,
        ));
    }

    /// Handle a stage completion.
    fn handle_stage_complete(&self, file_id: &str, generation: u64, task_kind: TaskKind) {
        let node = match self.nodes.get(file_id) {
            Some(n) => n.clone(),
            None => return,
        };

        if node.generation() != generation {
            return;
        }

        // Inherit the highest priority among pending requests so Critical
        // hover/completion work stays Critical through all transitions.
        let inherited_priority = node
            .pending_requests
            .highest_priority_for_generation(generation)
            .unwrap_or(Priority::Background);

        match task_kind {
            TaskKind::Source => {
                // Extract dependencies from the committed source snapshot.
                // The executor returns forward deps and blocker IDs based on
                // the host-specific parse data (imports, macro type deps, etc.).
                if let Some(source) = node.current_source() {
                    let deps = self.executor.extract_deps(file_id, &source);

                    // Merge extract_deps output with any exact-resolved bare deps
                    // already recorded by register_resolved_deps. This ensures
                    // bare/aliased edges aren't overwritten by the relative-only
                    // extract_deps output.
                    let mut new_deps = self.edges.get_forward_deps(file_id);
                    new_deps.extend(deps.forward_deps);
                    self.edges.record_forward_deps(file_id, new_deps);

                    // Merge any deferred bare/aliased blocker IDs that were
                    // recorded by register_resolved_deps while the node was at gen 0.
                    let mut all_blocker_ids = deps.blocker_ids;
                    if let Some((_, deferred)) = self.deferred_blocker_ids.remove(file_id) {
                        all_blocker_ids.extend(deferred);
                    }

                    // Register blockers for deps that haven't reached Analysis yet.
                    // Auto-ingest: enqueue Source jobs for deps not yet in the scheduler.
                    if !all_blocker_ids.is_empty() {
                        let mut blockers = Vec::new();
                        for dep_id in &all_blocker_ids {
                            // Skip tombstoned deps — they were deleted and
                            // should not be recreated.
                            if self.tombstones.contains_key(dep_id) {
                                continue;
                            }
                            if !self.nodes.contains_key(dep_id) {
                                // Auto-ingest: create node and enqueue Source job.
                                let dep_node = self.create_node(dep_id, None);
                                let dep_gen = dep_node.bump_generation();
                                self.nodes.insert(dep_id.clone(), dep_node);

                                // Propagate the parent request's context onto
                                // the dep's Source job so the worker re-installs
                                // it as TLS for the dep read. Without this,
                                // VFS-sink fan-out events for auto-ingested
                                // deps drop because `current_request_id()`
                                // returns None on the dep worker thread — this
                                // handler runs on the driver thread (TLS
                                // empty), so we must read the parent's context
                                // off the parent node's pending_requests
                                // (winner_context is still present at this
                                // generation while the parent's target stage,
                                // e.g. Analysis, is outstanding).
                                // Capture-site audit.
                                let parent_ctx = node
                                    .pending_requests
                                    .winner_context_at_generation(generation);
                                let mut job_index = self.job_index.lock();
                                job_index.insert(
                                    QueueEntry::new(
                                        JobKey {
                                            file_id: dep_id.clone(),
                                            generation: dep_gen,
                                            task_kind: TaskKind::Source,
                                        },
                                        // Inherit priority from the dependent.
                                        std::cmp::min(inherited_priority, Priority::Interactive),
                                        Instant::now(),
                                        None,
                                    )
                                    .with_request_context(parent_ctx),
                                );
                            }

                            // Check if dep already has current analysis — if so, no blocker needed.
                            let needs_blocker = self
                                .nodes
                                .get(dep_id)
                                .map(|n| n.current_analysis().is_none())
                                .unwrap_or(true);

                            if needs_blocker {
                                blockers.push(crate::edges::BlockerRef {
                                    file_id: dep_id.clone(),
                                    required_stage: TaskKind::Analysis,
                                });
                            }
                        }

                        if !blockers.is_empty() {
                            // Register: this file's artifacts are blocked until all
                            // deps are analyzed. We use TaskKind::Analysis as the
                            // blocked task_kind — a file-level gate that prevents
                            // any Artifact from proceeding. handle_stage_complete
                            // for Analysis checks has_pending_blockers before
                            // enqueuing Artifact jobs; handle_blocker_resolved
                            // enqueues them when all blockers clear.
                            self.edges.blockers.register(
                                file_id,
                                generation,
                                TaskKind::Analysis, // file-level gate, not per-profile
                                blockers,
                                inherited_priority,
                            );
                        }
                    }
                }

                // Source → Analysis transition
                let mut job_index = self.job_index.lock();
                job_index.insert(QueueEntry::new(
                    JobKey {
                        file_id: file_id.to_string(),
                        generation,
                        task_kind: TaskKind::Analysis,
                    },
                    inherited_priority,
                    Instant::now(),
                    None,
                ));
            }
            TaskKind::Analysis => {
                // Analysis done — enqueue Artifact jobs ONLY if there are no
                // outstanding blockers (macro type deps waiting on other files).
                // If blockers exist, handle_blocker_resolved will enqueue them
                // once all deps reach Analysis.
                if self
                    .edges
                    .blockers
                    .has_pending_blockers(file_id, generation)
                {
                    // Artifacts gated — will be enqueued by handle_blocker_resolved.
                    return;
                }

                self.enqueue_pending_artifacts(file_id, generation, &node);
            }
            TaskKind::Artifact { .. } => {}
        }
    }

    /// Enqueue pending Artifact jobs for a file that has cleared both
    /// Analysis and all blockers.
    fn enqueue_pending_artifacts(&self, file_id: &str, generation: u64, node: &FileNode) {
        let pending_profiles = node
            .pending_requests
            .get_pending_artifact_profiles(generation);
        if !pending_profiles.is_empty() {
            let mut job_index = self.job_index.lock();
            for (profile_hash, priority) in pending_profiles {
                job_index.insert(QueueEntry::new(
                    JobKey {
                        file_id: file_id.to_string(),
                        generation,
                        task_kind: TaskKind::Artifact { profile_hash },
                    },
                    priority,
                    Instant::now(),
                    None,
                ));
            }
        }
    }

    /// Handle a blocker resolution.
    fn handle_blocker_resolved(&self, file_id: &str, generation: u64, completed_stage: &TaskKind) {
        // Generation fence: only process if the blocker file's current
        // generation matches the one that completed Analysis. If it has
        // advanced (re-upserted), this resolution is stale.
        if let Some(node) = self.nodes.get(file_id) {
            if node.generation() != generation {
                return; // stale — the blocker file has been re-upserted
            }
        } else {
            return; // blocker file was removed
        }

        let unblocked = self.edges.blockers.resolve(file_id, completed_stage);

        for job in unblocked {
            if let Some(node) = self.nodes.get(&job.file_id) {
                // Also verify the dependent's generation is still current.
                if node.generation() == job.generation {
                    self.enqueue_pending_artifacts(&job.file_id, job.generation, &node);
                }
            }
        }
    }

    /// Dispatch all ready work from the queue to the dedicated pools.
    ///
    /// - **Source** jobs: dispatched to the I/O pool (which loads content from
    ///   disk), then the I/O closure dispatches the parse step to the CPU pool.
    ///   This ensures blocking file reads don't consume CPU pool capacity.
    /// - **Analysis/Artifact** jobs: dispatched directly to the CPU pool.
    #[cfg(not(target_arch = "wasm32"))]
    fn dispatch_ready_work(&self) {
        loop {
            // Sample queue depth BEFORE dequeueing so the audit
            // figure matches the depth observed by the entry that is
            // about to leave. The sample is best-effort under the
            // single-driver-thread invariant; we read while holding
            // the lock to avoid a torn dequeue/depth pair.
            let (entry, queue_depth_pre_dequeue) = {
                let mut job_index = self.job_index.lock();
                let depth = job_index.len();
                let dequeued = job_index.dequeue();
                (dequeued, depth as u32)
            };

            let entry = match entry {
                Some(e) => e,
                None => break,
            };

            // Compute queue dwell ms: time the entry spent waiting in
            // the priority queue between enqueue and this dispatch.
            let dequeue_at = Instant::now();
            let queue_dwell_ms = dequeue_at
                .saturating_duration_since(entry.enqueue_time)
                .as_secs_f64()
                * 1000.0;
            // Inbox depth at dispatch time. The atomic length read
            // is consistent with the existing `inbox_depth_max`
            // counter sampling.
            let inbox_depth = self.inbox.sender.len() as u32;

            let inbox_sender = self.inbox.sender.clone();
            let executor = Arc::clone(&self.executor);
            let source_loader = Arc::clone(&self.source_loader);

            let file_id = entry.job_key.file_id.clone();
            let generation = entry.job_key.generation;
            let task_kind = entry.job_key.task_kind;

            let node = match self.nodes.get(&file_id) {
                Some(n) => n.clone(),
                None => continue,
            };
            if node.generation() != generation {
                continue;
            }

            // Look up the session-side context for this job. Preference:
            //   1. Context propagated by an auto-ingest dep enqueue (the
            //      parent request that caused the dep to be loaded; plan
            // §3.B).
            //   2. Winner context from a direct `submit_request` caller
            // at this generation (the original path).
            // The closure below installs the chosen context into TLS for
            // the duration of the stage so `current_request_id()`
            // returns the right id while the worker runs.
            let winner_ctx = entry.request_context.clone().or_else(|| {
                node.pending_requests
                    .winner_context_at_generation(generation)
            });

            if matches!(task_kind, TaskKind::Source) {
                // Source jobs: I/O pool loads content, then hands off to CPU pool
                // for parse. This keeps disk reads off the CPU threads.
                let node_for_panic = Arc::clone(&node);
                self.io_pool.execute(move || {
                    // install_tls populates BOTH the scheduler-side
                    // and session-side TLS slots in one go. Pre-Q this call
                    // routed through `OpaqueContextGuard::install` directly,
                    // populating only scheduler-side TLS — host-side audit
                    // helpers that read `current_request_context()` on the
                    // worker thread silently no-op'd, leaving counters such
                    // as `dep_signature_merges` at 0.
                    let _guard: Option<Box<dyn crate::request_context::TlsUninstall + Send>> =
                        winner_ctx.map(|opaque| Arc::clone(&opaque.0).install_tls());
                    // Publish scheduler-side attribution AFTER install
                    // so the audit observer trait routes through the
                    // active session-side `RequestContext`. The
                    // observer crate's `record_scheduler_dispatch`
                    // hook receives worker thread / pool / depths /
                    // dwell facts.
                    Self::publish_scheduler_dispatch(
                        crate::audit_publish::WorkerPoolTag::Io,
                        crate::audit_publish::SchedulerDepthsSnapshot {
                            inbox: inbox_depth,
                            queue: queue_depth_pre_dequeue,
                        },
                        queue_dwell_ms,
                    );
                    // Catch panics so the worker thread (and its TLS
                    // guard's RAII drop) stays intact for subsequent
                    // jobs on the same pool.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        Self::execute_stage_on_worker(
                            &node,
                            generation,
                            task_kind,
                            executor.as_ref(),
                            source_loader.as_ref(),
                            &inbox_sender,
                        );
                    }));
                    if result.is_err() {
                        // Surface the panic as Failed to all pending
                        // groups at this generation so callers aren't
                        // left hanging.
                        Self::surface_stage_panic_as_failed(
                            &node_for_panic,
                            generation,
                            &task_kind,
                            &inbox_sender,
                        );
                    }
                });
            } else {
                // Analysis/Artifact jobs: pure CPU work.
                let node_for_panic = Arc::clone(&node);
                self.cpu_pool.spawn(move || {
                    // See pool.rs:79 + io_pool branch above for
                    // the same install_tls bridging pattern; both TLS slots
                    // populated in one go so host-side audit helpers fire
                    // correctly on the CPU pool worker thread.
                    let _guard: Option<Box<dyn crate::request_context::TlsUninstall + Send>> =
                        winner_ctx.map(|opaque| Arc::clone(&opaque.0).install_tls());
                    Self::publish_scheduler_dispatch(
                        crate::audit_publish::WorkerPoolTag::Cpu,
                        crate::audit_publish::SchedulerDepthsSnapshot {
                            inbox: inbox_depth,
                            queue: queue_depth_pre_dequeue,
                        },
                        queue_dwell_ms,
                    );
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        Self::execute_stage_on_worker(
                            &node,
                            generation,
                            task_kind,
                            executor.as_ref(),
                            source_loader.as_ref(),
                            &inbox_sender,
                        );
                    }));
                    if result.is_err() {
                        Self::surface_stage_panic_as_failed(
                            &node_for_panic,
                            generation,
                            &task_kind,
                            &inbox_sender,
                        );
                    }
                });
            }
        }
    }

    /// Publish a single scheduler-dispatch fact through the audit
    /// observer TLS slot if one is installed. The session-side
    /// `RequestContext` impl writes the supplied facts into its
    /// per-request scheduler-audit slot; non-audit callers are a
    /// no-op via [`verter_audit::observer::AuditObserver`]'s default
    /// implementation. Static so it can be called from worker
    /// closures without holding `&self`.
    #[cfg(not(target_arch = "wasm32"))]
    fn publish_scheduler_dispatch(
        pool: crate::audit_publish::WorkerPoolTag,
        depths: crate::audit_publish::SchedulerDepthsSnapshot,
        queue_dwell_ms: f64,
    ) {
        if let Some(observer) = verter_audit::current_observer() {
            let audit = verter_audit::SchedulerAudit {
                worker_thread_id: format!("{:?}", std::thread::current().id()),
                worker_pool: pool.into(),
                depths: depths.into(),
                queue_dwell_ms,
                dispatch_count: 1,
            };
            observer.record_scheduler_dispatch(audit);
        }
    }

    /// Dispatch work inline (used by `drive_one`/`drive_all` in sync mode and WASM).
    fn execute_stage_inline(&self, entry: QueueEntry) {
        let file_id = &entry.job_key.file_id;
        let generation = entry.job_key.generation;

        let node = match self.nodes.get(file_id) {
            Some(n) => n.clone(),
            None => return,
        };

        if node.generation() != generation {
            return;
        }

        Self::execute_stage_on_worker(
            &node,
            generation,
            entry.job_key.task_kind,
            self.executor.as_ref(),
            self.source_loader.as_ref(),
            &self.inbox.sender,
        );
    }

    /// Surface a worker-stage panic as `Failed` on all pending groups
    /// at this `(generation, task_kind)` so callers never hang on a
    /// crashed stage. The panic has been swallowed by the worker's
    /// `catch_unwind` — this helper completes the signalling that the
    /// executor's normal error path would have done.
    fn surface_stage_panic_as_failed(
        node: &FileNode,
        generation: u64,
        task_kind: &TaskKind,
        _inbox_sender: &crossbeam_channel::Sender<Submission>,
    ) {
        if node.generation() != generation {
            return;
        }
        let error = crate::job::SchedulerError::StageFailed {
            file_id: node.canonical_id.clone(),
            stage: format!("{task_kind:?}"),
            message: "stage executor panicked".to_string(),
        };
        node.pending_requests
            .signal_failed_for_stage(generation, task_kind, error);
    }

    /// Execute a stage on a worker (rayon thread or inline).
    ///
    /// This is a static method so it can be called from rayon::spawn closures
    /// without holding a reference to &self. All shared state is passed explicitly.
    fn execute_stage_on_worker(
        node: &FileNode,
        generation: u64,
        task_kind: TaskKind,
        executor: &dyn StageExecutor,
        source_loader: &dyn SourceLoader,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
    ) {
        match task_kind {
            TaskKind::Source => {
                Self::execute_source_stage(node, generation, executor, source_loader, inbox_sender);
            }
            TaskKind::Analysis => {
                Self::execute_analysis_stage(node, generation, executor, inbox_sender);
            }
            TaskKind::Artifact { profile_hash } => {
                Self::execute_artifact_stage(
                    node,
                    generation,
                    profile_hash,
                    executor,
                    inbox_sender,
                );
            }
        }
    }

    /// Execute the Source stage: load content, run executor, commit.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn execute_source_stage(
        node: &FileNode,
        generation: u64,
        executor: &dyn StageExecutor,
        source_loader: &dyn SourceLoader,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
    ) {
        use crate::job::SchedulerError;

        // Load content: pending_source (from submit) → source_loader (from disk/memory)
        let content = {
            let pending = node.pending_source.load();
            match pending.as_ref() {
                Some((gen, buf)) if *gen == generation => Some(Arc::clone(buf)),
                _ => None,
            }
        };
        let content = content.or_else(|| source_loader.load(&node.canonical_id));

        let content = match content {
            Some(c) => c,
            None => {
                // File not found — signal Failed, not Ready with empty content
                node.pending_requests.signal_failed(
                    generation,
                    SchedulerError::FileNotFound {
                        file_id: node.canonical_id.clone(),
                    },
                );
                return;
            }
        };

        let snapshot = match executor.execute_source(
            &node.canonical_id,
            node.file_kind,
            content,
            generation,
        ) {
            Ok(snap) => Arc::new(snap),
            Err(e) => {
                node.pending_requests.signal_failed(
                    generation,
                    SchedulerError::StageFailed {
                        file_id: node.canonical_id.clone(),
                        stage: "Source".to_string(),
                        message: e.message,
                    },
                );
                return;
            }
        };

        if node.generation() == generation {
            node.source.store(Arc::new(Some(Arc::clone(&snapshot))));

            let pending = node.pending_source.load();
            if let Some((gen, _)) = pending.as_ref() {
                if *gen == generation {
                    node.pending_source.store(Arc::new(None));
                }
            }

            let result = RequestResult::Source(snapshot);
            node.pending_requests
                .signal_stage_complete(generation, &TaskKind::Source, &result);

            let _ = inbox_sender.send(Submission::StageComplete {
                file_id: node.canonical_id.clone(),
                generation,
                task_kind: TaskKind::Source,
            });
        }
    }

    /// Execute the Analysis stage via the executor.
    fn execute_analysis_stage(
        node: &FileNode,
        generation: u64,
        executor: &dyn StageExecutor,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
    ) {
        use crate::job::SchedulerError;

        let source = match node.current_source() {
            Some(s) => s,
            None => return, // Source not ready — will be retried after Source completes
        };

        let snapshot = match executor.execute_analysis(&node.canonical_id, &source, generation) {
            Ok(snap) => Arc::new(snap),
            Err(e) => {
                node.pending_requests.signal_failed(
                    generation,
                    SchedulerError::StageFailed {
                        file_id: node.canonical_id.clone(),
                        stage: "Analysis".to_string(),
                        message: e.message,
                    },
                );
                return;
            }
        };

        if node.generation() == generation {
            node.analysis.store(Arc::new(Some(Arc::clone(&snapshot))));

            let result = RequestResult::Analysis(snapshot);
            node.pending_requests
                .signal_stage_complete(generation, &TaskKind::Analysis, &result);

            let _ = inbox_sender.send(Submission::StageComplete {
                file_id: node.canonical_id.clone(),
                generation,
                task_kind: TaskKind::Analysis,
            });

            let _ = inbox_sender.send(Submission::BlockerResolved {
                file_id: node.canonical_id.clone(),
                generation,
                completed_stage: TaskKind::Analysis,
            });
        }
    }

    /// Execute the Artifact stage via the executor.
    fn execute_artifact_stage(
        node: &FileNode,
        generation: u64,
        profile_hash: u64,
        executor: &dyn StageExecutor,
        _inbox_sender: &crossbeam_channel::Sender<Submission>,
    ) {
        use crate::job::SchedulerError;

        let source = match node.current_source() {
            Some(s) => s,
            None => return,
        };
        let analysis = match node.current_analysis() {
            Some(a) => a,
            None => return,
        };

        let snapshot = match executor.execute_artifact(
            &node.canonical_id,
            &source,
            &analysis,
            profile_hash,
            generation,
        ) {
            Ok(snap) => Arc::new(snap),
            Err(e) => {
                // Signal failure only for this specific profile, not all artifacts.
                node.pending_requests.signal_failed_for_stage(
                    generation,
                    &TaskKind::Artifact { profile_hash },
                    SchedulerError::StageFailed {
                        file_id: node.canonical_id.clone(),
                        stage: "Artifact".to_string(),
                        message: e.message,
                    },
                );
                return;
            }
        };

        if node.generation() == generation {
            node.artifacts.insert(profile_hash, Arc::clone(&snapshot));

            let result = RequestResult::Artifact(snapshot);
            node.pending_requests.signal_stage_complete(
                generation,
                &TaskKind::Artifact { profile_hash },
                &result,
            );
        }
    }

    // ── Test/WASM Driver Control ──

    /// Process one submission + dispatch one job. Returns false if nothing to do.
    pub fn drive_one(&self) -> bool {
        // Drain inbox
        self.drain_inbox();

        // Try to dispatch one job
        let entry = {
            let mut job_index = self.job_index.lock();
            job_index.dequeue()
        };

        if let Some(entry) = entry {
            self.execute_stage_inline(entry);
            true
        } else {
            false
        }
    }

    /// Process until queue empty + no pending completions.
    pub fn drive_all(&self) {
        let mut iterations = 0;
        loop {
            self.drain_inbox();
            let entry = {
                let mut job_index = self.job_index.lock();
                job_index.dequeue()
            };

            match entry {
                Some(entry) => {
                    self.execute_stage_inline(entry);
                    iterations = 0; // reset — new work may have been submitted
                }
                None => {
                    // Check if there are pending submissions from stage completions
                    if self.inbox.receiver.is_empty() {
                        iterations += 1;
                        if iterations > 2 {
                            break; // stable empty
                        }
                    } else {
                        iterations = 0;
                    }
                }
            }
        }
    }

    /// Block until `handle` resolves, driving the scheduler inline when no
    /// driver thread can do so (WASM single-threaded, sync mode). On native
    /// with a driver thread installed this delegates to the condvar-based
    /// `handle.wait()`; on WASM it loops `drain_inbox` + `dequeue` +
    /// `execute_stage_inline` until either the handle resolves or the queue
    /// stably runs out of progress (in which case it returns
    /// `CompletionState::Failed("scheduler stably empty with handle pending")`).
    ///
    /// This is the canonical cross-target wait API — call it instead of
    /// `handle.wait()` from any code that may run on WASM.
    pub fn wait_or_drive<T: Clone>(
        self: &Arc<Self>,
        handle: &crate::job::CompletionHandle<T>,
    ) -> crate::job::CompletionState<T> {
        // Native with driver thread: condvar wait works (driver does the work).
        // No driver thread (WASM, or sync-test scheduler): drive inline.
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.driver_handle.lock().is_some() {
                return handle.wait();
            }
        }

        // Inline drive loop.
        let mut idle_iterations = 0u32;
        loop {
            if let Some(state) = handle.try_get() {
                return state;
            }
            self.drain_inbox();
            let entry = {
                let mut job_index = self.job_index.lock();
                job_index.dequeue()
            };
            match entry {
                Some(entry) => {
                    self.execute_stage_inline(entry);
                    idle_iterations = 0;
                }
                None => {
                    if self.inbox.receiver.is_empty() {
                        idle_iterations += 1;
                        if idle_iterations > 2 {
                            // Stably empty queue + handle still pending —
                            // controlled failure (caller treats this as "load
                            // failed" rather than blocking forever).
                            return crate::job::CompletionState::Failed(
                                crate::job::SchedulerError::StageFailed {
                                    file_id: String::new(),
                                    stage: "wait_or_drive".into(),
                                    message: "scheduler stably empty with handle pending".into(),
                                },
                            );
                        }
                    } else {
                        idle_iterations = 0;
                    }
                }
            }
        }
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        // Set shutdown flag
        self.shutdown.store(true, Ordering::Release);
        let _ = self.inbox.sender.send(Submission::Wake);

        // Close inbox (causes driver recv to return Disconnected)
        // Drop the sender to close the channel
        // Note: The inbox sender is shared, but dropping the Scheduler
        // signals shutdown via the flag

        // Join driver thread
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(handle) = self.driver_handle.lock().take() {
                if should_join_driver_thread(handle.thread().id(), std::thread::current().id()) {
                    let _ = handle.join();
                }
            }
        }

        // Signal shutdown to all pending requests
        for entry in self.nodes.iter() {
            entry.value().pending_requests.signal_shutdown();
        }
    }
}

// Bridge between source_loader::FileKind and node::FileKind
impl crate::node::FileKind {
    pub(crate) fn from_source_loader_kind(kind: crate::source_loader::FileKind) -> Self {
        match kind {
            crate::source_loader::FileKind::VueSfc => crate::node::FileKind::VueSfc,
            crate::source_loader::FileKind::NonSfc => crate::node::FileKind::NonSfc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_loader::MemorySourceLoader;

    fn _test_scheduler() -> Arc<Scheduler> {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));
        loader.insert("/b.vue".to_string(), Arc::from("<template>bye</template>"));
        Scheduler::new_sync(SchedulerConfig::default(), loader)
    }

    fn test_scheduler_with_loader(loader: Arc<MemorySourceLoader>) -> Arc<Scheduler> {
        Scheduler::new_sync(SchedulerConfig::default(), loader)
    }

    // ── Basic Pipeline ──

    #[test]
    fn submit_source_request_and_drive() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));
        let sched = test_scheduler_with_loader(loader);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        sched.drive_all();

        let state = handle.try_get().unwrap();
        assert!(state.is_ready());
        match state {
            CompletionState::Ready(RequestResult::Source(snap)) => {
                assert_eq!(&*snap.source, "<template>hi</template>");
                assert_eq!(snap.generation, 1);
            }
            _ => panic!("expected Source"),
        }
    }

    #[test]
    fn submit_analysis_request_and_drive() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));
        let sched = test_scheduler_with_loader(loader);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        sched.drive_all();

        let state = handle.try_get().unwrap();
        assert!(state.is_ready());
        match state {
            CompletionState::Ready(RequestResult::Analysis(snap)) => {
                assert_eq!(snap.generation, 1);
            }
            _ => panic!("expected Analysis"),
        }
    }

    #[test]
    fn submit_artifact_request_and_drive() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));
        let sched = test_scheduler_with_loader(loader);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 42 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        sched.drive_all();

        let state = handle.try_get().unwrap();
        assert!(state.is_ready());
        match state {
            CompletionState::Ready(RequestResult::Artifact(snap)) => {
                assert_eq!(snap.generation, 1);
                assert_eq!(snap.profile_hash, 42);
            }
            _ => panic!("expected Artifact"),
        }
    }

    // ── Source Provided ──

    #[test]
    fn submit_with_source_uses_provided_content() {
        let sched = Scheduler::new_sync(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()),
        );

        let handle = sched.submit_request(Request {
            file_id: "/new.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("provided content")),
            file_kind: None,
            request_context: None,
        });

        sched.drive_all();

        match handle.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Source(snap)) => {
                assert_eq!(&*snap.source, "provided content");
            }
            _ => panic!("expected Source"),
        }
    }

    // ── Generation Staleness ──

    #[test]
    fn newer_source_supersedes_older_request() {
        let sched = Scheduler::new_sync(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()),
        );

        // First request
        let h1 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v1")),
            file_kind: None,
            request_context: None,
        });

        // Second request (newer source) — before first is processed
        let h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v2")),
            file_kind: None,
            request_context: None,
        });

        sched.drive_all();

        // First request should be superseded
        match h1.try_get().unwrap() {
            CompletionState::Superseded => {}
            other => panic!("expected Superseded, got {:?}", other),
        }

        // Second request should succeed with v2
        match h2.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Analysis(_)) => {}
            other => panic!("expected Ready(Analysis), got {:?}", other),
        }
    }

    // ── Fast Path: Already Satisfied ──

    #[test]
    fn already_satisfied_returns_immediately() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("content"));
        let sched = test_scheduler_with_loader(loader);

        // First: drive to Analysis
        let h1 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(h1.try_get().unwrap().is_ready());

        // Second: should be satisfied immediately (no drive needed)
        let h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        // Process the submission (but no stage work needed)
        sched.drain_inbox();

        assert!(h2.try_get().unwrap().is_ready());
    }

    // ── Multiple Independent Files ──

    #[test]
    fn multiple_independent_files() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/b.vue".to_string(), Arc::from("b"));
        loader.insert("/c.vue".to_string(), Arc::from("c"));
        let sched = test_scheduler_with_loader(loader);

        let ha = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 0 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        let hb = sched.submit_request(Request {
            file_id: "/b.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 0 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        let hc = sched.submit_request(Request {
            file_id: "/c.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 0 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        sched.drive_all();

        assert!(ha.try_get().unwrap().is_ready());
        assert!(hb.try_get().unwrap().is_ready());
        assert!(hc.try_get().unwrap().is_ready());
    }

    // ── Try-Get Cache Reads ──

    #[test]
    fn try_get_source_after_drive() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("content"));
        let sched = test_scheduler_with_loader(loader);

        assert!(sched.try_get_source("/a.vue").is_none());

        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();

        let src = sched.try_get_source("/a.vue").unwrap();
        assert_eq!(&*src.source, "content");
    }

    // ── Close File ──

    #[test]
    fn close_file_clears_overlay() {
        let sched = Scheduler::new_sync(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()),
        );

        // Submit with source
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("editor content")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();

        assert!(sched.overlay().has("/a.vue"));

        sched.close_file("/a.vue");

        // Overlay should be cleared
        assert!(!sched.overlay().has("/a.vue"));
        // Source snapshot should be stale (generation bumped)
        assert!(sched.try_get_source("/a.vue").is_none());
    }

    // ── Shutdown ──

    #[test]
    fn shutdown_signals_pending_handles() {
        let loader = Arc::new(MemorySourceLoader::new());
        // Don't insert the file — so source stage can't complete
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        let handle = sched.submit_request(Request {
            file_id: "/missing.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 0 },
            priority: Priority::Background,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // Process submission but DON'T drive stages — the handle stays pending
        sched.drain_inbox();
        // Don't call drive_all — handle should still be pending

        // Drop triggers shutdown signaling
        drop(sched);

        let state = handle.try_get();
        assert!(state.is_some(), "handle should be resolved after shutdown");
        match state.unwrap() {
            CompletionState::Shutdown => {}
            CompletionState::Ready(_) => {
                // It's also acceptable if source stage ran (empty file)
                // then analysis, but artifact can't complete for missing file
            }
            other => panic!("expected Shutdown or Ready, got {:?}", other),
        }
    }

    // ── Priority Ordering ──

    #[test]
    fn critical_priority_processes_first() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/low.vue".to_string(), Arc::from("low"));
        loader.insert("/high.vue".to_string(), Arc::from("high"));
        let sched = test_scheduler_with_loader(loader);

        // Submit low priority first
        sched.submit_request(Request {
            file_id: "/low.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Background,
            source: None,
            file_kind: None,
            request_context: None,
        });
        // Submit high priority second
        sched.submit_request(Request {
            file_id: "/high.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Critical,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // Drain inbox so jobs are in the queue
        sched.drain_inbox();

        // Drive one — should process Critical first
        sched.drive_one();

        // High should be done, low should not
        assert!(sched.try_get_source("/high.vue").is_some());
        // Low may or may not be done depending on internal ordering,
        // but high must be done first
    }

    // ── Native Driver Thread ──

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_driver_processes_request() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("content"));
        let sched = Scheduler::new(SchedulerConfig::default(), loader);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Critical,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // Wait for completion (driver thread processes it)
        let state = handle.wait();
        assert!(state.is_ready());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_driver_shutdown_clean() {
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::new(SchedulerConfig::default(), loader);

        // Just drop it — should not hang or panic
        drop(sched);
    }

    // ── Blocker gating ──

    /// Custom executor that returns blocker_ids from extract_deps.
    struct BlockingExecutor {
        /// Maps file_id → list of dep file_ids that block its artifacts.
        blockers: std::collections::HashMap<String, Vec<String>>,
    }

    impl StageExecutor for BlockingExecutor {
        fn extract_deps(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
        ) -> crate::executor::ExtractedDeps {
            let blocker_ids = self.blockers.get(canonical_id).cloned().unwrap_or_default();
            let forward_deps = blocker_ids.clone();
            crate::executor::ExtractedDeps {
                forward_deps,
                blocker_ids,
            }
        }
    }

    #[test]
    fn blockers_gate_artifact_until_dep_analyzed() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep content"));

        // A depends on /dep.ts — A's artifacts should not proceed until dep is analyzed.
        let mut blockers = std::collections::HashMap::new();
        blockers.insert("/a.vue".to_string(), vec!["/dep.ts".to_string()]);

        let executor = Arc::new(BlockingExecutor { blockers });
        let sched = Scheduler::new_sync_with_executor(SchedulerConfig::default(), loader, executor);

        // Request Artifact for A
        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 42 },
            priority: Priority::Critical,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // Drive: Source(A) → Analysis(A), but Artifact(A) should be gated
        // because /dep.ts hasn't been analyzed yet.
        // The scheduler should auto-ingest /dep.ts via Source job.
        sched.drive_all();

        // The handle should resolve because drive_all processes the auto-ingested
        // dep through Source→Analysis, which resolves the blocker, which then
        // enqueues A's Artifact.
        let state = handle.try_get();
        assert!(
            state.is_some(),
            "handle should resolve after blocker clears"
        );
        assert!(
            state.unwrap().is_ready(),
            "handle should be Ready, not Failed"
        );

        // Verify dep was auto-ingested
        assert!(
            sched.has_node("/dep.ts"),
            "dependency should have been auto-ingested"
        );
        assert!(
            sched.try_get_analysis("/dep.ts").is_some(),
            "dependency should have completed Analysis"
        );
    }

    #[test]
    fn no_blockers_allows_immediate_artifact() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));

        // No blockers — artifact should proceed immediately after Analysis.
        let executor = Arc::new(BlockingExecutor {
            blockers: std::collections::HashMap::new(),
        });
        let sched = Scheduler::new_sync_with_executor(SchedulerConfig::default(), loader, executor);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 7 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        sched.drive_all();

        match handle.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Artifact(snap)) => {
                assert_eq!(snap.profile_hash, 7);
            }
            other => panic!("expected Ready(Artifact), got {:?}", other),
        }
    }

    #[test]
    fn file_not_found_signals_failed() {
        let sched = Scheduler::new_sync(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()), // empty — no files
        );

        let handle = sched.submit_request(Request {
            file_id: "/missing.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        sched.drive_all();

        match handle.try_get().unwrap() {
            CompletionState::Failed(e) => {
                assert!(
                    e.to_string().contains("file not found"),
                    "error should mention file not found, got: {}",
                    e
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    // ── P1 lifecycle tests ──

    #[test]
    fn remove_and_readd_uses_higher_generation() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("v1"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader.clone());

        // Upsert v1 → Source + Analysis at gen 1
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v1")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let gen1 = match h.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.generation,
            other => panic!("expected Analysis, got {:?}", other),
        };

        // Remove
        sched.remove("/a.vue");
        assert!(!sched.has_node("/a.vue"));

        // Re-add v2 → must get a generation > gen1
        loader.insert("/a.vue".to_string(), Arc::from("v2"));
        let h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v2")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let gen2 = match h2.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.generation,
            other => panic!("expected Analysis, got {:?}", other),
        };

        assert!(
            gen2 > gen1,
            "re-added file must have generation ({gen2}) > removed generation ({gen1})"
        );
    }

    #[test]
    fn deferred_blockers_are_replaced_not_appended() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep1.ts".to_string(), Arc::from("dep1"));
        loader.insert("/dep2.ts".to_string(), Arc::from("dep2"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // File exists but at gen 0 (not yet admitted)
        // First call: blockers = [dep1]
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep1.ts".to_string()],
            vec!["/dep1.ts".to_string()],
        );

        // Second call: blockers = [dep2] — should REPLACE, not append
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep2.ts".to_string()],
            vec!["/dep2.ts".to_string()],
        );

        // Check deferred state
        let deferred = sched.deferred_blocker_ids.get("/a.vue").map(|v| v.clone());
        assert_eq!(
            deferred,
            Some(vec!["/dep2.ts".to_string()]),
            "deferred blockers should be replaced, not appended"
        );
        // Negative: dep1 should NOT be in the deferred list
        assert!(
            !deferred.as_ref().unwrap().contains(&"/dep1.ts".to_string()),
            "old deferred blocker should be replaced"
        );
    }

    #[test]
    fn source_completion_merges_exact_resolved_deps() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Submit and drive to get a node at gen > 0
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(h.try_get().unwrap().is_ready());

        // Register exact-resolved bare dep (like set_import_dependencies)
        sched.register_resolved_deps("/a.vue", vec!["/bare-dep.ts".to_string()], vec![]);

        // Verify the bare dep is in forward edges
        let deps = sched.edges.get_forward_deps("/a.vue");
        assert!(
            deps.contains("/bare-dep.ts"),
            "exact-resolved dep should be in forward edges"
        );

        // Now re-upsert (triggers Source → extract_deps which only returns relative deps)
        let h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content v2")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(h2.try_get().unwrap().is_ready());

        // The bare dep should still be in forward edges (merged, not overwritten)
        let deps_after = sched.edges.get_forward_deps("/a.vue");
        assert!(
            deps_after.contains("/bare-dep.ts"),
            "exact-resolved bare dep must survive Source completion"
        );
    }

    #[test]
    fn removed_file_deferred_blockers_cleared() {
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Register deferred blockers for a file at gen 0
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );
        assert!(sched.deferred_blocker_ids.contains_key("/a.vue"));

        // Remove the file
        sched.remove("/a.vue");

        // Deferred blockers should be cleared
        assert!(
            !sched.deferred_blocker_ids.contains_key("/a.vue"),
            "deferred blockers must be cleared on remove"
        );
    }

    #[test]
    fn tombstone_rejects_pre_remove_source_submission() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("content"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Submit a request (stamped with current epoch=0)
        let h1 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v1")),
            file_kind: None,
            request_context: None,
        });

        // Remove BEFORE the driver processes h1 (bumps epoch to 1)
        sched.remove("/a.vue");

        // Now drive — h1 was stamped at epoch 0, tombstone is at epoch 1
        // so it should be rejected even though it carries source.
        sched.drive_all();

        match h1.try_get().unwrap() {
            CompletionState::Failed(_) => {} // correct — pre-remove submission rejected
            CompletionState::Shutdown => {}  // also acceptable — node removed
            other => panic!("expected Failed or Shutdown, got {:?}", other),
        }
        // Negative: the file should NOT be resurrected
        assert!(
            !sched.has_node("/a.vue"),
            "pre-remove submission must not resurrect file"
        );
    }

    #[test]
    fn auto_ingress_skips_tombstoned_deps() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/deleted-dep.ts".to_string(), Arc::from("dep"));

        // Executor that says /a.vue depends on /deleted-dep.ts
        let mut blockers_map = std::collections::HashMap::new();
        blockers_map.insert("/a.vue".to_string(), vec!["/deleted-dep.ts".to_string()]);
        let executor = Arc::new(BlockingExecutor {
            blockers: blockers_map,
        });
        let sched = Scheduler::new_sync_with_executor(SchedulerConfig::default(), loader, executor);

        // Tombstone the dep (simulating a prior deletion)
        sched.remove("/deleted-dep.ts");

        // Now process /a.vue — auto-ingress should skip the tombstoned dep
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 1 },
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();

        // The handle should resolve (not hang on a blocker for a deleted dep)
        assert!(
            h.try_get().is_some(),
            "should not hang on blocker for tombstoned dep"
        );

        // Negative: the deleted dep should NOT be recreated
        assert!(
            !sched.has_node("/deleted-dep.ts"),
            "tombstoned dep must not be auto-ingested"
        );
    }

    #[test]
    fn blocker_resolution_checks_generation() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep"));

        let mut blockers_map = std::collections::HashMap::new();
        blockers_map.insert("/a.vue".to_string(), vec!["/dep.ts".to_string()]);
        let executor = Arc::new(BlockingExecutor {
            blockers: blockers_map,
        });
        let sched =
            Scheduler::new_sync_with_executor(SchedulerConfig::default(), loader.clone(), executor);

        // Submit /a.vue which depends on /dep.ts
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 1 },
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();

        // /dep.ts should have been auto-ingested and /a.vue should complete
        assert!(sched.has_node("/dep.ts"), "dep should be auto-ingested");
        assert!(
            h.try_get().is_some(),
            "should complete after dep is analyzed"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reset_clears_all_state() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::new(SchedulerConfig::default(), loader);

        // Populate state
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        h.wait();
        assert!(sched.has_node("/a.vue"));

        // Reset
        sched.reset();

        // All state cleared
        assert!(!sched.has_node("/a.vue"), "nodes must be cleared");
        assert!(sched.tombstones.is_empty(), "tombstones must be cleared");
        assert!(
            sched.deferred_blocker_ids.is_empty(),
            "deferred blockers must be cleared"
        );

        // Can restart and use again
        sched.restart_driver();
        let h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v2")),
            file_kind: None,
            request_context: None,
        });
        let state = h2.wait();
        assert!(
            state.is_ready(),
            "scheduler should work after reset+restart"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reset_seeds_generation_floors_for_cleared_nodes() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::new(SchedulerConfig::default(), loader);

        // Build up to gen N
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        let pre_gen = match h.wait() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.generation,
            other => panic!("expected Analysis, got {:?}", other),
        };

        // Reset
        sched.reset();
        sched.restart_driver();

        // Re-add same file — must get generation > pre_gen
        let h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v2")),
            file_kind: None,
            request_context: None,
        });
        let post_gen = match h2.wait() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.generation,
            other => panic!("expected Analysis, got {:?}", other),
        };

        assert!(
            post_gen > pre_gen,
            "post-reset generation ({post_gen}) must be > pre-reset ({pre_gen})"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn driver_join_guard_skips_current_thread() {
        let current = std::thread::current().id();
        let other = std::thread::spawn(|| std::thread::current().id())
            .join()
            .expect("thread id probe should succeed");

        assert!(
            !should_join_driver_thread(current, current),
            "driver join guard must skip self-join",
        );
        assert!(
            should_join_driver_thread(other, current),
            "driver join guard should still join distinct threads",
        );
    }

    #[test]
    fn register_resolved_deps_after_upsert_uses_correct_generation() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Upsert + drive to get real generation
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let _gen = match h.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.generation,
            other => panic!("expected Analysis, got {:?}", other),
        };

        // Now upsert again (new source) — gen bumps in the driver
        let _h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v2")),
            file_kind: None,
            request_context: None,
        });
        // DON'T drive yet — the new gen hasn't been assigned

        // Call register_resolved_deps — should defer blockers (gen mismatch)
        // or attach to the latest processed generation, NOT to a stale one.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Drive to process the upsert
        sched.drive_all();

        // The blocker should be properly registered at the new generation,
        // not lost due to generation mismatch.
        // Verify by checking that /dep.ts was auto-ingested (blocker was registered)
        assert!(
            sched.has_node("/dep.ts"),
            "dep should have been auto-ingested via deferred blocker replay"
        );
    }

    #[test]
    fn artifact_commit_captures_generation_at_compile_start() {
        // This test verifies the concept: a compile result should be tagged
        // with the generation that was current when compilation STARTED,
        // not whatever generation is current when it finishes.
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Get to a stable generation
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let gen = match h.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.generation,
            _ => panic!("expected Analysis"),
        };

        // Commit an artifact at the correct generation
        sched.commit_artifact(
            "/a.vue",
            42,
            crate::node::ArtifactSnapshot {
                generation: gen,
                profile_hash: 42,
                data: Arc::new(crate::node::EmptyData),
            },
        );

        // Should be readable
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_some(),
            "artifact committed at correct generation should be readable"
        );

        // Commit at wrong generation should be dropped
        sched.commit_artifact(
            "/a.vue",
            99,
            crate::node::ArtifactSnapshot {
                generation: gen + 100, // wrong generation
                profile_hash: 99,
                data: Arc::new(crate::node::EmptyData),
            },
        );

        assert!(
            sched.try_get_artifact("/a.vue", 99).is_none(),
            "artifact committed at wrong generation should be dropped"
        );
    }

    #[test]
    fn register_resolved_deps_defers_when_upsert_pending() {
        // Scenario: file at gen G, new upsert queued but not yet admitted (gen still G).
        // register_resolved_deps should defer blockers so they're replayed at G+1.
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep"));

        let mut blockers_map = std::collections::HashMap::new();
        blockers_map.insert("/a.vue".to_string(), vec!["/dep.ts".to_string()]);
        let executor = Arc::new(BlockingExecutor {
            blockers: blockers_map,
        });
        let sched = Scheduler::new_sync_with_executor(SchedulerConfig::default(), loader, executor);

        // Step 1: Initial upsert → drive to gen G
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v1")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let gen_g = sched.try_get_source("/a.vue").unwrap().generation;

        // Step 2: Queue a new upsert (gen G+1) but DON'T drive
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 1 },
            priority: Priority::Interactive,
            source: Some(Arc::from("a v2")),
            file_kind: None,
            request_context: None,
        });
        // Node is still at gen G — the G+1 bump hasn't happened yet.
        assert_eq!(
            sched.try_get_source("/a.vue").unwrap().generation,
            gen_g,
            "upsert not yet admitted"
        );

        // Step 3: register_resolved_deps arrives (from set_import_dependencies)
        // while the node is still at gen G but the edit is for G+1.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Step 4: Drive everything — the upsert becomes G+1, Source completion
        // should replay deferred blockers so /dep.ts is auto-ingested.
        sched.drive_all();

        // Verify the blocker dep was ingested
        assert!(
            sched.has_node("/dep.ts"),
            "bare dep from register_resolved_deps must be auto-ingested at G+1"
        );

        // Verify /a.vue reached artifact (blockers resolved)
        let snap = sched.try_get_artifact("/a.vue", 1);
        assert!(
            snap.is_some(),
            "artifact should complete after deferred blockers resolved"
        );
    }

    #[test]
    fn commit_artifact_requires_coherent_analysis() {
        // commit_artifact should only succeed if Source AND Analysis exist
        // at the matching generation — not just node.generation().
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Submit and drive exactly ONE job (Source) — don't let Analysis run.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_one(); // processes the Source job only
        let gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Verify Analysis is NOT yet committed
        assert!(
            sched.try_get_analysis("/a.vue").is_none(),
            "precondition: Analysis must not exist yet"
        );

        // Attempt to commit artifact WITHOUT Analysis
        sched.commit_artifact(
            "/a.vue",
            42,
            crate::node::ArtifactSnapshot {
                generation: gen,
                profile_hash: 42,
                data: Arc::new(crate::node::EmptyData),
            },
        );

        // Should NOT be readable — Analysis not committed yet
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_none(),
            "artifact must not be committed without current Analysis"
        );
    }

    #[test]
    fn bare_blocker_from_register_defers_across_generation_bump() {
        // Verify that register_resolved_deps at gen G provides a bare blocker
        // that is also processed at gen G+1 (via deferred replay).
        // The dep doesn't exist on disk, so auto-ingress creates a node that
        // fails Source. This proves the blocker was processed.
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        // /bare-dep.ts intentionally NOT in loader
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Step 1: Initial upsert → drive to gen G
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v1")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();

        // Step 2: Register bare blocker dep at gen G
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/bare-dep.ts".to_string()],
            vec!["/bare-dep.ts".to_string()],
        );

        // Step 3: Queue new upsert for gen G+1 + drive everything
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 1 },
            priority: Priority::Interactive,
            source: Some(Arc::from("a v2")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();

        // The dep node should exist (auto-ingested via blocker at G or G+1)
        assert!(
            sched.has_node("/bare-dep.ts"),
            "bare dep must be auto-ingested (proves blocker was processed)"
        );
    }

    #[test]
    fn host_artifact_commit_skips_when_scheduler_behind() {
        // Verify that when the scheduler hasn't committed Source yet (async lag),
        // commit_artifact is a no-op rather than committing at gen 0.
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Create a node but don't run Source
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        // DON'T drive — scheduler hasn't processed anything

        // Drain inbox so node exists but Source hasn't committed
        sched.drain_inbox();
        assert!(sched.has_node("/a.vue"), "node should exist after drain");
        assert!(
            sched.try_get_source("/a.vue").is_none(),
            "precondition: Source not yet committed"
        );

        let gen = sched.nodes.get("/a.vue").map(|n| n.generation()).unwrap();

        // Attempt to commit artifact
        sched.commit_artifact(
            "/a.vue",
            42,
            crate::node::ArtifactSnapshot {
                generation: gen,
                profile_hash: 42,
                data: Arc::new(crate::node::EmptyData),
            },
        );

        // Must be rejected — Source not committed
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_none(),
            "artifact must not be committed when Source hasn't been committed"
        );
        // Negative: even via last_known_good
        assert!(
            sched.try_get_last_known_good("/a.vue", 42).is_none(),
            "artifact must not exist at all when Source is absent"
        );
    }

    #[test]
    fn late_register_resolved_deps_gates_current_generation() {
        // Scenario: file is at gen G with Source+Analysis committed.
        // register_resolved_deps arrives AFTER Source completed.
        // A subsequent Artifact request must still be gated by the bare blocker.
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/bare-dep.ts".to_string(), Arc::from("dep"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Step 1: upsert + drive to Analysis (Source already completed)
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let gen = sched.try_get_source("/a.vue").unwrap().generation;
        assert!(sched.try_get_analysis("/a.vue").is_some(), "precondition");

        // Step 2: register_resolved_deps arrives AFTER Source completed
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/bare-dep.ts".to_string()],
            vec!["/bare-dep.ts".to_string()],
        );

        // Step 3: request Artifact — must be gated by the bare blocker
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 7 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        // Drive just admission — don't drive the dep's Source/Analysis
        sched.drain_inbox();

        // KEY ASSERTION: the artifact must NOT complete yet because
        // /bare-dep.ts hasn't been analyzed
        assert!(
            h.try_get().is_none() || !h.try_get().unwrap().is_ready(),
            "artifact must be gated until bare blocker dep is analyzed"
        );

        // Verify the blocker is registered
        assert!(
            sched.edges.blockers.has_pending_blockers("/a.vue", gen),
            "bare blocker must be registered at current generation"
        );

        // Step 4: drive everything — dep gets analyzed, blocker resolves
        sched.drive_all();
        assert!(
            h.try_get().unwrap().is_ready(),
            "artifact should complete after blocker resolved"
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // Scheduler request context + worker TLS install
    // ──────────────────────────────────────────────────────────────────

    use crate::node::PendingRequests;
    use crate::request_context::{
        CacheEventKind, OpaqueRequestContext, RequestContextLike, TlsUninstall,
    };
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
    use std::sync::Mutex as StdMutex;

    /// Test-only implementation of `RequestContextLike` that captures
    /// the observations each probe wants to assert on:
    ///
    /// - `seen_request_ids`: every distinct `current_request_id()`
    ///   observed from inside the stage closure (workers record into
    ///   this field via `record_cache_event` and a thread-local probe).
    /// - `dedup_joiner_calls`: every `on_dedup_joiner` invocation,
    ///   including the winner details.
    /// - `capture_enabled` mirrors the plan's
    ///   `RequestContext::footprint_capture`.
    struct TestContext {
        request_id: u64,
        capture: bool,
        dedup_joiner_calls: StdMutex<Vec<(Arc<str>, u64, bool)>>,
    }

    impl TestContext {
        fn new(request_id: u64, capture: bool) -> Arc<Self> {
            Arc::new(Self {
                request_id,
                capture,
                dedup_joiner_calls: StdMutex::new(Vec::new()),
            })
        }
        fn joiner_calls(&self) -> Vec<(Arc<str>, u64, bool)> {
            self.dedup_joiner_calls.lock().unwrap().clone()
        }
    }

    struct TestGuardBox(#[allow(dead_code)] crate::request_context::OpaqueContextGuard);
    impl TlsUninstall for TestGuardBox {
        fn uninstall(self: Box<Self>) {}
    }

    impl RequestContextLike for TestContext {
        fn request_id(&self) -> u64 {
            self.request_id
        }
        fn capture_enabled(&self) -> bool {
            self.capture
        }
        fn on_dedup_joiner(
            &self,
            canonical_id: Arc<str>,
            winner_request_id: u64,
            winner_audited: bool,
        ) {
            self.dedup_joiner_calls.lock().unwrap().push((
                canonical_id,
                winner_request_id,
                winner_audited,
            ));
        }
        fn record_cache_event(&self, _event: CacheEventKind) {}
        fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
            let guard = crate::request_context::OpaqueContextGuard::install(OpaqueRequestContext(
                self as Arc<dyn RequestContextLike>,
            ));
            Box::new(TestGuardBox(guard))
        }
    }

    /// Probe executor that records `current_request_id()` as it sees it
    /// at each stage. Uses an Arc-shared `AtomicU64` (per-stage) so the
    /// test thread can read what the worker thread observed.
    struct ProbeExecutor {
        source_observed: Arc<AtomicU64>,
        analysis_observed: Arc<AtomicU64>,
        artifact_observed: Arc<AtomicU64>,
        panic_on_analysis: Arc<AtomicBool>,
    }

    impl ProbeExecutor {
        fn new() -> Self {
            Self {
                source_observed: Arc::new(AtomicU64::new(0)),
                analysis_observed: Arc::new(AtomicU64::new(0)),
                artifact_observed: Arc::new(AtomicU64::new(0)),
                panic_on_analysis: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    impl StageExecutor for ProbeExecutor {
        fn execute_source(
            &self,
            _canonical_id: &str,
            _file_kind: crate::node::FileKind,
            content: Arc<str>,
            generation: u64,
        ) -> Result<SourceSnapshot, crate::executor::StageError> {
            let id = crate::request_context::current_request_id().unwrap_or(0);
            self.source_observed.store(id, AtomicOrdering::SeqCst);
            Ok(SourceSnapshot::new_empty(content, generation))
        }
        fn execute_analysis(
            &self,
            _canonical_id: &str,
            _source: &SourceSnapshot,
            generation: u64,
        ) -> Result<AnalysisSnapshot, crate::executor::StageError> {
            let id = crate::request_context::current_request_id().unwrap_or(0);
            self.analysis_observed.store(id, AtomicOrdering::SeqCst);
            if self.panic_on_analysis.load(AtomicOrdering::SeqCst) {
                panic!("probe executor panic_on_analysis");
            }
            Ok(AnalysisSnapshot::new_empty(generation))
        }
        fn execute_artifact(
            &self,
            _canonical_id: &str,
            _source: &SourceSnapshot,
            _analysis: &AnalysisSnapshot,
            profile_hash: u64,
            generation: u64,
        ) -> Result<ArtifactSnapshot, crate::executor::StageError> {
            let id = crate::request_context::current_request_id().unwrap_or(0);
            self.artifact_observed.store(id, AtomicOrdering::SeqCst);
            Ok(ArtifactSnapshot {
                generation,
                profile_hash,
                data: Arc::new(crate::node::EmptyData),
            })
        }
    }

    fn async_scheduler_with_executor(executor: Arc<dyn StageExecutor>) -> Arc<Scheduler> {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/ctx.vue".to_string(), Arc::from("<template>x</template>"));
        Scheduler::with_executor(SchedulerConfig::default(), loader, executor)
    }

    /// A CPU-worker stage (`Analysis` at minimum) must observe the
    /// request's context via `current_request_id()` while executing.
    #[test]
    fn scheduler_request_context_installed_as_tls_on_cpu_worker() {
        let probe = Arc::new(ProbeExecutor::new());
        let analysis_observed = Arc::clone(&probe.analysis_observed);
        let sched = async_scheduler_with_executor(probe as Arc<dyn StageExecutor>);
        let ctx = TestContext::new(42, true);
        let opaque = OpaqueRequestContext(Arc::clone(&ctx) as Arc<dyn RequestContextLike>);

        let handle = sched.submit_request(Request {
            file_id: "/ctx.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: Some(opaque),
        });
        let state = handle.wait();
        assert!(
            state.is_ready(),
            "analysis must have completed, got {state:?}"
        );
        assert_eq!(
            analysis_observed.load(AtomicOrdering::SeqCst),
            42,
            "CPU worker must have observed request_id=42 via current_request_id()",
        );
    }

    /// The Source stage runs on the I/O pool — the same TLS-install
    /// guarantee applies.
    #[test]
    fn scheduler_request_context_installed_as_tls_on_io_worker() {
        let probe = Arc::new(ProbeExecutor::new());
        let source_observed = Arc::clone(&probe.source_observed);
        let sched = async_scheduler_with_executor(probe as Arc<dyn StageExecutor>);
        let ctx = TestContext::new(7, true);
        let opaque = OpaqueRequestContext(Arc::clone(&ctx) as Arc<dyn RequestContextLike>);

        let handle = sched.submit_request(Request {
            file_id: "/ctx.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: Some(opaque),
        });
        let state = handle.wait();
        assert!(state.is_ready());
        assert_eq!(
            source_observed.load(AtomicOrdering::SeqCst),
            7,
            "I/O worker must have observed request_id=7 via current_request_id()",
        );
    }

    /// After a job completes, the TLS slot on the worker thread must
    /// be clear — subsequent jobs on the same thread (reused from the
    /// pool) must not inherit a stale context.
    #[test]
    fn scheduler_request_context_dropped_after_job_completes() {
        // We observe "after" state by submitting a SECOND request that
        // carries NO context; the probe must then see `None` (== 0) at
        // execution time.
        let probe = Arc::new(ProbeExecutor::new());
        let analysis_observed = Arc::clone(&probe.analysis_observed);
        let sched = async_scheduler_with_executor(probe as Arc<dyn StageExecutor>);

        let ctx = TestContext::new(11, false);
        let opaque = OpaqueRequestContext(Arc::clone(&ctx) as Arc<dyn RequestContextLike>);
        let h1 = sched.submit_request(Request {
            file_id: "/ctx.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: Some(opaque),
        });
        h1.wait();
        assert_eq!(analysis_observed.load(AtomicOrdering::SeqCst), 11);

        // Now submit a request with a fresh source (bumping generation)
        // and no context — TLS must be clean when the worker runs.
        let h2 = sched.submit_request(Request {
            file_id: "/ctx.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("<template>y</template>")),
            file_kind: None,
            request_context: None,
        });
        h2.wait();
        assert_eq!(
            analysis_observed.load(AtomicOrdering::SeqCst),
            0,
            "worker TLS must be clean for the context-less request",
        );
    }

    /// Panic inside the stage executor must still unwind the TLS guard
    /// so the worker thread's slot is clean afterwards.
    #[test]
    fn scheduler_worker_tls_cleared_on_panic_unwind() {
        let probe = Arc::new(ProbeExecutor::new());
        let panic_flag = Arc::clone(&probe.panic_on_analysis);
        let analysis_observed = Arc::clone(&probe.analysis_observed);
        panic_flag.store(true, AtomicOrdering::SeqCst);
        let sched = async_scheduler_with_executor(probe as Arc<dyn StageExecutor>);
        let ctx = TestContext::new(91, true);
        let opaque = OpaqueRequestContext(Arc::clone(&ctx) as Arc<dyn RequestContextLike>);

        let handle = sched.submit_request(Request {
            file_id: "/ctx.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: Some(opaque),
        });
        let state = handle.wait();
        assert!(
            matches!(state, CompletionState::Failed(_)),
            "panicked stage must surface as Failed, got {state:?}"
        );
        assert_eq!(
            analysis_observed.load(AtomicOrdering::SeqCst),
            91,
            "panicking stage must still have observed the installed context",
        );
        panic_flag.store(false, AtomicOrdering::SeqCst);

        // Run another job without context; worker TLS must be clean.
        let h2 = sched.submit_request(Request {
            file_id: "/ctx.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("<template>z</template>")),
            file_kind: None,
            request_context: None,
        });
        h2.wait();
        assert_eq!(
            analysis_observed.load(AtomicOrdering::SeqCst),
            0,
            "worker TLS must have been cleared by the panicking guard's Drop",
        );
    }

    /// Pool isolation: a panicking job must not leave state that the
    /// next job on the same pool observes. Covered by the previous test
    /// but spelled out as its own case for the plan test list.
    #[test]
    fn scheduler_pool_isolation_next_job_sees_clean_tls_after_preceding_panic() {
        let probe = Arc::new(ProbeExecutor::new());
        let panic_flag = Arc::clone(&probe.panic_on_analysis);
        let analysis_observed = Arc::clone(&probe.analysis_observed);
        let sched = async_scheduler_with_executor(probe as Arc<dyn StageExecutor>);

        panic_flag.store(true, AtomicOrdering::SeqCst);
        let ctx = TestContext::new(13, true);
        let opaque = OpaqueRequestContext(Arc::clone(&ctx) as Arc<dyn RequestContextLike>);
        let h1 = sched.submit_request(Request {
            file_id: "/ctx.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: Some(opaque),
        });
        h1.wait();
        panic_flag.store(false, AtomicOrdering::SeqCst);

        // Next job, no context — must see clean TLS.
        let h2 = sched.submit_request(Request {
            file_id: "/ctx.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("<template>q</template>")),
            file_kind: None,
            request_context: None,
        });
        h2.wait();
        assert_eq!(analysis_observed.load(AtomicOrdering::SeqCst), 0);
    }

    /// A request with `request_context: None` runs to completion without
    /// installing any TLS — `current_request_id()` returns `None` inside
    /// the worker.
    #[test]
    fn scheduler_request_context_absent_when_request_has_none() {
        let probe = Arc::new(ProbeExecutor::new());
        let analysis_observed = Arc::clone(&probe.analysis_observed);
        let sched = async_scheduler_with_executor(probe as Arc<dyn StageExecutor>);

        let handle = sched.submit_request(Request {
            file_id: "/ctx.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        handle.wait();
        assert_eq!(
            analysis_observed.load(AtomicOrdering::SeqCst),
            0,
            "absent context means TLS stays None → current_request_id() returns None",
        );
    }

    /// Scheduler dedup hook: joiner's `on_dedup_joiner` is called with
    /// `winner_audited = true` when the winner captures.
    #[test]
    fn scheduler_dedup_calls_on_dedup_joiner_with_winner_audited_true_when_winner_captures() {
        let pending = PendingRequests::new();
        let (_h1, s1) = completion_pair::<RequestResult>();
        let (_h2, s2) = completion_pair::<RequestResult>();

        let winner_ctx = TestContext::new(100, true); // captures
        let joiner_ctx = TestContext::new(200, true);

        let canonical: Arc<str> = Arc::from("/x.vue");
        pending.register(
            1,
            TargetStage::Analysis,
            Priority::Interactive,
            s1,
            Some(OpaqueRequestContext(
                Arc::clone(&winner_ctx) as Arc<dyn RequestContextLike>
            )),
            &canonical,
        );
        pending.register(
            1,
            TargetStage::Analysis,
            Priority::Interactive,
            s2,
            Some(OpaqueRequestContext(
                Arc::clone(&joiner_ctx) as Arc<dyn RequestContextLike>
            )),
            &canonical,
        );

        let calls = joiner_ctx.joiner_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.as_ref(), "/x.vue");
        assert_eq!(calls[0].1, 100, "winner request_id must be relayed");
        assert!(calls[0].2, "winner_audited must be true when capture=true");
    }

    /// Dedup hook: `winner_audited = false` when the winner does NOT
    /// capture.
    #[test]
    fn scheduler_dedup_calls_on_dedup_joiner_with_winner_audited_false_when_winner_does_not_capture(
    ) {
        let pending = PendingRequests::new();
        let (_h1, s1) = completion_pair::<RequestResult>();
        let (_h2, s2) = completion_pair::<RequestResult>();

        let winner_ctx = TestContext::new(101, false); // no capture
        let joiner_ctx = TestContext::new(201, true);

        let canonical: Arc<str> = Arc::from("/y.vue");
        pending.register(
            2,
            TargetStage::Analysis,
            Priority::Interactive,
            s1,
            Some(OpaqueRequestContext(
                Arc::clone(&winner_ctx) as Arc<dyn RequestContextLike>
            )),
            &canonical,
        );
        pending.register(
            2,
            TargetStage::Analysis,
            Priority::Interactive,
            s2,
            Some(OpaqueRequestContext(
                Arc::clone(&joiner_ctx) as Arc<dyn RequestContextLike>
            )),
            &canonical,
        );

        let calls = joiner_ctx.joiner_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, 101);
        assert!(!calls[0].2);
    }

    /// Capture-site fix. When analysis of a
    /// parent file extracts dep imports, the scheduler auto-ingests a
    /// Source job for each dep. That job runs on a worker thread whose
    /// TLS is empty by default; without the 7.B propagation fix, the
    /// dep's stage observes `current_request_id() == None` and any
    /// VFS-sink fan-out event for the dep read drops on the audit
    /// floor. The fix: the auto-ingest site reads the parent node's
    /// `winner_context_at_generation` and attaches it to the dep's
    /// `QueueEntry`, which the dispatch loop then installs as TLS.
    ///
    /// This regression probe records the parent's and the dep's
    /// observed `current_request_id()` separately via a
    /// canonical-dispatched probe. Both must equal the parent request's
    /// id.
    #[test]
    fn auto_ingested_dep_source_job_inherits_parent_request_context_as_tls() {
        use crate::executor::ExtractedDeps;

        const PARENT: &str = "/parent.vue";
        const DEP: &str = "/dep.ts";
        const PARENT_REQ_ID: u64 = 4242;

        struct ParentAndDepProbe {
            parent_analysis_observed: Arc<AtomicU64>,
            dep_source_observed: Arc<AtomicU64>,
        }
        impl StageExecutor for ParentAndDepProbe {
            fn extract_deps(&self, canonical_id: &str, _source: &SourceSnapshot) -> ExtractedDeps {
                if canonical_id == PARENT {
                    ExtractedDeps {
                        forward_deps: vec![DEP.to_string()],
                        blocker_ids: vec![DEP.to_string()],
                    }
                } else {
                    ExtractedDeps::default()
                }
            }
            fn execute_source(
                &self,
                canonical_id: &str,
                _file_kind: crate::node::FileKind,
                content: Arc<str>,
                generation: u64,
            ) -> Result<SourceSnapshot, crate::executor::StageError> {
                let id = crate::request_context::current_request_id().unwrap_or(0);
                if canonical_id == DEP {
                    self.dep_source_observed.store(id, AtomicOrdering::SeqCst);
                }
                Ok(SourceSnapshot::new_empty(content, generation))
            }
            fn execute_analysis(
                &self,
                canonical_id: &str,
                _source: &SourceSnapshot,
                generation: u64,
            ) -> Result<AnalysisSnapshot, crate::executor::StageError> {
                let id = crate::request_context::current_request_id().unwrap_or(0);
                if canonical_id == PARENT {
                    self.parent_analysis_observed
                        .store(id, AtomicOrdering::SeqCst);
                }
                Ok(AnalysisSnapshot::new_empty(generation))
            }
        }

        let parent_observed = Arc::new(AtomicU64::new(0));
        let dep_observed = Arc::new(AtomicU64::new(0));

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert(PARENT.to_string(), Arc::from("<template>x</template>"));
        loader.insert(DEP.to_string(), Arc::from("export type T = 0;"));

        let executor: Arc<dyn StageExecutor> = Arc::new(ParentAndDepProbe {
            parent_analysis_observed: Arc::clone(&parent_observed),
            dep_source_observed: Arc::clone(&dep_observed),
        });
        let sched = Scheduler::with_executor(SchedulerConfig::default(), loader, executor);

        let ctx = TestContext::new(PARENT_REQ_ID, true);
        let opaque = OpaqueRequestContext(Arc::clone(&ctx) as Arc<dyn RequestContextLike>);

        let handle = sched.submit_request(Request {
            file_id: PARENT.to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: Some(opaque),
        });
        let state = handle.wait();
        assert!(state.is_ready(), "parent must complete, got {state:?}");

        assert_eq!(
            parent_observed.load(AtomicOrdering::SeqCst),
            PARENT_REQ_ID,
            "parent stage must observe parent's request_id via TLS",
        );
        assert_eq!(
            dep_observed.load(AtomicOrdering::SeqCst),
            PARENT_REQ_ID,
            "auto-ingested dep Source job must observe the parent's \
             request_id via TLS. Without the capture-site fix, this \
             observes 0 because the dep worker thread has an empty TLS.",
        );
    }

    /// 16-thread stress: distinct requests on distinct files must not
    /// see each other's contexts. Each worker records `current_request_id()`
    /// per-file via the probe executor — we then confirm the per-file
    /// observation equals the per-file request id.
    #[test]
    fn scheduler_16_thread_stress_contexts_never_cross_contaminate() {
        use std::thread;

        const THREADS: usize = 16;

        // Per-file AtomicU64 that stores the observed request_id when
        // that file's stage runs.
        let observed: Arc<Vec<Arc<AtomicU64>>> =
            Arc::new((0..THREADS).map(|_| Arc::new(AtomicU64::new(0))).collect());

        struct PerFileProbe {
            slots: Arc<Vec<Arc<AtomicU64>>>,
        }
        impl StageExecutor for PerFileProbe {
            fn execute_analysis(
                &self,
                canonical_id: &str,
                _source: &SourceSnapshot,
                generation: u64,
            ) -> Result<AnalysisSnapshot, crate::executor::StageError> {
                // Extract file index from canonical_id like "/f{N}.vue".
                let idx = canonical_id
                    .trim_start_matches("/f")
                    .trim_end_matches(".vue")
                    .parse::<usize>()
                    .unwrap_or(0);
                let id = crate::request_context::current_request_id().unwrap_or(0);
                self.slots[idx].store(id, AtomicOrdering::SeqCst);
                Ok(AnalysisSnapshot::new_empty(generation))
            }
        }

        let loader = Arc::new(MemorySourceLoader::new());
        for i in 0..THREADS {
            loader.insert(format!("/f{i}.vue"), Arc::from("<template>z</template>"));
        }
        let executor: Arc<dyn StageExecutor> = Arc::new(PerFileProbe {
            slots: Arc::clone(&observed),
        });
        let sched = Scheduler::with_executor(SchedulerConfig::default(), loader, executor);

        let handles: Vec<_> = (0..THREADS)
            .map(|i| {
                let sched = Arc::clone(&sched);
                thread::spawn(move || {
                    // request_id = 1000+i, per-thread unique.
                    let ctx = TestContext::new(1000 + i as u64, true);
                    let opaque = OpaqueRequestContext(ctx as Arc<dyn RequestContextLike>);
                    let h = sched.submit_request(Request {
                        file_id: format!("/f{i}.vue"),
                        target: TargetStage::Analysis,
                        priority: Priority::Interactive,
                        source: None,
                        file_kind: None,
                        request_context: Some(opaque),
                    });
                    h.wait()
                })
            })
            .collect();
        for h in handles {
            h.join().expect("worker joined");
        }

        for i in 0..THREADS {
            let want = 1000 + i as u64;
            let got = observed[i].load(AtomicOrdering::SeqCst);
            assert_eq!(
                got, want,
                "file f{i}.vue observed request_id {got}, expected {want} — \
                 TLS cross-contamination between workers",
            );
        }
    }

    /// Pre-commit behavior: a request with `request_context: None`
    /// must route and complete exactly as before (no regression).
    #[test]
    fn scheduler_submit_without_context_matches_pre_commit_behavior() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));
        let sched = test_scheduler_with_loader(loader);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let state = handle.try_get().unwrap();
        assert!(state.is_ready());
        match state {
            CompletionState::Ready(RequestResult::Source(snap)) => {
                assert_eq!(&*snap.source, "<template>hi</template>");
            }
            other => panic!("expected Source ready, got {other:?}"),
        }
    }

    /// Discriminator: `remove_artifact_if_not_newer_than(file, profile,
    /// N)` MUST NOT clobber an artifact whose stored generation is
    /// strictly greater than `N`.
    ///
    /// Race scenario: a slow compile started at generation `N` reaches
    /// its refusal arm AFTER a faster compile at `N+k` (k > 0) has
    /// already committed a fresh artifact. The slow compile's
    /// captured start-generation is `N`; passing `max_generation = N`
    /// to this eviction MUST observe the stored `generation = N+k > N`
    /// and skip the remove. The newer artifact at `N+k` survives;
    /// `try_get_artifact` continues to serve it.
    ///
    /// The symmetric `commit_artifact` already rejects publishes whose
    /// generation does not match the node's current generation; this
    /// eviction is the inverse asymmetry: a slow refused compile must
    /// not delete a newer winner.
    ///
    /// Discriminating property: an unconditional
    /// `node.artifacts.remove(&profile_hash)` would delete the newer
    /// artifact and `try_get_artifact` would return `None` after the
    /// call. The generation-aware `remove_if(..., snap.generation <=
    /// max_generation)` preserves the newer artifact and
    /// `try_get_artifact` returns the same `Arc` it returned before.
    #[test]
    fn remove_artifact_if_not_newer_than_preserves_newer_generation_artifact() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a v1"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader.clone());

        // Drive the node to a stable generation N with Source +
        // Analysis committed.
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v1")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let gen_n = match h.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.generation,
            _ => panic!("expected Analysis ready at gen N"),
        };

        // Commit a successful artifact at generation N. This is the
        // "slow compile's view" — the artifact it expects to evict if
        // its refusal arm runs.
        sched.commit_artifact(
            "/a.vue",
            42,
            crate::node::ArtifactSnapshot {
                generation: gen_n,
                profile_hash: 42,
                data: Arc::new(crate::node::EmptyData),
            },
        );
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_some(),
            "fixture invariant: an artifact must be committed at gen N \
             so the race scenario is reproducible — without it the \
             newer-artifact survival assertion is vacuous."
        );

        // Advance to generation N+1 (k = 1, sufficient for the
        // discriminator). A re-upsert is the natural generation bump.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v2")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let gen_n_plus_k = sched.try_get_source("/a.vue").unwrap().generation;
        assert!(
            gen_n_plus_k > gen_n,
            "fixture invariant: the second upsert must bump the node \
             generation strictly past gen N (observed gen_n = {gen_n}, \
             gen_n_plus_k = {gen_n_plus_k})"
        );

        // The "fast successful compile at N+k" commits a fresh artifact
        // at the bumped generation. This is the artifact the slow
        // refused compile must NOT clobber.
        sched.commit_artifact(
            "/a.vue",
            42,
            crate::node::ArtifactSnapshot {
                generation: gen_n_plus_k,
                profile_hash: 42,
                data: Arc::new(crate::node::EmptyData),
            },
        );
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_some(),
            "fixture invariant: the fresh artifact at gen N+k must be \
             committed — without it the race scenario does not run."
        );

        // The slow refused compile reaches its eviction arm carrying
        // its captured START generation (gen_n). The eviction MUST be
        // gated on `stored_generation <= max_generation`. Since the
        // stored snapshot is at gen_n_plus_k > gen_n, the remove must
        // be a no-op.
        sched.remove_artifact_if_not_newer_than("/a.vue", 42, gen_n);

        // KEY DISCRIMINATOR: the newer artifact at gen N+k MUST
        // survive. An unconditional remove would clobber it and this
        // assertion would fail.
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_some(),
            "DISCRIMINATOR: a slow refused compile at gen N MUST NOT \
             clobber a fresher artifact at gen N+k. An unconditional \
             `remove_artifact(file, profile)` would delete the newer \
             artifact; the generation-gated \
             `remove_artifact_if_not_newer_than(file, profile, N)` \
             observes `stored_generation = N+k > N` and skips the \
             remove. (gen_n = {gen_n}, gen_n_plus_k = {gen_n_plus_k})"
        );

        // Symmetric positive case: a same-or-older max_generation MUST
        // actually evict. Otherwise the new method would never remove
        // anything — masking the legitimate refused-compile-cleanup
        // path. Pass the CURRENT stored generation (N+k); the eviction
        // should proceed.
        sched.remove_artifact_if_not_newer_than("/a.vue", 42, gen_n_plus_k);
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_none(),
            "carrier invariant: `remove_artifact_if_not_newer_than` \
             with `max_generation >= stored.generation` MUST evict \
             the snapshot — the eviction is the normal-case behavior \
             on the host's compile-refusal arm."
        );
    }
}
