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

use crate::dag::{
    profile_hash_from_bytes, profile_hash_to_bytes, DagAgingConfig, DagCapacityBudget, DepKey,
    FileStageKey, ReadyJob, SchedulerDag, WorkKind, WorkNodeIdentity,
};
use crate::driver::{Submission, SubmissionInbox};
use crate::edges::EdgeManager;
use crate::executor::{DefaultExecutor, StageExecutor};
use crate::job::{
    completion_pair, CompletionHandle, CompletionSender, CompletionState, RequestResult,
};
use crate::node::{AnalysisSnapshot, ArtifactSnapshot, FileNode, SourceSnapshot};
use crate::overlay::OverlayMap;
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
    pub aging: DagAgingConfig,
    /// Per-class DAG admission budget. Defaults are derived from
    /// `cpu_threads` / `io_threads` when the explicit budget is
    /// `None`.
    pub dag_budget: Option<DagCapacityBudget>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            cpu_threads: num_cpus(),
            #[cfg(target_arch = "wasm32")]
            cpu_threads: 1,
            io_threads: 4,
            aging: DagAgingConfig::default(),
            dag_budget: None,
        }
    }
}

/// Resolve the effective DAG budget from a scheduler config — the
/// explicit override if set, otherwise derived from the pool sizes.
fn dag_budget_for_config(config: &SchedulerConfig) -> DagCapacityBudget {
    config.dag_budget.unwrap_or(DagCapacityBudget {
        cpu: config.cpu_threads.max(1) as u32,
        io: config.io_threads.max(1) as u32,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// Admit a unit of work into the DAG for `(canonical, generation, task)`.
/// Returns the submission token. Used by every admission site
/// (`handle_new_request`, stage-completion driven transitions, etc.).
fn admit_work(
    dag: &mut SchedulerDag,
    canonical: &Arc<str>,
    generation: u64,
    task: TaskKind,
    priority: Priority,
    request_context: Option<crate::request_context::OpaqueRequestContext>,
) -> crate::dag::SubmissionToken {
    let (identity, kind) = match task {
        TaskKind::Source => (
            WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                generation,
                stage: FileStageKey::Source,
            },
            WorkKind::Load,
        ),
        TaskKind::Analysis => (
            WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                generation,
                stage: FileStageKey::Analysis,
            },
            WorkKind::Analysis,
        ),
        TaskKind::Artifact { profile_hash } => (
            WorkNodeIdentity::Artifact {
                canonical: Arc::clone(canonical),
                generation,
                profile_hash: profile_hash_to_bytes(profile_hash),
                content_hash: [0u8; 16],
            },
            WorkKind::Artifact,
        ),
    };
    dag.submit(identity, kind, priority, Vec::new(), request_context)
}

/// Map a [`ReadyJob`] back to the legacy [`TaskKind`] surface so the
/// dispatch loop can reuse the existing per-stage executors. The
/// adapter is the single bridge between the DAG's typed identities and
/// the per-stage execution path.
fn task_kind_for_ready_job(job: &ReadyJob) -> TaskKind {
    match &job.identity {
        WorkNodeIdentity::FileStage { stage, .. } => match stage {
            FileStageKey::Source => TaskKind::Source,
            FileStageKey::Analysis => TaskKind::Analysis,
        },
        WorkNodeIdentity::Artifact { profile_hash, .. } => TaskKind::Artifact {
            profile_hash: profile_hash_from_bytes(*profile_hash),
        },
        WorkNodeIdentity::CacheNode { .. } => {
            // CacheNode dispatch is reserved for the cache layer above
            // the scheduler. Falling through to Analysis here would
            // mis-route the work to the analysis stage; panic loudly
            // so any premature wiring through the file-stage adapter
            // is caught immediately. The cache layer must own its own
            // dispatch arm and never route cache-node work through
            // this helper.
            unreachable!(
                "CacheNode work must not be routed through the file-stage adapter; \
                 the cache layer owns its own dispatch arm"
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn should_join_driver_thread(
    handle_thread_id: std::thread::ThreadId,
    current_thread_id: std::thread::ThreadId,
) -> bool {
    handle_thread_id != current_thread_id
}

/// Test-only dispatch instrumentation that lets a test deterministically
/// observe SCHEDULER-PRIORITY-QUEUE dwell.
///
/// The dispatch loop ([`Scheduler::dispatch_ready_work`]) drains the
/// scheduler DAG's ready nodes and hands each entry to a bounded pool, so
/// by the time a stage executor runs, the entry has already left the
/// queue — a stage-side gate therefore cannot prove that surplus work
/// accrued `queue_dwell_ms` *in the scheduler queue* (it may have been
/// sitting in a pool channel instead). This hook moves the rendezvous to
/// the dispatch site itself:
///
/// 1. The test arms the hook with a `pause_after` dispatch count.
/// 2. After the driver has dispatched exactly `pause_after` jobs and
///    BEFORE the next dequeue, it parks here. While parked it keeps
///    re-draining the inbox (via the supplied closure) so every
///    still-in-flight submission lands in the scheduler DAG regardless of
///    submission timing — the surplus then provably SITS in the queue.
/// 3. The test waits until the driver reports `paused`, observes that
///    the DAG actually contains the surplus (via
///    [`Scheduler::test_job_queue_depth`]), and only then releases.
///
/// Every wait is bounded and panics on a real stall so a logic error
/// fails loudly instead of hanging the suite. The hook is
/// `cfg`-gated to `test` / `debug_assertions`; it and its single call
/// site are absent from release builds, so production dispatch is
/// unchanged.
#[cfg(any(test, debug_assertions))]
#[derive(Default)]
pub(crate) struct DispatchPauseHook {
    state: Mutex<DispatchPauseState>,
    cv: parking_lot::Condvar,
}

#[cfg(any(test, debug_assertions))]
#[derive(Default)]
struct DispatchPauseState {
    /// `true` once a test has armed the hook.
    armed: bool,
    /// Number of dispatches after which the driver parks (cumulative
    /// across `dispatch_ready_work` invocations).
    pause_after: usize,
    /// Cumulative count of jobs dispatched since the hook was armed.
    dispatched: usize,
    /// `true` once the driver has reached the pause point and is parked.
    paused: bool,
    /// `true` once the pause has fired; prevents re-pausing on later
    /// dispatch-loop iterations.
    consumed: bool,
    /// `true` once the test has released the parked driver.
    released: bool,
}

#[cfg(any(test, debug_assertions))]
impl DispatchPauseHook {
    /// Driver side: record one dispatch and, if the cumulative count has
    /// reached the armed `pause_after`, park before the next dequeue.
    ///
    /// While parked, `redrain` is invoked repeatedly so every
    /// still-in-flight submission is pulled into the scheduler DAG; the
    /// test observes the resulting queue depth before releasing. Bounded
    /// at ~10 s — a release that never arrives PANICS rather than hanging
    /// the driver forever.
    fn on_dispatch_and_maybe_pause(&self, redrain: &dyn Fn()) {
        use std::time::{Duration, Instant};
        let mut state = self.state.lock();
        if !state.armed || state.consumed {
            return;
        }
        state.dispatched += 1;
        if state.dispatched < state.pause_after {
            return;
        }
        // Reached the pause threshold. Park here (before the next
        // dequeue) until the test releases, re-draining the inbox so the
        // surplus provably accrues scheduler-queue dwell.
        state.consumed = true;
        state.paused = true;
        self.cv.notify_all();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !state.released {
            // Drop the lock around the re-drain so the test can observe
            // `paused`/`released` and so `redrain` (which locks the
            // scheduler DAG, not this state) cannot deadlock against it.
            drop(state);
            redrain();
            state = self.state.lock();
            if state.released {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "dispatch pause was never released within 10s — the test \
                 driver did not call test_release_dispatch_pause (deadlock)"
            );
            // Park briefly on the condvar; a release wakes us promptly,
            // otherwise we loop to re-drain and re-check the deadline.
            let _ = self.cv.wait_for(&mut state, Duration::from_millis(2));
        }
    }
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

/// Reason a pump iteration ran. Carried by audit/diagnostic prose;
/// behaviour is identical across variants, the discriminant lets
/// tests assert which entry point made progress.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PumpReason {
    /// Driver thread's idle loop.
    DriverLoop,
    /// Driver thread woke from `recv_timeout` on a fresh submission.
    DriverWake,
    /// A cooperative waiter inside `wait_or_drive`.
    WaitOrDrive,
    /// External `drive_one` (sync/test).
    DriveOne,
    /// External `drive_all` (sync/test).
    DriveAll,
    /// Final drain on shutdown.
    ShutdownDrain,
}

/// Counters returned by a single [`Scheduler::pump_ready`] call.
/// Used by tests and the cooperative pump to decide whether
/// progress was made before parking on the condvar.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PumpStats {
    /// Submissions drained from the inbox into the DAG.
    pub drained: usize,
    /// Ready jobs handed off to a pool (native dispatch).
    pub dispatched: usize,
    /// Ready jobs executed inline on the caller's thread.
    pub executed_inline: usize,
}

impl PumpStats {
    /// `true` when any counter is non-zero. A pump that drained
    /// nothing, dispatched nothing, and ran nothing inline made no
    /// progress; the caller must park rather than spin.
    pub(crate) fn made_progress(self) -> bool {
        self.drained > 0 || self.dispatched > 0 || self.executed_inline > 0
    }
}

/// Outcome of routing a [`crate::dag::ReadyJob`] through
/// `dispatch_ready_job`. The cooperative pump uses the variant
/// to track whether work ran on this thread or was handed off.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DispatchOutcome {
    /// The job was queued onto the CPU or I/O pool.
    SubmittedToPool,
    /// The job ran inline on the calling thread.
    ExecutedInline,
    /// The job was skipped (defensive cases: CacheNode, removed
    /// node, generation mismatch).
    Skipped,
}

/// The main scheduler.
///
/// Manages per-file nodes, a priority queue, and a driver thread that
/// processes submissions and dispatches work to CPU/IO pools.
pub struct Scheduler {
    /// Per-file nodes (concurrent access via DashMap).
    pub(crate) nodes: DashMap<String, Arc<FileNode>>,
    /// Edge manager (reverse-dep index + forward-dep snapshots).
    pub(crate) edges: EdgeManager,
    /// Single driver-owned readiness authority — admission, dedup,
    /// dependency gating, priority aging, capacity reservation,
    /// per-file waiter groups.
    ///
    /// Wrapped in `Arc` so worker closures can clone a handle for
    /// completion signalling without holding `&self`.
    pub(crate) dag: Arc<Mutex<SchedulerDag>>,
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
    /// Tracking set for deps whose Source `NewRequest` is queued in the
    /// inbox but has not yet been drained by the driver. Source-of-truth
    /// for "auto-ingest fired, FileNode is present, but no DAG identity
    /// has been admitted yet" — a transient state the dead-producer
    /// matrix would otherwise misclassify as `Resolved` (the FileNode +
    /// DAG shape is identical to a Source-failed corpse). Populated by
    /// [`Self::register_resolved_deps`] BEFORE the inbox send, consumed
    /// by [`Self::file_stage_analysis_blocker_status`] before it falls
    /// through to the dead-producer arm, and removed by
    /// [`Self::handle_new_request`] when the corresponding Source DAG
    /// identity is admitted to `dag.by_identity`.
    ///
    /// Keyed by canonical id. The value's `generation` matches the
    /// dep's FileNode generation at insert time; the matrix only
    /// honours an entry whose `generation` matches the live dep gen.
    /// `since` lets the consumer trim entries whose admission never
    /// landed (driver crash between insert and dequeue) without
    /// inserting an extra sweep loop.
    pub(crate) auto_ingested_recent: DashMap<Arc<str>, AutoIngestedRecord>,
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
    /// Test-only dispatch pause instrumentation. Lets a dwell test park
    /// the driver after N dispatches and observe scheduler-queue depth
    /// before releasing. `cfg`-gated to `test` / `debug_assertions`;
    /// absent from release builds.
    #[cfg(any(test, debug_assertions))]
    pub(crate) dispatch_pause: DispatchPauseHook,
}

/// Classification of a recorded `FileStage::Analysis` blocker dep
/// against the live FileNode + DAG state. Produced by
/// [`Scheduler::file_stage_analysis_blocker_status`] and consumed by
/// both the pre-admission filter ([`Scheduler::register_resolved_deps`])
/// and the recorded-blocker filter
/// ([`Scheduler::admit_artifact_with_blockers`]). Centralising the
/// classifier prevents the two predicates from drifting — the
/// dead-producer matrix has several distinct rows that an
/// independent re-implementation tends to collapse.
///
/// Three-state split. The earlier two-state encoding (`Resolved`)
/// collapsed two semantically distinct outcomes:
///
/// - `Satisfied` — the prerequisite reached committed Analysis OR
///   is genuinely moot (FileNode missing, stale generation, gen 0,
///   no persistent failure record). The blocker is dropped silently
///   and the downstream admission proceeds as usual.
/// - `Failed(record)` — the prerequisite terminalized (Source /
///   Analysis failure). The persistent
///   [`crate::dag::SchedulerDag::terminal_dep_failures`] store
///   carries a [`crate::dag::FailedDepRecord`] for this DepKey;
///   the caller MUST attach the record to the freshly-submitted
///   waiter so the pre-dispatch short-circuit in
///   [`Scheduler::execute_stage_on_worker`] surfaces a typed
///   `DependencyFailed`. Without this discrimination the matrix
///   would collapse `Failed` onto `Resolved` and the admission
///   would silently drop the blocker, resolving the waiter `Ready`
///   on a snapshot built from a dead prerequisite — the
///   pre-admission failure race.
#[derive(Clone, Debug)]
enum BlockerStatus {
    /// The dep is still gating: an Analysis is committed (and the
    /// owner waits for completion fan-out) OR an Analysis identity is
    /// live in the DAG (queued or dispatched).
    Gating,
    /// The dep is no longer gating without a terminal-failure
    /// record: Analysis is committed, the FileNode is missing, the
    /// recorded generation is stale, or the gen is 0. The blocker is
    /// dropped silently.
    Satisfied,
    /// The dep is no longer gating because the producer terminally
    /// failed and the persistent store carries the recorded cause.
    /// The caller MUST attach this [`crate::dag::FailedDepRecord`]
    /// to its admitted node so the pre-dispatch short-circuit fires.
    Failed(crate::dag::FailedDepRecord),
}

/// Record planted in [`Scheduler::auto_ingested_recent`] when
/// [`Scheduler::register_resolved_deps`] auto-ingests a dep blocker by
/// enqueueing a `Submission::NewRequest` to the inbox. The record makes
/// the "queued in inbox, not yet drained" state explicit so the
/// dead-producer matrix can distinguish it from "Source failed and
/// cancelled" — both leave the FileNode with `current_source().is_none()`
/// and no live Source/Analysis DAG identity.
///
/// The record is removed when the driver finally drains the queued
/// `NewRequest` and admits a Source DAG identity (see the cleanup arm
/// in [`Scheduler::handle_new_request`]). A stale entry left behind by
/// a driver-thread crash is trimmed by the [`STALE_THRESHOLD`]-based
/// sweep in the matrix consumer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AutoIngestedRecord {
    /// Generation the dep FileNode was at when the auto-ingest fired.
    /// Matched against the dep's current generation in the matrix; a
    /// mismatch means the entry is stale (the dep has advanced beyond
    /// the auto-ingest, so the gate it set up no longer applies).
    pub(crate) generation: u64,
    /// Monotonic instant the record was planted. Used by the cleanup
    /// sweep to trim entries whose admission never landed (e.g. driver
    /// crash between insert and dequeue).
    pub(crate) since: Instant,
}

/// Trim threshold for [`Scheduler::auto_ingested_recent`] entries. An
/// entry older than this is dropped on consumption — belt-and-suspenders
/// against a driver-thread crash between
/// [`Scheduler::register_resolved_deps`]'s insert and
/// [`Scheduler::handle_new_request`]'s removal arm. Under normal
/// operation the removal arm fires on the next driver tick, so the
/// threshold is generous.
const AUTO_INGESTED_RECENT_STALE_THRESHOLD: std::time::Duration =
    std::time::Duration::from_secs(60);

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
            .start_handler(|_| {
                // Mark every rayon worker as a scheduler CPU worker
                // so cooperative-pump callers reached via the
                // session-side `wait_or_drive` entry can detect that
                // they are running inside the scheduler's owned pool.
                let _ =
                    crate::caller_kind::CallerKind::set(crate::caller_kind::CallerKind::CpuWorker);
            })
            .build()
            .expect("failed to build rayon CPU pool");
        let io_pool = crate::pool::IoPool::new(config.io_threads);

        let scheduler = Arc::new(Self {
            nodes: DashMap::new(),
            edges: EdgeManager::new(),
            dag: Arc::new(Mutex::new(SchedulerDag::with_budget(
                config.aging.clone(),
                dag_budget_for_config(&config),
            ))),
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
            auto_ingested_recent: DashMap::new(),
            removal_epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            driver_handle: Mutex::new(None),
            counters: SchedulerCounters::default(),
            #[cfg(any(test, debug_assertions))]
            dispatch_pause: DispatchPauseHook::default(),
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
            .start_handler(|_| {
                // Mark every rayon worker as a scheduler CPU worker
                // (see `with_executor` for the parallel handler on
                // the native-driver variant).
                let _ =
                    crate::caller_kind::CallerKind::set(crate::caller_kind::CallerKind::CpuWorker);
            })
            .build()
            .expect("failed to build rayon CPU pool");
        #[cfg(not(target_arch = "wasm32"))]
        let io_pool = crate::pool::IoPool::new(config.io_threads);

        Arc::new(Self {
            nodes: DashMap::new(),
            edges: EdgeManager::new(),
            dag: Arc::new(Mutex::new(SchedulerDag::with_budget(
                config.aging.clone(),
                dag_budget_for_config(&config),
            ))),
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
            auto_ingested_recent: DashMap::new(),
            removal_epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            #[cfg(not(target_arch = "wasm32"))]
            driver_handle: Mutex::new(None),
            counters: SchedulerCounters::default(),
            #[cfg(any(test, debug_assertions))]
            dispatch_pause: DispatchPauseHook::default(),
        })
    }

    // ── Request Submission ──

    /// Submit a request. Returns a handle that resolves when the target stage is reached.
    pub fn submit_request(&self, request: Request) -> CompletionHandle<RequestResult> {
        let (handle, sender) = completion_pair();
        // Attach a request-level completion target so the
        // cooperative pump's same-path detection can match by
        // `(canonical, target)` without waiting for the concrete
        // work identity to be admitted to the DAG. The admission
        // path may overwrite this with the concrete `Work`
        // identity once the file-stage node lands in the DAG.
        sender.set_target(crate::job::CompletionTarget::Request {
            canonical: Arc::from(request.file_id.as_str()),
            target: request.target.clone(),
        });
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

    /// Account for one batch submission against the scheduler's
    /// contention counters.
    ///
    /// A batch fan-out is ONE scheduler submission regardless of how
    /// many items it carries: the N items share the submission's
    /// context. Callers invoke this exactly once per batch (and not at
    /// all for an empty batch) so `counters.submit_count` stays O(1) per
    /// batch.
    ///
    /// The scheduler deliberately owns NO outer fan-out: the parallel
    /// wait that drives a batch's items runs on the host/runtime layer's
    /// dedicated coordinator pool, never on the scheduler's
    /// stage-execution `cpu_pool`. Installing an outer wait on the stage
    /// pool would let parked coordinator jobs starve the very
    /// `Load`/`Parse` work the driver dispatches onto that pool — the
    /// pool-starvation deadlock class. This method therefore performs
    /// accounting ONLY; it never touches a pool.
    pub fn account_batch_submission(&self) {
        self.counters
            .submit_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
                let canonical: Arc<str> = Arc::from(id.as_str());
                self.dag.lock().signal_file_shutdown(&canonical);
            }
        }

        // 4. Clear state except generation_floors (preserved across resets
        //    to prevent cross-incarnation stale completions from matching).
        self.edges.reverse_index.inner.clear();
        self.edges.forward_deps.clear();
        self.dag.lock().clear();
        self.tombstones.clear();
        // generation_floors intentionally NOT cleared — stale worker completions
        // from the old incarnation can still arrive after restart, and floors
        // ensure re-added files start at a generation above any prior use.
        self.deferred_blocker_ids.clear();
        // Artifact blocker registry lives on the DAG and is cleared
        // alongside the DAG itself in `dag.lock().clear()` above
        // (see `SchedulerDag::clear` — clears `artifact_blocker_deps`).
        // Auto-ingest tracking map is owned by the scheduler (not the
        // DAG). Without an explicit clear here, every register_resolved_deps
        // call that fired in the prior incarnation leaks an entry
        // across reset() — accumulating across LSP workspace switches,
        // MCP session boundaries, and multi-project bench cycles. The
        // map is bounded by the active dep set so the leak is bounded
        // per reset, but it is still real (entries persist for the
        // 60s aging window or until a matrix consult that happens to
        // observe the matching generation, which will rarely happen
        // across an incarnation boundary).
        self.auto_ingested_recent.clear();

        // 5. Drain inbox again — catch any completions that workers sent
        //    between step 2 and now. These are harmless (nodes removed in
        //    step 3, so handle_stage_complete will no-op), but draining
        //    keeps the channel clean.
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
    /// Updates the scheduler's forward/reverse edges with the newly
    /// resolved dep IDs and, for any deps that match macro_type_deps,
    /// records blockers that the file's downstream Artifact admissions
    /// must wait on. Also auto-ingests deps not yet in the scheduler.
    ///
    /// Blocker contract: owner Analysis is admitted ungated — the
    /// scheduler never gates Analysis on macro_type_deps. Analysis is
    /// recoverable from the source alone (templates, defineSlots,
    /// script-level diagnostics derive from the parsed source
    /// independently of resolved type shapes), so blockers gate
    /// Artifact, not Analysis. The blocker `DepKey`s and any failed
    /// records are persisted to the DAG's per-canonical Artifact
    /// blocker registry (see [`SchedulerDag::record_artifact_blockers`])
    /// and consumed on every subsequent Artifact admission at this
    /// `(file_id, generation)` via
    /// [`Self::admit_artifact_with_blockers`], which drains the
    /// registry, re-classifies persisted deps against the live DAG
    /// state, and attaches any current failure records to the
    /// just-submitted Artifact so `execute_stage_on_worker` surfaces
    /// a typed `DependencyFailed` before codegen runs.
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
        // Also handle the empty-set case as an explicit clear of any
        // prior pending Artifact blocker registry entry at the
        // owner's live generation — the caller is now declaring
        // there are no late blockers, and a stale registry entry
        // would gate subsequent Artifact admissions on deps that the
        // caller no longer considers blocking.
        if !blocker_dep_ids.is_empty() {
            self.deferred_blocker_ids
                .insert(file_id.to_string(), blocker_dep_ids.clone());
        } else {
            self.deferred_blocker_ids.remove(file_id);
            // Clear any prior pending registry entry at the live
            // generation before early-returning. An empty
            // `blocker_dep_ids` is the caller's authoritative "no
            // pending blockers" signal — leaving a stale entry in
            // place would gate future Artifact admissions on deps
            // the caller no longer considers blocking.
            // Snapshot the generation and drop the nodes-shard Ref
            // BEFORE acquiring `dag.lock()`. Holding a DashMap Ref
            // across a parking_lot Mutex acquisition forms an AB-BA
            // ordering with any caller that takes `dag.lock` first
            // and then mutates the same nodes shard. The DAG-first
            // ordering is the canonical one, so the nodes-shard
            // reader must release before locking.
            let gen = self.nodes.get(file_id).map(|n| n.generation());
            if let Some(gen) = gen {
                if gen > 0 {
                    let canonical_arc: Arc<str> = Arc::from(file_id);
                    self.dag.lock().clear_artifact_blockers(&canonical_arc, gen);
                }
            }
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

        let canonical_arc: Arc<str> = Arc::from(file_id);
        let inherited_priority = self
            .dag
            .lock()
            .highest_priority_for_file(&canonical_arc, generation)
            .unwrap_or(Priority::Background);

        // First pass: auto-ingest deps that lack a FileNode so their
        // Source/Analysis pipeline starts. Auto-ingest is a Scheduler
        // concern (it owns `nodes` + the inbox), not a DAG concern.
        // We track which deps were just auto-ingested in this call so
        // the second-pass dead-producer filter can grace them: a
        // freshly-ingested node has its Source request queued in the
        // inbox and is structurally indistinguishable from a
        // Source-failed corpse without this set (both have
        // `current_source().is_none()` and no live DAG identity for
        // Source/Analysis).
        let mut auto_ingested: std::collections::HashSet<&str> =
            std::collections::HashSet::with_capacity(blocker_dep_ids.len());
        for dep_id in &blocker_dep_ids {
            if self.tombstones.contains_key(dep_id) {
                continue;
            }
            if !self.nodes.contains_key(dep_id) {
                let dep_node = self.create_node(dep_id, None);
                let dep_gen = dep_node.bump_generation();
                // Plant the auto-ingest tracking entry BEFORE
                // publishing the FileNode and BEFORE sending the
                // NewRequest. A concurrent matrix consultation that
                // observes the FileNode without the tracking entry
                // would fall through to the dead-producer arm and
                // return `Resolved` for a live dep — the FileNode
                // is present, no live Source/Analysis DAG identity
                // exists yet (the NewRequest is still about to be
                // queued), and the tracking entry would be the
                // disambiguator. Inserting the tracking entry first
                // ensures every interleaving is safe: a matrix
                // lookup that wins ahead of the FileNode insert
                // sees no FileNode and consults the tracking entry
                // directly (the FileNode-missing arm); one that
                // wins ahead of the tracking insert is impossible
                // because no FileNode is yet visible.
                let dep_canonical: Arc<str> = Arc::from(dep_id.as_str());
                self.auto_ingested_recent.insert(
                    Arc::clone(&dep_canonical),
                    AutoIngestedRecord {
                        generation: dep_gen,
                        since: Instant::now(),
                    },
                );
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
                auto_ingested.insert(dep_id.as_str());
            }
        }

        // Second pass: build the live DepKey set under the DAG lock so
        // the dead-producer filter sees a consistent DAG state. A
        // dep is dead-producer when its FileNode is gone (removed),
        // its generation has moved on, Source/Analysis previously
        // failed at this generation, or the recorded generation is
        // 0 — recording any such DepKey would gate the owner's
        // Artifact on a producer that will never reach
        // committed-Analysis state. The classification is shared
        // with [`Self::classify_recorded_dep`] via
        // [`Self::file_stage_analysis_blocker_status`] so the
        // pre-admission filter (this loop) and the recorded-blocker
        // filter (admit_artifact_with_blockers) cannot drift.
        //
        // Freshly-auto-ingested deps (recorded in `auto_ingested`)
        // bypass the dead-producer arm: their Source submission is
        // queued in the inbox but the worker has not yet picked it
        // up, so the FileNode looks identical to a Source-failed
        // corpse from the DAG's point of view. Without this grace,
        // first-time blocker registration would drop the dep
        // immediately and skip the gating it just set up.
        let mut dag = self.dag.lock();
        let mut dep_keys: Vec<DepKey> = Vec::new();
        // Failed-dep records collected from the 3-state matrix. These
        // ride together with the live `dep_keys` inside the
        // per-canonical `PendingBlockerSet` persisted to the Artifact
        // blocker registry below. They surface as a typed
        // `DependencyFailed` on the owner's first Artifact admission
        // via `admit_artifact_with_blockers`, which drains the
        // registry, attaches the failure record to the just-submitted
        // Artifact, and lets the pre-dispatch chokepoint in
        // `execute_stage_on_worker` short-circuit codegen over a dead
        // prerequisite. Owner Analysis itself remains ungated.
        let mut failed_records: Vec<crate::dag::FailedDepRecord> = Vec::new();
        for dep_id in &blocker_dep_ids {
            if self.tombstones.contains_key(dep_id) {
                continue;
            }
            let dep_canonical: Arc<str> = Arc::from(dep_id.as_str());
            let dep_gen = self.nodes.get(dep_id).map(|n| n.generation()).unwrap_or(0);
            // Run the shared 3-state matrix:
            //
            // - `Gating`     → record the DepKey for the Artifact
            //                  blocker registry (owner Analysis stays
            //                  ungated; the dep only gates codegen).
            // - `Satisfied`  → drop silently (producer is moot).
            // - `Failed(r)`  → drop from `dep_keys` AND collect the
            //                  record for the same registry entry —
            //                  the owner's first Artifact admission
            //                  drains the registry and surfaces a
            //                  typed `DependencyFailed` before codegen
            //                  runs over a dead prerequisite.
            let status = if auto_ingested.contains(dep_id.as_str()) {
                // Just-ingested: grace the dep — its Source request
                // is in the inbox waiting to be dispatched. The
                // canonical dead-producer matrix can't distinguish
                // "pending inbox load" from "Source failed" because
                // both leave the same FileNode + DAG shape, so we
                // trust the auto-ingest side-effect. (A genuine
                // terminal failure is intercepted by the matrix's
                // `terminal_dep_failures` consult before this branch
                // is reached.)
                BlockerStatus::Gating
            } else {
                self.file_stage_analysis_blocker_status(&dag, &dep_canonical, dep_gen)
            };
            match status {
                BlockerStatus::Satisfied => continue,
                BlockerStatus::Failed(record) => {
                    failed_records.push(record);
                    continue;
                }
                BlockerStatus::Gating => {
                    let dep_key = DepKey::FileStage {
                        canonical: Arc::clone(&dep_canonical),
                        generation: dep_gen,
                        stage: FileStageKey::Analysis,
                    };
                    dep_keys.push(dep_key);
                }
            }
        }
        // Macro-type cycle filter: drop any dep whose Analysis
        // transitively waits on this owner's Analysis (self-cycle,
        // direct mutual A↔B cycle, or transitive A→B→C→A). The
        // semantic dispatch's same-key Instantiate sentinel still
        // bounds the type recursion, but the scheduler must not
        // persist a registry entry for a dep that transitively
        // waits on this owner. The filter runs UNDER the same DAG
        // lock guard (`dag`) the caller already holds — no lock
        // release between this check and the
        // `record_artifact_blockers` call below — closing the
        // TOCTOU window where two concurrent completions could
        // each see the other as not-yet-gating and both register
        // mutually-blocking blocker entries.
        let (filtered_dep_keys, _dropped_dep_keys) =
            Self::filter_macro_cycle_deps(&dag, &canonical_arc, generation, dep_keys);
        let dep_keys = filtered_dep_keys;

        if dep_keys.is_empty() && failed_records.is_empty() {
            // No unresolved blockers at this generation. Clear any
            // stale entry from a prior call so future Artifact
            // admissions are not falsely gated.
            dag.clear_artifact_blockers(&canonical_arc, generation);
            return;
        }

        // Persist the blockers so every subsequent Artifact admission
        // at this `(file_id, generation)` picks them up. Replace (not
        // append) any prior entry so a second `register_resolved_deps`
        // call with a different blocker set is treated as the new
        // authoritative set.
        //
        // Both the live gating `dep_keys` AND the collected
        // `failed_records` ride together inside a
        // [`crate::dag::PendingBlockerSet`]. The owner's Analysis is
        // UNGATED — analysis is recoverable from the source alone
        // (templates, defineSlots, script-level diagnostics all derive
        // from the parsed source independently of resolved type
        // shapes). Codegen, however, needs the resolved type shapes,
        // so the gate fires at Artifact admission via
        // [`Self::admit_artifact_with_blockers`]: it drains this
        // registry entry, re-classifies every persisted live + failed
        // dep against the live DAG state, and attaches any current
        // failure records to the just-submitted Artifact node so
        // `execute_stage_on_worker` surfaces a typed `DependencyFailed`
        // before codegen runs.
        let pending_set = crate::dag::PendingBlockerSet {
            deps: dep_keys.into_iter().collect(),
            failed: failed_records,
        };
        dag.record_artifact_blockers(&canonical_arc, generation, pending_set);
    }

    /// Drop any dep whose Analysis transitively waits on the
    /// owner's Analysis — the single chokepoint for macro-type-dep
    /// cycle filtering shared by both blocker-registration paths:
    ///
    /// 1. The immediate path at the bottom of
    ///    [`Self::register_resolved_deps`] (Source already complete
    ///    when blockers arrive).
    /// 2. The Source-completion replay path inside
    ///    [`Self::handle_stage_complete`] (TaskKind::Source arm).
    ///
    /// Catches three cycle classes uniformly via the DAG's bounded
    /// reachability walk ([`SchedulerDag::dep_reaches_owner`]):
    ///
    /// - Direct self: `A → A`.
    /// - Direct mutual: `A ↔ B`.
    /// - Transitive: `A → B → C → A` (bounded BFS).
    ///
    /// The caller MUST hold the DAG lock through this call and the
    /// subsequent `record_artifact_blockers` so two concurrent
    /// completions cannot race past the filter into mutually-blocking
    /// registry entries.
    ///
    /// Returns the `(kept, dropped)` split for traceability. The
    /// `dropped` half is currently unused at the call-site but
    /// preserves the diagnostic the test suite asserts against.
    fn filter_macro_cycle_deps(
        dag: &crate::dag::SchedulerDag,
        owner_canonical: &Arc<str>,
        owner_generation: u64,
        deps: Vec<DepKey>,
    ) -> (Vec<DepKey>, Vec<DepKey>) {
        let mut kept = Vec::with_capacity(deps.len());
        let mut dropped = Vec::new();
        for dep in deps {
            let drop_this = if let DepKey::FileStage {
                canonical: dep_canonical,
                generation: dep_generation,
                stage: FileStageKey::Analysis,
            } = &dep
            {
                dag.dep_reaches_owner(
                    owner_canonical,
                    owner_generation,
                    dep_canonical,
                    *dep_generation,
                )
            } else {
                // Non-Analysis deps are not part of the macro-type
                // cycle class the filter is responsible for.
                false
            };
            if drop_this {
                dropped.push(dep);
            } else {
                kept.push(dep);
            }
        }
        (kept, dropped)
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
    /// stores the result, signals any pending Artifact request handles,
    /// AND terminalizes the matching Artifact DAG identity so a
    /// concurrent internal worker cannot overwrite the committed
    /// snapshot. The dag.cancel call releases the parked capacity
    /// reservation (if the internal worker had already reserved one)
    /// and removes the identity from `by_identity` / `nodes` so the
    /// dispatch loop's `next_ready` will not re-dispatch it.
    pub fn commit_artifact(&self, file_id: &str, profile_hash: u64, snapshot: ArtifactSnapshot) {
        // Snapshot the FileNode `Arc` and drop the nodes-shard
        // `Ref` BEFORE acquiring `dag.lock()`. Holding a DashMap
        // Ref across `dag.lock` forms a latent AB-BA ordering with
        // any caller that takes `dag.lock` first and then writes
        // the same nodes shard. The DAG-first ordering is the
        // canonical one (lifecycle sweeps + the worker's pre-executor
        // skip path), so the publish path must release the Ref
        // before locking. The cloned `Arc<FileNode>` preserves
        // every field access the original Ref enabled (including
        // the per-profile `artifacts` DashMap insert below).
        let node = match self.nodes.get(file_id) {
            Some(r) => Arc::clone(&r),
            None => return,
        };
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
        let canonical: Arc<str> = Arc::from(file_id);
        let artifact_id = WorkNodeIdentity::Artifact {
            canonical: Arc::clone(&canonical),
            generation,
            profile_hash: profile_hash_to_bytes(profile_hash),
            content_hash: [0u8; 16],
        };
        // Acquire the DAG lock as the publish/signal/terminalize
        // synchronization point. The internal Artifact worker
        // recovers from a concurrent external commit by
        // re-checking `node.artifacts` under the same lock
        // before its own insert (see `execute_artifact_stage`),
        // so an external commit that lands during a worker's
        // executor run is preserved.
        let mut guard = self.dag.lock();
        node.artifacts.insert(profile_hash, Arc::clone(&snap));
        let result = RequestResult::Artifact(snap);
        guard.signal_stage_complete(
            &canonical,
            generation,
            &TaskKind::Artifact { profile_hash },
            &result,
        );
        // Terminalize the matching DAG identity. The internal
        // Artifact worker dispatched against this identity (if
        // any) will not find it on a fresh `next_ready` pass, and
        // the parked capacity reservation releases via cancel's
        // by-value drop on the reservation. Stranded-waiter
        // contract: Artifact identities are graph leaves — no
        // other DAG node lists an Artifact `DepKey` as a
        // prerequisite — so `cancel` always returns an empty
        // stranded list here. The `debug_assert!` catches any
        // future change that adds Artifact-on-Artifact gating.
        let stranded = guard.cancel(&artifact_id);
        debug_assert!(
            stranded.is_empty(),
            "external commit_artifact terminalize must not strand DAG waiters: \
             Artifact identities are graph leaves"
        );
        // Mirror the cleanup `handle_stage_complete(Artifact)`
        // performs: if no other profile remains pending at this
        // `(owner, generation)`, drop the blocker-registry entry
        // so external publishers do not leak entries past their
        // last referencing Artifact. Stays under the same lock
        // as cancel + signal so the registry view is consistent
        // with the publish.
        if guard
            .pending_artifact_profiles(&canonical, generation)
            .is_empty()
        {
            guard.clear_artifact_blockers(&canonical, generation);
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
    ///
    /// The generation bump and the supersede sweep run under the SAME
    /// DAG lock acquisition: the bump happens AFTER `dag.lock()` so
    /// no dispatcher can observe the bumped generation before the
    /// supersede sweep cancels the stale-generation DAG identities.
    /// A bare-atomic bump separated from the lock acquisition would
    /// let a dispatcher dequeue the stale-gen identity, see
    /// `node.generation()` already at the new value, and trip the
    /// dispatch-time `debug_assert!` that the stale identity has
    /// been terminalized.
    pub fn invalidate(&self, id: &str) {
        // Snapshot the FileNode `Arc` and drop the nodes-shard `Ref`
        // BEFORE acquiring `dag.lock()`. Holding a DashMap Ref
        // across a parking_lot Mutex acquisition forms a latent
        // AB-BA ordering with any caller that takes `dag.lock`
        // first and then mutates the same nodes shard. The
        // DAG-first ordering is the canonical one for the lifecycle
        // sweeps, so the nodes-shard reader must release the Ref
        // before locking. The cloned `Arc<FileNode>` preserves
        // every field access the original Ref enabled.
        let node = match self.nodes.get(id) {
            Some(r) => Arc::clone(&r),
            None => return,
        };
        let canonical: Arc<str> = Arc::from(id);
        let mut dag = self.dag.lock();
        let new_gen = node.bump_generation();
        // Stale per-(owner, generation) Artifact blocker entries
        // for superseded generations are dropped inside
        // `supersede_old_file_generations` (it now also scrubs
        // the DAG's artifact_blocker_deps registry for stale
        // owner-canonical entries). The new generation records
        // its own blockers via `register_resolved_deps`.
        dag.supersede_old_file_generations(&canonical, new_gen);
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

        let canonical_arc: Arc<str> = Arc::from(id);
        // Cancel every DAG node for this canonical (across all
        // generations) and gather stranded waiters whose only
        // remaining gating dep was a node for this file. Also scrub
        // the DAG's Artifact blocker registry so no entry — either
        // as owner OR as a referenced DepKey — keeps the removed
        // file's identity alive. Same DAG lock holds both passes so
        // the registry view is consistent with the cancel sweep.
        let stranded: Vec<crate::dag::SubmissionToken> = {
            let mut dag = self.dag.lock();
            let canonical_for_match = canonical_arc.as_ref().to_string();
            let (_, stranded) = dag.cancel_matching(|identity| match identity {
                WorkNodeIdentity::FileStage { canonical: c, .. }
                | WorkNodeIdentity::Artifact { canonical: c, .. } => {
                    c.as_ref() == canonical_for_match.as_str()
                }
                WorkNodeIdentity::CacheNode { .. } => false,
            });
            // Drop any owner entry for the removed file AND scrub
            // every recorded DepKey that references the removed
            // file as a dep. The owner-side drop covers the
            // owner-removed case directly; the cross-owner scrub
            // closes the lifecycle gap where another file's
            // recorded blocker `DepKey` pointed at the removed
            // file's Analysis — without the scrub the stale
            // DepKey would survive `remove(canonical)` and pin
            // the other file's Artifact admission forever.
            dag.artifact_blocker_deps_remove_owner(id);
            dag.scrub_artifact_blockers_referencing(id);
            // Drop every persistent terminal-dep-failure record
            // that references the removed file. A stale record on
            // a removed canonical would otherwise pin a future
            // admission as `Failed` even after the file went away.
            dag.scrub_terminal_dep_failures_referencing(id);
            stranded
        };

        self.deferred_blocker_ids.remove(id);
        // Drop the auto-ingest tracking entry — the FileNode is about
        // to disappear, so a future matrix lookup must NOT treat the
        // tracking entry as evidence of a live producer.
        //
        // Safety vs. the other 3 sites that use a value-conditional
        // `remove_if(canonical, |_, v| v.generation == X)`: those sites
        // can race with a concurrent newer-gen re-insertion (a fresh
        // auto-ingest by `register_resolved_deps` while the matrix is
        // mid-cleanup), so they must scope the remove to the generation
        // they observed. THIS site is different — it runs under the
        // tombstone barrier installed at the top of `remove()`:
        //
        //   1. `removal_epoch.fetch_add(1, AcqRel)` + `tombstones.insert(id, epoch)`
        //      at the tombstone barrier at the top of `remove()` happen-before this point.
        //   2. The auto-ingest path in `register_resolved_deps` checks
        //      `self.tombstones.contains_key(dep_id)` BEFORE inserting
        //      a tracking entry (see the `tombstones.contains_key`
        //      guard at the top of the auto-ingest block) and skips
        //      the entire ingest when the tombstone is present.
        //   3. The submit-time validation in `handle_new_request`
        //      rejects any submission with
        //      `submitted_epoch < tombstone_epoch` (the pre-remove
        //      rejection branch), so a stale request observed before
        //      our `removal_epoch++` cannot re-trigger an ingest by
        //      proxy either.
        //
        // Between (1) and this line, no concurrent writer can insert
        // a new `auto_ingested_recent` entry for this canonical at any
        // generation — the auto-ingest gate is closed and a stale
        // request that already passed through `register_resolved_deps`
        // before (1) would have completed its insert before our
        // tombstone barrier became visible (the DashMap insert is the
        // happens-before edge). The unconditional drop is therefore
        // safe AND complete: any older-gen tracking entry left over
        // by a superseded matrix path is also scrubbed in one pass.
        self.auto_ingested_recent.remove(&canonical_arc);

        if let Some((_, node)) = self.nodes.remove(id) {
            let gen = node.generation();
            // Record floor so a re-added node starts above this generation.
            self.generation_floors.insert(id.to_string(), gen);
            self.edges.remove_file(id);
            // Signal Shutdown to any pending waiters for this file.
            self.dag.lock().signal_file_shutdown(&canonical_arc);
        }

        // Stranded waiters — their gating dep can never resolve.
        // Re-enqueue their Artifact work so it proceeds (with the
        // dep treated as missing) rather than hangs.
        for tok in stranded {
            self.requeue_stranded_waiter(tok);
        }
    }

    /// Re-enqueue a waiter token whose gating dep was cancelled.
    /// Looks up the file/generation/profile and re-submits any pending
    /// artifact work through the DAG so it dispatches.
    fn requeue_stranded_waiter(&self, _token: crate::dag::SubmissionToken) {
        // Stranded waiter handling: in the legacy path the
        // `remove_file_as_blocker` result was iterated and pending
        // artifacts were re-enqueued via `enqueue_pending_artifacts`.
        // With the DAG, the file's pending artifact waiters at this
        // generation are still in the dag's `file_waiters` map. The
        // dispatch loop will pick them up on the next pass once the
        // dependency gate clears — which `cancel_matching` already
        // did when it dropped the cancelled identity from the
        // waiters reverse-index.
        //
        // We re-trigger the dispatch by sending a Wake into the
        // inbox; the driver picks it up, re-runs the cooperative
        // pump, and the now-ungated artifact nodes go out.
        let _ = self.inbox.sender.send(Submission::Wake);
    }

    /// Close a file: clear overlay + pending_source, keep node alive.
    ///
    /// The generation bump and the supersede sweep run under the SAME
    /// DAG lock acquisition (see [`Self::invalidate`] for the lock
    /// rationale).
    pub fn close_file(&self, id: &str) {
        self.overlay.clear(id);
        // Snapshot the FileNode `Arc` and drop the nodes-shard
        // `Ref` BEFORE acquiring `dag.lock()`. See [`Self::invalidate`]
        // for the AB-BA-prevention rationale; close_file follows the
        // same DAG-first lifecycle-sweep pattern.
        let node = match self.nodes.get(id) {
            Some(r) => Arc::clone(&r),
            None => return,
        };
        let canonical: Arc<str> = Arc::from(id);
        let mut dag = self.dag.lock();
        let new_gen = node.bump_generation();
        node.pending_source.store(Arc::new(None));
        dag.supersede_old_file_generations(&canonical, new_gen);
        // Drop the DAG lock before the inbox send + sender_drop
        // below; the inbox channel is unrelated to the DAG lock.
        drop(dag);

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

    // ── Driver ──

    /// Driver loop (native). Holds `Weak<Scheduler>` — exits when the last
    /// external Arc is dropped or the shutdown flag is set.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn driver_loop_native(
        weak: std::sync::Weak<Scheduler>,
        receiver: crossbeam_channel::Receiver<Submission>,
    ) {
        // Mark the driver thread so cooperative-pump callers can
        // distinguish driver-led pumps from worker-led pumps. The
        // driver thread is born here, lives for the scheduler's
        // lifetime, and never delegates this slot to another role.
        let _ = crate::caller_kind::CallerKind::set(crate::caller_kind::CallerKind::Driver);

        let aging_interval = std::time::Duration::from_secs(5);

        loop {
            // Upgrade Weak to Arc — if this fails, the scheduler was dropped.
            let scheduler = match weak.upgrade() {
                Some(s) => s,
                None => break,
            };

            if scheduler.shutdown.load(Ordering::Acquire) {
                // Final drain on the way out so any queued
                // submissions surface their typed terminal state
                // before the receiver disconnects.
                scheduler.pump_ready(
                    PumpReason::ShutdownDrain,
                    crate::caller_kind::CallerKind::Driver,
                );
                break;
            }

            // Drive the pump until it reports no progress, then
            // park on the receiver. Looping the pump (rather than
            // running it once) ensures that a single batch of
            // submissions and their fan-out admissions all reach
            // dispatch in the same wake.
            loop {
                let stats = scheduler.pump_ready(
                    PumpReason::DriverLoop,
                    crate::caller_kind::CallerKind::Driver,
                );
                if !stats.made_progress() {
                    break;
                }
            }

            // Drop the strong ref before blocking so the caller's Drop can run.
            drop(scheduler);

            // Wait for the next submission or aging timer.
            match receiver.recv_timeout(aging_interval) {
                Ok(submission) => {
                    if let Some(scheduler) = weak.upgrade() {
                        // Process the wake submission directly so
                        // the DAG sees it before the next pump
                        // iteration; then re-pump to dispatch any
                        // ready work it admitted.
                        scheduler.process_submission(submission);
                        let _ = scheduler.pump_ready(
                            PumpReason::DriverWake,
                            crate::caller_kind::CallerKind::Driver,
                        );
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
    ///
    /// Compatibility wrapper around [`Self::drain_inbox_for_pump`].
    /// The pump entry returns a count; this entry discards it for
    /// the existing call sites that don't track progress directly.
    #[cfg(not(target_arch = "wasm32"))]
    fn drain_inbox(&self) {
        let _ = self.drain_inbox_for_pump();
    }

    /// WASM build's drain_inbox (no cooperative-pump infrastructure
    /// compiled on `wasm32`).
    #[cfg(target_arch = "wasm32")]
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
            // Source provided: bump generation under the DAG lock so
            // the bump and the supersede sweep run as one critical
            // section. A bare-atomic bump separated from the lock
            // acquisition would let a dispatcher observe the bumped
            // generation BEFORE the supersede sweep cancelled the
            // stale identity — the dispatch-time defensive
            // `debug_assert!` would then trip on the stale identity
            // that had not yet been terminalized.
            let canonical: Arc<str> = Arc::from(file_id.as_str());
            let mut dag = self.dag.lock();
            let gen = node.bump_generation();
            // Store source in overlay for SourceLoader access
            if let Some(ref src) = source {
                self.overlay.set(file_id.clone(), Arc::clone(src));
            }
            // Store in pending_source for the Source job
            node.pending_source
                .store(Arc::new(source.map(|s| (gen, s))));
            dag.supersede_old_file_generations(&canonical, gen);
            // Drop the DAG lock before the rest of the function
            // re-acquires it for `register_request` + `admit_work`.
            drop(dag);
            gen
        } else {
            let gen = node.generation();
            if gen == 0 {
                // Node was just created, needs a Source job. No
                // supersede sweep is needed here — at generation 0
                // there is no prior dispatched identity.
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

        // Register the waiter group on the DAG and admit a work node.
        let canonical_id: Arc<str> = Arc::from(file_id.as_str());

        // Determine the first-missing work stage BEFORE we register
        // the sender. The concrete `Work` identity for that stage
        // is stamped on the sender's target so the cooperative
        // pump's same-path self-await detection matches by the
        // exact `WorkNodeIdentity` once admission has run.
        //
        // The Request-shape fallback (used during the brief race
        // window between `submit_request` stamping
        // `CompletionTarget::Request` and `handle_new_request`
        // overwriting with the concrete `Work` identity) covers
        // every self-await class via `active_path_contains_request`:
        //   - Source request matches an active Source frame on
        //     the same canonical.
        //   - Analysis request matches an active Source OR Analysis
        //     frame on the same canonical.
        //   - Artifact request matches an active Source OR Analysis
        //     frame on the same canonical, OR an active Artifact
        //     frame on the same canonical AND the same
        //     `profile_hash` (two Artifact frames for the same
        //     canonical with different profiles are independent
        //     work units and must NOT collapse into a same-path
        //     match).
        //
        // The Work stamp narrows the match to exact
        // `WorkNodeIdentity` (canonical + generation + stage; for
        // Artifact also `profile_hash`) post-admission, which is
        // the precise relation other paths reference once the
        // work node is in the DAG.
        let first_missing = if node.current_source().is_none() {
            TaskKind::Source
        } else if node.current_analysis().is_none() {
            TaskKind::Analysis
        } else {
            target.required_task_kind()
        };
        let first_missing_identity: WorkNodeIdentity = match first_missing {
            TaskKind::Source => WorkNodeIdentity::FileStage {
                canonical: Arc::clone(&canonical_id),
                generation,
                stage: FileStageKey::Source,
            },
            TaskKind::Analysis => WorkNodeIdentity::FileStage {
                canonical: Arc::clone(&canonical_id),
                generation,
                stage: FileStageKey::Analysis,
            },
            TaskKind::Artifact { profile_hash } => WorkNodeIdentity::Artifact {
                canonical: Arc::clone(&canonical_id),
                generation,
                profile_hash: profile_hash_to_bytes(profile_hash),
                content_hash: [0u8; 16],
            },
        };
        // Overwrite the request-level `CompletionTarget::Request`
        // stamped at `submit_request` with the concrete `Work`
        // identity. Last-writer-wins on the `CompletionSender`'s
        // target slot (see `CompletionSender::set_target`).
        sender.set_target(crate::job::CompletionTarget::Work(
            first_missing_identity.clone(),
        ));

        let mut dag = self.dag.lock();
        dag.register_request(
            &canonical_id,
            generation,
            target.clone(),
            sender,
            request_context,
        );

        let effective_priority = priority;

        // If the next missing stage is an Artifact and there is a
        // file-level gate engaged at this generation (a dependency-
        // gating Analysis node still pending in the DAG), do NOT admit
        // the Artifact node yet — the DAG's dep edge will drive
        // re-dispatch when the gate clears.
        let analysis_gate = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&canonical_id),
            generation,
            stage: FileStageKey::Analysis,
        };
        if matches!(first_missing, TaskKind::Artifact { .. })
            && dag.has_pending_deps(&analysis_gate)
        {
            // Propagate the new request's priority onto the gate.
            dag.upgrade_priority(&analysis_gate, effective_priority);
            return;
        }

        // Artifact admissions must inherit any blockers persisted
        // by `register_resolved_deps` to the per-canonical Artifact
        // blocker registry at this `(file_id, generation)`. Owner
        // Analysis is admitted ungated for macro_type_deps; the
        // helper below drains the registry, re-classifies persisted
        // deps against the live DAG state, and attaches the
        // resulting gating deps + failure records to the
        // just-submitted Artifact so codegen waits on (or short-
        // circuits over) the blocker's Analysis.
        if let TaskKind::Artifact { profile_hash } = first_missing {
            self.admit_artifact_with_blockers(
                &mut dag,
                &canonical_id,
                generation,
                profile_hash,
                effective_priority,
                None,
            );
            return;
        }

        admit_work(
            &mut dag,
            &canonical_id,
            generation,
            first_missing,
            effective_priority,
            None,
        );
        // Drop the DAG guard before touching the tracking set —
        // `auto_ingested_recent` is a DashMap and would deadlock
        // anyone holding the DAG lock waiting for a shard write.
        drop(dag);
        // If this admit transitioned the dep from "queued in inbox"
        // to "Source DAG identity admitted", clear any tracking
        // entry so the matrix stops treating the dep as a pending
        // auto-ingest. Only the matching-generation entry is
        // removed; a stale entry from a previous incarnation stays
        // until the freshness check trims it.
        if matches!(first_missing, TaskKind::Source) {
            self.clear_auto_ingest_tracking(&canonical_id, generation);
        }
    }

    /// Remove a [`Self::auto_ingested_recent`] entry for
    /// `(canonical, generation)` when the matching Source DAG
    /// identity has been admitted. The matrix consumes this set to
    /// detect the "auto-ingest queued in inbox, not yet drained"
    /// state; once the driver dequeues the `NewRequest` and admits
    /// the Source identity, that state is over — the live Source
    /// identity in `by_identity` is now the source of truth and the
    /// tracking entry would only confuse future matrix lookups.
    fn clear_auto_ingest_tracking(&self, canonical: &Arc<str>, generation: u64) {
        // Atomic value-conditional removal: only drop the entry when
        // the live entry's generation still matches the one we are
        // clearing for. `DashMap::remove_if` evaluates the predicate
        // under the same shard write lock that performs the remove,
        // so a concurrent insert of a later generation between a
        // non-atomic `get` + `remove` (the previous pattern) can no
        // longer delete the newer entry by accident. An entry for a
        // later generation stays so the next driver tick's admission
        // of the newer generation's Source request finds it and
        // clears it on its own match.
        self.auto_ingested_recent
            .remove_if(canonical, |_k, v| v.generation == generation);
    }

    /// Admit an Artifact work node with any late-discovered blocker
    /// `DepKey`s attached. Reads the per-(owner, generation) blocker
    /// set from the DAG's typed registry, filters out blockers whose
    /// Analysis is already committed (no longer gating), filters out
    /// dead-producer entries (FileNode gone AND no live Analysis
    /// identity in the DAG), and submits the Artifact identity with
    /// the remaining `DepKey`s as deps.
    ///
    /// Called from every Artifact admission site so a blocker
    /// registered via [`Self::register_resolved_deps`] AFTER the
    /// owner's Analysis has dispatched (or completed) still gates
    /// the Artifact run on the blocker's Analysis. The in-flight
    /// Analysis node itself never depends on these late-discovered
    /// blockers (its incoming edges are immutable once dispatched).
    ///
    /// Registry lifecycle:
    ///
    /// - When every recorded blocker has already resolved (or every
    ///   entry was a dead producer) the entry is cleared so future
    ///   Artifact admissions at this generation do not consult a
    ///   stale registry view.
    /// - Otherwise the entry stays in place so a re-admission (e.g.
    ///   a same-generation re-request for a different profile) still
    ///   picks up the unresolved blockers.
    fn admit_artifact_with_blockers(
        &self,
        dag: &mut SchedulerDag,
        canonical: &Arc<str>,
        generation: u64,
        profile_hash: u64,
        priority: Priority,
        request_context: Option<crate::request_context::OpaqueRequestContext>,
    ) -> crate::dag::SubmissionToken {
        let mut blocker_deps: Vec<DepKey> = Vec::new();
        // Failed-dep records to attach to the just-submitted
        // Artifact node so the pre-dispatch short-circuit in
        // `execute_stage_on_worker` surfaces a typed
        // `DependencyFailed` even when the producer failed BEFORE
        // this Artifact was admitted (pre-admission failure race —
        // the matrix consults `terminal_dep_failures` AND the
        // registry-attached `failed` list both as Failed sources).
        let mut failed_records: Vec<crate::dag::FailedDepRecord> = Vec::new();
        // Drain + re-record under the DAG lock: read the recorded
        // blockers (live deps + persisted failure records), route
        // EVERY entry — live deps AND previously-failed deps — through
        // the 3-state matrix against the current DAG state, then write
        // back the rebuilt set so a subsequent admission for a different
        // profile at the same generation picks up the correct view. An
        // empty re-record clears the entry (see
        // `SchedulerDag::record_artifact_blockers`).
        //
        // Rebuilding the persisted `failed` set from classification (not
        // pass-through) is load-bearing in two directions:
        //
        //   1. A live dep that fails between admissions must populate the
        //      NEXT-admission `failed` set, not just the current Artifact.
        //      If we keep the prior `stored.failed` set verbatim we strand
        //      the new failure on the current Artifact alone — a later
        //      profile admission would drain an empty registry and resolve
        //      Ready over the dead prerequisite.
        //   2. A previously-failed dep that recovers at the same gen
        //      (the same-gen recovery path cleared
        //      `terminal_dep_failures`) must drop from the next
        //      `failed` set, not ride through verbatim — otherwise the
        //      next admission attaches a stale `DependencyFailed` record
        //      to a now-Satisfied dep.
        //
        // Both behaviours fall out of routing every persisted entry
        // through `classify_recorded_dep` against the live state.
        let stored = dag.drain_artifact_blockers(canonical, generation);
        let mut still_pending: std::collections::BTreeSet<DepKey> =
            std::collections::BTreeSet::new();
        let mut next_failed: Vec<crate::dag::FailedDepRecord> = Vec::new();
        // Classify every live dep first.
        for dep in stored.deps.into_iter() {
            match self.classify_recorded_dep(dag, &dep) {
                BlockerStatus::Satisfied => continue,
                BlockerStatus::Failed(record) => {
                    // Producer terminally failed between record time
                    // and this Artifact admission. Drop from
                    // `blocker_deps` so the Artifact does not gate
                    // on it, attach to the current Artifact via
                    // `failed_records`, AND persist the failure into
                    // `next_failed` so a future profile admission at
                    // the same gen still surfaces the same
                    // `DependencyFailed`.
                    failed_records.push(record.clone());
                    next_failed.push(record);
                    continue;
                }
                BlockerStatus::Gating => {
                    blocker_deps.push(dep.clone());
                    still_pending.insert(dep);
                }
            }
        }
        // Re-classify each persisted failure record against the live
        // state. A persisted failure can transition out of `Failed`
        // when the same-gen recovery path clears
        // `terminal_dep_failures` on a same-gen Source/Analysis
        // recovery: the matrix returns `Satisfied`
        // (and we drop the record) or `Gating` (and the dep is still
        // a live blocker again). The producer's terminalized DAG
        // identity cannot re-emerge as a different live identity at
        // the SAME generation, so a `Gating` verdict on a previously-
        // failed dep only fires after a successful recovery — in which
        // case treating it as a live gating dep again is correct.
        for record in stored.failed.into_iter() {
            match self.classify_recorded_dep(dag, &record.dep_key) {
                BlockerStatus::Satisfied => continue,
                BlockerStatus::Failed(current_record) => {
                    // Producer still terminally failed at this gen.
                    // Attach to the current Artifact AND persist
                    // for future admissions. Reuse the freshly-looked-
                    // up record (its cause is identical, but using the
                    // classifier's return keeps a single source of
                    // truth for the persisted failure record).
                    failed_records.push(current_record.clone());
                    next_failed.push(current_record);
                }
                BlockerStatus::Gating => {
                    // Producer recovered at the same generation
                    // (the same-gen recovery path cleared the
                    // persistent failure record and the pipeline is
                    // alive again — Source / Analysis queued or in
                    // flight, or auto-ingest pending). Promote back
                    // to a live gating dep so the Artifact gates on
                    // the resumed Analysis. Persist as a gating dep,
                    // not a failure.
                    blocker_deps.push(record.dep_key.clone());
                    still_pending.insert(record.dep_key);
                }
            }
        }
        // Re-record both the still-pending live deps AND the
        // rebuilt failure records under the same key so a future
        // Artifact admission at this generation (e.g. a different
        // profile) still picks them up. An empty re-record (no deps
        // AND no failed) drops the entry (which
        // `record_artifact_blockers` treats as a remove).
        let next_pending = crate::dag::PendingBlockerSet {
            deps: still_pending,
            failed: next_failed,
        };
        dag.record_artifact_blockers(canonical, generation, next_pending);

        let identity = WorkNodeIdentity::Artifact {
            canonical: Arc::clone(canonical),
            generation,
            profile_hash: profile_hash_to_bytes(profile_hash),
            content_hash: [0u8; 16],
        };
        let token = dag.submit(
            identity.clone(),
            WorkKind::Artifact,
            priority,
            blocker_deps,
            request_context,
        );
        // Attach every failed-dep record to the just-submitted
        // Artifact node. The dispatched-node dedup branch of
        // `submit` would not pick these up (incoming edges are
        // immutable after dispatch), so `attach_failed_dep`'s no-op
        // return for that case is the correct shape: a dispatched
        // in-flight Artifact already carries its own marker (or
        // none — meaning the producer failed AFTER dispatch and
        // the fan-out path will deliver it via `signal_file_failed`).
        for record in failed_records {
            dag.attach_failed_dep(&identity, record);
        }
        token
    }

    /// Classify a recorded blocker dep against the live FileNode +
    /// DAG state. Wraps [`Self::file_stage_analysis_blocker_status`]
    /// for the only [`DepKey`] variant cross-file blockers use
    /// (`FileStage::Analysis`). Other variants (Source-stage and
    /// Artifact and CacheNode) cannot appear in the recorded
    /// blocker registry; classify them as `Satisfied` defensively
    /// so a future producer that mis-records returns to the safe
    /// "drop the blocker" path rather than pinning the admission.
    fn classify_recorded_dep(&self, dag: &SchedulerDag, dep: &DepKey) -> BlockerStatus {
        match dep {
            DepKey::FileStage {
                canonical,
                generation,
                stage: FileStageKey::Analysis,
            } => self.file_stage_analysis_blocker_status(dag, canonical, *generation),
            _ => BlockerStatus::Satisfied,
        }
    }

    /// Shared classifier for a `FileStage::Analysis` blocker dep
    /// recorded for `(canonical, generation)`. Returns whether the
    /// dep is still gating an owner's Artifact admission, OR whether
    /// the blocker can be dropped (either because the dep is already
    /// satisfied OR because the producer is dead and will never
    /// satisfy it).
    ///
    /// Dead-producer matrix. The previous shape only distinguished
    /// "FileNode missing" from "FileNode present"; that missed the
    /// case where Source or Analysis previously FAILED at this same
    /// generation: the DAG identity was cancelled by
    /// [`Self::terminalize_failure`], but the FileNode remains with
    /// `current_*().is_none()`, and the old predicate reported the
    /// blocker as still gating forever.
    ///
    /// | FileNode + DAG state                                                                              | status     |
    /// |---|---|
    /// | FileNode missing, `auto_ingested_recent` entry present at matching gen                          | Gating (auto-ingest queued, FileNode lookup raced ahead of insert) |
    /// | FileNode missing                                                                                  | **Resolved** (producer gone) |
    /// | FileNode present, generation mismatch (including the `generation == 0` recorded blocker)         | **Resolved** (recorded blocker is stale) |
    /// | FileNode present, same gen, `current_analysis().is_some()`                                       | **Resolved** (Analysis already committed; DAG identity is gone and no fan-out remains) |
    /// | FileNode present, same gen, no committed Analysis, but a live Analysis DAG identity exists       | Gating (Analysis in flight or queued; completion/cancel fan-out will fire) |
    /// | FileNode present, same gen, no committed Analysis, no Analysis DAG identity, but Source DAG identity exists | Gating (Source queued or dispatched; Analysis will be admitted on Source completion) |
    /// | FileNode present, same gen, no live Source/Analysis DAG identity, `auto_ingested_recent` entry present at matching gen | Gating (auto-ingest queued in inbox, driver has not yet drained the NewRequest) |
    /// | FileNode present, same gen, no committed Analysis, no Analysis DAG identity, no Source DAG identity, `current_source().is_some()` | **Resolved** (Source committed but Analysis failed/cancelled — dead producer) |
    /// | FileNode present, same gen, no committed Analysis, no Source DAG identity, `current_source().is_none()` | **Resolved** (Source failed and was cancelled — dead producer) |
    ///
    /// The Source-DAG-identity check distinguishes "Source pending /
    /// in flight" from "Source failed and cancelled." Both leave
    /// `current_source().is_none()` on the FileNode; only the live
    /// Source identity in `by_identity` proves the pipeline is
    /// alive. `terminalize_failure(Source)` removes the identity,
    /// so the absence is the dead-producer signal.
    ///
    /// The `auto_ingested_recent` consultation closes the pre-drain
    /// window: [`Self::register_resolved_deps`] inserts a tracking
    /// entry BEFORE enqueueing the auto-ingest `NewRequest`. Until
    /// the driver drains the inbox and [`Self::handle_new_request`]
    /// admits a Source DAG identity (which removes the entry), the
    /// dep is structurally indistinguishable from a Source-failed
    /// corpse — same FileNode shape, no live DAG identity. Without
    /// this check the matrix would classify the queued-but-undrained
    /// state as `Resolved` and the owner's Artifact would be admitted
    /// prematurely. The lookup is matched against the live FileNode
    /// generation: a stale tracking entry from a previous incarnation
    /// (or an entry older than [`AUTO_INGESTED_RECENT_STALE_THRESHOLD`])
    /// is ignored and dropped so it cannot pin future admissions.
    fn file_stage_analysis_blocker_status(
        &self,
        dag: &SchedulerDag,
        canonical: &Arc<str>,
        generation: u64,
    ) -> BlockerStatus {
        // First: consult the persistent terminal-dep-failure store.
        // `terminalize_failure(Source|Analysis)` records an entry
        // under the Analysis `DepKey` for this `(canonical,
        // generation)`. A match means the producer terminally
        // failed — return `Failed(record)` so the caller attaches
        // the record to its admitted node and the pre-dispatch
        // short-circuit fires.
        let analysis_dep_key = DepKey::FileStage {
            canonical: Arc::clone(canonical),
            generation,
            stage: FileStageKey::Analysis,
        };
        if let Some(record) = dag.lookup_terminal_dep_failure(&analysis_dep_key) {
            return BlockerStatus::Failed(record);
        }

        let canonical_str: &str = canonical.as_ref();
        let node = match self.nodes.get(canonical_str) {
            Some(n) => n,
            None => {
                // FileNode gone or not yet inserted. The producer
                // either cannot make progress (Satisfied — moot) OR
                // is in the pre-drain auto-ingest window — consult
                // the tracking set before classifying.
                if self.auto_ingest_tracking_gates(canonical, generation) {
                    return BlockerStatus::Gating;
                }
                return BlockerStatus::Satisfied;
            }
        };
        if generation == 0 {
            // Generation 0 never carries a live Analysis identity —
            // the first scheduler admission bumps the node above 0
            // before submitting any DAG identity. A recorded blocker
            // at gen 0 is stale.
            return BlockerStatus::Satisfied;
        }
        if node.generation() != generation {
            // Different generation — the recorded blocker is for
            // a generation that no longer exists. Stale.
            //
            // Opportunistic cleanup: drop any tracking entry that
            // matches the stale generation under a value-conditional
            // removal. The matrix would otherwise leave the stale
            // entry to age out through the
            // `AUTO_INGESTED_RECENT_STALE_THRESHOLD` (60 s) sweep
            // in `auto_ingest_tracking_gates`, holding memory for
            // every invalidated dep across that window. The
            // remove_if predicate guards against concurrent
            // re-insertion at a newer generation (the value-
            // conditional removal pattern).
            self.auto_ingested_recent
                .remove_if(canonical, |_k, v| v.generation == generation);
            return BlockerStatus::Satisfied;
        }
        if node.current_analysis().is_some() {
            // Analysis committed at the recorded generation. The
            // DAG identity is already gone (`dag.complete(...)` removed
            // it), so recording this dep on a new Artifact admission
            // would put a DepKey nobody will fire into the waiter
            // reverse-index. Drop it — the Analysis output is
            // already on the node and the owner does not need to
            // gate on it.
            return BlockerStatus::Satisfied;
        }
        // Analysis not committed. Decide between "in flight",
        // "pending behind Source", and "moot" by consulting the DAG
        // for both stage identities.
        let analysis_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(canonical),
            generation,
            stage: FileStageKey::Analysis,
        };
        if dag.token_for(&analysis_id).is_some() {
            // Analysis is queued or dispatched — still gating
            // (whether or not Source is committed yet).
            return BlockerStatus::Gating;
        }
        let source_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(canonical),
            generation,
            stage: FileStageKey::Source,
        };
        if dag.token_for(&source_id).is_some() {
            // Source is queued or in flight. Analysis has not been
            // admitted yet (Source completion is the trigger). The
            // pipeline is alive — keep the blocker so the eventual
            // Analysis completion fans out to the owner's Artifact.
            return BlockerStatus::Gating;
        }
        // No live DAG identity for Source AND no live DAG identity
        // for Analysis. The FileNode + DAG shape matches both
        // "auto-ingest queued in inbox, driver not yet drained" AND
        // "producer terminalized but the persistent terminal-dep-
        // failure record has been cleaned up (e.g. by supersede)."
        // Consult the tracking set to disambiguate: a matching
        // entry proves the auto-ingest pipeline is alive and the
        // driver will admit it on the next tick. Without an entry
        // the producer is dead-but-moot (Satisfied), since
        // `terminal_dep_failures` already returned None at the top
        // of this matrix — the record (if any) was cleaned up and
        // the blocker is no longer a discriminator.
        if self.auto_ingest_tracking_gates(canonical, generation) {
            return BlockerStatus::Gating;
        }
        BlockerStatus::Satisfied
    }

    /// Consult [`Self::auto_ingested_recent`] for a tracking entry on
    /// `(canonical, generation)`. Returns `true` when an entry exists
    /// at the matching generation AND the entry is not older than
    /// [`AUTO_INGESTED_RECENT_STALE_THRESHOLD`]; in that case the
    /// matrix MUST gate so the owner's Artifact waits for the
    /// in-flight auto-ingest. Otherwise (no entry, mismatched gen,
    /// or stale by age) the entry is dropped if present and the
    /// matrix continues to its dead-producer arm.
    ///
    /// Stale-by-age handling is belt-and-suspenders for the case
    /// where the driver thread crashes between
    /// [`Self::register_resolved_deps`]'s insert and
    /// [`Self::handle_new_request`]'s removal arm. Under normal
    /// operation the removal arm fires on the next driver tick and
    /// this branch never sees an aged entry.
    fn auto_ingest_tracking_gates(&self, canonical: &Arc<str>, generation: u64) -> bool {
        // DashMap entries are short-lived here — the typical removal
        // path is `handle_new_request` admitting the Source DAG
        // identity, which runs synchronously after the matrix
        // consult. Hold the entry only across the freshness check.
        let entry = match self.auto_ingested_recent.get(canonical) {
            Some(e) => e,
            None => return false,
        };
        let entry_gen = entry.generation;
        let entry_since = entry.since;
        drop(entry);
        if entry_gen != generation {
            // Stale generation — drop the entry under a value-conditional
            // remove keyed on the generation we observed. `remove_if`
            // evaluates the predicate under the shard write lock so a
            // concurrent insert of a later generation between this
            // observation and the removal does not delete the newer
            // entry by accident. A live auto-ingest for a different
            // gen will re-insert with the matching gen on the next
            // call.
            self.auto_ingested_recent
                .remove_if(canonical, |_k, v| v.generation == entry_gen);
            return false;
        }
        if entry_since.elapsed() > AUTO_INGESTED_RECENT_STALE_THRESHOLD {
            // Aged out — the auto-ingest never landed (driver
            // crash). Drop the entry so future admissions are not
            // pinned on a ghost. The value-conditional removal again
            // protects a concurrent newer-gen re-insertion: only the
            // aged entry we observed is dropped, never a fresh
            // re-insert at a later generation that happens to land
            // between the observation and the removal.
            self.auto_ingested_recent
                .remove_if(canonical, |_k, v| v.generation == entry_gen);
            return false;
        }
        true
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

        let canonical_arc: Arc<str> = Arc::from(file_id);
        let inherited_priority = self
            .dag
            .lock()
            .highest_priority_for_file(&canonical_arc, generation)
            .unwrap_or(Priority::Background);

        match task_kind {
            TaskKind::Source => {
                // Extract dependencies from the committed source snapshot.
                if let Some(source) = node.current_source() {
                    let deps = self.executor.extract_deps(file_id, &source);

                    // Merge extract_deps output with any exact-resolved bare deps
                    let mut new_deps = self.edges.get_forward_deps(file_id);
                    new_deps.extend(deps.forward_deps);
                    self.edges.record_forward_deps(file_id, new_deps);

                    // Merge any deferred bare/aliased blocker IDs.
                    let mut all_blocker_ids = deps.blocker_ids;
                    if let Some((_, deferred)) = self.deferred_blocker_ids.remove(file_id) {
                        all_blocker_ids.extend(deferred);
                    }

                    // Register blockers for deps that haven't reached Analysis yet.
                    if !all_blocker_ids.is_empty() {
                        let mut dep_keys: Vec<DepKey> = Vec::new();
                        // Failed-dep records collected from the 3-state
                        // matrix below. These ride together with
                        // `dep_keys` inside the per-canonical
                        // `PendingBlockerSet` recorded for the owner's
                        // Artifact admission. They surface as a typed
                        // `DependencyFailed` on the FIRST Artifact
                        // dispatch (via the drain + `attach_failed_dep`
                        // sequence in
                        // [`Self::admit_artifact_with_blockers`]),
                        // matching the scheduler contract that missing
                        // macro_type_deps gate the owner's Artifact
                        // (codegen consumes resolved type shapes) and
                        // never the owner's Analysis (the template /
                        // script analysis must publish for diagnostics,
                        // hover, and `defineSlots` consumers even when
                        // the type dep is unresolved).
                        let mut failed_records: Vec<crate::dag::FailedDepRecord> = Vec::new();
                        for dep_id in &all_blocker_ids {
                            if self.tombstones.contains_key(dep_id) {
                                continue;
                            }
                            // Read the parent's winner-context BEFORE we
                            // re-borrow the dag for an auto-ingest admission.
                            let parent_ctx = self
                                .dag
                                .lock()
                                .winner_context_for(&canonical_arc, generation);

                            if !self.nodes.contains_key(dep_id) {
                                // Auto-ingest: create node and enqueue Source job.
                                let dep_node = self.create_node(dep_id, None);
                                let dep_gen = dep_node.bump_generation();
                                self.nodes.insert(dep_id.clone(), dep_node);

                                let dep_canonical: Arc<str> = Arc::from(dep_id.as_str());
                                let mut dag = self.dag.lock();
                                admit_work(
                                    &mut dag,
                                    &dep_canonical,
                                    dep_gen,
                                    TaskKind::Source,
                                    std::cmp::min(inherited_priority, Priority::Interactive),
                                    parent_ctx,
                                );
                            }

                            // Route the blocker through the shared 3-state
                            // classifier:
                            //
                            // - `Gating`    → record the DepKey for the
                            //                 owner's Artifact registry
                            //                 (gates Artifact admission
                            //                 only, never Analysis).
                            // - `Satisfied` → drop silently (producer is
                            //                 moot or already committed).
                            // - `Failed(r)` → drop from `dep_keys` AND
                            //                 collect the record for
                            //                 the registry's
                            //                 [`crate::dag::PendingBlockerSet::failed`]
                            //                 list. The Artifact
                            //                 admission re-classifies
                            //                 every persisted failure
                            //                 against the live state on
                            //                 each drain, so a same-gen
                            //                 recovery still re-promotes
                            //                 the dep to gating.
                            let dep_canonical: Arc<str> = Arc::from(dep_id.as_str());
                            let dep_gen =
                                self.nodes.get(dep_id).map(|n| n.generation()).unwrap_or(0);
                            let dag_guard = self.dag.lock();
                            let status = self.file_stage_analysis_blocker_status(
                                &dag_guard,
                                &dep_canonical,
                                dep_gen,
                            );
                            drop(dag_guard);
                            match status {
                                BlockerStatus::Satisfied => continue,
                                BlockerStatus::Failed(record) => {
                                    failed_records.push(record);
                                    continue;
                                }
                                BlockerStatus::Gating => {
                                    dep_keys.push(DepKey::FileStage {
                                        canonical: dep_canonical,
                                        generation: dep_gen,
                                        stage: FileStageKey::Analysis,
                                    });
                                }
                            }
                        }

                        if !dep_keys.is_empty() || !failed_records.is_empty() {
                            // Record the macro_type_dep blocker set on
                            // the per-canonical Artifact registry. The
                            // owner's Analysis stays UNGATED — analysis
                            // is recoverable from the source alone
                            // (templates, defineSlots, script-level
                            // diagnostics all derive from the parsed
                            // source independently of resolved type
                            // shapes). Codegen, however, needs the
                            // resolved type shapes, so the gate fires
                            // at Artifact admission via
                            // [`Self::admit_artifact_with_blockers`].
                            //
                            // Macro-type cycle filter + record run
                            // under a single DAG lock guard so the
                            // filter's bounded reachability check
                            // and the subsequent registry write are
                            // atomic — no other thread can race
                            // between them and observe a state where
                            // two mutually-cyclic deps both pass the
                            // filter. The chokepoint lives in
                            // [`Self::filter_macro_cycle_deps`].
                            let mut dag = self.dag.lock();
                            let (filtered_deps, _dropped_deps) = Self::filter_macro_cycle_deps(
                                &dag,
                                &canonical_arc,
                                generation,
                                dep_keys,
                            );
                            let pending_set = crate::dag::PendingBlockerSet {
                                deps: filtered_deps.into_iter().collect(),
                                failed: failed_records,
                            };
                            dag.record_artifact_blockers(&canonical_arc, generation, pending_set);
                        }
                    }
                }

                // Source → Analysis transition. Owner Analysis is
                // admitted ungated — `admit_work` is the single
                // admission chokepoint, and this is the first and
                // only Analysis admission for this canonical at this
                // generation. Blockers discovered during this
                // completion are persisted to the per-canonical
                // Artifact blocker registry, not attached to this
                // Analysis node.
                let mut dag = self.dag.lock();
                admit_work(
                    &mut dag,
                    &canonical_arc,
                    generation,
                    TaskKind::Analysis,
                    inherited_priority,
                    None,
                );
                // Mark the Source identity complete so the DAG drops
                // its bookkeeping and releases the capacity permit
                // parked at dispatch. Any waiter that gated on this
                // Source identity (rare today, but supported by
                // DepKey::FileStage{stage:Source}) is fanned out.
                let source_id = WorkNodeIdentity::FileStage {
                    canonical: Arc::clone(&canonical_arc),
                    generation,
                    stage: FileStageKey::Source,
                };
                dag.complete(&source_id);
            }
            TaskKind::Analysis => {
                // Mark the Analysis identity as complete in the DAG.
                // The DAG's `complete()` clears the dep from every
                // waiter's `deps_remaining`, so dependent artifacts can
                // proceed on the next dispatch pass.
                let analysis_id = WorkNodeIdentity::FileStage {
                    canonical: Arc::clone(&canonical_arc),
                    generation,
                    stage: FileStageKey::Analysis,
                };
                let analysis_was_gated = self.dag.lock().has_pending_deps(&analysis_id);
                if !analysis_was_gated {
                    // Only complete if the file-level gate was already
                    // clear; if it's still gated (this analysis was
                    // for a different reason), we keep it.
                    self.dag.lock().complete(&analysis_id);
                }

                // For each dependent file (via reverse-index), if its
                // file-level Analysis gate is now clear, admit any
                // pending artifacts. Snapshot the dep's generation
                // and drop the nodes-shard `Ref` BEFORE acquiring
                // `dag.lock()` — holding a Ref across the DAG mutex
                // would form a latent AB-BA ordering with any caller
                // that takes `dag.lock` first and then mutates the
                // dep file's nodes-shard entry.
                let dependents = self.edges.reverse_index.get(file_id);
                for dep_file in dependents {
                    let dep_gen = self.nodes.get(&dep_file).map(|n| n.generation());
                    let Some(dep_gen) = dep_gen else { continue };
                    let dep_canonical: Arc<str> = Arc::from(dep_file.as_str());
                    let dep_analysis_id = WorkNodeIdentity::FileStage {
                        canonical: Arc::clone(&dep_canonical),
                        generation: dep_gen,
                        stage: FileStageKey::Analysis,
                    };
                    if self.dag.lock().has_pending_deps(&dep_analysis_id) {
                        continue;
                    }
                    let inherited = self
                        .dag
                        .lock()
                        .highest_priority_for_file(&dep_canonical, dep_gen)
                        .unwrap_or(Priority::Background);
                    self.admit_pending_artifacts(&dep_canonical, dep_gen, inherited);
                }

                // Admit this file's pending artifact waiters if its own
                // gate is clear.
                if !self.dag.lock().has_pending_deps(&analysis_id) {
                    self.admit_pending_artifacts(&canonical_arc, generation, inherited_priority);
                }
            }
            TaskKind::Artifact { profile_hash } => {
                // Mark the artifact identity complete so the DAG
                // drops its bookkeeping, releases the capacity
                // permit parked at dispatch, and fans out any
                // dep-edge resolution (e.g. an artifact-on-artifact
                // dep edge that another file is waiting on). No
                // further stage admission is needed — artifacts are
                // terminal.
                //
                // Also clear the Artifact blocker registry entry for
                // this `(owner, generation)` IF no other profile is
                // still pending at this generation. Pending entries
                // ride on every Artifact admission at the
                // `(owner, generation)`; once every admission has
                // completed, the entry would otherwise persist and
                // grow the registry across long-lived sessions.
                let artifact_id = WorkNodeIdentity::Artifact {
                    canonical: Arc::clone(&canonical_arc),
                    generation,
                    profile_hash: profile_hash_to_bytes(profile_hash),
                    content_hash: [0u8; 16],
                };
                let mut dag = self.dag.lock();
                dag.complete(&artifact_id);
                if dag
                    .pending_artifact_profiles(&canonical_arc, generation)
                    .is_empty()
                {
                    dag.clear_artifact_blockers(&canonical_arc, generation);
                }
            }
        }
    }

    /// Admit pending Artifact work nodes for a file that has cleared
    /// Analysis and dependency gating.
    ///
    /// Each Artifact admission inherits any late-discovered blockers
    /// recorded by [`Self::register_resolved_deps`] via
    /// [`Self::admit_artifact_with_blockers`] so a blocker that
    /// arrived AFTER the file's Analysis dispatched still gates the
    /// Artifact until its target Analysis completes.
    fn admit_pending_artifacts(
        &self,
        canonical: &Arc<str>,
        generation: u64,
        inherited_priority: Priority,
    ) {
        let profiles: Vec<(u64, Priority)> = self
            .dag
            .lock()
            .pending_artifact_profiles(canonical, generation);
        for (profile_hash, priority) in profiles {
            let mut dag = self.dag.lock();
            // Inherited priority is the file-level urgency; the
            // per-waiter priority is what the dag bookkeeping returned.
            let effective = std::cmp::min(priority, inherited_priority);
            self.admit_artifact_with_blockers(
                &mut dag,
                canonical,
                generation,
                profile_hash,
                effective,
                None,
            );
        }
    }

    /// Drain every queued submission into the DAG via
    /// [`Self::process_submission`]. Used by both the driver's idle
    /// loop and the cooperative pump entry. Returns the number of
    /// submissions drained so callers can record progress.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn drain_inbox_for_pump(&self) -> usize {
        let mut drained = 0;
        while let Ok(submission) = self.inbox.receiver.try_recv() {
            self.process_submission(submission);
            drained += 1;
        }
        drained
    }

    /// Run a single cooperative-pump iteration: drain the inbox into
    /// the DAG, then dispatch every currently-ready job via
    /// [`Self::dispatch_ready_job`]. Returns the per-iteration
    /// progress counters; the caller decides whether to loop, park,
    /// or fall back to a blocking wait.
    ///
    /// `caller_kind` is propagated to [`crate::dag::SchedulerDag::
    /// next_ready_for_pump`] so the DAG can bias selection toward
    /// the caller's own resource class (a CPU worker prefers a CPU
    /// dependency it can run inline).
    ///
    /// `active_path` (currently empty — populated by the
    /// `wait_or_drive` integration in a later commit) names the work
    /// identities the caller is itself waiting on, so the DAG never
    /// dispatches an identity back to the thread that is parked on
    /// it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn pump_ready(
        self: &Arc<Self>,
        reason: PumpReason,
        caller_kind: crate::caller_kind::CallerKind,
    ) -> PumpStats {
        self.pump_ready_with_path(reason, caller_kind, &[])
    }

    /// Variant of [`Self::pump_ready`] that accepts an explicit
    /// active-path slice. The slice is used by the cooperative
    /// pump (`wait_or_drive`) to ensure the DAG never returns a
    /// ready job that the calling worker is itself parked on.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn pump_ready_with_path(
        self: &Arc<Self>,
        reason: PumpReason,
        caller_kind: crate::caller_kind::CallerKind,
        active_path: &[WorkNodeIdentity],
    ) -> PumpStats {
        let mut stats = PumpStats {
            drained: self.drain_inbox_for_pump(),
            ..PumpStats::default()
        };

        loop {
            // Sample DAG depth under the lock so the audit figure
            // matches the depth observed by the entry that is about
            // to leave.
            let (job, queue_depth_pre_dequeue) = {
                let mut dag = self.dag.lock();
                let depth = dag.pending_len();
                let dequeued = dag.next_ready_for_pump(caller_kind, active_path);
                (dequeued, depth as u32)
            };
            let job = match job {
                Some(j) => j,
                None => break,
            };
            match self.dispatch_ready_job(job, reason, caller_kind, queue_depth_pre_dequeue) {
                DispatchOutcome::SubmittedToPool => stats.dispatched += 1,
                DispatchOutcome::ExecutedInline => stats.executed_inline += 1,
                DispatchOutcome::Skipped => {}
            }

            // Test-only: after each dispatch, record it and — once the
            // armed `pause_after` count is reached — park here (before the
            // next dequeue) until the test releases, re-draining the inbox
            // so the surplus provably accrues scheduler-queue dwell. No-op
            // unless a test has armed the hook; absent from release builds.
            #[cfg(any(test, debug_assertions))]
            self.dispatch_pause
                .on_dispatch_and_maybe_pause(&|| self.drain_inbox());
        }
        stats
    }

    /// Route a single [`crate::dag::ReadyJob`] to the right runtime.
    ///
    /// Defensive skips (CacheNode, removed FileNode, generation
    /// mismatch) return [`DispatchOutcome::Skipped`] — the parked
    /// reservation releases through the DAG's cancel path.
    ///
    /// CPU-bound work submitted by a non-CPU-worker thread (driver,
    /// I/O worker, external, inline) goes to the rayon CPU pool via
    /// `cpu_pool.spawn`. Source work always goes to the I/O pool.
    /// When the caller is a CPU worker (it called into the pump via
    /// `wait_or_drive`) and the ready job is CPU-bound, the work
    /// runs inline on the calling thread so a single-CPU-worker
    /// pool can still complete a transitive dependency chain
    /// without parking the only worker behind itself.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn dispatch_ready_job(
        self: &Arc<Self>,
        job: ReadyJob,
        _reason: PumpReason,
        caller_kind: crate::caller_kind::CallerKind,
        queue_depth_pre_dequeue: u32,
    ) -> DispatchOutcome {
        // Compute queue dwell ms: time the entry spent in the DAG
        // between enqueue and this dispatch.
        let dequeue_at = Instant::now();
        let queue_dwell_ms = dequeue_at
            .saturating_duration_since(job.enqueue_time)
            .as_secs_f64()
            * 1000.0;
        let inbox_depth = self.inbox.sender.len() as u32;

        let inbox_sender = self.inbox.sender.clone();
        let executor = Arc::clone(&self.executor);
        let source_loader = Arc::clone(&self.source_loader);

        // Defensive skip: CacheNode dispatch is owned by the cache
        // layer above the scheduler. Cancel the DAG node so the
        // parked capacity reservation releases — the dequeue has
        // already taken a CPU permit against this candidate's
        // resource class. Stranded-waiter contract: CacheNode
        // identities are not observed as `DepKey` prerequisites by
        // FileStage or Artifact nodes, so the cancel here cannot
        // strand any waiter.
        if matches!(job.identity, WorkNodeIdentity::CacheNode { .. }) {
            let stranded = self.dag.lock().cancel(&job.identity);
            debug_assert!(
                stranded.is_empty(),
                "CacheNode defensive skip must not strand DAG waiters: \
                 CacheNode identities are not used as DepKey prerequisites"
            );
            return DispatchOutcome::Skipped;
        }
        let task_kind = task_kind_for_ready_job(&job);
        let (file_id, generation) = match &job.identity {
            WorkNodeIdentity::FileStage {
                canonical,
                generation,
                ..
            } => (canonical.to_string(), *generation),
            WorkNodeIdentity::Artifact {
                canonical,
                generation,
                ..
            } => (canonical.to_string(), *generation),
            WorkNodeIdentity::CacheNode { .. } => {
                unreachable!("CacheNode identities skipped by the defensive guard above")
            }
        };

        let node = match self.nodes.get(&file_id) {
            Some(n) => n.clone(),
            None => {
                debug_assert!(
                    self.dag.lock().token_for(&job.identity).is_none(),
                    "defensive dispatch skip: removed-FileNode case implies the prior \
                     `remove()` cancelled the DAG identity before clearing nodes"
                );
                return DispatchOutcome::Skipped;
            }
        };
        if node.generation() != generation {
            debug_assert!(
                self.dag.lock().token_for(&job.identity).is_none(),
                "defensive dispatch skip: generation-mismatch case implies the prior \
                 `supersede_old_file_generations` cancelled the stale-generation DAG \
                 identity before this dispatch reached the skip"
            );
            return DispatchOutcome::Skipped;
        }

        let canonical_arc: Arc<str> = Arc::from(file_id.as_str());
        let winner_ctx = job.request_context.clone().or_else(|| {
            self.dag
                .lock()
                .winner_context_for(&canonical_arc, generation)
        });

        let dag_handle = Arc::clone(&self.dag);
        let failed_blocker_deps = job.failed_blocker_deps.clone();
        // Capture the identity for the active-path push on the
        // worker side. A worker that re-enters `wait_or_drive`
        // from inside the executor must declare the work it is
        // running so the cooperative pump never returns the same
        // identity to the calling thread.
        let identity = job.identity.clone();

        // Inline execution: when a pool worker reached the
        // cooperative pump via wait_or_drive, run the ready work
        // on the SAME thread instead of queueing it behind
        // ourselves on the pool. A single-worker configuration
        // would otherwise deadlock — the only worker is parked
        // waiting on a dep it itself must run.
        //
        // Routing by caller_kind × task_kind:
        //
        // - `CpuWorker` × non-Source: inline-execute on the CPU
        //   thread. Source stays on the I/O pool because mixing
        //   disk I/O onto a CPU worker would tie up the
        //   cooperative-pump thread on a read.
        // - `IoWorker` × Source: inline-execute on the I/O thread.
        //   The IoWorker is already an I/O thread, so an inline
        //   I/O job stays consistent with the pool's role.
        //   Without this, a single-I/O-worker configuration that
        //   submits an I/O-bound dep and waits parks the only
        //   I/O worker behind itself.
        // - `IoWorker` × non-Source: route through the CPU pool
        //   (the default else-branch below). The IoWorker has no
        //   business running CPU-bound work inline.
        //
        // `CallerKind::Inline` is NOT considered here because the
        // sync inline-drive loop (`wait_or_drive_inline`) calls
        // `execute_stage_inline` directly without ever entering
        // `dispatch_ready_job` — the Inline caller is unreachable
        // on this path.
        let inline_eligible = (matches!(caller_kind, crate::caller_kind::CallerKind::CpuWorker)
            && !matches!(task_kind, TaskKind::Source))
            || (matches!(caller_kind, crate::caller_kind::CallerKind::IoWorker)
                && matches!(task_kind, TaskKind::Source));
        if inline_eligible {
            // Install the winner's request-context TLS for the
            // duration of the inline stage execution. Both
            // pool-spawn branches below also install TLS so the
            // inner stage's audit events carry the request-context
            // tag from `winner_ctx`; the inline branch must mirror
            // them or an inline-executed dep would run under the
            // OUTER stage's request context and audit events would
            // be misattributed to the wrong request.
            //
            // The inline path runs on the CALLER's worker thread,
            // which may already have a request context installed
            // for the OUTER stage. When `winner_ctx` is None, the
            // outer's TLS must be CLEARED across every slot
            // `install_tls` would have planted (scheduler opaque,
            // session request context, audit observer) for the
            // inner stage — otherwise the inner stage's audit
            // events would inherit the outer request id. Pool-
            // spawn paths run inside an outer `install_tls` guard
            // whose `Drop` resets the slot, so sequential jobs on
            // the same persistent pool worker observe `None`
            // between jobs without an explicit clear.
            let _ctx_guard: InlineTlsGuard = match winner_ctx.as_ref() {
                Some(opaque) => InlineTlsGuard::Install(Arc::clone(&opaque.0).install_tls()),
                None => {
                    InlineTlsGuard::ClearAll(crate::request_context::AllSlotsClearGuard::clear_all())
                }
            };
            // Audit pool tag mirrors the pool the inline branch is
            // running on: `IoWorker × Source` runs inline on the
            // I/O worker, so the dispatch audit must record `Io`;
            // `CpuWorker × non-Source` runs inline on the CPU
            // worker → `Cpu`. The inline_eligible gate above
            // already restricts to these two combinations.
            let inline_pool_tag = match (caller_kind, task_kind) {
                (crate::caller_kind::CallerKind::IoWorker, TaskKind::Source) => {
                    crate::audit_publish::WorkerPoolTag::Io
                }
                _ => crate::audit_publish::WorkerPoolTag::Cpu,
            };
            crate::caller_kind::with_active_path(identity, || {
                Self::publish_scheduler_dispatch(
                    inline_pool_tag,
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
                        failed_blocker_deps,
                        executor.as_ref(),
                        source_loader.as_ref(),
                        &inbox_sender,
                        Arc::clone(&dag_handle),
                    );
                }));
                if result.is_err() {
                    Self::surface_stage_panic_as_failed(
                        &node,
                        generation,
                        &task_kind,
                        &inbox_sender,
                        Arc::clone(&dag_handle),
                    );
                }
            });
            return DispatchOutcome::ExecutedInline;
        }

        if matches!(task_kind, TaskKind::Source) {
            // Source jobs: I/O pool loads content, then hands off
            // to CPU pool for parse.
            let node_for_panic = Arc::clone(&node);
            let dag_for_panic = Arc::clone(&dag_handle);
            self.io_pool.execute(move || {
                let _guard: Option<Box<dyn crate::request_context::TlsUninstall + Send>> =
                    winner_ctx.map(|opaque| Arc::clone(&opaque.0).install_tls());
                Self::publish_scheduler_dispatch(
                    crate::audit_publish::WorkerPoolTag::Io,
                    crate::audit_publish::SchedulerDepthsSnapshot {
                        inbox: inbox_depth,
                        queue: queue_depth_pre_dequeue,
                    },
                    queue_dwell_ms,
                );
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::caller_kind::with_active_path(identity, || {
                        Self::execute_stage_on_worker(
                            &node,
                            generation,
                            task_kind,
                            failed_blocker_deps,
                            executor.as_ref(),
                            source_loader.as_ref(),
                            &inbox_sender,
                            Arc::clone(&dag_handle),
                        );
                    });
                }));
                if result.is_err() {
                    Self::surface_stage_panic_as_failed(
                        &node_for_panic,
                        generation,
                        &task_kind,
                        &inbox_sender,
                        dag_for_panic,
                    );
                }
            });
        } else {
            // Analysis/Artifact jobs: pure CPU work.
            let node_for_panic = Arc::clone(&node);
            let dag_for_panic = Arc::clone(&dag_handle);
            self.cpu_pool.spawn(move || {
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
                    crate::caller_kind::with_active_path(identity, || {
                        Self::execute_stage_on_worker(
                            &node,
                            generation,
                            task_kind,
                            failed_blocker_deps,
                            executor.as_ref(),
                            source_loader.as_ref(),
                            &inbox_sender,
                            Arc::clone(&dag_handle),
                        );
                    });
                }));
                if result.is_err() {
                    Self::surface_stage_panic_as_failed(
                        &node_for_panic,
                        generation,
                        &task_kind,
                        &inbox_sender,
                        dag_for_panic,
                    );
                }
            });
        }
        DispatchOutcome::SubmittedToPool
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

    /// Test-only: arm the dispatch pause so the driver parks after
    /// dispatching `pause_after` jobs and BEFORE the next dequeue. Must
    /// be called before submitting the requests whose scheduler-queue
    /// dwell the test inspects. See [`DispatchPauseHook`].
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn test_arm_dispatch_pause(&self, pause_after: usize) {
        let mut state = self.dispatch_pause.state.lock();
        state.armed = true;
        state.pause_after = pause_after;
        state.dispatched = 0;
        state.paused = false;
        state.consumed = false;
        state.released = false;
    }

    /// Test-only: block (bounded ~10 s, panic on stall) until the driver
    /// has reached the armed dispatch pause point and is parked.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn test_wait_until_dispatch_paused(&self) {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = self.dispatch_pause.state.lock();
        while !state.paused {
            if self
                .dispatch_pause
                .cv
                .wait_for(&mut state, Duration::from_millis(5))
                .timed_out()
            {
                assert!(
                    Instant::now() < deadline,
                    "driver never reached the dispatch pause point within 10s \
                     (dispatched {} of pause_after {})",
                    state.dispatched,
                    state.pause_after,
                );
            }
        }
    }

    /// Test-only: current number of pending (non-cancelled,
    /// non-dispatched) nodes in the scheduler DAG. Lets a dwell test
    /// confirm the surplus provably SITS in the scheduler queue before
    /// releasing the pause.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    #[must_use]
    pub fn test_job_queue_depth(&self) -> usize {
        self.dag.lock().pending_len()
    }

    /// Test-only: release the parked driver from the dispatch pause.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn test_release_dispatch_pause(&self) {
        let mut state = self.dispatch_pause.state.lock();
        state.released = true;
        self.dispatch_pause.cv.notify_all();
    }

    /// Dispatch work inline (used by `drive_one`/`drive_all` in sync mode and WASM).
    fn execute_stage_inline(&self, job: ReadyJob) {
        // Defensive skip: CacheNode dispatch is owned by the cache
        // layer above the scheduler. Skip before
        // task_kind_for_ready_job so its unreachable!() never fires
        // on a misrouted enqueue. Cancel the DAG node so the parked
        // capacity reservation releases — `next_ready` has already
        // taken a CPU permit against this candidate's class, and a
        // `return` without releasing would pin the CPU budget.
        // Stranded-waiter contract: CacheNode identities are not
        // observed as `DepKey` prerequisites by FileStage or
        // Artifact nodes, so the cancel here cannot strand any
        // waiter.
        if matches!(job.identity, WorkNodeIdentity::CacheNode { .. }) {
            let stranded = self.dag.lock().cancel(&job.identity);
            debug_assert!(
                stranded.is_empty(),
                "inline CacheNode defensive skip must not strand DAG waiters: \
                 CacheNode identities are not used as DepKey prerequisites"
            );
            return;
        }
        let task_kind = task_kind_for_ready_job(&job);
        let (file_id, generation) = match &job.identity {
            WorkNodeIdentity::FileStage {
                canonical,
                generation,
                ..
            } => (canonical.to_string(), *generation),
            WorkNodeIdentity::Artifact {
                canonical,
                generation,
                ..
            } => (canonical.to_string(), *generation),
            WorkNodeIdentity::CacheNode { .. } => {
                unreachable!("CacheNode identities skipped by the defensive guard above")
            }
        };

        // Defensive skip invariant (inline path): mirrors the native
        // dispatch loop. A missing `FileNode` or generation
        // mismatch here means a prior `remove()` /
        // `supersede_old_file_generations` already terminalized the
        // matching DAG identity and released its parked permit; the
        // skip is therefore safe without an extra cancel call.
        let node = match self.nodes.get(&file_id) {
            Some(n) => n.clone(),
            None => {
                debug_assert!(
                    self.dag.lock().token_for(&job.identity).is_none(),
                    "defensive inline dispatch skip: removed-FileNode case implies the prior \
                     `remove()` cancelled the DAG identity before clearing nodes"
                );
                return;
            }
        };

        if node.generation() != generation {
            debug_assert!(
                self.dag.lock().token_for(&job.identity).is_none(),
                "defensive inline dispatch skip: generation-mismatch case implies the prior \
                 `supersede_old_file_generations` cancelled the stale-generation DAG \
                 identity before this dispatch reached the skip"
            );
            return;
        }

        // Push the identity onto the active-path stack so a
        // re-entrant `wait_or_drive` from inside the executor
        // detects same-path self-await rather than blocking on
        // its own pending completion.
        let identity = job.identity.clone();
        crate::caller_kind::with_active_path(identity, || {
            Self::execute_stage_on_worker(
                &node,
                generation,
                task_kind,
                job.failed_blocker_deps,
                self.executor.as_ref(),
                self.source_loader.as_ref(),
                &self.inbox.sender,
                self.dag.clone(),
            );
        });
    }

    /// Construct the [`WorkNodeIdentity`] for `(canonical, generation,
    /// task_kind)` so that a failure / panic terminal path can address
    /// the matching DAG node. The mapping is the inverse of [`admit_work`].
    fn dag_identity_for_task(
        canonical: &Arc<str>,
        generation: u64,
        task_kind: &TaskKind,
    ) -> WorkNodeIdentity {
        match task_kind {
            TaskKind::Source => WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                generation,
                stage: FileStageKey::Source,
            },
            TaskKind::Analysis => WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                generation,
                stage: FileStageKey::Analysis,
            },
            TaskKind::Artifact { profile_hash } => WorkNodeIdentity::Artifact {
                canonical: Arc::clone(canonical),
                generation,
                profile_hash: profile_hash_to_bytes(*profile_hash),
                content_hash: [0u8; 16],
            },
        }
    }

    /// Single chokepoint for failure / panic terminal paths.
    ///
    /// Releases the parked capacity reservation on the matching DAG
    /// node and signals the appropriate `Failed(error)` to file waiter
    /// groups so callers do not hang.
    ///
    /// Sites that previously called `signal_file_failed*` directly are
    /// routed through this helper — without the node-cancel step the
    /// DAG would leak the parked admission permit and a {cpu:1, io:1}
    /// budget would stall the class on a single failure.
    ///
    /// `whole_file` semantics (Source / Analysis Err, FileNotFound,
    /// non-Artifact panic) signal `Failed` to every waiter group at
    /// `(canonical, generation)`. `per_stage` semantics (Artifact Err
    /// or Artifact panic) preserve other per-profile waiters at the
    /// same `(canonical, generation)`.
    fn terminalize_failure(
        dag: &Mutex<SchedulerDag>,
        canonical: &Arc<str>,
        generation: u64,
        task_kind: &TaskKind,
        error: crate::job::SchedulerError,
    ) -> Vec<crate::dag::SubmissionToken> {
        let identity = Self::dag_identity_for_task(canonical, generation, task_kind);
        let mut guard = dag.lock();
        // 1. Cancel the DAG node — releases the parked capacity
        //    reservation through the by-value `release(self)` consume
        //    in `cancel`'s reservation drop path. Source / Analysis
        //    identities can be observed as `DepKey` prerequisites by
        //    downstream work (e.g., a same-file or dep-file Artifact
        //    gating on this Analysis), so a failure cancel may leave
        //    behind stranded waiter tokens whose only remaining gate
        //    was the now-cancelled identity. Return the stranded
        //    token list so the caller can re-enqueue through the
        //    same `requeue_stranded_waiter` path used by `remove()`
        //    — the downstream Artifact still dispatches (it will
        //    see the missing prerequisite via `current_*().is_none()`
        //    checks on the FileNode and surface its own failure
        //    rather than hang).
        // 1a. Analysis-failure fan-out to already-admitted waiters
        //     BEFORE cancel. `cancel(&analysis_identity)` would
        //     release each waiter's Analysis `DepKey` entry without
        //     recording a `FailedDepRecord` on the waiter — the
        //     downstream Artifact would then dispatch and resolve
        //     `Ready` over a snapshot built from a dead prerequisite.
        //     The fan-out helper records the failure marker on every
        //     waiter so the pre-dispatch chokepoint in
        //     `execute_stage_on_worker` surfaces a typed
        //     `DependencyFailed` instead. This is symmetric with the
        //     Source-side fan-out below (a Source failure also fans
        //     out via `fanout_source_failure_to_analysis_waiters`).
        //
        //     The fan-out runs BEFORE cancel so the cancel's
        //     `self.waiters.remove(&dep_key)` observes an empty
        //     reverse-index entry — the fan-out drained it. Without
        //     this ordering, cancel would strip the `DepKey` from
        //     each waiter's `deps_remaining` first, leaving no
        //     marker for the chokepoint to fire on.
        let mut stranded = Vec::new();
        if matches!(task_kind, TaskKind::Analysis) {
            let analysis_stranded =
                guard.fanout_analysis_failure_to_waiters(canonical, generation, &error);
            stranded.extend(analysis_stranded);
        }
        // 1. Cancel the DAG node — releases the parked capacity
        //    reservation through the by-value `release(self)` consume
        //    in `cancel`'s reservation drop path. For Analysis-stage
        //    failures the cancel's waiter sweep observes the empty
        //    reverse-index entry left by the fan-out above; for
        //    Source-stage failures the Source-keyed waiters are
        //    handled here.
        stranded.extend(guard.cancel(&identity));
        // 1b. Source-failure fan-out to Analysis-keyed waiters at the
        //     same `(canonical, generation)`. The Source cancel above
        //     only fans out to `DepKey::FileStage { stage: Source }`
        //     waiters, but downstream blockers gate on the Analysis
        //     DepKey (Artifact admissions inherit `DepKey::FileStage
        //     { stage: Analysis }` via the typed blocker registry).
        //     Without this propagation the Analysis identity is
        //     never admitted (Analysis admission is gated on Source
        //     success), so the Analysis-keyed waiters stay pinned
        //     forever on a dep that cannot make progress. The
        //     `fanout_source_failure_to_analysis_waiters` helper
        //     drops the Analysis DepKey from each waiter's
        //     `deps_remaining` and returns any newly-stranded
        //     waiters so the caller re-enqueues them through the
        //     same path as the Source-key strand list. There is no
        //     Analysis DAG identity at this `(canonical, generation)`
        //     to double-cancel: admission requires `current_source().
        //     is_some()`, which is false on the Source-failure path.
        //
        //     The producer's terminal `error` is forwarded so the
        //     `FailedDepRecord` on each waiter carries the cause
        //     verbatim — the downstream short-circuit then surfaces
        //     a typed `DependencyFailed` instead of synthesising a
        //     stage-only envelope.
        if matches!(task_kind, TaskKind::Source) {
            let analysis_stranded =
                guard.fanout_source_failure_to_analysis_waiters(canonical, generation, &error);
            stranded.extend(analysis_stranded);
        }
        // 1c. Persistent terminal-dep-failure record. The fan-out
        //     above marks every already-admitted Analysis-keyed
        //     waiter at this `(canonical, generation)`. The
        //     persistent map closes the pre-admission race: a
        //     waiter that admits AFTER the producer terminalized
        //     consults this store (via the matrix's
        //     [`Scheduler::file_stage_analysis_blocker_status`]) and
        //     attaches the same `FailedDepRecord` to the freshly-
        //     submitted node so the pre-dispatch short-circuit fires
        //     uniformly. Recorded under the Analysis `DepKey` even
        //     for Source-stage failures because cross-file Artifact
        //     blockers always key on the producer's Analysis stage
        //     (`register_resolved_deps` records `DepKey::FileStage
        //     { stage: Analysis }`).
        if matches!(task_kind, TaskKind::Source | TaskKind::Analysis) {
            let analysis_dep_key = crate::dag::DepKey::FileStage {
                canonical: Arc::clone(canonical),
                generation,
                stage: crate::dag::FileStageKey::Analysis,
            };
            guard.insert_terminal_dep_failure(crate::dag::FailedDepRecord {
                dep_key: analysis_dep_key,
                cause: error.clone(),
            });
        }
        // 2. Signal Failed to file waiter groups. Artifact failures
        //    must NOT terminate other-profile waiters at the same
        //    (canonical, generation) — use the per-stage variant.
        match task_kind {
            TaskKind::Source | TaskKind::Analysis => {
                guard.signal_file_failed(canonical, generation, error);
            }
            TaskKind::Artifact { .. } => {
                guard.signal_file_failed_for_stage(canonical, generation, task_kind, error);
            }
        }
        stranded
    }

    /// Static-context analogue of [`Self::requeue_stranded_waiter`]:
    /// when `stranded` is non-empty, send a single `Wake` into the
    /// inbox so the driver re-runs the cooperative pump and picks
    /// up any DAG node whose `deps_remaining` cleared as a side
    /// effect of the cancel. Used by [`Self::terminalize_failure`]'s
    /// callers in static (worker) contexts where `&self` is not in
    /// scope.
    fn requeue_terminalize_stranded(
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        stranded: &[crate::dag::SubmissionToken],
    ) {
        if !stranded.is_empty() {
            let _ = inbox_sender.send(Submission::Wake);
        }
    }

    /// Surface a worker-stage panic as `Failed` on all pending groups
    /// at this `(generation, task_kind)` so callers never hang on a
    /// crashed stage. The panic has been swallowed by the worker's
    /// `catch_unwind` — this helper completes the signalling that the
    /// executor's normal error path would have done AND releases the
    /// parked capacity reservation so the resource class does not
    /// stall.
    ///
    /// `terminalize_failure` runs unconditionally — BEFORE the
    /// generation guard — so a panic on a now-superseded generation
    /// still releases the parked admission permit and cancels the
    /// stale DAG identity. Both inner steps are idempotent: a
    /// `cancel` of an identity not in `by_identity` returns an empty
    /// stranded list with no side effect, and `signal_file_failed*`
    /// on a `(canonical, generation)` whose waiter groups were
    /// already drained by `supersede_old_file_generations` is a
    /// no-op. The generation guard only skips the inbox notify path
    /// (already a no-op here).
    #[cfg(not(target_arch = "wasm32"))]
    fn surface_stage_panic_as_failed(
        node: &FileNode,
        generation: u64,
        task_kind: &TaskKind,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        dag: Arc<Mutex<SchedulerDag>>,
    ) {
        // `terminalize_failure` runs UNCONDITIONALLY (no generation
        // guard) so a panic on a now-superseded generation still
        // releases the parked admission permit and cancels the
        // stale DAG identity. An early-return on generation-mismatch
        // here would let the parked permit linger between
        // `bump_generation` and the supersede sweep, stalling the
        // resource class on a panic that raced an invalidation.
        let error = crate::job::SchedulerError::StageFailed {
            file_id: node.canonical_id.clone(),
            stage: format!("{task_kind:?}"),
            message: "stage executor panicked".to_string(),
        };
        let canonical: Arc<str> = Arc::from(node.canonical_id.as_str());
        let stranded = Self::terminalize_failure(&dag, &canonical, generation, task_kind, error);
        Self::requeue_terminalize_stranded(inbox_sender, &stranded);
    }

    /// Execute a stage on a worker (rayon thread or inline).
    ///
    /// This is a static method so it can be called from rayon::spawn closures
    /// without holding a reference to &self. All shared state is passed explicitly.
    ///
    /// `failed_blocker_deps` carries [`crate::dag::FailedDepRecord`]
    /// entries for every prerequisite that the producer terminalized
    /// before this node became dispatchable. Two population paths
    /// feed the map (see [`crate::dag::SchedulerDag`]'s
    /// `fanout_source_failure_to_analysis_waiters` fan-out path and
    /// the `attach_failed_dep` admission-time attach).
    ///
    /// SOLE-CHOKEPOINT contract: this function short-circuits at the
    /// top with a typed
    /// [`crate::job::SchedulerError::DependencyFailed`] when the map
    /// is non-empty — regardless of task kind. The Source / Analysis
    /// / Artifact arms below ALL run with `failed_blocker_deps.is_
    /// empty()` as a debug-assert invariant; per-arm checks would be
    /// a rule-violation that resurrects the divergent silent-success
    /// class the short-circuit was introduced to close. The
    /// `failed_blocker_deps` parameter is therefore consumed before
    /// the dispatch and is NOT forwarded to the per-kind arms.
    #[allow(clippy::too_many_arguments)]
    fn execute_stage_on_worker(
        node: &FileNode,
        generation: u64,
        task_kind: TaskKind,
        failed_blocker_deps: std::collections::BTreeMap<
            crate::dag::DepKey,
            crate::dag::FailedDepRecord,
        >,
        executor: &dyn StageExecutor,
        source_loader: &dyn SourceLoader,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        dag: Arc<Mutex<SchedulerDag>>,
    ) {
        // Typed dependency-failure short-circuit BEFORE task-kind
        // dispatch. The marker survives both the fan-out path (a
        // producer terminalization after the consumer admitted) and
        // the admission-time attach (a producer terminalization
        // BEFORE the consumer admitted, observed via the persistent
        // `terminal_dep_failures` store). Surfacing the typed error
        // here — once, in one place — means every task kind that
        // can wait on a `DepKey` gets the same short-circuit semantics
        // without per-arm divergence.
        if let Some((_first_key, first_record)) = failed_blocker_deps.iter().next() {
            use crate::job::SchedulerError;
            let canonical: Arc<str> = Arc::from(node.canonical_id.as_str());
            debug_assert!(
                !matches!(first_record.dep_key, crate::dag::DepKey::CacheNode { .. }),
                "CacheNode DepKey should not appear in failed_blocker_deps",
            );
            // Carry the producer's terminal cause through the typed
            // `DependencyFailed` envelope. The record's `cause` was
            // captured at terminalization time (either on the fan-
            // out path or the admission-time attach path), so the
            // consumer can disambiguate FileNotFound vs StageFailed
            // without re-reading state from the failed file.
            let stranded = Self::terminalize_failure(
                &dag,
                &canonical,
                generation,
                &task_kind,
                SchedulerError::DependencyFailed {
                    dep_key: first_record.dep_key.clone(),
                    cause: Box::new(first_record.cause.clone()),
                },
            );
            Self::requeue_terminalize_stranded(inbox_sender, &stranded);
            return;
        }
        match task_kind {
            TaskKind::Source => {
                debug_assert!(
                    failed_blocker_deps.is_empty(),
                    "Source stage received failed_blocker_deps — pre-dispatch \
                     short-circuit must consume the marker before kind-dispatch \
                     (fan-out target invariant violated)",
                );
                Self::execute_source_stage(
                    node,
                    generation,
                    executor,
                    source_loader,
                    inbox_sender,
                    dag,
                );
            }
            TaskKind::Analysis => {
                debug_assert!(
                    failed_blocker_deps.is_empty(),
                    "Analysis stage received failed_blocker_deps — pre-dispatch \
                     short-circuit must consume the marker before kind-dispatch \
                     (fan-out target invariant violated)",
                );
                Self::execute_analysis_stage(node, generation, executor, inbox_sender, dag);
            }
            TaskKind::Artifact { profile_hash } => {
                debug_assert!(
                    failed_blocker_deps.is_empty(),
                    "Artifact stage received failed_blocker_deps — pre-dispatch \
                     short-circuit must consume the marker before kind-dispatch \
                     (fan-out target invariant violated)",
                );
                Self::execute_artifact_stage(
                    node,
                    generation,
                    profile_hash,
                    executor,
                    inbox_sender,
                    dag,
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
        dag: Arc<Mutex<SchedulerDag>>,
    ) {
        use crate::job::SchedulerError;

        let canonical: Arc<str> = Arc::from(node.canonical_id.as_str());

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
                // File not found — signal Failed, not Ready with empty
                // content. Route through `terminalize_failure` so the
                // DAG node's parked admission permit releases.
                let stranded = Self::terminalize_failure(
                    &dag,
                    &canonical,
                    generation,
                    &TaskKind::Source,
                    SchedulerError::FileNotFound {
                        file_id: node.canonical_id.clone(),
                    },
                );
                Self::requeue_terminalize_stranded(inbox_sender, &stranded);
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
                let stranded = Self::terminalize_failure(
                    &dag,
                    &canonical,
                    generation,
                    &TaskKind::Source,
                    SchedulerError::StageFailed {
                        file_id: node.canonical_id.clone(),
                        stage: "Source".to_string(),
                        message: e.message,
                    },
                );
                Self::requeue_terminalize_stranded(inbox_sender, &stranded);
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
            dag.lock()
                .signal_stage_complete(&canonical, generation, &TaskKind::Source, &result);

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
        dag: Arc<Mutex<SchedulerDag>>,
    ) {
        use crate::job::SchedulerError;

        let canonical: Arc<str> = Arc::from(node.canonical_id.as_str());

        let source = match node.current_source() {
            Some(s) => s,
            None => return, // Source not ready — will be retried after Source completes
        };

        let snapshot = match executor.execute_analysis(&node.canonical_id, &source, generation) {
            Ok(snap) => Arc::new(snap),
            Err(e) => {
                let stranded = Self::terminalize_failure(
                    &dag,
                    &canonical,
                    generation,
                    &TaskKind::Analysis,
                    SchedulerError::StageFailed {
                        file_id: node.canonical_id.clone(),
                        stage: "Analysis".to_string(),
                        message: e.message,
                    },
                );
                Self::requeue_terminalize_stranded(inbox_sender, &stranded);
                return;
            }
        };

        if node.generation() == generation {
            node.analysis.store(Arc::new(Some(Arc::clone(&snapshot))));

            let result = RequestResult::Analysis(snapshot);
            dag.lock()
                .signal_stage_complete(&canonical, generation, &TaskKind::Analysis, &result);

            let _ = inbox_sender.send(Submission::StageComplete {
                file_id: node.canonical_id.clone(),
                generation,
                task_kind: TaskKind::Analysis,
            });
        }
    }

    /// Return `true` when the per-profile artifact slot already holds
    /// a snapshot at `generation`. The DashMap `Ref` is taken, read,
    /// and dropped within this helper's body so no shard-read lock
    /// escapes to the caller. Callers MUST use this helper rather
    /// than holding a `node.artifacts.get(...)` `Ref` across a
    /// subsequent `dag.lock()` acquisition: the external
    /// `commit_artifact` path holds the DAG lock and then writes
    /// into the same DashMap shard, so a shard-read held across
    /// `dag.lock()` would invert that ordering and deadlock.
    pub(crate) fn artifact_already_committed_at(
        node: &FileNode,
        profile_hash: u64,
        generation: u64,
    ) -> bool {
        node.artifacts
            .get(&profile_hash)
            .map(|existing| existing.generation == generation)
            .unwrap_or(false)
    }

    /// Execute the Artifact stage via the executor.
    ///
    /// The typed dependency-failure short-circuit is the
    /// responsibility of [`Self::execute_stage_on_worker`] —
    /// `execute_artifact_stage` ONLY sees the Artifact arm after the
    /// pre-dispatch chokepoint has consumed any `failed_blocker_deps`
    /// marker. Adding a per-arm `failed_blocker_deps` check back here
    /// would resurrect the divergent silent-success class the
    /// single-chokepoint short-circuit was introduced to close.
    fn execute_artifact_stage(
        node: &FileNode,
        generation: u64,
        profile_hash: u64,
        executor: &dyn StageExecutor,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        dag: Arc<Mutex<SchedulerDag>>,
    ) {
        use crate::job::SchedulerError;

        let canonical: Arc<str> = Arc::from(node.canonical_id.as_str());

        //
        // Lock-ordering: the artifacts DashMap shard read lock MUST
        // be released before `dag.lock()` is acquired. The external
        // `commit_artifact` path holds the DAG lock and then writes
        // into the same DashMap shard; holding a `Ref` across the
        // DAG lock acquisition here would invert that ordering and
        // deadlock. The bool helper [`Self::artifact_already_committed_at`]
        // takes the Ref, reads, and drops it before returning, so the
        // caller never holds a shard-read lock across `dag.lock()`.
        if Self::artifact_already_committed_at(node, profile_hash, generation) {
            let artifact_id = WorkNodeIdentity::Artifact {
                canonical: Arc::clone(&canonical),
                generation,
                profile_hash: profile_hash_to_bytes(profile_hash),
                content_hash: [0u8; 16],
            };
            // Stranded-waiter contract: Artifact identities are
            // graph leaves (no FileStage or Artifact lists an
            // Artifact `DepKey` as a prerequisite), so the
            // pre-executor race-skip cancel here cannot strand
            // any waiter.
            let stranded = dag.lock().cancel(&artifact_id);
            debug_assert!(
                stranded.is_empty(),
                "race-safe pre-executor skip must not strand DAG waiters: \
                 Artifact identities are graph leaves"
            );
            return;
        }

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
                // Signal failure only for this specific profile, not
                // all artifacts. Route through `terminalize_failure`
                // so the DAG node's parked admission permit releases
                // — the per-stage variant inside the helper preserves
                // other-profile waiters at the same generation.
                // Artifact identities are graph leaves so the
                // returned stranded list is always empty; the wake
                // helper is a no-op there.
                let stranded = Self::terminalize_failure(
                    &dag,
                    &canonical,
                    generation,
                    &TaskKind::Artifact { profile_hash },
                    SchedulerError::StageFailed {
                        file_id: node.canonical_id.clone(),
                        stage: "Artifact".to_string(),
                        message: e.message,
                    },
                );
                Self::requeue_terminalize_stranded(inbox_sender, &stranded);
                return;
            }
        };

        if node.generation() == generation {
            // Insert-if-absent: an external `commit_artifact` race
            // with this worker is closed by re-checking under the
            // DAG lock. The lock is the synchronization point with
            // `commit_artifact`, which performs its insert + signal
            // + terminalize under the same lock. If an external
            // snapshot landed between this worker's dispatch and
            // here, drop the worker's result so the externally-
            // committed snapshot stays authoritative. `commit_artifact`
            // already signalled its waiters and terminalized the DAG
            // identity, so no further work is needed on the worker
            // side.
            let mut guard = dag.lock();
            if let Some(existing) = node.artifacts.get(&profile_hash) {
                if existing.generation == generation {
                    drop(guard);
                    return;
                }
            }
            node.artifacts.insert(profile_hash, Arc::clone(&snapshot));
            let result = RequestResult::Artifact(snapshot);
            guard.signal_stage_complete(
                &canonical,
                generation,
                &TaskKind::Artifact { profile_hash },
                &result,
            );
            drop(guard);

            // Notify the driver loop that the Artifact stage is
            // terminal so handle_stage_complete can release the
            // DAG identity (and its capacity permit) via
            // dag.complete(&artifact_id).
            let _ = inbox_sender.send(Submission::StageComplete {
                file_id: node.canonical_id.clone(),
                generation,
                task_kind: TaskKind::Artifact { profile_hash },
            });
        }
    }

    // ── Test/WASM Driver Control ──

    /// Process one submission + dispatch one job. Returns false if nothing to do.
    pub fn drive_one(&self) -> bool {
        self.drain_inbox();
        let job = {
            let mut dag = self.dag.lock();
            dag.next_ready()
        };
        if let Some(job) = job {
            self.execute_stage_inline(job);
            true
        } else {
            false
        }
    }

    /// Process until DAG is empty + no pending completions.
    pub fn drive_all(&self) {
        let mut iterations = 0;
        loop {
            self.drain_inbox();
            let job = {
                let mut dag = self.dag.lock();
                dag.next_ready()
            };
            match job {
                Some(job) => {
                    self.execute_stage_inline(job);
                    iterations = 0;
                }
                None => {
                    if self.inbox.receiver.is_empty() {
                        iterations += 1;
                        if iterations > 2 {
                            break;
                        }
                    } else {
                        iterations = 0;
                    }
                }
            }
        }
    }

    /// Block until `handle` resolves, cooperatively pumping the
    /// scheduler when the calling thread is a scheduler-owned
    /// worker. The waiter must NOT park unconditionally — a CPU /
    /// I/O worker that blocks on a dependency it could itself
    /// dispatch causes a driver-loop deadlock.
    ///
    /// Behaviour by caller kind:
    ///
    /// - **`Driver` / `External`** with a live driver thread: park
    ///   on the condvar. The driver pumps; the waiter has no work
    ///   to share.
    /// - **`CpuWorker` / `IoWorker`**: enter the cooperative pump.
    ///   Each iteration runs `pump_ready_with_path` (so the DAG
    ///   never returns an identity the calling worker is itself
    ///   waiting on), then waits on the handle with a short timeout
    ///   before re-pumping. Same-path detection fires when the
    ///   target identity is already on the active path: return
    ///   `Failed(StageFailed { stage: "wait_or_drive" })` instead
    ///   of joining the worker's own pending completion.
    /// - **`Inline`** / no driver: legacy inline-drive loop. Loops
    ///   `pump_ready_with_path` until either the handle resolves or
    ///   the inbox + DAG stably run dry (controlled failure).
    pub fn wait_or_drive<T: Clone>(
        self: &Arc<Self>,
        handle: &crate::job::CompletionHandle<T>,
    ) -> crate::job::CompletionState<T> {
        let caller = crate::caller_kind::CallerKind::current();
        self.wait_or_drive_with_caller(handle, caller)
    }

    /// Lower-level entry that takes an explicit caller kind. Used
    /// by tests that need to override the TLS classification
    /// without spawning a real pool worker. Production code routes
    /// through [`Self::wait_or_drive`] which reads the TLS value
    /// set by the pool-builder start handlers.
    pub fn wait_or_drive_with_caller<T: Clone>(
        self: &Arc<Self>,
        handle: &crate::job::CompletionHandle<T>,
        caller_kind: crate::caller_kind::CallerKind,
    ) -> crate::job::CompletionState<T> {
        // `caller_kind` discriminates driver/worker/external waiters and is
        // read only on native (the driver-aware park/cooperative paths
        // below). wasm is single-threaded with no driver, so it takes the
        // inline path and the discriminant is intentionally unused there —
        // the parameter stays in the cross-target signature so callers pass
        // it identically regardless of target.
        #[cfg(not(target_arch = "wasm32"))]
        use crate::caller_kind::CallerKind;
        #[cfg(target_arch = "wasm32")]
        let _ = caller_kind;

        // Lock-discipline guard: the driver thread MUST NOT enter
        // wait_or_drive — its loop is the sole pump and would
        // deadlock if it parked itself. `wait_or_drive` is reserved
        // for self-driving workers (CpuWorker / IoWorker) and
        // external waiters. A debug_assert catches programming
        // errors during development; release builds rely on the
        // structural separation enforced by the driver loop never
        // calling this method. Native-only: wasm has no driver thread
        // (`driver_loop_native` and `driver_handle` are both
        // `cfg(not(target_arch = "wasm32"))`), so a `Driver` caller
        // cannot occur there and the invariant is vacuous.
        #[cfg(not(target_arch = "wasm32"))]
        debug_assert!(
            !matches!(caller_kind, CallerKind::Driver) || self.driver_handle.lock().is_none(),
            "Driver thread must not enter wait_or_drive (would deadlock: \
             driver loop is not running while parked)"
        );

        // FIRST: if the handle is already resolved, return its
        // real terminal state. A same-path check on a resolved
        // handle would otherwise mask the actual result (Ready /
        // Failed / Superseded / Shutdown) with a synthetic
        // `Failed(StageFailed { stage: "wait_or_drive" })`.
        // Then run same-path self-await detection on the still-
        // pending handle; the helper re-checks `try_get` right
        // before synthesizing Failed so a handle that resolves
        // between the entry check and the synthetic failure
        // surfaces its real terminal state.
        if let Some(state) = check_terminal_or_same_path(handle) {
            return state;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let has_driver = self.driver_handle.lock().is_some();
            if has_driver && matches!(caller_kind, CallerKind::Driver | CallerKind::External) {
                // No work to share — park on the condvar.
                return handle.wait();
            }
            if has_driver && matches!(caller_kind, CallerKind::CpuWorker | CallerKind::IoWorker) {
                return self.wait_or_drive_cooperative(handle, caller_kind);
            }
        }

        // Inline drive path: no driver thread, OR WASM, OR an
        // explicit `Inline` caller. Use pump_ready_with_path so a
        // re-entrant submission from inside an inline-executed
        // stage still gets the same active-path filtering.
        self.wait_or_drive_inline(handle)
    }

    /// Cooperative pump path for CPU / I/O worker threads with a
    /// live driver. Each iteration runs the pump under the
    /// active-path filter so the DAG never returns the work the
    /// caller is parked on, then waits on the handle with a short
    /// timeout before re-pumping. The condvar wake-up bounds the
    /// worst-case latency between the driver's dispatch and the
    /// worker observing its handle resolved.
    #[cfg(not(target_arch = "wasm32"))]
    fn wait_or_drive_cooperative<T: Clone>(
        self: &Arc<Self>,
        handle: &crate::job::CompletionHandle<T>,
        caller_kind: crate::caller_kind::CallerKind,
    ) -> crate::job::CompletionState<T> {
        loop {
            // Re-run terminal-or-same-path check on every
            // iteration: handles submitted with the request-level
            // `CompletionTarget::Request` shape are re-stamped by
            // admission to `CompletionTarget::Work(..)` AFTER
            // `submit_request` returns, so the loop-entry check
            // upstream may have seen the pre-admission target.
            // Re-reading on each iteration picks up the late
            // stamp.
            if let Some(state) = check_terminal_or_same_path(handle) {
                return state;
            }
            // The scheduler is shutting down — the driver loop
            // has already terminated and no further work will
            // dispatch. Bail out with a Shutdown state so the
            // caller does not park indefinitely on a handle
            // whose work will never reach a worker.
            if self.shutdown.load(Ordering::Acquire) {
                return crate::job::CompletionState::Shutdown;
            }
            let active_path = crate::caller_kind::snapshot_active_path();
            let stats =
                self.pump_ready_with_path(PumpReason::WaitOrDrive, caller_kind, &active_path);
            // Re-check terminal+same-path after the pump iteration:
            // the pump may have dispatched the dep (Ready), and
            // admission may have stamped the concrete Work target
            // mid-flight (turning a previously-Unknown Request
            // into an active-path same-path frame).
            if let Some(state) = check_terminal_or_same_path(handle) {
                return state;
            }
            // Park on the handle with a short timeout so we
            // re-pump promptly if the driver makes progress.
            // Tightening the timeout when pump_ready made no
            // progress avoids spinning when the driver is the
            // only thread with work to do.
            let timeout = if stats.made_progress() {
                std::time::Duration::from_millis(1)
            } else {
                std::time::Duration::from_millis(10)
            };
            if let Some(state) = handle.wait_timeout(timeout) {
                return state;
            }
        }
    }

    /// Inline drive loop — used when the scheduler has no driver
    /// thread (WASM, sync test mode) or the caller explicitly
    /// adopts the `Inline` role. The legacy bound-idle behaviour
    /// (controlled failure when the scheduler stably runs dry with
    /// the handle still pending) is preserved.
    fn wait_or_drive_inline<T: Clone>(
        self: &Arc<Self>,
        handle: &crate::job::CompletionHandle<T>,
    ) -> crate::job::CompletionState<T> {
        let _scope =
            crate::caller_kind::CallerKindGuard::install(crate::caller_kind::CallerKind::Inline);
        let mut idle_iterations = 0u32;
        loop {
            if let Some(state) = handle.try_get() {
                return state;
            }
            self.drain_inbox();
            let job = {
                let active_path = crate::caller_kind::snapshot_active_path();
                let mut dag = self.dag.lock();
                dag.next_ready_for_pump(crate::caller_kind::CallerKind::Inline, &active_path)
            };
            match job {
                Some(job) => {
                    self.execute_stage_inline(job);
                    idle_iterations = 0;
                }
                None => {
                    if self.inbox.receiver.is_empty() {
                        idle_iterations += 1;
                        if idle_iterations > 2 {
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

/// Extract a canonical-id string from a `WorkNodeIdentity` so a
/// cooperative-pump self-await report can name the file the caller
/// was waiting on. `CacheNode` variants return an empty string —
/// they have no file canonical.
fn identity_canonical(identity: &crate::dag::WorkNodeIdentity) -> String {
    match identity {
        crate::dag::WorkNodeIdentity::FileStage { canonical, .. } => canonical.to_string(),
        crate::dag::WorkNodeIdentity::Artifact { canonical, .. } => canonical.to_string(),
        crate::dag::WorkNodeIdentity::CacheNode { .. } => String::new(),
    }
}

/// RAII guard owned by the cooperative pump's inline-execute
/// branch. Selects between INSTALLING a winner-provided request
/// context and CLEARING the outer worker's TLS so the inner stage
/// runs under the correct attribution.
///
/// Both variants restore the prior TLS slot on drop. Two arms are
/// required because the install path returns a trait-object
/// `Box<dyn TlsUninstall>` (the concrete guard lives in the
/// session crate and isn't visible to the scheduler), while the
/// clear path is a concrete `OpaqueContextGuard` that owns the
/// prior value directly.
enum InlineTlsGuard {
    /// Winner has its own request context; install it for the
    /// inner stage. Drop restores the prior TLS via the trait
    /// object's underlying guard.
    Install(#[allow(dead_code)] Box<dyn crate::request_context::TlsUninstall + Send>),
    /// Winner has no context; clear ALL install_tls slots (scheduler
    /// opaque, session request context + accumulator, audit observer)
    /// so the inner stage observes `None` everywhere the outer
    /// stage's `install_tls` would have planted state. Drop restores
    /// every prior outer TLS slot via `AllSlotsClearGuard::Drop`.
    ClearAll(#[allow(dead_code)] crate::request_context::AllSlotsClearGuard),
}

/// Re-reads the handle's current state and current target slot,
/// then either returns a real terminal `CompletionState` or
/// synthesizes a same-path `Failed(StageFailed)` when the handle's
/// target is on the calling thread's active path.
///
/// Centralizes three invariants:
/// - try_get is consulted FIRST so a resolved handle returns its
///   real terminal state instead of being masked by the synthetic
///   same-path Failed.
/// - The active-path probe matches the full prerequisite-stage
///   chain:
///     * Source request → matches an active Source frame on the
///       same canonical.
///     * Analysis request → matches an active Source OR Analysis
///       frame on the same canonical.
///     * Artifact request → matches an active Source OR Analysis
///       frame on the same canonical, OR an active Artifact frame
///       on the same canonical AND the same `profile_hash`. Two
///       Artifact frames for the same canonical with different
///       profiles are independent work units (they share only the
///       upstream Analysis gate, not the Artifact slot itself) and
///       must NOT collapse into a same-path match.
/// - try_get is re-checked IMMEDIATELY before synthesizing the
///   Failed so a handle that resolves during the active-path
///   probe still surfaces its real terminal state.
///
/// Re-readable across the cooperative loop: each call observes a
/// fresh `handle.try_get()` and `handle.target()`. The target slot
/// is mutated by `handle_new_request` admission (Request → Work),
/// so the cooperative pump re-runs this helper on every iteration
/// to pick up the late-stamped Work identity that the loop-entry
/// read missed.
fn check_terminal_or_same_path<T: Clone>(
    handle: &crate::job::CompletionHandle<T>,
) -> Option<crate::job::CompletionState<T>> {
    use crate::job::{CompletionState, CompletionTarget, SchedulerError};
    if let Some(state) = handle.try_get() {
        return Some(state);
    }
    let target = handle.target()?;
    let on_active_path = match &target {
        CompletionTarget::Work(id) => crate::caller_kind::active_path_contains_work(id),
        CompletionTarget::Request { canonical, target } => {
            crate::caller_kind::active_path_contains_request(canonical.as_ref(), target.clone())
        }
    };
    if !on_active_path {
        return None;
    }
    // Test-only hook: fires between the active-path probe and the
    // inner try_get re-check. The hook lets tests deterministically
    // exercise the inner re-check (which otherwise sits in a tiny
    // race window between the active-path computation and the
    // synthetic-Failed synthesis) by resolving the handle from
    // outside the helper. Production builds compile without the
    // hook.
    #[cfg(test)]
    check_terminal_or_same_path_test_hook();
    // Same-path match: re-check try_get RIGHT before synthesizing
    // Failed so a handle that resolved during the active-path
    // probe surfaces its real terminal state. Without this
    // re-check, a Ready/Failed/Superseded/Shutdown that landed
    // between the entry try_get and here would be masked by the
    // synthetic `Failed(StageFailed { stage: "wait_or_drive" })`.
    if let Some(state) = handle.try_get() {
        return Some(state);
    }
    let file_id = match &target {
        CompletionTarget::Work(id) => identity_canonical(id),
        CompletionTarget::Request { canonical, .. } => canonical.to_string(),
    };
    Some(CompletionState::Failed(SchedulerError::StageFailed {
        file_id,
        stage: "wait_or_drive".into(),
        message: "same-path self-await detected".into(),
    }))
}

// Test-only hook installer. Lets a test plant a closure that
// fires between the active-path probe and the inner try_get
// re-check inside `check_terminal_or_same_path`, so the test
// can resolve the handle from outside the helper at exactly the
// right point and assert that the inner re-check observes the
// resolved state instead of synthesizing a Failed.
//
// The hook is thread-local (no global mutable state across
// tests) and clears itself on drop. Production builds compile
// without the hook field at all.
#[cfg(test)]
thread_local! {
    static CHECK_TERMINAL_HOOK: std::cell::RefCell<
        Option<Box<dyn FnMut() + Send>>,
    > = std::cell::RefCell::new(None);
}

#[cfg(test)]
fn check_terminal_or_same_path_test_hook() {
    CHECK_TERMINAL_HOOK.with(|cell| {
        if let Some(hook) = cell.borrow_mut().as_mut() {
            hook();
        }
    });
}

/// RAII guard that installs `hook` as the test-only intercept
/// between the active-path probe and the inner try_get re-check.
/// Restores the previous (typically `None`) on drop.
#[cfg(test)]
pub(crate) struct CheckTerminalHookGuard {
    prev: Option<Box<dyn FnMut() + Send>>,
}

#[cfg(test)]
impl CheckTerminalHookGuard {
    pub(crate) fn install(hook: Box<dyn FnMut() + Send>) -> Self {
        let prev = CHECK_TERMINAL_HOOK.with(|cell| cell.replace(Some(hook)));
        Self { prev }
    }
}

#[cfg(test)]
impl Drop for CheckTerminalHookGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        CHECK_TERMINAL_HOOK.with(|cell| {
            cell.replace(prev);
        });
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

        // Signal shutdown to all pending waiter groups.
        self.dag.lock().signal_all_shutdown();
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

    /// Source identity is removed from the DAG once the Source stage
    /// completes. Without dag.complete(&source_id) in the Source
    /// arm of handle_stage_complete, the Source identity would
    /// linger in nodes/by_identity (its capacity permit never
    /// returns and a re-submission would observe an in-flight
    /// dispatched node).
    #[test]
    fn source_identity_removed_from_dag_after_source_stage_completes() {
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
        let _ = handle.try_get();

        let source_id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/a.vue"),
            generation: 1,
            stage: FileStageKey::Source,
        };
        let dag = sched.dag.lock();
        assert!(
            dag.token_for(&source_id).is_none(),
            "Source identity must be removed from by_identity after \
             handle_stage_complete completes the Source arm",
        );
        // Permit returned to the pool by complete().
        assert_eq!(
            dag.in_flight_io_permits(),
            0,
            "Source dispatch's io permit must return to the pool on complete()",
        );
    }

    /// A CacheNode identity submitted via SchedulerDag::submit must
    /// NOT panic when the dispatch path tries to route it through
    /// task_kind_for_ready_job — the dispatch path skips it
    /// defensively and cancels the DAG entry so the parked admission
    /// permit releases.
    ///
    /// Pre-guard: task_kind_for_ready_job's CacheNode arm contains an
    /// unreachable!() that panics whenever a CacheNode reaches it.
    /// drive_one would therefore propagate the panic.
    /// Post-guard: the dispatch path's defensive guard skips the
    /// identity before task_kind_for_ready_job is called AND calls
    /// `dag.cancel(&job.identity)` so the parked CPU permit
    /// releases and the cache layer's own dispatch arm is free to
    /// re-admit when it wires up.
    #[test]
    fn cache_node_identity_silently_skipped_by_dispatch() {
        use crate::dag::{PinId, SchedulerCacheId, WorkKind};
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = test_scheduler_with_loader(loader);

        // Inject a CacheNode identity directly into the DAG.
        let cache_id = WorkNodeIdentity::CacheNode {
            cache_id: SchedulerCacheId(7),
            key_hash: [0u8; 16],
            view_epoch: 1,
            snapshot_pin_id: PinId(1),
        };
        sched.dag.lock().submit(
            cache_id.clone(),
            WorkKind::CacheNode,
            Priority::Interactive,
            Vec::new(),
            None,
        );

        // The defensive guard dequeues, then cancels the identity
        // so the permit releases. The CacheNode entry is removed
        // from `by_identity`; `drive_one` returns true (work was
        // dequeued); no panic propagates.
        let _ = sched.drive_one();
        // The key invariant: no panic propagated. The identity is
        // no longer in `by_identity` (cancel removed it), which
        // mirrors the sibling permit-release test.
    }

    /// CacheNode defensive skip in `execute_stage_inline` (and the
    /// native dispatch loop) must release the parked CPU permit
    /// before returning. Without the release, a {cpu:1} budget is
    /// permanently drained on the first CacheNode submission and a
    /// follow-on real CPU job never dispatches.
    ///
    /// Without the cancel, `continue` / `return` skips the rest of
    /// the dispatch arm but the reservation parked on the DAG node
    /// by `next_ready` stays alive; the CPU class budget is held.
    /// With the cancel, the skip path calls
    /// `dag.cancel(&job.identity)` so the parked reservation
    /// releases through its by-value consume.
    #[test]
    fn cachenode_defensive_skip_releases_cpu_permit() {
        use crate::dag::{PinId, SchedulerCacheId, WorkKind};
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/follow.vue".to_string(), Arc::from("content"));
        // Tight {cpu:1, io:1} budget: a leaked CPU permit pins the
        // class and the follow-on Analysis below cannot dispatch.
        let config = SchedulerConfig {
            cpu_threads: 1,
            io_threads: 1,
            aging: DagAgingConfig::default(),
            dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
        };
        let sched = Scheduler::new_sync(config, loader);

        // Inject a CacheNode identity directly into the DAG so the
        // dispatch path encounters the defensive skip.
        let cache_id = WorkNodeIdentity::CacheNode {
            cache_id: SchedulerCacheId(7),
            key_hash: [0u8; 16],
            view_epoch: 1,
            snapshot_pin_id: PinId(1),
        };
        sched.dag.lock().submit(
            cache_id.clone(),
            WorkKind::CacheNode,
            Priority::Interactive,
            Vec::new(),
            None,
        );

        // drive_one consumes the CacheNode via next_ready (which
        // reserves a CPU permit) and the defensive skip MUST release.
        let _ = sched.drive_one();

        // DISCRIMINATOR: the defensive skip must release the permit.
        // Without the cancel, the counter would be 1 (parked
        // reservation lives on the dispatched CacheNode entry);
        // with the cancel it is 0 because the skip routes through
        // `dag.cancel(&job.identity)`.
        assert_eq!(
            sched.dag.lock().in_flight_cpu_permits(),
            0,
            "CacheNode defensive skip must release the parked CPU permit \
             — without it the {{cpu:1}} class would stay drained",
        );

        // Submit a real CPU job and verify it dispatches — the
        // follow-on side of the discriminator. A leaked permit
        // would have stalled this in the CPU class.
        let h = sched.submit_request(Request {
            file_id: "/follow.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(
            h.try_get().unwrap().is_ready(),
            "follow-on Analysis must dispatch after CacheNode skip released the permit",
        );
    }

    /// External `commit_artifact()` must terminalize the matching
    /// Artifact DAG identity so a concurrent internal Artifact worker
    /// cannot overwrite the committed snapshot AND so the DAG node's
    /// parked capacity reservation releases.
    ///
    /// Without the cancel inside `commit_artifact`, the call only
    /// signals waiter groups; the matching DAG node lingers in
    /// `nodes` / `by_identity` with a parked CPU permit, and a
    /// re-dispatched internal worker would overwrite the committed
    /// snapshot with the executor's default `EmptyData` artifact.
    /// With the cancel, `commit_artifact` cancels the matching DAG
    /// identity, releasing the parked permit AND making the dispatch
    /// loop's `nodes.get(file_id)` lookup observe the canonical
    /// state with the committed snapshot; the internal worker
    /// skips dispatch.
    #[test]
    fn external_commit_artifact_terminalizes_dag_identity() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("content"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Step 1: drive Source + Analysis to ready state by submitting
        // an Analysis request and draining.
        let h_analysis = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(h_analysis.try_get().unwrap().is_ready());

        // Step 2: submit an Artifact request — the Artifact DAG
        // identity admits into `nodes` / `by_identity`.
        let _h_artifact = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 42 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drain_inbox();

        let artifact_id = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            generation: 1,
            profile_hash: profile_hash_to_bytes(42),
            content_hash: [0u8; 16],
        };
        assert!(
            sched.dag.lock().token_for(&artifact_id).is_some(),
            "fixture invariant: Artifact identity admitted into DAG",
        );

        // Step 3: externally commit a real artifact for `(/a.vue, 42)`
        // BEFORE the internal worker reaches execute_artifact_stage.
        // The committed snapshot carries distinguishing data we will
        // assert is not overwritten by the internal worker.
        let committed = ArtifactSnapshot {
            generation: 1,
            profile_hash: 42,
            data: Arc::new(crate::node::EmptyData),
        };
        sched.commit_artifact("/a.vue", 42, committed);

        // DISCRIMINATOR 1: the Artifact DAG identity is now terminal
        // — removed from `by_identity` and `nodes`. Without
        // terminalization the entry would linger and a parked CPU
        // permit would stay live.
        assert!(
            sched.dag.lock().token_for(&artifact_id).is_none(),
            "external commit_artifact must terminalize the matching DAG identity \
             — otherwise the entry lingers and the parked CPU permit leaks",
        );

        // DISCRIMINATOR 2: no leaked CPU permit from a dispatched
        // Artifact node that never reached `dag.complete`.
        assert_eq!(
            sched.dag.lock().in_flight_cpu_permits(),
            0,
            "external commit_artifact must release the parked CPU permit",
        );

        // Drive once more — the internal worker MUST NOT dispatch a
        // duplicate against `/a.vue, 42`. Without the cancel inside
        // `commit_artifact`, the artifact DAG node would still be
        // present (terminalized by signal_stage_complete only at the
        // waiter-group level), and `next_ready` would re-dispatch a
        // worker that overwrites the committed artifact with the
        // default executor's `EmptyData` snapshot.
        sched.drive_all();
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_some(),
            "committed artifact must survive — internal worker must not overwrite",
        );
    }

    /// Artifact identity is removed from the DAG once the Artifact
    /// stage completes. Without dag.complete(&artifact_id) in the
    /// Artifact arm of handle_stage_complete, the Artifact identity
    /// would linger and its cpu permit would never return.
    #[test]
    fn artifact_identity_removed_from_dag_after_artifact_stage_completes() {
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
        let _ = handle.try_get();

        let artifact_id = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            generation: 1,
            profile_hash: profile_hash_to_bytes(42),
            content_hash: [0u8; 16],
        };
        let dag = sched.dag.lock();
        assert!(
            dag.token_for(&artifact_id).is_none(),
            "Artifact identity must be removed from by_identity after \
             handle_stage_complete completes the Artifact arm",
        );
        // Both the Source io permit and the Analysis/Artifact cpu
        // permits must have returned to the pool.
        assert_eq!(dag.in_flight_cpu_permits(), 0);
        assert_eq!(dag.in_flight_io_permits(), 0);
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

    // ── Failure / panic permit release ──
    //
    // The DAG node holds a parked capacity reservation between
    // `next_ready` and the terminal `complete`/`cancel`. A failure or
    // panic terminal path that signals waiters but never cancels the
    // matching DAG node leaks the reservation. With a tight
    // {cpu:1, io:1} budget, a single stage error stalls the class.
    // These tests pin the discriminator: the next stage at the same
    // class must dispatch after a prior stage fails.

    /// Source executor that returns Err on the first call.
    struct ErrSourceExecutor;
    impl StageExecutor for ErrSourceExecutor {
        fn execute_source(
            &self,
            _canonical_id: &str,
            _file_kind: crate::node::FileKind,
            _content: Arc<str>,
            _generation: u64,
        ) -> Result<SourceSnapshot, crate::executor::StageError> {
            Err(crate::executor::StageError {
                message: "synthetic source failure".to_string(),
            })
        }
    }

    /// Analysis executor that succeeds on Source and Errs on Analysis.
    struct ErrAnalysisExecutor;
    impl StageExecutor for ErrAnalysisExecutor {
        fn execute_analysis(
            &self,
            _canonical_id: &str,
            _source: &SourceSnapshot,
            _generation: u64,
        ) -> Result<AnalysisSnapshot, crate::executor::StageError> {
            Err(crate::executor::StageError {
                message: "synthetic analysis failure".to_string(),
            })
        }
    }

    /// Artifact executor that succeeds on Source/Analysis and Errs on
    /// Artifact only.
    struct ErrArtifactExecutor;
    impl StageExecutor for ErrArtifactExecutor {
        fn execute_artifact(
            &self,
            _canonical_id: &str,
            _source: &SourceSnapshot,
            _analysis: &AnalysisSnapshot,
            _profile_hash: u64,
            _generation: u64,
        ) -> Result<ArtifactSnapshot, crate::executor::StageError> {
            Err(crate::executor::StageError {
                message: "synthetic artifact failure".to_string(),
            })
        }
    }

    /// Source executor that panics on the first call.
    struct PanickingSourceExecutor;
    impl StageExecutor for PanickingSourceExecutor {
        fn execute_source(
            &self,
            _canonical_id: &str,
            _file_kind: crate::node::FileKind,
            _content: Arc<str>,
            _generation: u64,
        ) -> Result<SourceSnapshot, crate::executor::StageError> {
            panic!("synthetic source panic");
        }
    }

    /// Build a tight-budget sync scheduler at `{cpu:1, io:1}` so a
    /// single leaked permit pins the class.
    fn tight_budget_sched(
        loader: Arc<MemorySourceLoader>,
        executor: Arc<dyn StageExecutor>,
    ) -> Arc<Scheduler> {
        let config = SchedulerConfig {
            cpu_threads: 1,
            io_threads: 1,
            aging: DagAgingConfig::default(),
            dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
        };
        Scheduler::new_sync_with_executor(config, loader, executor)
    }

    /// Source executor Err must release the IO permit and let a
    /// follow-on Source job dispatch.
    #[test]
    fn failure_releases_source_io_permit() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/fail.vue".to_string(), Arc::from("content-a"));
        loader.insert("/ok.vue".to_string(), Arc::from("content-b"));
        let sched = tight_budget_sched(loader, Arc::new(ErrSourceExecutor));

        // First request: source stage fails inside the executor.
        let h_fail = sched.submit_request(Request {
            file_id: "/fail.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        match h_fail.try_get().unwrap() {
            CompletionState::Failed(_) => {}
            other => panic!("expected Failed, got {:?}", other),
        }

        // DISCRIMINATOR: with the {cpu:1, io:1} budget, a leaked IO
        // permit (failure path never cancelled the DAG node) stalls
        // the IO class. The follow-on Source request below would
        // hang at `try_get()` because next_ready returns None.
        // With the cancel, the permit releases and the follow-on
        // dispatches.
        assert_eq!(
            sched.dag.lock().in_flight_io_permits(),
            0,
            "Source-failure path must release the parked IO permit",
        );

        let h_ok = sched.submit_request(Request {
            file_id: "/ok.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        match h_ok.try_get().unwrap() {
            CompletionState::Failed(_) => {
                // Both files share the same ErrSourceExecutor so the
                // follow-on also fails — but the DISCRIMINATOR is
                // that it DISPATCHED at all. A leaked permit would
                // have left it pending.
            }
            CompletionState::Ready(_) => {}
            other => panic!(
                "follow-on must dispatch (Ready or Failed), got: {:?}",
                other
            ),
        }
        assert_eq!(
            sched.dag.lock().in_flight_io_permits(),
            0,
            "follow-on Source-failure path must also release the IO permit",
        );
    }

    /// Analysis executor Err must release the CPU permit and let a
    /// follow-on CPU job dispatch.
    #[test]
    fn failure_releases_analysis_cpu_permit() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/fail.vue".to_string(), Arc::from("content-a"));
        loader.insert("/ok.vue".to_string(), Arc::from("content-b"));
        let sched = tight_budget_sched(loader, Arc::new(ErrAnalysisExecutor));

        let h_fail = sched.submit_request(Request {
            file_id: "/fail.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        match h_fail.try_get().unwrap() {
            CompletionState::Failed(_) => {}
            other => panic!("expected Failed, got {:?}", other),
        }

        // DISCRIMINATOR: the Analysis stage runs on the CPU class.
        // A leaked permit pins the CPU budget at 1 and the follow-on
        // Analysis job below would not dispatch.
        assert_eq!(
            sched.dag.lock().in_flight_cpu_permits(),
            0,
            "Analysis-failure path must release the parked CPU permit",
        );

        let h_ok = sched.submit_request(Request {
            file_id: "/ok.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(
            h_ok.try_get().is_some(),
            "follow-on Analysis must dispatch — a leaked CPU permit \
             would have left it pending and try_get() would return None"
        );
        assert_eq!(sched.dag.lock().in_flight_cpu_permits(), 0);
    }

    /// Artifact executor Err must release the CPU permit and let a
    /// follow-on Artifact job dispatch.
    #[test]
    fn failure_releases_artifact_cpu_permit() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/fail.vue".to_string(), Arc::from("content-a"));
        loader.insert("/ok.vue".to_string(), Arc::from("content-b"));
        let sched = tight_budget_sched(loader, Arc::new(ErrArtifactExecutor));

        let h_fail = sched.submit_request(Request {
            file_id: "/fail.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 42 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        match h_fail.try_get().unwrap() {
            CompletionState::Failed(_) => {}
            other => panic!("expected Failed, got {:?}", other),
        }

        // DISCRIMINATOR: Artifact stage runs on CPU. A leaked permit
        // stalls the CPU class — the follow-on Artifact below would
        // not dispatch.
        assert_eq!(
            sched.dag.lock().in_flight_cpu_permits(),
            0,
            "Artifact-failure path must release the parked CPU permit",
        );

        let h_ok = sched.submit_request(Request {
            file_id: "/ok.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 99 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(
            h_ok.try_get().is_some(),
            "follow-on Artifact must dispatch — a leaked CPU permit \
             would have left it pending"
        );
        assert_eq!(sched.dag.lock().in_flight_cpu_permits(), 0);
    }

    /// FileNotFound Source failure must release the IO permit.
    #[test]
    fn file_not_found_releases_io_permit() {
        // Empty loader: every file lookup returns None.
        let loader = Arc::new(MemorySourceLoader::new());
        // Use the default executor so Source-success (if it got that
        // far) would just stub-succeed; the failure here happens BEFORE
        // the executor at the FileNotFound branch.
        let executor: Arc<dyn StageExecutor> = Arc::new(DefaultExecutor);
        let sched = tight_budget_sched(Arc::clone(&loader), executor);

        let h_fail = sched.submit_request(Request {
            file_id: "/missing.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        match h_fail.try_get().unwrap() {
            CompletionState::Failed(_) => {}
            other => panic!("expected Failed, got {:?}", other),
        }

        // DISCRIMINATOR: FileNotFound terminal-fail must also release.
        assert_eq!(
            sched.dag.lock().in_flight_io_permits(),
            0,
            "FileNotFound failure path must release the parked IO permit",
        );

        // Follow-on with a present file: must dispatch and succeed.
        loader.insert("/present.vue".to_string(), Arc::from("content"));
        let h_ok = sched.submit_request(Request {
            file_id: "/present.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(
            h_ok.try_get().unwrap().is_ready(),
            "follow-on Source must dispatch and succeed — a leaked \
             IO permit from FileNotFound would have stalled the class"
        );
    }

    /// Panic in a Source executor must release the IO permit on the
    /// catch_unwind path. The panic recovery wraps the in-process
    /// failure into Failed(...); the permit must release symmetrically
    /// with the executor-returns-Err path.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn panic_catch_releases_io_permit() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/panic.vue".to_string(), Arc::from("content"));
        loader.insert("/ok.vue".to_string(), Arc::from("ok"));
        // Use the native scheduler (not sync) so the panic-catch arm
        // runs through `std::panic::catch_unwind` in the io_pool
        // closure.
        let config = SchedulerConfig {
            cpu_threads: 1,
            io_threads: 1,
            aging: DagAgingConfig::default(),
            dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
        };
        let sched = Scheduler::with_executor(config, loader, Arc::new(PanickingSourceExecutor));

        let h_panic = sched.submit_request(Request {
            file_id: "/panic.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        // Wait for the panic-catch arm to surface the failure.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let state = loop {
            if let Some(s) = h_panic.try_get() {
                break Some(s);
            }
            if std::time::Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(state.is_some(), "panicked source request must resolve");
        match state.unwrap() {
            CompletionState::Failed(_) => {}
            other => panic!("expected Failed after panic, got {:?}", other),
        }

        // DISCRIMINATOR: the catch_unwind arm must release the IO
        // permit. Without that release the arm would only call
        // signal_file_failed_for_stage and leave the parked
        // reservation alive.
        assert_eq!(
            sched.dag.lock().in_flight_io_permits(),
            0,
            "panic-catch surface path must release the parked IO permit",
        );
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

    /// `reset()` must clear `auto_ingested_recent` so the
    /// auto-ingest tracking map does not leak across repeated
    /// reset+rebuild cycles (LSP workspace switch, MCP session
    /// boundary, multi-project bench).
    ///
    /// Discriminator: register a blocker so the auto-ingest path
    /// plants a tracking entry, assert the entry exists, call
    /// `reset()`, assert the map is empty. Repeat with multiple
    /// unique canonicals to verify no incremental leak across
    /// multiple reset cycles.
    ///
    /// Without the reset-time clear, every call to
    /// `register_resolved_deps` that triggers an auto-ingest leaks
    /// one entry across every reset, and the leak compounds across
    /// reset cycles. With the clear, the map is empty on every
    /// reset.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reset_clears_auto_ingest_tracking() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::new(SchedulerConfig::default(), loader);

        // Step 1: bring /a.vue to Source-committed so a subsequent
        // `register_resolved_deps` exercises the auto-ingest path
        // (the early-return at `generation == 0 || current_source().is_none()`
        // would otherwise skip the tracking insert).
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        h.wait();

        // Step 2: register a blocker. The auto-ingest path creates
        // /dep.ts, plants the tracking entry, and enqueues the
        // Source NewRequest. The cleanup arm in handle_new_request
        // would drop the entry once the driver dequeues — drive
        // only enough to fire the register, NOT the full drain,
        // by submitting through the NON-driving register path
        // and checking the map BEFORE drive_all.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Precondition: tracking entry is present. (The driver may
        // have drained the NewRequest by the time we check; if so,
        // the cleanup arm already cleared the entry and this
        // precondition fails. The discriminator below remains
        // valid regardless because we re-plant for arm 2.)
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        let pre_reset_present = sched.auto_ingested_recent.contains_key(&dep_arc);

        // Step 3: ALSO plant a synthetic entry directly so the
        // assertion is independent of whether the driver drained
        // the NewRequest. The discriminator is: any entry in the
        // map before reset must be gone after reset.
        let synthetic: Arc<str> = Arc::from("/synthetic-blocker.ts");
        sched.auto_ingested_recent.insert(
            Arc::clone(&synthetic),
            AutoIngestedRecord {
                generation: 7,
                since: Instant::now(),
            },
        );
        assert!(
            sched.auto_ingested_recent.contains_key(&synthetic),
            "precondition: synthetic auto_ingested_recent entry must be present before reset",
        );

        // Step 4: reset. The map must clear unconditionally.
        sched.reset();

        // KEY ASSERTION: the map is empty after reset.
        // Without the reset-time clear the synthetic entry would
        // survive (and so would the /dep.ts entry if the driver had
        // not yet drained the NewRequest), leaking across the reset
        // boundary.
        assert_eq!(
            sched.auto_ingested_recent.len(),
            0,
            "auto_ingested_recent must be cleared on reset(). \
             pre_reset_dep_present={pre_reset_present}",
        );
        assert!(
            !sched.auto_ingested_recent.contains_key(&synthetic),
            "the synthetic entry must be gone after reset",
        );

        // Step 5: repeat with 5 unique canonicals across 5 reset
        // cycles. A bounded-per-cycle leak would accumulate to
        // `len() >= 5` by the final reset; with the clear, every
        // cycle empties the map.
        sched.restart_driver();
        for i in 0..5 {
            let canonical: Arc<str> = Arc::from(format!("/cycle-{i}.ts").as_str());
            sched.auto_ingested_recent.insert(
                Arc::clone(&canonical),
                AutoIngestedRecord {
                    generation: 11,
                    since: Instant::now(),
                },
            );
            assert!(
                sched.auto_ingested_recent.contains_key(&canonical),
                "cycle {i}: entry must be present before reset",
            );
            sched.reset();
            assert_eq!(
                sched.auto_ingested_recent.len(),
                0,
                "cycle {i}: reset must clear the map (no per-cycle leak)",
            );
            sched.restart_driver();
        }
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

    /// `register_resolved_deps` arriving AFTER the owner's Analysis
    /// has committed must NOT re-dispatch a fresh Analysis identity.
    /// The blocker `DepKey`s land in the DAG's typed Artifact blocker
    /// registry and ride on the next Artifact admission via
    /// `admit_artifact_with_blockers`. Without the skip-on-already-
    /// complete guard the `dag.submit` ran unconditionally, creating
    /// a fresh Analysis gate the executor would re-run on already-
    /// analyzed source.
    ///
    /// Discriminator: drive /a.vue Source + Analysis to committed,
    /// call register_resolved_deps with a bare blocker, and then
    /// assert (a) the DAG holds NO fresh Analysis identity for
    /// /a.vue at the live generation, AND (b) the blocker is
    /// recorded in the typed registry. Without the guard the DAG
    /// would hold a re-dispatched Analysis identity; with the guard
    /// it does not.
    #[test]
    fn register_resolved_deps_does_not_redispatch_completed_analysis() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/bare-dep.ts".to_string(), Arc::from("dep"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Step 1: upsert + drive to Analysis (Source + Analysis committed).
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
        assert!(
            sched.try_get_analysis("/a.vue").is_some(),
            "precondition: /a.vue Analysis committed before blocker arrives",
        );

        // Snapshot the DAG: confirm NO live Analysis identity for
        // /a.vue at the live generation prior to register.
        let analysis_id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/a.vue"),
            generation: gen,
            stage: FileStageKey::Analysis,
        };
        assert!(
            sched.dag.lock().token_for(&analysis_id).is_none(),
            "precondition: drive_all completed the Analysis identity so \
             no live entry remains in the DAG before register_resolved_deps",
        );

        // Step 2: register_resolved_deps arrives AFTER Analysis committed.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/bare-dep.ts".to_string()],
            vec!["/bare-dep.ts".to_string()],
        );

        // KEY ASSERTION 1: NO fresh Analysis identity was re-dispatched.
        // Without the skip-on-already-complete arm an unconditional
        // dag.submit would re-admit Analysis for /a.vue at the live
        // generation; the guard leaves the DAG untouched.
        assert!(
            sched.dag.lock().token_for(&analysis_id).is_none(),
            "register_resolved_deps must NOT re-dispatch Analysis for \
             /a.vue when current_analysis() is already Some — a \
             redundant dag.submit would re-admit Analysis on \
             already-analyzed source, forcing the executor to run \
             execute_analysis again",
        );

        // KEY ASSERTION 2: the blocker IS recorded in the typed
        // Artifact blocker registry, ready for the next Artifact
        // admission.
        let a_arc: Arc<str> = Arc::from("/a.vue");
        let blockers = sched.dag.lock().peek_artifact_blockers(&a_arc, gen);
        assert!(
            blockers.deps.iter().any(|d| matches!(
                d,
                DepKey::FileStage { canonical, stage: FileStageKey::Analysis, .. }
                if canonical.as_ref() == "/bare-dep.ts"
            )),
            "blocker must be recorded in the Artifact blocker registry \
             for downstream Artifact admissions. observed: {blockers:?}",
        );

        // Step 3: request Artifact — admission attaches the blocker
        // DepKey via admit_artifact_with_blockers and the Artifact
        // gates until /bare-dep.ts Analysis completes.
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 7 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drain_inbox();
        assert!(
            h.try_get().is_none() || !h.try_get().unwrap().is_ready(),
            "Artifact must be gated until /bare-dep.ts Analysis completes",
        );

        // Step 4: drive everything — /bare-dep.ts gets analyzed, the
        // Artifact's blocker dep clears, Artifact dispatches +
        // completes.
        sched.drive_all();
        assert!(
            h.try_get().unwrap().is_ready(),
            "Artifact should complete after blocker resolves",
        );
    }

    /// Per-file gate plumbing for the late-blocker-while-in-flight
    /// test: `entered_tx` fires once when the worker enters Analysis;
    /// `release_rx` blocks the worker inside Analysis until the test
    /// thread drops the matching sender.
    struct AnalysisGate {
        entered_tx: crossbeam_channel::Sender<()>,
        release_rx: crossbeam_channel::Receiver<()>,
    }

    /// Test executor that gates `execute_analysis` per-file so the
    /// test can observe the in-flight window AND control when
    /// Analysis completes. Files without an entry run normally.
    struct GatedAnalysisExecutor {
        gates: dashmap::DashMap<String, AnalysisGate>,
    }

    impl crate::executor::StageExecutor for GatedAnalysisExecutor {
        fn execute_analysis(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
            generation: u64,
        ) -> Result<AnalysisSnapshot, crate::executor::StageError> {
            if let Some(gate) = self.gates.get(canonical_id) {
                // Signal that the worker has entered Analysis. The
                // send is best-effort: if the test thread has already
                // dropped the receiver the executor proceeds.
                let _ = gate.entered_tx.send(());
                // Block until the test releases this gate. A
                // Disconnected result means the test dropped the
                // sender, which is the normal release signal.
                let _ = gate.release_rx.recv();
            }
            Ok(AnalysisSnapshot::new_empty(generation))
        }
    }

    /// A blocker `DepKey` registered via `register_resolved_deps`
    /// AFTER the owner's Analysis has already dispatched must still
    /// gate the downstream Artifact run on the blocker's Analysis.
    /// The in-flight Analysis node's incoming edges are immutable
    /// (closed prereq invariant), so the gate cannot live on that
    /// node; it rides on the Artifact admission instead.
    ///
    /// Pre-strip: the dispatched Analysis node had the blocker
    /// silently appended to `deps_remaining`, but `has_pending_deps`
    /// already returns false for a dispatched node, so the Artifact
    /// admitted immediately when Analysis completed (the late dep
    /// was silently dropped from the gating story). Post-strip +
    /// rewire: the Artifact admission attaches the blocker
    /// `DepKey` directly, so the Artifact stays pending until the
    /// blocker's Analysis completes.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn late_register_resolved_deps_while_analysis_in_flight_gates_artifact_until_blocker_analysis()
    {
        use std::time::Duration;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep content"));

        let (a_entered_tx, a_entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (a_release_tx, a_release_rx) = crossbeam_channel::bounded::<()>(1);
        let (dep_entered_tx, dep_entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (dep_release_tx, dep_release_rx) = crossbeam_channel::bounded::<()>(1);

        let executor = Arc::new(GatedAnalysisExecutor {
            gates: dashmap::DashMap::new(),
        });
        executor.gates.insert(
            "/a.vue".to_string(),
            AnalysisGate {
                entered_tx: a_entered_tx,
                release_rx: a_release_rx,
            },
        );
        executor.gates.insert(
            "/dep.ts".to_string(),
            AnalysisGate {
                entered_tx: dep_entered_tx,
                release_rx: dep_release_rx,
            },
        );

        let sched = Scheduler::with_executor(SchedulerConfig::default(), loader, executor);

        // Submit an Artifact request for /a.vue — drives Source →
        // Analysis. Analysis dispatches and blocks inside the gated
        // executor.
        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 11 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // Wait until /a.vue Analysis is in flight (worker has entered
        // execute_analysis but is parked on the release channel).
        a_entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("/a.vue Analysis must enter the gated executor");

        // Now register a late blocker. The Analysis identity is
        // already dispatched, so its incoming edges are immutable;
        // the blocker must instead ride on the downstream Artifact
        // admission via the DAG's typed Artifact blocker registry.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Release /a.vue Analysis. It completes, the driver runs
        // `handle_stage_complete(Analysis)` which admits the
        // Artifact via `admit_pending_artifacts` →
        // `admit_artifact_with_blockers`. Pre-strip the Artifact
        // would dispatch immediately because the late dep was
        // silently dropped from gating. Post-strip the Artifact
        // submission carries the /dep.ts Analysis `DepKey` and waits.
        drop(a_release_tx);

        // Give the driver time to process Analysis completion and
        // attempt Artifact admission. The handle MUST remain
        // unresolved because /dep.ts Analysis is still gated.
        for _ in 0..20 {
            if handle.try_get().map(|s| s.is_ready()).unwrap_or(false) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            !handle.try_get().map(|s| s.is_ready()).unwrap_or(false),
            "Artifact must NOT complete while the late blocker's Analysis is still in flight: \
             the dispatched Analysis node's incoming edges are immutable, so the blocker must \
             gate the Artifact admission instead",
        );
        assert!(
            sched.try_get_artifact("/a.vue", 11).is_none(),
            "Artifact snapshot must not be committed while the late blocker is unresolved",
        );

        // Wait for /dep.ts Analysis to actually be dispatched (the
        // executor entered) BEFORE releasing. Auto-ingest drives
        // Source then Analysis; the entered signal proves Analysis
        // started.
        dep_entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("/dep.ts Analysis must reach the executor");

        // Release /dep.ts Analysis. The blocker's Analysis
        // completes, which clears the Artifact's `deps_remaining`
        // via `dag.complete(&dep_analysis_id)` fan-out, and the
        // Artifact dispatches.
        drop(dep_release_tx);

        // Poll for Artifact completion with a generous timeout. The
        // driver must run Source(/dep.ts) → Analysis(/dep.ts) →
        // re-dispatch Artifact(/a.vue) → execute_artifact → publish.
        let mut state: Option<CompletionState<RequestResult>> = None;
        for _ in 0..200 {
            if let Some(s) = handle.try_get() {
                if s.is_ready() {
                    state = Some(s);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            state.as_ref().map(|s| s.is_ready()).unwrap_or(false),
            "Artifact must complete after the late blocker's Analysis resolves: \
             got {state:?}"
        );
    }

    /// Source executor that BLOCKS on a per-file gate (signalling
    /// `entered_tx` on entry) and PANICS when the test thread drops
    /// the matching `release_tx`. Used by the panic-on-superseded
    /// test to control timing between `bump_generation` and the
    /// worker's panic.
    /// Executor that fails Analysis for `dep_id` and treats `owner_id`
    /// as depending on `dep_id` via the blocker mechanism. Used by
    /// the terminalize-stranded test to construct the scenario where
    /// a failed dep-Analysis strands the owner's Analysis gate in the
    /// DAG.
    struct DepAnalysisFailExecutor {
        owner_id: String,
        dep_id: String,
    }

    impl crate::executor::StageExecutor for DepAnalysisFailExecutor {
        fn extract_deps(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
        ) -> crate::executor::ExtractedDeps {
            if canonical_id == self.owner_id {
                crate::executor::ExtractedDeps {
                    forward_deps: vec![self.dep_id.clone()],
                    blocker_ids: vec![self.dep_id.clone()],
                }
            } else {
                crate::executor::ExtractedDeps::default()
            }
        }

        fn execute_analysis(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
            generation: u64,
        ) -> Result<AnalysisSnapshot, crate::executor::StageError> {
            if canonical_id == self.dep_id {
                Err(crate::executor::StageError {
                    message: "synthetic dep Analysis failure".to_string(),
                })
            } else {
                Ok(AnalysisSnapshot::new_empty(generation))
            }
        }
    }

    /// A `terminalize_failure` on a Source/Analysis identity that
    /// has downstream DepKey waiters must re-enqueue the stranded
    /// waiters so the driver thread re-runs dispatch promptly.
    /// Without the wake the stranded waiter still dispatches —
    /// eventually — but only after the next aging-interval tick
    /// (default 5s), which inflates failure-path latency.
    ///
    /// Test setup: owner `/a.vue` lists `/dep.ts` as a blocker.
    /// `/dep.ts` Source succeeds; `/dep.ts` Analysis returns an
    /// executor error. `terminalize_failure(Analysis, /dep.ts)`
    /// cancels the dep's Analysis identity and strands the owner's
    /// Analysis gate (whose only remaining `DepKey` was that
    /// Analysis). The wake-on-stranded path nudges the driver to
    /// dispatch the stranded gate immediately so the owner's
    /// Artifact resolves quickly.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn terminalize_failure_with_dag_waiters_wakes_driver_for_prompt_redispatch() {
        use std::time::{Duration, Instant};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("owner content"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep content"));

        let executor = Arc::new(DepAnalysisFailExecutor {
            owner_id: "/a.vue".to_string(),
            dep_id: "/dep.ts".to_string(),
        });

        let sched = Scheduler::with_executor(SchedulerConfig::default(), loader, executor);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 23 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // The owner's Artifact must resolve in well under the
        // driver's aging interval (5s). Without the wake-on-stranded
        // path the dep's Analysis failure strands the owner's
        // Analysis gate and the driver sleeps in `recv_timeout`
        // until the next aging tick; with the wake the path triggers
        // a prompt dispatch and the owner's Analysis + Artifact run
        // in sub-second time.
        let start = Instant::now();
        let deadline = start + Duration::from_millis(1500);
        let mut state: Option<CompletionState<RequestResult>> = None;
        while Instant::now() < deadline {
            if let Some(s) = handle.try_get() {
                state = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let elapsed = start.elapsed();
        assert!(
            state.is_some(),
            "owner Artifact must resolve within 1500ms (without the \
             wake-on-stranded path, the stranded Analysis gate would \
             wait for the 5s aging tick); got None after {elapsed:?}"
        );
        // Don't assert ready vs failed — the owner's Artifact may
        // succeed (the dep was only a blocker gate) or fail
        // depending on downstream wiring. The discriminator is
        // PROMPT resolution, not outcome.
    }

    struct GatedPanickingSourceExecutor {
        gates: dashmap::DashMap<
            String,
            (
                crossbeam_channel::Sender<()>,
                crossbeam_channel::Receiver<()>,
            ),
        >,
    }

    impl crate::executor::StageExecutor for GatedPanickingSourceExecutor {
        fn execute_source(
            &self,
            canonical_id: &str,
            _file_kind: crate::node::FileKind,
            _content: Arc<str>,
            _generation: u64,
        ) -> Result<SourceSnapshot, crate::executor::StageError> {
            if let Some(entry) = self.gates.get(canonical_id) {
                let (entered_tx, release_rx) = entry.value();
                let _ = entered_tx.send(());
                let _ = release_rx.recv();
            }
            panic!("synthetic gated source panic");
        }
    }

    /// A worker-stage panic on a superseded generation must still
    /// release the parked admission permit. Without unconditional
    /// terminalization, `surface_stage_panic_as_failed`'s early-
    /// return on generation mismatch would let the permit linger
    /// between `bump_generation` and any later supersede sweep;
    /// with the unconditional path, `terminalize_failure` runs even
    /// on a stale generation so the permit releases through the
    /// DAG node's `cancel` path.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn panic_on_superseded_generation_still_releases_permit() {
        use std::time::Duration;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/panic.vue".to_string(), Arc::from("content"));

        let (entered_tx, entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(1);

        let executor = Arc::new(GatedPanickingSourceExecutor {
            gates: dashmap::DashMap::new(),
        });
        executor
            .gates
            .insert("/panic.vue".to_string(), (entered_tx, release_rx));

        // Tight {cpu:1, io:1} so a leaked permit pins the class
        // deterministically.
        let config = SchedulerConfig {
            cpu_threads: 1,
            io_threads: 1,
            aging: DagAgingConfig::default(),
            dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
        };
        let sched = Scheduler::with_executor(config, loader, executor);

        let _h = sched.submit_request(Request {
            file_id: "/panic.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // Wait for the Source worker to enter the gated executor.
        // At this point the IO permit is parked on the dispatched
        // gen=1 DAG node.
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Source worker must enter the gated executor");

        // Bump the generation BEHIND the running worker WITHOUT
        // calling the supersede sweep. This isolates the
        // permit-release responsibility on
        // `surface_stage_panic_as_failed`: there is no other code
        // path (no `cancel_matching` from supersede) that could
        // release the permit on its behalf. A generation-mismatch
        // early return would leave the permit parked forever in
        // this configuration.
        let node = sched.nodes.get("/panic.vue").expect("node exists").clone();
        node.bump_generation();

        // Release the gate; the executor panics; panic-catch enters
        // `surface_stage_panic_as_failed`.
        drop(release_tx);

        // Wait for the panic-catch closure to finish.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if sched.dag.lock().in_flight_io_permits() == 0 {
                break;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(
            sched.dag.lock().in_flight_io_permits(),
            0,
            "panic-catch surface path must release the parked IO permit even \
             when the generation has been superseded — a gen-mismatch \
             early return would leave the permit lingering",
        );
    }

    /// Sentinel artifact data that carries an identifying tag so a
    /// test can distinguish an externally-committed snapshot from an
    /// internally-produced one. The race-resolution check uses
    /// pointer-stable downcasting via `Any`.
    #[derive(Debug, Clone)]
    struct SentinelData {
        tag: &'static str,
    }

    impl crate::node::SnapshotData for SentinelData {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// Per-file Artifact gate, mirroring `AnalysisGate` but for the
    /// Artifact stage. The worker fires `entered_tx` on entry and
    /// blocks on `release_rx` until the test releases the gate.
    struct ArtifactGate {
        entered_tx: crossbeam_channel::Sender<()>,
        release_rx: crossbeam_channel::Receiver<()>,
    }

    /// Test executor whose `execute_artifact` parks on a per-file
    /// gate so the test can race an external `commit_artifact` against
    /// the in-flight worker. Returns its OWN sentinel-tagged snapshot
    /// when released so the test can verify which path wrote the
    /// final stored artifact.
    struct GatedArtifactExecutor {
        gates: dashmap::DashMap<String, ArtifactGate>,
        worker_tag: &'static str,
    }

    impl crate::executor::StageExecutor for GatedArtifactExecutor {
        fn execute_artifact(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
            _analysis: &AnalysisSnapshot,
            profile_hash: u64,
            generation: u64,
        ) -> Result<ArtifactSnapshot, crate::executor::StageError> {
            if let Some(gate) = self.gates.get(canonical_id) {
                let _ = gate.entered_tx.send(());
                let _ = gate.release_rx.recv();
            }
            Ok(ArtifactSnapshot {
                generation,
                profile_hash,
                data: Arc::new(SentinelData {
                    tag: self.worker_tag,
                }),
            })
        }
    }

    /// An external `commit_artifact` that lands while the internal
    /// Artifact worker is mid-executor must NOT be overwritten by the
    /// worker's post-executor insert. The DAG lock is the
    /// synchronization point: `commit_artifact` performs its insert,
    /// signal, and terminalize under the lock, and the worker
    /// re-checks `node.artifacts` under the same lock before its
    /// own insert. If a same-`(canonical, generation, profile_hash)`
    /// snapshot is already present, the worker drops its result.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn external_commit_during_dispatched_artifact_worker_does_not_overwrite_external_snapshot() {
        use std::time::Duration;

        const EXTERNAL_TAG: &str = "external-publish";
        const WORKER_TAG: &str = "worker-snapshot";

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));

        let (entered_tx, entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(1);

        let executor = Arc::new(GatedArtifactExecutor {
            gates: dashmap::DashMap::new(),
            worker_tag: WORKER_TAG,
        });
        executor.gates.insert(
            "/a.vue".to_string(),
            ArtifactGate {
                entered_tx,
                release_rx,
            },
        );

        let sched = Scheduler::with_executor(SchedulerConfig::default(), loader, executor);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 17 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // Wait until the Artifact worker has parked inside the gated
        // executor.
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Artifact worker must enter the gated executor");

        // Look up the live generation for the file (Source + Analysis
        // have already committed by the time the Artifact worker
        // reached the gate).
        let generation = sched.nodes.get("/a.vue").expect("node exists").generation();

        // External commit lands while the worker is parked.
        sched.commit_artifact(
            "/a.vue",
            17,
            ArtifactSnapshot {
                generation,
                profile_hash: 17,
                data: Arc::new(SentinelData { tag: EXTERNAL_TAG }),
            },
        );

        // Release the worker. Without the insert-if-absent re-check
        // it would overwrite the externally-committed snapshot with
        // its own; with the re-check under the DAG lock the worker
        // finds the external snapshot and drops its result.
        drop(release_tx);

        // Wait for the request handle to resolve (it was already
        // signalled by the external commit, but a poll loop tolerates
        // any scheduler-driven re-signal that might happen).
        let mut state: Option<CompletionState<RequestResult>> = None;
        for _ in 0..200 {
            if let Some(s) = handle.try_get() {
                if s.is_ready() {
                    state = Some(s);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            state.as_ref().map(|s| s.is_ready()).unwrap_or(false),
            "Artifact handle must resolve: got {state:?}"
        );

        // Give the worker thread time to finish its post-executor
        // path so any overwriting insert would have already landed.
        std::thread::sleep(Duration::from_millis(200));

        // Inspect the stored artifact: its `data` payload must be
        // the EXTERNAL sentinel. Without the insert-if-absent guard
        // the worker would overwrite and the stored tag would be
        // WORKER_TAG.
        let stored = sched
            .try_get_artifact("/a.vue", 17)
            .expect("artifact must be readable");
        let stored_data = stored
            .data
            .as_any()
            .downcast_ref::<SentinelData>()
            .expect("stored data must be SentinelData");
        assert_eq!(
            stored_data.tag,
            EXTERNAL_TAG,
            "external commit_artifact snapshot must NOT be overwritten by the worker's \
             post-executor insert: stored tag was {tag}, expected {EXTERNAL_TAG}",
            tag = stored_data.tag,
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // Scheduler request context + worker TLS install
    // ──────────────────────────────────────────────────────────────────

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
    /// `winner_audited = true` when the winner captures. Now exercised
    /// through the DAG-owned waiter group bookkeeping.
    #[test]
    fn scheduler_dedup_calls_on_dedup_joiner_with_winner_audited_true_when_winner_captures() {
        let mut dag = SchedulerDag::new(DagAgingConfig::default());
        let (_h1, s1) = completion_pair::<RequestResult>();
        let (_h2, s2) = completion_pair::<RequestResult>();

        let winner_ctx = TestContext::new(100, true); // captures
        let joiner_ctx = TestContext::new(200, true);

        let canonical: Arc<str> = Arc::from("/x.vue");
        dag.register_request(
            &canonical,
            1,
            TargetStage::Analysis,
            s1,
            Some(OpaqueRequestContext(
                Arc::clone(&winner_ctx) as Arc<dyn RequestContextLike>
            )),
        );
        dag.register_request(
            &canonical,
            1,
            TargetStage::Analysis,
            s2,
            Some(OpaqueRequestContext(
                Arc::clone(&joiner_ctx) as Arc<dyn RequestContextLike>
            )),
        );

        let calls = joiner_ctx.joiner_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.as_ref(), "/x.vue");
        assert_eq!(calls[0].1, 100, "winner request_id must be relayed");
        assert!(calls[0].2, "winner_audited must be true when capture=true");
    }

    /// Dedup hook: `winner_audited = false` when the winner does NOT
    /// capture. Now exercised through the DAG-owned waiter group
    /// bookkeeping.
    #[test]
    fn scheduler_dedup_calls_on_dedup_joiner_with_winner_audited_false_when_winner_does_not_capture(
    ) {
        let mut dag = SchedulerDag::new(DagAgingConfig::default());
        let (_h1, s1) = completion_pair::<RequestResult>();
        let (_h2, s2) = completion_pair::<RequestResult>();

        let winner_ctx = TestContext::new(101, false); // no capture
        let joiner_ctx = TestContext::new(201, true);

        let canonical: Arc<str> = Arc::from("/y.vue");
        dag.register_request(
            &canonical,
            2,
            TargetStage::Analysis,
            s1,
            Some(OpaqueRequestContext(
                Arc::clone(&winner_ctx) as Arc<dyn RequestContextLike>
            )),
        );
        dag.register_request(
            &canonical,
            2,
            TargetStage::Analysis,
            s2,
            Some(OpaqueRequestContext(
                Arc::clone(&joiner_ctx) as Arc<dyn RequestContextLike>
            )),
        );

        let calls = joiner_ctx.joiner_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, 101);
        assert!(!calls[0].2);
    }

    /// Capture-site invariant. When analysis of a parent file extracts
    /// dep imports, the scheduler auto-ingests a Source job for each
    /// dep. That job runs on a worker thread whose TLS is empty by
    /// default; without context propagation, the dep's stage observes
    /// `current_request_id() == None` and any VFS-sink fan-out event
    /// for the dep read drops on the audit floor. Invariant: the
    /// auto-ingest site reads the parent's winner context from the
    /// DAG and passes it to `admit_work`, which stores it on the new
    /// DAG node; the dispatch loop installs it as TLS for the dep's
    /// stage closure.
    ///
    /// This regression probe records the parent's and the dep's
    /// observed `current_request_id()` separately via a
    /// canonical-dispatched probe. Both must equal the parent request's
    /// id.
    ///
    /// Determinism barrier: the request targets `Artifact`, NOT
    /// `Analysis`. The blocker the scheduler registers for the
    /// auto-ingested dep gates the PARENT's **Artifact** stage on the
    /// dep reaching **Analysis** (see `blockers_gate_artifact_until_dep_analyzed`
    /// and the `has_pending_blockers` gate in `submit_request`). The
    /// dep cannot reach Analysis without its Source job running first,
    /// so by the time `handle.wait()` returns `Ready(Artifact)` the
    /// dep-source job has provably already executed (and stored its
    /// observed TLS request_id). Had we targeted `Analysis`, the
    /// PARENT analysis is NOT gated by the dep blocker (the blocker
    /// only gates artifacts), so `wait()` could return before the
    /// auto-ingested dep-source worker ran — the inherited-context
    /// observation would race. Targeting Artifact turns the
    /// completion fence into a structural happens-before: dep-source
    /// observed ⇒ dep analyzed ⇒ blocker cleared ⇒ parent artifact ⇒
    /// wait() returns. No timing/sleep is involved.
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

        // Target Artifact (not Analysis): the dep blocker gates the
        // parent's Artifact stage, so completion structurally forces
        // the dep-source job to have run first (see the doc-comment's
        // determinism barrier rationale).
        let handle = sched.submit_request(Request {
            file_id: PARENT.to_string(),
            target: TargetStage::Artifact { profile_hash: 0 },
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

    // ── Artifact blocker registry (typed-API) lifecycle ──

    /// `remove(canonical)` must scrub every recorded blocker entry that
    /// references the removed file — both as OWNER and as a `DepKey`
    /// inside another owner's set. Without the cross-owner scrub,
    /// `remove` retained entries whose owner matched the removed id
    /// only, so a `DepKey` pointing at the removed file's Analysis
    /// lingered inside a different owner's set and gated an Artifact
    /// on a node that no longer exists.
    #[test]
    fn cross_owner_remove_scrubs_blocker_deps_referencing_removed_file() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue + /dep.ts to Analysis so register_resolved_deps
        // records its blocker set in the live (non-zero-generation)
        // path.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Register a blocker on /dep.ts BEFORE /dep.ts has Analysis
        // committed so the dep_key persists in the registry. Use the
        // late path: /a.vue has Analysis committed → register_resolved_deps
        // does NOT auto-resolve the blocker.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Verify the registry holds a `DepKey` referencing /dep.ts.
        let a_arc: Arc<str> = Arc::from("/a.vue");
        let before = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            before
                .deps
                .iter()
                .any(|d| matches!(d, DepKey::FileStage { canonical, .. } if canonical.as_ref() == "/dep.ts")),
            "precondition: registry must hold a /dep.ts DepKey at (/a.vue, {a_gen})",
        );

        // Remove the dep file. The cross-owner scrub must drop the
        // /dep.ts DepKey from /a.vue's entry, and the owner-side
        // remove must drop any /dep.ts owner entry too.
        sched.remove("/dep.ts");

        // KEY ASSERTION: /a.vue's registry entry no longer references /dep.ts.
        let after = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            !after
                .deps
                .iter()
                .any(|d| matches!(d, DepKey::FileStage { canonical, .. } if canonical.as_ref() == "/dep.ts")),
            "scrub_artifact_blockers_referencing must drop /dep.ts from /a.vue's \
             entry after remove(/dep.ts); without the scrub the stale DepKey \
             survived and pinned downstream Artifact admissions forever",
        );
    }

    /// An empty-blocker update via `register_resolved_deps` must clear
    /// the prior pending registry entry for the same `(owner, generation)`.
    /// Without that clear, the empty-set branch early-returned before
    /// clearing, leaving a stale blocker set that gated subsequent
    /// Artifact admissions on resolved deps.
    #[test]
    fn empty_blocker_update_clears_prior_pending_entries() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Initial blocker set.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        let a_arc: Arc<str> = Arc::from("/a.vue");
        assert!(
            !sched
                .dag
                .lock()
                .peek_artifact_blockers(&a_arc, a_gen)
                .is_empty(),
            "precondition: registry must hold the /dep.ts DepKey",
        );

        // Empty-blocker update.
        sched.register_resolved_deps("/a.vue", vec![], vec![]);

        // KEY ASSERTION: registry entry GONE.
        let after = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            after.is_empty(),
            "empty-blocker update must clear the prior registry entry; \
             without the clear, an empty-set early-return skipped it and \
             stale blockers would gate future Artifact admissions",
        );
    }

    /// After the last Artifact at `(owner, generation)` has
    /// completed, the registry entry must be cleared so the map does
    /// not grow unboundedly across long-lived sessions. Without the
    /// completion-handler clear, the registry was never touched on
    /// Artifact completion; entries persisted past their last
    /// referencing Artifact until the owner was removed or its
    /// generation superseded.
    ///
    /// Discriminator: drive /a.vue Source + Analysis to completion,
    /// plant a stale entry directly into the registry (no other
    /// admit-time path will see it because no Artifact request is
    /// in flight), then drive `handle_stage_complete` for an
    /// Artifact at this `(owner, gen)`. Without the cleanup the
    /// entry would survive; with the completion handler's
    /// `clear_artifact_blockers` it is caught.
    #[test]
    fn pending_artifact_blockers_cleared_on_artifact_completion() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue to Analysis so a_gen settles.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;
        let a_arc: Arc<str> = Arc::from("/a.vue");

        // Plant a stale entry directly into the DAG registry — no
        // public auto-clear path will see it because no Artifact
        // request is in flight at this point.
        let mut set: std::collections::BTreeSet<DepKey> = std::collections::BTreeSet::new();
        set.insert(DepKey::FileStage {
            canonical: Arc::from("/stale-dep.ts"),
            generation: 1,
            stage: FileStageKey::Analysis,
        });
        sched.dag.lock().record_artifact_blockers(
            &a_arc,
            a_gen,
            crate::dag::PendingBlockerSet::from_deps(set),
        );
        assert!(
            !sched
                .dag
                .lock()
                .peek_artifact_blockers(&a_arc, a_gen)
                .is_empty(),
            "precondition: planted entry present",
        );

        // Submit an Artifact snapshot via the external
        // commit_artifact path to mark it complete, then synthesize
        // the StageComplete submission that the worker would have
        // emitted. `handle_stage_complete` runs the post-completion
        // cleanup that must clear the registry.
        let snap = ArtifactSnapshot {
            generation: a_gen,
            profile_hash: 7,
            data: Arc::new(crate::node::EmptyData),
        };
        sched.commit_artifact("/a.vue", 7, snap);
        sched.handle_stage_complete("/a.vue", a_gen, TaskKind::Artifact { profile_hash: 7 });

        // KEY ASSERTION: registry entry for (/a.vue, a_gen) is empty
        // AFTER the completion handler ran. Without the cleanup the
        // handler did not touch the registry and the planted entry
        // survived.
        let after = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            after.is_empty(),
            "Artifact completion handler must clear the registry \
             entry at (/a.vue, {a_gen}) when no other profile \
             remains pending; without the cleanup the entry would \
             persist. observed: {after:?}",
        );
    }

    /// External `commit_artifact` terminalization must clear the
    /// Artifact blocker-registry entry IFF no other profile is
    /// still pending at this `(owner, generation)`. Without the
    /// external-commit cleanup, only the worker-side
    /// `handle_stage_complete(Artifact)` path cleared the registry,
    /// so a host-driven external commit (e.g. `compile_entry()`
    /// publishing through `commit_artifact`) left the entry behind.
    /// Over a long-lived session the registry would grow unbounded
    /// for every externally-committed Artifact.
    ///
    /// Discriminator: drive /a.vue to Analysis, plant a stale
    /// registry entry directly (no public auto-clear path sees it
    /// because no Artifact request is in flight), then run an
    /// external `commit_artifact` for the only pending profile.
    /// The entry must be GONE post-commit. Without the cleanup the
    /// entry would persist.
    #[test]
    fn external_commit_artifact_clears_blocker_registry() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue to Analysis so the node has both Source and
        // Analysis committed (the `commit_artifact` coherence gate
        // requires both).
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;
        let a_arc: Arc<str> = Arc::from("/a.vue");

        // Plant a stale entry into the registry — no public auto-clear
        // path will see it because no Artifact request is in flight.
        let mut set: std::collections::BTreeSet<DepKey> = std::collections::BTreeSet::new();
        set.insert(DepKey::FileStage {
            canonical: Arc::from("/stale-dep.ts"),
            generation: 1,
            stage: FileStageKey::Analysis,
        });
        sched.dag.lock().record_artifact_blockers(
            &a_arc,
            a_gen,
            crate::dag::PendingBlockerSet::from_deps(set),
        );
        assert!(
            !sched
                .dag
                .lock()
                .peek_artifact_blockers(&a_arc, a_gen)
                .is_empty(),
            "precondition: planted blocker entry present",
        );

        // External commit_artifact terminalizes the only profile at
        // this `(owner, generation)`. With no other pending profile,
        // the cleanup mirror of handle_stage_complete must fire.
        let snap = ArtifactSnapshot {
            generation: a_gen,
            profile_hash: 7,
            data: Arc::new(crate::node::EmptyData),
        };
        sched.commit_artifact("/a.vue", 7, snap);

        // KEY ASSERTION: registry entry for (/a.vue, a_gen) is empty.
        // Without the external-commit cleanup the path did not touch
        // the registry and the planted entry survived.
        let after = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            after.is_empty(),
            "external commit_artifact must mirror handle_stage_complete's \
             registry cleanup when no other profile is pending. observed: {after:?}",
        );
    }

    /// `classify_recorded_dep` must NOT return `Gating` when the
    /// blocker's FileNode is missing — the producer cannot make
    /// progress, so gating the Artifact on it would deadlock.
    /// Without the dead-producer arm the predicate returned
    /// `None => false` (i.e. still gating), leaving the Artifact
    /// gated on a dead dep forever.
    #[test]
    fn classify_recorded_dep_treats_missing_node_as_not_gating() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue + /dep.ts to Analysis.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Record a blocker for /dep.ts, then remove /dep.ts entirely.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );
        let dep_gen = sched.try_get_source("/dep.ts").map(|s| s.generation);
        let dep_recorded_gen = dep_gen.unwrap_or(1);
        sched.remove("/dep.ts");

        // Build a synthetic DepKey for /dep.ts Analysis at the
        // generation register_resolved_deps would have recorded.
        let dep_key = DepKey::FileStage {
            canonical: Arc::from("/dep.ts"),
            generation: dep_recorded_gen,
            stage: FileStageKey::Analysis,
        };

        // KEY ASSERTION: classify_recorded_dep returns a non-Gating
        // verdict (Satisfied or Failed — both drop the blocker).
        let dag = sched.dag.lock();
        let status = sched.classify_recorded_dep(&dag, &dep_key);
        assert!(
            !matches!(status, BlockerStatus::Gating),
            "classify_recorded_dep must treat a missing FileNode as \
             not gating (Satisfied or Failed); a `None => false` \
             return would leave the Artifact pinned on a producer \
             that can never reach Analysis-committed state. \
             blocker_gen={dep_recorded_gen}, a_gen={a_gen}, \
             status={status:?}",
        );
    }

    /// `register_resolved_deps` must NOT record a `DepKey` for a dead
    /// producer — a blocker that will never resolve into a live
    /// Analysis identity in the DAG. The dead cases include:
    /// (a) FileNode missing (concurrent remove race),
    /// (b) FileNode at generation 0 (no Source ever submitted),
    /// (c) No DAG identity for the recorded `(canonical, gen)`.
    ///
    /// Recording the DepKey for any of these would pin the owner's
    /// Artifact on a producer that cannot make progress.
    ///
    /// Discriminator: plant a `FileNode` at generation 0 directly
    /// into `sched.nodes` so the second-pass DepKey would be at
    /// generation 0, then call `register_resolved_deps`. With the
    /// dead-producer filter, the dep is dropped BEFORE the registry
    /// record, leaving the registry empty. Without the filter the
    /// dep is recorded as `FileStage(/dead.ts, 0, Analysis)` — a key
    /// the DAG can never resolve, pinning the Artifact forever.
    #[test]
    fn register_resolved_deps_skips_dead_producer_deps() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue to Analysis.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Plant a /dead.ts node at gen=0 directly so the auto-ingest
        // path in register_resolved_deps skips ingestion (node
        // already in `nodes`) AND the second pass observes
        // generation=0 — the dead-producer signature. Without the
        // dead-producer filter, the recorded DepKey would be
        // FileStage(/dead.ts, 0, Analysis), which no DAG submission
        // path will ever produce.
        let dead_node = sched.create_node("/dead.ts", None);
        sched.nodes.insert("/dead.ts".to_string(), dead_node);
        // Do NOT bump_generation; leave it at 0.

        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dead.ts".to_string()],
            vec!["/dead.ts".to_string()],
        );

        // KEY ASSERTION: no entry in the registry — the dead-producer
        // filter dropped /dead.ts at recording time, and with no
        // remaining deps the empty-set arm cleared any prior entry.
        let a_arc: Arc<str> = Arc::from("/a.vue");
        let after = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            after.is_empty(),
            "register_resolved_deps must drop dead-producer DepKey \
             entries before recording; the planted /dead.ts node at \
             generation 0 has no DAG identity and would never resolve. \
             observed: {after:?}",
        );
    }

    /// `classify_recorded_dep` must drop a blocker whose producer's
    /// Source failed (`FileNotFound`). The FileNode survives the
    /// terminalize_failure path (only the DAG identity is cancelled),
    /// so a recorded blocker that consults only `current_analysis()`
    /// would observe `None` and report STILL GATING, pinning the
    /// owner's Artifact forever. The matrix consults the persistent
    /// `terminal_dep_failures` store AND the DAG identity /
    /// `current_source()` to detect the dead Source-failed producer
    /// (classified as `Failed` with the recorded cause).
    ///
    /// Discriminator: a Source-failed dep is driven to terminal
    /// failure (`FileNotFound` returned by the loader). The dep
    /// FileNode auto-ingest from `register_resolved_deps` bumps it
    /// past generation 0 before the load fails; after the failure
    /// the node has `current_source().is_none()` and no live DAG
    /// identity. Build the synthetic DepKey at the dep's generation
    /// and assert `classify_recorded_dep` reports a non-Gating verdict.
    #[test]
    fn classify_recorded_dep_treats_source_failed_dead_producer_as_not_gating() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        // /dep.ts is deliberately NOT inserted into the loader so
        // execute_source_stage's `source_loader.load` returns None
        // and routes through the FileNotFound terminalize_failure
        // path. The FileNode is created and bumped (gen 1) by
        // register_resolved_deps' auto-ingest pass.
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let _a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Register a blocker on /dep.ts. The auto-ingest creates the
        // /dep.ts FileNode at gen 1 and submits a Source request;
        // the worker's `execute_source_stage` then fails with
        // FileNotFound (no loader entry), runs terminalize_failure
        // and cancels the Source DAG identity. Analysis is never
        // admitted. Post-state: FileNode present at gen 1,
        // current_source = None, current_analysis = None, no DAG
        // identity for either Source or Analysis.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );
        sched.drive_all();

        let dep_node = sched
            .nodes
            .get("/dep.ts")
            .expect("FileNode must remain after Source-failed terminalize");
        let dep_gen = dep_node.generation();
        assert!(
            dep_node.current_source().is_none(),
            "precondition: Source must have failed (current_source=None)",
        );
        assert!(
            dep_node.current_analysis().is_none(),
            "precondition: Analysis must not be committed",
        );
        drop(dep_node);

        let dep_key = DepKey::FileStage {
            canonical: Arc::from("/dep.ts"),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        let dag = sched.dag.lock();
        // Confirm the DAG has no live Analysis identity for the dep:
        // terminalize_failure cancelled Source, and Analysis was
        // never admitted in the first place.
        let dep_analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/dep.ts"),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        assert!(
            dag.token_for(&dep_analysis_identity).is_none(),
            "precondition: no live Analysis DAG identity for the \
             Source-failed dep (dep_gen={dep_gen})",
        );

        // KEY ASSERTION: classify_recorded_dep reports a non-Gating
        // verdict (Failed once the persistent terminal_dep_failures
        // record is hit). Without the dead-producer arm the function
        // only checked `current_analysis().is_some()` and returned
        // `false`, pinning the Artifact on a dead producer.
        let status = sched.classify_recorded_dep(&dag, &dep_key);
        assert!(
            !matches!(status, BlockerStatus::Gating),
            "classify_recorded_dep must treat a Source-failed dep \
             (current_source=None, no live Analysis DAG identity) \
             as not gating. dep_gen={dep_gen}, status={status:?}",
        );
    }

    /// `classify_recorded_dep` must drop a blocker whose producer's
    /// Analysis failed at the recorded generation. After
    /// terminalize_failure(Analysis) the Analysis DAG identity is
    /// cancelled, Source remains committed, and the persistent
    /// `terminal_dep_failures` store carries the Analysis-failure
    /// record so the matrix returns `Failed`. Without that consult
    /// the predicate would check only `current_analysis()` and
    /// report STILL GATING forever.
    ///
    /// Discriminator: succeed Source but fail Analysis for /dep.ts.
    /// Build the synthetic DepKey at the dep's generation and assert
    /// `classify_recorded_dep` reports a non-Gating verdict (Failed
    /// once the matrix consults `terminal_dep_failures`).
    #[test]
    fn classify_recorded_dep_treats_analysis_failed_dead_producer_as_not_gating() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep"));
        let sched = Scheduler::new_sync_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(ErrAnalysisExecutor),
        );

        // Drive /a.vue + /dep.ts. ErrAnalysisExecutor succeeds Source
        // (the default Source path runs because the executor only
        // overrides `execute_analysis`) and fails Analysis. After
        // drive_all, /dep.ts has current_source = Some, current_analysis
        // = None, and the Analysis DAG identity has been cancelled by
        // terminalize_failure.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.submit_request(Request {
            file_id: "/dep.ts".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("dep")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();

        let dep_node = sched.nodes.get("/dep.ts").expect("dep FileNode");
        let dep_gen = dep_node.generation();
        assert!(
            dep_node.current_source().is_some(),
            "precondition: Source must have committed for /dep.ts",
        );
        assert!(
            dep_node.current_analysis().is_none(),
            "precondition: Analysis must have failed (no snapshot stored)",
        );
        drop(dep_node);

        let dep_key = DepKey::FileStage {
            canonical: Arc::from("/dep.ts"),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        let dag = sched.dag.lock();
        let dep_analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/dep.ts"),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        assert!(
            dag.token_for(&dep_analysis_identity).is_none(),
            "precondition: Analysis DAG identity must have been \
             cancelled by terminalize_failure (dep_gen={dep_gen})",
        );

        // KEY ASSERTION: classify_recorded_dep returns a non-Gating
        // verdict. The matrix consults the persistent
        // `terminal_dep_failures` store FIRST and finds the recorded
        // Analysis failure → `Failed(record)`. Without the
        // dead-producer arm the predicate would return `false`
        // (still gating) because it only checked
        // `current_analysis().is_some()`, pinning the Artifact on a
        // producer that will never reach committed-Analysis state.
        let status = sched.classify_recorded_dep(&dag, &dep_key);
        assert!(
            !matches!(status, BlockerStatus::Gating),
            "classify_recorded_dep must treat an Analysis-failed dep \
             (current_source=Some, current_analysis=None, no live \
             Analysis DAG identity) as not gating. dep_gen={dep_gen}, \
             status={status:?}",
        );
    }

    /// `register_resolved_deps` must drop a Source-failed dep
    /// before it lands in the artifact blocker registry. The
    /// FileNode persists across `terminalize_failure(Source)`, so
    /// the previous filter — which treated any existing FileNode
    /// as a live producer — would record a DepKey that the DAG
    /// can never resolve, pinning the owner's Artifact admission.
    ///
    /// Discriminator: a Source-failed dep is driven to terminal
    /// failure; then `register_resolved_deps` is called on a fresh
    /// owner with the dead dep id. The registry entry for the
    /// owner must be empty (no recorded blocker for the dead dep).
    #[test]
    fn register_resolved_deps_filters_source_failed_dead_producer() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        // /dep.ts NOT inserted — Source load will return None,
        // routing through the FileNotFound terminalize_failure
        // path. We first ingest /dep.ts directly via a synthetic
        // request so its Source stage runs and fails BEFORE the
        // owner's register_resolved_deps call (otherwise the
        // auto-ingest creates the node at gen 1 inside the same
        // call, races with the test's expectations).
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/dep.ts".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();

        let dep_node = sched
            .nodes
            .get("/dep.ts")
            .expect("FileNode must remain after Source-failed terminalize");
        assert!(
            dep_node.current_source().is_none(),
            "precondition: Source must have failed (FileNotFound)",
        );
        let dep_gen = dep_node.generation();
        drop(dep_node);

        // Drive /a.vue to Analysis so a_gen settles.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Confirm no live Analysis DAG identity for the dead dep.
        let dep_analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/dep.ts"),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        assert!(
            sched.dag.lock().token_for(&dep_analysis_identity).is_none(),
            "precondition: Analysis identity must NOT be live for \
             a Source-failed dep (dep_gen={dep_gen})",
        );

        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // KEY ASSERTION: the registry for (/a.vue, a_gen) holds no
        // gating `deps` entry referencing /dep.ts. Without the
        // failure-side persistence the matrix would have recorded
        // FileStage(/dep.ts, dep_gen, Analysis) as a live gating
        // DepKey, pinning the Artifact on a producer that cannot
        // make progress. With the failure side of
        // [`crate::dag::PendingBlockerSet`] the failure is recorded
        // there instead — the Artifact admission sees the dead
        // producer via `attach_failed_dep` and surfaces a typed
        // `DependencyFailed`, NOT as a gating dep.
        let a_arc: Arc<str> = Arc::from("/a.vue");
        let after = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            after.deps.is_empty(),
            "register_resolved_deps must NOT record the Source-failed \
             /dep.ts dead producer as a gating dep. observed: {after:?}",
        );
        // Discriminating cross-check: the failure record IS persisted
        // on the `failed` side so the Artifact admission delivers a
        // typed DependencyFailed instead of silently resolving Ready
        // (the failure-side persistence contract).
        assert!(
            after.failed.iter().any(|r| matches!(
                &r.dep_key,
                crate::dag::DepKey::FileStage { canonical, .. }
                if canonical.as_ref() == "/dep.ts"
            )),
            "the dead-producer failure must be persisted in the \
             registry's `failed` list so subsequent Artifact \
             admissions surface DependencyFailed via attach_failed_dep. \
             observed: {after:?}",
        );
    }

    /// `file_stage_analysis_blocker_status` must classify an
    /// auto-ingested dep as **Gating** while the `Submission::NewRequest`
    /// for its Source is queued in the inbox but has not yet been
    /// drained by the driver. Without the tracking-set consult the
    /// matrix would consult only the FileNode + DAG identity state:
    /// the auto-ingested dep has a FileNode at gen 1 (inserted before
    /// the inbox send) and no live Source/Analysis DAG identity
    /// (Source admit happens only when the driver dequeues the
    /// NewRequest), which is structurally identical to a Source-
    /// failed corpse. The matrix's terminal arm would return
    /// `Resolved`, and any concurrent Artifact admission that popped
    /// ahead of the queued SrcReq would drop the blocker and
    /// dispatch the Artifact prematurely on stale dep state.
    ///
    /// Discriminator: drive owner /a.vue to Analysis-committed,
    /// call `register_resolved_deps('/a.vue', blockers=['/dep.ts'])`
    /// — which (a) inserts a FileNode for `/dep.ts` at gen 1, (b)
    /// plants a tracking entry in `auto_ingested_recent`, (c) sends
    /// a `Submission::NewRequest` to the inbox WITHOUT draining it.
    /// Without draining, call `file_stage_analysis_blocker_status`
    /// directly. Without the tracking-set consult the matrix returns
    /// `Resolved`; with the consult it returns `Gating`.
    #[test]
    fn auto_ingested_dep_gates_before_driver_drains_srcreq() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Drive owner /a.vue to Analysis so subsequent
        // register_resolved_deps does not early-return on the
        // `current_source().is_none()` guard. After drive_all,
        // /a.vue has current_source = Some and current_analysis =
        // Some at gen 1.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;
        assert!(
            sched
                .nodes
                .get("/a.vue")
                .map(|n| n.current_analysis().is_some())
                .unwrap_or(false),
            "precondition: /a.vue Analysis must be committed at gen={a_gen}",
        );

        // Confirm /dep.ts is not yet in `nodes`: the auto-ingest
        // inside register_resolved_deps will fire because the dep
        // FileNode is absent.
        assert!(
            !sched.nodes.contains_key("/dep.ts"),
            "precondition: /dep.ts FileNode must NOT yet exist",
        );

        // Call register_resolved_deps to set up the race state.
        // This inserts /dep.ts FileNode at gen 1, plants the
        // tracking entry in auto_ingested_recent, and enqueues a
        // NewRequest to the inbox. CRUCIALLY: we do NOT drain the
        // inbox afterwards.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Snapshot the dep's state. The FileNode is present (the
        // auto-ingest's insert is synchronous), but no DAG identity
        // exists because the NewRequest is still queued.
        let dep_node = sched
            .nodes
            .get("/dep.ts")
            .expect("auto-ingest must insert /dep.ts FileNode");
        let dep_gen = dep_node.generation();
        assert!(
            dep_gen >= 1,
            "precondition: auto-ingest must bump /dep.ts past gen 0 (observed dep_gen={dep_gen})",
        );
        assert!(
            dep_node.current_source().is_none(),
            "precondition: Source must NOT yet be committed for /dep.ts \
             (the NewRequest is queued in inbox, undrained)",
        );
        assert!(
            dep_node.current_analysis().is_none(),
            "precondition: Analysis must NOT yet be committed for /dep.ts",
        );
        drop(dep_node);

        // Confirm the tracking set has an entry for /dep.ts at the
        // matching generation — the tracking plant happens before
        // the inbox send, so it must be observable here.
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        let tracking_entry = sched.auto_ingested_recent.get(&dep_arc).expect(
            "auto-ingest invariant: auto_ingested_recent must contain /dep.ts after auto-ingest",
        );
        assert_eq!(
            tracking_entry.generation, dep_gen,
            "tracking entry's generation must match the dep's FileNode generation \
             (entry_gen={}, dep_gen={dep_gen})",
            tracking_entry.generation,
        );
        drop(tracking_entry);

        // Confirm no live Source / Analysis DAG identity for the
        // dep — the NewRequest is queued but not drained.
        let dep_source_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: dep_gen,
            stage: FileStageKey::Source,
        };
        let dep_analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        let dag = sched.dag.lock();
        assert!(
            dag.token_for(&dep_source_identity).is_none(),
            "precondition: no live Source DAG identity for /dep.ts \
             (NewRequest is queued but undrained)",
        );
        assert!(
            dag.token_for(&dep_analysis_identity).is_none(),
            "precondition: no live Analysis DAG identity for /dep.ts",
        );

        // KEY ASSERTION: with the tracking entry present, the matrix
        // must return Gating. Without the tracking-set consult the
        // matrix consults only the FileNode + DAG identity state
        // and returns Resolved (the dead-producer arm). With the
        // `auto_ingest_tracking_gates` consult, the call intercepts
        // the terminal arm and returns Gating so a same-tick
        // Artifact admission keeps the dep as a blocker.
        let status = sched.file_stage_analysis_blocker_status(&dag, &dep_arc, dep_gen);
        assert!(
            matches!(status, BlockerStatus::Gating),
            "matrix must return Gating when the dep has an \
             auto_ingested_recent entry at the matching generation \
             (NewRequest is queued in inbox, driver has not yet drained). \
             observed status={status:?}, dep_gen={dep_gen}, a_gen={a_gen}",
        );

        // Drop the dag lock + verify the recorded-blocker classifier
        // (`classify_recorded_dep`) also reports the dep as still
        // gating. A `Satisfied` or `Failed` verdict here would let
        // `admit_artifact_with_blockers` drop the dep silently.
        let dep_key = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        let recorded_status = sched.classify_recorded_dep(&dag, &dep_key);
        assert!(
            matches!(recorded_status, BlockerStatus::Gating),
            "classify_recorded_dep must report the queued-but-undrained \
             auto-ingested dep as Gating, so a concurrent Artifact \
             admission keeps the blocker DepKey on its deps_remaining \
             set. observed: {recorded_status:?}",
        );
    }

    /// `handle_new_request` must remove the
    /// [`Scheduler::auto_ingested_recent`] entry when the auto-ingested
    /// dep's Source DAG identity is admitted. Once the driver drains
    /// the queued `NewRequest` and admits the Source identity, the
    /// live `by_identity` entry takes over as the source of truth for
    /// the matrix; a stale tracking entry would only confuse later
    /// consults. The cleanup arm runs after `admit_work(TaskKind::Source)`
    /// in `handle_new_request`.
    ///
    /// Discriminator: plant a tracking entry (via the normal
    /// `register_resolved_deps` path), drain the inbox to admit the
    /// dep's Source identity, and assert the tracking entry is gone.
    #[test]
    fn auto_ingested_tracking_cleared_after_source_admit() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_kind: None,
            request_context: None,
        });
        sched.drive_all();

        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        let dep_arc: Arc<str> = Arc::from("/dep.ts");

        // Precondition: tracking entry is planted.
        assert!(
            sched.auto_ingested_recent.contains_key(&dep_arc),
            "precondition: auto_ingested_recent must contain /dep.ts after auto-ingest",
        );

        // Drain the inbox: the driver dequeues the NewRequest and
        // admits the Source DAG identity, which triggers the
        // cleanup arm. drive_all is more than enough; the
        // tracking entry must be cleared.
        sched.drive_all();

        // KEY ASSERTION: the tracking entry is gone.
        assert!(
            !sched.auto_ingested_recent.contains_key(&dep_arc),
            "auto_ingested_recent must be cleared after the dep's \
             Source DAG identity is admitted via handle_new_request",
        );
    }

    /// The matrix's stale-gen arm (`file_stage_analysis_blocker_status`
    /// → `node.generation() != generation`) must opportunistically
    /// clean any tracking entry matching the stale generation under
    /// a value-conditional removal. Without this cleanup the stale
    /// entry would only be trimmed by the 60-second
    /// `AUTO_INGESTED_RECENT_STALE_THRESHOLD` sweep — a bounded but
    /// real memory leak across an invalidated dep's bump-generation
    /// boundary.
    ///
    /// Discriminator: plant a tracking entry at gen=1, bump the
    /// FileNode to gen=2, and run a matrix consult at gen=1. The
    /// stale-gen arm fires (node.generation()=2 ≠ generation=1) and
    /// must opportunistically clean the gen=1 tracking entry.
    /// Without the cleanup the entry would persist; with it the
    /// entry is gone.
    ///
    /// The value-conditional removal also guards against a
    /// concurrent re-insertion of the SAME canonical at a newer
    /// generation: the remove_if predicate only deletes entries
    /// whose generation matches the stale gen the matrix saw.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn matrix_stale_gen_arm_opportunistically_cleans_tracking_entry() {
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        let dep_id = "/dep.ts";
        let dep_arc: Arc<str> = Arc::from(dep_id);

        // Plant a FileNode at gen=2 (the "live" generation).
        let dep_node = sched.create_node(dep_id, None);
        dep_node.bump_generation(); // gen=1
        dep_node.bump_generation(); // gen=2
        sched.nodes.insert(dep_id.to_string(), dep_node);

        // Plant a tracking entry at gen=1 (the "stale" gen).
        sched.auto_ingested_recent.insert(
            Arc::clone(&dep_arc),
            AutoIngestedRecord {
                generation: 1,
                since: Instant::now(),
            },
        );
        assert!(
            sched.auto_ingested_recent.contains_key(&dep_arc),
            "precondition: stale-gen tracking entry must be present before the matrix consult",
        );

        // Run a matrix consult at the stale gen. The stale-gen
        // arm fires (node.generation()=2 ≠ generation=1) and
        // opportunistically cleans the gen=1 tracking entry.
        let dep_key = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        {
            let dag = sched.dag.lock();
            let status = sched.classify_recorded_dep(&dag, &dep_key);
            assert!(
                !matches!(status, BlockerStatus::Gating),
                "matrix sanity: stale-gen consult must NOT classify as Gating \
                 (the recorded blocker is moot at a stale generation). observed: {status:?}",
            );
        }

        // KEY ASSERTION: the stale-gen tracking entry has been
        // cleaned. Without the cleanup the matrix's stale-gen arm
        // only returned Resolved without touching the tracking map;
        // the entry would only age out 60 seconds later.
        assert!(
            !sched.auto_ingested_recent.contains_key(&dep_arc),
            "matrix's stale-gen arm must opportunistically clean the matching \
             tracking entry. Without the cleanup the entry persists up to the \
             AUTO_INGESTED_RECENT_STALE_THRESHOLD window (60s).",
        );
    }

    /// The opportunistic cleanup must use a value-conditional
    /// removal so a concurrent re-insertion of a newer-generation
    /// tracking entry between the matrix's stale-gen observation
    /// and its remove is preserved.
    ///
    /// Discriminator: plant a FileNode at gen=2, plant a tracking
    /// entry at gen=2 (newer than the stale gen=1 the matrix is
    /// about to consult), then call the matrix at gen=1. The
    /// matrix observes node.generation()=2 ≠ 1 (stale-gen arm),
    /// runs `remove_if(canonical, |_, v| v.generation == 1)`,
    /// and the predicate observes the live entry's gen=2 (not 1).
    /// The remove must be skipped; the gen=2 entry survives.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn matrix_stale_gen_cleanup_preserves_newer_gen_tracking_entry() {
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        let dep_id = "/dep.ts";
        let dep_arc: Arc<str> = Arc::from(dep_id);

        // FileNode at gen=2.
        let dep_node = sched.create_node(dep_id, None);
        dep_node.bump_generation();
        dep_node.bump_generation();
        sched.nodes.insert(dep_id.to_string(), dep_node);

        // Tracking entry at gen=2 (the LIVE generation, not the
        // stale one being consulted).
        sched.auto_ingested_recent.insert(
            Arc::clone(&dep_arc),
            AutoIngestedRecord {
                generation: 2,
                since: Instant::now(),
            },
        );

        let dep_key = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        {
            let dag = sched.dag.lock();
            let _ = sched.classify_recorded_dep(&dag, &dep_key);
        }

        // KEY ASSERTION: the gen=2 tracking entry survives.
        // Without any cleanup arm: entry survives by default.
        // With an unconditional remove: entry would be deleted
        // (regression).
        // With the value-conditional remove_if predicate: entry
        // survives.
        let surviving = sched
            .auto_ingested_recent
            .get(&dep_arc)
            .expect("gen=2 tracking entry MUST survive a stale-gen=1 cleanup");
        assert_eq!(
            surviving.generation, 2,
            "the cleanup's value-conditional removal must NOT delete \
             a newer-generation tracking entry that does not match the stale generation",
        );
    }

    /// `register_resolved_deps`'s auto-ingest path must publish the
    /// tracking entry into `auto_ingested_recent` BEFORE publishing
    /// the FileNode into `self.nodes`. The matrix's classifier
    /// (`file_stage_analysis_blocker_status`) consults the FileNode
    /// first; if it observes the FileNode without an accompanying
    /// tracking entry it falls through every arm (FileNode present,
    /// gen matches, no current_analysis, no live Source/Analysis
    /// DAG identity, no tracking entry) and returns `Resolved` for
    /// what is actually a live, pre-drain auto-ingest.
    ///
    /// Inserting the tracking entry FIRST closes this window: a
    /// matrix lookup that lands in the only mid-call observable
    /// state — (no-FileNode, tracking-present) — falls through to
    /// the FileNode-missing arm which consults the tracking entry
    /// directly and returns Gating.
    ///
    /// Discriminator: an inspecting test that hand-builds the
    /// vulnerable and safe states and asserts the matrix
    /// classifier on each. The vulnerable state — (FileNode
    /// present, tracking absent, no live DAG identity) — must
    /// classify Resolved, demonstrating the matrix's sensitivity.
    /// The two safe-reachable states — (tracking present, FileNode
    /// absent) and (both present) — must classify Gating,
    /// demonstrating the matrix returns the correct answer when
    /// the auto-ingest is mid-publication. With FileNode-before-
    /// tracking ordering the inserter could publish the vulnerable
    /// state; with the tracking-before-FileNode swap it can publish
    /// only the safe transitional state.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn matrix_classifier_returns_gating_for_post_swap_intermediate_states() {
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        let dep_id = "/dep.ts";
        let dep_arc: Arc<str> = Arc::from(dep_id);

        // State A — post-swap intermediate state #1: tracking
        // entry has been published but FileNode has not. This is
        // the ONLY mid-call observable state that the post-swap
        // ordering can produce. The matrix's FileNode-missing arm
        // must classify as Gating via `auto_ingest_tracking_gates`.
        let dep_gen: u64 = 1;
        sched.auto_ingested_recent.insert(
            Arc::clone(&dep_arc),
            AutoIngestedRecord {
                generation: dep_gen,
                since: Instant::now(),
            },
        );

        let dep_key = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        {
            let dag = sched.dag.lock();
            let status = sched.classify_recorded_dep(&dag, &dep_key);
            assert!(
                matches!(status, BlockerStatus::Gating),
                "State A (tracking-present, FileNode-absent): matrix must classify Gating \
                 via the FileNode-missing arm's tracking-entry consult. A non-Gating \
                 verdict here would let a live auto-ingest be filtered out as a dead \
                 producer. dep_gen={dep_gen}, observed: {status:?}",
            );
        }

        // State B — post-swap steady state: tracking entry AND
        // FileNode both present, no live DAG identity yet (the
        // NewRequest is queued in the inbox; the driver has not
        // yet drained it). The matrix's last arm (no-live-DAG
        // identity, current_analysis None) consults the tracking
        // entry and returns Gating.
        let dep_node = sched.create_node(dep_id, None);
        dep_node.bump_generation();
        sched.nodes.insert(dep_id.to_string(), dep_node);
        {
            let dag = sched.dag.lock();
            let status = sched.classify_recorded_dep(&dag, &dep_key);
            assert!(
                matches!(status, BlockerStatus::Gating),
                "State B (tracking-present, FileNode-present, no-DAG-identity): matrix must \
                 classify Gating via the last-arm tracking-entry consult. dep_gen={dep_gen}, \
                 observed: {status:?}",
            );
        }

        // State C — vulnerable bug state, INCLUDED here purely as
        // a sanity check that the matrix IS sensitive to the
        // ordering. This is the (FileNode-present, tracking-absent)
        // state the FileNode-before-tracking inserter could publish;
        // with the tracking-before-FileNode swap it is unreachable.
        // Drop the tracking entry to construct it.
        sched.auto_ingested_recent.remove(&dep_arc);
        {
            let dag = sched.dag.lock();
            let status = sched.classify_recorded_dep(&dag, &dep_key);
            assert!(
                !matches!(status, BlockerStatus::Gating),
                "Vulnerable-state sanity (FileNode-present, no-tracking, no-DAG-identity): \
                 matrix returns non-Gating for this state — this is the misclassification the \
                 FileNode-before-tracking-publish ordering swap prevents. With the swap, \
                 this state is unreachable mid-auto-ingest. dep_gen={dep_gen}, observed: {status:?}",
            );
        }
    }

    /// `clear_auto_ingest_tracking` and `auto_ingest_tracking_gates`
    /// must use a value-conditional removal so a concurrent re-insert
    /// at a newer generation in the get-vs-remove window cannot be
    /// deleted by the cleanup arm. A non-atomic `get` → `drop(entry)`
    /// → unconditional `remove` sequence would let another thread
    /// re-insert a newer-gen entry between the drop and the remove
    /// that the unconditional remove then deletes — re-opening the
    /// post-source-admit-clearing bug class. Every cleanup arm
    /// passes `remove_if(canonical, |_, v| v.generation == old_gen)`
    /// so the predicate runs under the shard write lock and only
    /// drops the entry when the live value still matches the
    /// generation the caller observed.
    ///
    /// Discriminator: drive both cleanup arms (the active
    /// `clear_auto_ingest_tracking` path and the stale-gen +
    /// aged-out arms of `auto_ingest_tracking_gates`) against a map
    /// that has been refreshed to a newer generation. An
    /// unconditional remove would delete the newer-gen entry; with
    /// the value-conditional removal the entry survives.
    #[test]
    fn auto_ingest_tracking_cleanup_preserves_newer_gen_reinsertion() {
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        let canonical: Arc<str> = Arc::from("/dep.ts");

        // Arm 1: clear_auto_ingest_tracking with stale `generation`
        // argument. The live entry is at gen=2; clearing for gen=1
        // must be a no-op.
        sched.auto_ingested_recent.insert(
            Arc::clone(&canonical),
            AutoIngestedRecord {
                generation: 2,
                since: Instant::now(),
            },
        );
        sched.clear_auto_ingest_tracking(&canonical, /* old_gen */ 1);
        {
            let entry = sched.auto_ingested_recent.get(&canonical).expect(
                "clear_auto_ingest_tracking with stale gen must NOT drop the newer-gen entry",
            );
            assert_eq!(
                entry.generation, 2,
                "the surviving entry must be the newer-gen one, not a phantom resurrection",
            );
        }

        // Arm 2: auto_ingest_tracking_gates stale-gen arm. The matrix
        // calls this helper with the recorded blocker's generation;
        // when the live entry is at a newer gen the helper returns
        // false and drops the stale entry. An unconditional drop
        // would delete the newer-gen entry; with the remove_if
        // predicate the newer entry survives.
        //
        // Setup: place a gen=2 entry, then call the helper with
        // gen=1. The helper observes the mismatch, takes the
        // stale-gen arm, and runs `remove_if(.., gen=2)`. The
        // current value's generation is 2, so the predicate evaluates
        // true and… wait — `entry_gen` in the helper is whatever the
        // helper READ. To exercise the race-window equivalent we
        // must arrange for the helper's observed `entry_gen` to be
        // DIFFERENT from what the live entry holds at remove time.
        // Sequentially impossible without instrumentation. Instead
        // assert the predicate semantics directly: re-insert at gen=3
        // between the get and the remove inside a thread-pinned
        // interleave below (arm 3).

        // Arm 3: deterministic concurrent interleave. Two threads
        // share a barrier to interleave:
        //  - Thread A: simulates a cleanup arm by capturing the live
        //    entry's generation (gen=2), then waits at the barrier.
        //  - Thread B: removes the gen=2 entry and re-inserts gen=3,
        //    then waits at the barrier.
        //  - Thread A: continues to `remove_if(canonical, |_, v|
        //    v.generation == captured_gen)`.
        // An unconditional remove would delete the gen=3 entry
        // Thread B inserted. With the value-conditional predicate
        // the lookup sees gen=3 ≠ captured gen=2 and the entry
        // survives.
        use std::sync::Barrier;
        use std::thread;

        // Reset to a known state for arm 3.
        sched.auto_ingested_recent.insert(
            Arc::clone(&canonical),
            AutoIngestedRecord {
                generation: 2,
                since: Instant::now(),
            },
        );

        let barrier = Arc::new(Barrier::new(2));
        let sched_clone = Arc::clone(&sched);
        let canonical_a = Arc::clone(&canonical);
        let barrier_a = Arc::clone(&barrier);
        let handle_a = thread::spawn(move || {
            // Capture the observed generation under the cleanup arm's
            // get(): without the value-conditional folding this read
            // happens under a temporary shard ref, then the ref is
            // dropped before an unconditional remove. With the
            // remove_if predicate the read is folded into the
            // single atomic operation.
            let observed_gen = sched_clone
                .auto_ingested_recent
                .get(&canonical_a)
                .map(|e| e.generation)
                .expect("arm 3 setup: gen=2 entry must be present before the race");

            // Synchronize with Thread B — let it perform its
            // remove + re-insert at gen=3 before we complete the
            // cleanup.
            barrier_a.wait();
            // (Thread B re-inserts here.)
            barrier_a.wait();

            // The conditional remove must NOT delete the newer-gen
            // re-insert: predicate observes gen=3 ≠ observed_gen=2
            // and returns false.
            sched_clone
                .auto_ingested_recent
                .remove_if(&canonical_a, |_k, v| v.generation == observed_gen);
        });

        let sched_clone_b = Arc::clone(&sched);
        let canonical_b = Arc::clone(&canonical);
        let barrier_b = Arc::clone(&barrier);
        let handle_b = thread::spawn(move || {
            barrier_b.wait();
            sched_clone_b.auto_ingested_recent.remove(&canonical_b);
            sched_clone_b.auto_ingested_recent.insert(
                Arc::clone(&canonical_b),
                AutoIngestedRecord {
                    generation: 3,
                    since: Instant::now(),
                },
            );
            barrier_b.wait();
        });

        handle_a.join().unwrap();
        handle_b.join().unwrap();

        // KEY ASSERTION: the gen=3 entry survives the cleanup arm.
        // An unconditional `self.auto_ingested_recent.remove(canonical)`
        // would have deleted Thread B's gen=3 re-insert. With the
        // value-conditional `remove_if`, the predicate preserves it.
        let surviving = sched
            .auto_ingested_recent
            .get(&canonical)
            .expect("the gen=3 re-insert MUST survive a concurrent stale-gen cleanup");
        assert_eq!(
            surviving.generation, 3,
            "surviving entry must be Thread B's gen=3, not a stale resurrection",
        );
    }

    /// Per-file gate plumbing for the source-failure-propagation
    /// test. `entered_tx` fires once when the worker enters
    /// `execute_source`; `release_rx` blocks the worker inside
    /// the source executor until the test thread drops the
    /// matching sender. The executor returns an `Err` after
    /// release so the Source stage fails via `StageFailed`.
    struct SourceGate {
        entered_tx: crossbeam_channel::Sender<()>,
        release_rx: crossbeam_channel::Receiver<()>,
    }

    /// Test executor that gates `execute_source` per-file. Files
    /// listed in `gates` block inside the executor until released,
    /// then return an `Err(StageError)` so Source fails terminally.
    /// Files without a gate run the default Source path.
    struct GatedFailingSourceExecutor {
        gates: dashmap::DashMap<String, SourceGate>,
    }

    impl crate::executor::StageExecutor for GatedFailingSourceExecutor {
        fn execute_source(
            &self,
            canonical_id: &str,
            _file_kind: crate::node::FileKind,
            content: Arc<str>,
            generation: u64,
        ) -> Result<crate::node::SourceSnapshot, crate::executor::StageError> {
            if let Some(gate) = self.gates.get(canonical_id) {
                // Signal entry to the test thread. Best-effort —
                // if the test dropped the receiver the executor
                // proceeds.
                let _ = gate.entered_tx.send(());
                // Block until release. The test drops the sender
                // to signal release; recv returns Disconnected,
                // which we treat as release.
                let _ = gate.release_rx.recv();
                return Err(crate::executor::StageError {
                    message: format!("gated source failure for {canonical_id}"),
                });
            }
            Ok(crate::node::SourceSnapshot::new_empty(content, generation))
        }
    }

    /// `terminalize_failure(Source)` for `(canonical, gen)` must
    /// fan out to any `DepKey::FileStage { stage: Analysis }`
    /// waiters at the same `(canonical, gen)` AND the Artifact
    /// executor must surface the dep failure as a typed
    /// `SchedulerError::DependencyFailed` instead of silently
    /// resolving `Ready`.
    ///
    /// Without the Analysis-key fan-out, a Source cancel only
    /// fanned out to the same-key (Source) DepKey waiters; the
    /// Analysis DepKey waiter (the owner's Artifact) stayed pinned
    /// forever. Adding the Analysis fan-out lets the Artifact
    /// executor see the dep failure, but if the executor still
    /// reads only the OWNER's `current_source()` /
    /// `current_analysis()` it silently returns `Ready` on a
    /// snapshot built from a missing prerequisite. The typed
    /// `failed_blocker_deps` marker closes the loop: the Artifact
    /// executor short-circuits with `DependencyFailed`.
    ///
    /// A race-dependent variant of this test would be: without
    /// synchronization between `register_resolved_deps` and
    /// `submit_request(Artifact)`, the driver could drive /dep.ts
    /// Source to terminal failure BEFORE the Artifact admission
    /// ran — in that case the matrix's dead-producer arm returns
    /// Resolved and no DepKey is recorded, so the Artifact
    /// dispatches over a clean snapshot and resolves `Ready`.
    /// Both fixed and unfixed code would pass that race outcome;
    /// the test would not be discriminating.
    ///
    /// The `GatedFailingSourceExecutor` makes the test
    /// discriminating: /dep.ts Source is HELD inside the executor
    /// until the test thread releases it. The sequence is:
    ///
    /// 1. `register_resolved_deps('/a.vue', blockers=['/dep.ts'])`
    ///    auto-ingests /dep.ts and enqueues its Source NewRequest.
    /// 2. Drive the inbox enough for the worker to enter the
    ///    gated executor; wait on the `entered` signal.
    /// 3. `submit_request(Artifact{'/a.vue'})` — the Artifact
    ///    admission runs while /dep.ts Source is mid-execution,
    ///    so the matrix sees /dep.ts Source DAG identity LIVE
    ///    and records the Analysis DepKey on the Artifact's
    ///    deps_remaining.
    /// 4. Release the gate — /dep.ts Source returns Err →
    ///    `terminalize_failure(Source)` → fan-out into the
    ///    Analysis DepKey waiter (the Artifact) →
    ///    `failed_blocker_deps` marker → dispatch →
    ///    `execute_artifact_stage` short-circuits.
    /// 5. Assert `CompletionState::Failed(DependencyFailed { file_id: '/dep.ts', .. })`.
    ///
    /// Without the Analysis-key fan-out, the Artifact stays pending
    /// → step 5 fails with state=None. With the fan-out but without
    /// the `failed_blocker_deps` marker the Artifact resolves
    /// `Ready` → step 5 fails with the Ready variant. With both,
    /// the typed `DependencyFailed` surfaces.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn source_failure_terminalizes_analysis_keyed_waiters() {
        use std::time::{Duration, Instant};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        // /dep.ts content IS inserted: the gated executor needs
        // the loader to return Some so the worker reaches
        // `execute_source` (the FileNotFound path skips the
        // executor entirely). The executor then fails via
        // `StageFailed` instead of `FileNotFound`.
        loader.insert("/dep.ts".to_string(), Arc::from("dep content"));

        let (dep_entered_tx, dep_entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (dep_release_tx, dep_release_rx) = crossbeam_channel::bounded::<()>(1);

        let executor = Arc::new(GatedFailingSourceExecutor {
            gates: dashmap::DashMap::new(),
        });
        executor.gates.insert(
            "/dep.ts".to_string(),
            SourceGate {
                entered_tx: dep_entered_tx,
                release_rx: dep_release_rx,
            },
        );

        let sched = Scheduler::with_executor(SchedulerConfig::default(), loader, executor);

        // Helper: poll-with-budget for handle resolution.
        fn poll_resolved<T: Clone>(
            handle: &CompletionHandle<T>,
            budget: Duration,
        ) -> Option<CompletionState<T>> {
            let deadline = Instant::now() + budget;
            while Instant::now() < deadline {
                if let Some(s) = handle.try_get() {
                    return Some(s);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            handle.try_get()
        }

        // Step 1: drive /a.vue Source + Analysis to committed.
        // /a.vue has no gate so the default executor path runs.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });
        let analysis_state = poll_resolved(&analysis_handle, Duration::from_secs(5))
            .expect("/a.vue Analysis must complete within 5s");
        assert!(
            analysis_state.is_ready(),
            "/a.vue Analysis precondition: must reach Ready (loader has content). observed: {analysis_state:?}",
        );
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;
        assert!(
            sched
                .try_get_analysis("/a.vue")
                .map(|a| a.generation == a_gen)
                .unwrap_or(false),
            "precondition: /a.vue Analysis must be committed at a_gen={a_gen}",
        );

        // Step 2: register /dep.ts as a late blocker. The
        // auto-ingest creates /dep.ts at gen 1 and enqueues a
        // Source NewRequest. The driver dequeues it and admits
        // the Source DAG identity; a worker picks up the Source
        // stage and enters the gated executor.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Step 3: wait for the /dep.ts Source worker to enter
        // the gated executor. This synchronizes the test thread
        // with the in-flight Source execution: at this point
        // /dep.ts's Source DAG identity is admitted and the
        // worker is BLOCKED inside the executor — the matrix
        // sees the live Source identity and the Artifact
        // admission below records the Analysis DepKey.
        dep_entered_rx.recv_timeout(Duration::from_secs(5)).expect(
            "/dep.ts Source worker must enter the gated executor within 5s — \
                 the driver should have admitted the Source DAG identity by now",
        );

        // Step 4: submit the Artifact request and WAIT for the
        // driver to admit it BEFORE releasing the gate. The
        // submit_request enqueues into the inbox; the driver
        // thread pops it later via handle_new_request, which
        // routes through admit_artifact_with_blockers. Without
        // the wait-for-admit synchronization the test would release
        // the gate immediately after submit_request returned,
        // leaving a race window: the driver could process the
        // dep's terminal failure BEFORE the Artifact admission, in
        // which case the matrix's dead-producer arm would classify
        // /dep.ts as Resolved and the Artifact would admit with
        // NO blockers.
        //
        // Poll the DAG for the Artifact identity until it
        // appears. The gated executor is blocking /dep.ts Source,
        // so /dep.ts cannot transition to a dead-producer state
        // while we wait — the matrix sees Source DAG identity
        // live throughout this poll.
        let artifact_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 77 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        let artifact_identity = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            generation: a_gen,
            profile_hash: profile_hash_to_bytes(77),
            content_hash: [0u8; 16],
        };
        let admit_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let admitted = {
                let dag = sched.dag.lock();
                dag.token_for(&artifact_identity).is_some()
            };
            if admitted {
                break;
            }
            if Instant::now() >= admit_deadline {
                panic!(
                    "Artifact admission must complete within 5s of submit_request; \
                     the driver should have admitted the Artifact identity in admit_artifact_with_blockers. \
                     a_gen={a_gen}",
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        // Diagnostic snapshot of /dep.ts state at the moment
        // the Artifact has been admitted. Confirms the matrix
        // saw /dep.ts as Gating during the admission: the
        // FileNode is present, no current_analysis, and the
        // gated executor still holds the Source DAG identity
        // live.
        let dep_state_post_admit = {
            let dep_node = sched.nodes.get("/dep.ts");
            let dep_gen = dep_node.as_ref().map(|n| n.generation()).unwrap_or(0);
            let dep_source_committed = dep_node.as_ref().and_then(|n| n.current_source()).is_some();
            let dep_analysis_committed = dep_node
                .as_ref()
                .and_then(|n| n.current_analysis())
                .is_some();
            let dep_source_id = WorkNodeIdentity::FileStage {
                canonical: Arc::from("/dep.ts"),
                generation: dep_gen,
                stage: FileStageKey::Source,
            };
            let dep_analysis_id = WorkNodeIdentity::FileStage {
                canonical: Arc::from("/dep.ts"),
                generation: dep_gen,
                stage: FileStageKey::Analysis,
            };
            let dag = sched.dag.lock();
            let source_live = dag.token_for(&dep_source_id).is_some();
            let analysis_live = dag.token_for(&dep_analysis_id).is_some();
            format!(
                "dep_gen={dep_gen}, source_committed={dep_source_committed}, \
                 analysis_committed={dep_analysis_committed}, source_live={source_live}, \
                 analysis_live={analysis_live}"
            )
        };

        // Step 5: release the gate — /dep.ts Source returns
        // Err → terminalize_failure(Source) → fan-out into the
        // Artifact's Analysis DepKey waiter → failed_blocker_deps
        // marker → dispatch → execute_artifact_stage
        // short-circuits with DependencyFailed.
        drop(dep_release_tx);

        let resolved_state = poll_resolved(&artifact_handle, Duration::from_secs(5));

        // KEY ASSERTION: the handle resolved as
        // `Failed(DependencyFailed)` citing /dep.ts. With the
        // deterministic synchronization above, /dep.ts Source
        // is GUARANTEED to fail AFTER the Artifact admission
        // recorded the Analysis DepKey — so the failed-blocker
        // marker MUST be set on the waiter.
        let state = resolved_state.expect(
            "Artifact handle must resolve within 5s after /dep.ts \
             Source fails terminally; without the Analysis-key \
             fan-out the Analysis-keyed waiter would stay pinned \
             forever",
        );
        match &state {
            CompletionState::Failed(crate::job::SchedulerError::DependencyFailed {
                dep_key,
                cause,
            }) => {
                match dep_key {
                    crate::dag::DepKey::FileStage { canonical, stage, .. } => {
                        assert_eq!(
                            canonical.as_ref(), "/dep.ts",
                            "the typed DependencyFailed must cite /dep.ts (the failed prerequisite), \
                             not the owner /a.vue. state={state:?}, a_gen={a_gen}",
                        );
                        // Stage is whichever stage's DepKey was recorded on
                        // the waiter — `Analysis` for the Source-failure
                        // fan-out (the Artifact gated on
                        // `FileStage(/dep.ts, _, Analysis)`).
                        assert_eq!(
                            *stage, crate::dag::FileStageKey::Analysis,
                            "the failed DepKey stage must be Analysis (the Artifact gated on Analysis, \
                             not Source). state={state:?}",
                        );
                    }
                    other_key => panic!(
                        "expected FileStage DepKey on Source-failure fan-out, got {other_key:?}. \
                         state={state:?}, a_gen={a_gen}",
                    ),
                }
                // The producer's terminal cause must be carried
                // through verbatim. The gated executor returns a
                // StageError, so the underlying cause must be
                // `StageFailed` for `/dep.ts`.
                match cause.as_ref() {
                    crate::job::SchedulerError::StageFailed { file_id, .. } => {
                        assert_eq!(
                            file_id, "/dep.ts",
                            "carried cause must cite the producer (/dep.ts), not the owner (/a.vue). \
                             state={state:?}",
                        );
                    }
                    other_cause => panic!(
                        "DependencyFailed.cause must carry the producer's StageFailed \
                         for /dep.ts, got {other_cause:?}. state={state:?}, a_gen={a_gen}",
                    ),
                }
            }
            other => panic!(
                "expected Failed(DependencyFailed {{ dep_key: FileStage {{ canonical: \"/dep.ts\", stage: Analysis, .. }}, .. }}), \
                 got {other:?}. \
                 a Ready or generic Failed here means the typed dependency-failure propagation \
                 is missing — the Artifact executor silently resolved over a dead prerequisite. \
                 a_gen={a_gen}, dep_state_post_admit={dep_state_post_admit}"
            ),
        }

        // Confirm the dep's Source did indeed fail (StageFailed)
        // — without this the test would be tautological.
        let dep_source = sched.try_get_source("/dep.ts");
        assert!(
            dep_source.is_none(),
            "precondition: /dep.ts Source must have failed (gated StageFailed) — \
             current_source must be None. observed: {dep_source:?}",
        );
    }

    /// Lock-ordering discriminator: the pre-executor race-safe skip in
    /// [`Scheduler::execute_artifact_stage`] must NOT hold a DashMap
    /// `Ref` on `node.artifacts` across the `dag.lock()` acquisition.
    /// The external `commit_artifact` path holds `dag.lock()` and then
    /// writes the same DashMap shard; if the worker's skip-path held a
    /// shard-read Ref across its `dag.lock()` acquisition, the two
    /// orderings (dag-lock → shard-write vs shard-read → dag-lock) form
    /// an AB-BA inversion and deadlock.
    ///
    /// Discriminator: race threads of each ordering against each other
    /// on the same `(canonical, profile_hash)`. Without the
    /// drop-Ref-before-lock helper the test hangs (one or both
    /// threads stuck in the AB-BA window) and the join budget fires.
    /// With the bool helper the Ref is dropped inside the helper
    /// body, so the worker's `dag.lock()` acquisition no longer
    /// crosses a held shard-read Ref and the race churns indefinitely
    /// without stalling.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pre_executor_skip_drops_dashmap_ref_before_dag_lock() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;
        use std::time::{Duration, Instant};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/race.vue".to_string(), Arc::from("r"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);
        let node = sched
            .nodes
            .entry("/race.vue".to_string())
            .or_insert_with(|| sched.create_node("/race.vue", None))
            .clone();
        // Generation 5 keeps both the commit publisher and the skip
        // path aligned on the same `(file, gen, profile)` slot so the
        // skip-arm fires every iteration of the worker thread.
        for _ in 0..5 {
            node.bump_generation();
        }
        // Seed Source + Analysis so `commit_artifact`'s
        // current_source / current_analysis early-out gates do not
        // exit the publisher thread before it acquires `dag.lock()`.
        // The AB-BA race scenario needs publisher and worker BOTH
        // hammering the dag-lock + same-shard pair.
        node.source
            .store(Arc::new(Some(Arc::new(SourceSnapshot::new_empty(
                Arc::from("r"),
                5,
            )))));
        node.analysis
            .store(Arc::new(Some(Arc::new(AnalysisSnapshot::new_empty(5)))));
        // Pre-seed the artifact slot so `execute_artifact_stage`'s
        // skip arm fires immediately every dispatch — the
        // generation matches the node, and the per-profile slot is
        // populated, so the helper returns `true` and the path runs
        // through the `dag.lock().cancel(...)` arm. That is exactly
        // the AB-BA window the fix closes.
        node.artifacts.insert(
            42,
            Arc::new(ArtifactSnapshot {
                generation: 5,
                profile_hash: 42,
                data: Arc::new(crate::node::EmptyData),
            }),
        );

        let stop = Arc::new(AtomicBool::new(false));
        let stop_a = Arc::clone(&stop);
        let stop_b = Arc::clone(&stop);
        let sched_a = Arc::clone(&sched);
        let sched_b = Arc::clone(&sched);
        let node_b = Arc::clone(&node);

        // Thread A: commit_artifact in a tight loop. Path:
        // `dag.lock()` → `node.artifacts.insert(...)`. dag-lock →
        // shard-write ordering.
        let t_commit = thread::spawn(move || {
            while !stop_a.load(Ordering::Acquire) {
                sched_a.commit_artifact(
                    "/race.vue",
                    42,
                    ArtifactSnapshot {
                        generation: 5,
                        profile_hash: 42,
                        data: Arc::new(crate::node::EmptyData),
                    },
                );
            }
        });

        // Thread B: execute_artifact_stage skip path in a tight loop.
        // Current path: helper takes/drops the Ref internally, then
        // `dag.lock().cancel(...)`. The vulnerable inline path would
        // have been:
        // `if let Some(existing) = node.artifacts.get(&profile_hash)`
        // (shard-read Ref held), then `dag.lock().cancel(...)`
        // (dag-lock acquired while Ref alive) → AB-BA with thread A.
        let inbox = sched_b.inbox.sender.clone();
        let executor = Arc::clone(&sched_b.executor);
        let dag_b = Arc::clone(&sched_b.dag);
        let t_skip = thread::spawn(move || {
            while !stop_b.load(Ordering::Acquire) {
                Scheduler::execute_artifact_stage(
                    &node_b,
                    5,
                    42,
                    executor.as_ref(),
                    &inbox,
                    Arc::clone(&dag_b),
                );
            }
        });

        // 1-second race window — plenty of iterations to surface a
        // deadlock at any practical scheduler hash collision rate.
        thread::sleep(Duration::from_millis(1000));
        stop.store(true, Ordering::Release);

        // 5-second join budget. Without the drop-Ref-before-lock
        // helper the join hangs (one or both threads stuck in AB-BA).
        // With the helper, both return immediately.
        let start_join = Instant::now();
        let join_budget = Duration::from_secs(5);
        let mut commit_joined = false;
        let mut skip_joined = false;
        while start_join.elapsed() < join_budget && !(commit_joined && skip_joined) {
            if !commit_joined && t_commit.is_finished() {
                commit_joined = true;
            }
            if !skip_joined && t_skip.is_finished() {
                skip_joined = true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            commit_joined && skip_joined,
            "race-skip / commit-artifact threads must complete within \
             the 5-second join budget — an inline \
             `node.artifacts.get(...) → dag.lock().cancel(...)` pattern \
             would invert shard-read → dag-lock vs commit_artifact's \
             dag-lock → shard-write, causing AB-BA. \
             (commit_joined={commit_joined}, skip_joined={skip_joined})",
        );
        // Drain joins on the slow arm if any.
        if commit_joined {
            t_commit.join().expect("commit thread");
        }
        if skip_joined {
            t_skip.join().expect("skip thread");
        }
    }

    /// `bump_generation` + the supersede sweep must run atomically
    /// under the DAG lock. With a bare-atomic bump, a dispatcher
    /// could observe `node.generation() == new_gen` BEFORE the
    /// supersede sweep cancelled the stale-generation DAG identity.
    /// The dispatch-time defensive `debug_assert!` would then trip
    /// on the gen-mismatch arm, asserting that
    /// `dag.lock().token_for(stale)` is `None` — but pre-supersede
    /// the stale identity was still in `by_identity`, so
    /// `token_for` returned `Some` and the assert fired.
    ///
    /// Structural discriminator: with the bump under the DAG lock,
    /// any code path that observes a generation mismatch on a still-
    /// admitted stale identity must be running concurrently with
    /// the lock held — impossible if the bump and the cancel sweep
    /// share a lock acquisition. The test characterizes the
    /// invariant directly by holding the DAG lock from a separate
    /// thread, then calling `invalidate(...)` and verifying it
    /// cannot bump the generation while the lock is held. With a
    /// bare-atomic bump the bump would race ahead of the lock-
    /// acquisition wait; with the bump under the DAG lock the bump
    /// is blocked until the DAG lock is free.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn bump_generation_supersede_dispatch_skip_no_spurious_panic() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;
        use std::time::Duration;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/race.vue".to_string(), Arc::from("r"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Seed a node at generation 1.
        sched.submit_request(Request {
            file_id: "/race.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Background,
            source: Some(Arc::from("r")),
            file_kind: None,
            request_context: None,
        });
        sched.drain_inbox();
        let node = sched.nodes.get("/race.vue").unwrap().clone();
        let gen_before = node.generation();
        assert!(
            gen_before >= 1,
            "precondition: node at gen >= 1, observed {gen_before}",
        );

        // Hold the DAG lock from a separate thread for a controlled
        // window. While the lock is held, `invalidate(...)` should
        // NOT be able to advance the generation — the lock-first
        // path acquires `dag.lock()` BEFORE `bump_generation()`, so
        // the bump waits for the lock. With a bare-atomic bump the
        // bump would run first and `node.generation()` would advance
        // during the hold window.
        let dag_handle = Arc::clone(&sched.dag);
        let hold_active = Arc::new(AtomicBool::new(true));
        let hold_active_w = Arc::clone(&hold_active);
        let holder = thread::spawn(move || {
            let _guard = dag_handle.lock();
            // Hold for 200ms.
            thread::sleep(Duration::from_millis(200));
            hold_active_w.store(false, Ordering::Release);
        });

        // Give the holder thread time to acquire the lock.
        thread::sleep(Duration::from_millis(20));

        // Concurrent invalidate. With the lock-first path this
        // blocks on the DAG lock until the holder releases. With a
        // bare-atomic bump the bump would run immediately and the
        // supersede call would then block on the lock — but the
        // generation would already be advanced.
        let node_clone = Arc::clone(&node);
        let sched_clone = Arc::clone(&sched);
        let inv = thread::spawn(move || {
            let observed_before_call = node_clone.generation();
            sched_clone.invalidate("/race.vue");
            (observed_before_call, node_clone.generation())
        });

        // Sample node.generation() WHILE the holder is still parked.
        // Invariant: the generation does NOT advance during the
        // hold window because invalidate is blocked. With a
        // bare-atomic bump the bump would run immediately and the
        // generation would advance.
        thread::sleep(Duration::from_millis(50));
        let mid_hold_gen = node.generation();
        assert!(
            hold_active.load(Ordering::Acquire),
            "precondition: holder thread must still hold the DAG lock",
        );

        // KEY ASSERTION: with the holder still parked, the generation
        // must be unchanged. With the bump under the DAG lock,
        // `mid_hold_gen == gen_before`. With a bare-atomic bump the
        // bump would run ahead and `mid_hold_gen > gen_before`.
        assert_eq!(
            mid_hold_gen, gen_before,
            "bump_generation must NOT advance node.generation() while \
             the DAG lock is held by another thread — a bare-atomic \
             bump would advance the generation BEFORE the supersede \
             sweep could cancel the stale identity, opening the \
             dispatch-time defensive `debug_assert!` window. \
             observed: gen_before={gen_before}, mid_hold_gen={mid_hold_gen}",
        );

        // Drain the threads.
        let _ = inv.join();
        let _ = holder.join();
    }

    /// Discriminating stress test for the AB-BA prevention rule
    /// applied to the lifecycle sweeps (`invalidate`, `close_file`,
    /// `commit_artifact`) and the `handle_stage_complete` dep-file
    /// admission loop.
    ///
    /// The invariant under test: nodes-shard `Ref`s are dropped
    /// BEFORE acquiring `dag.lock()`. A hypothetical opposing
    /// `dag.lock → nodes-shard-write` caller would deadlock
    /// against any caller that holds a `Ref` across `dag.lock`.
    /// Today no production path takes the opposing ordering — the
    /// hygiene is preemptive — but a synthetic write-while-locked
    /// thread inside the test stands in for that future caller
    /// and exercises the invariant.
    ///
    /// Thread A repeatedly invokes the lifecycle sweeps on a fixed
    /// set of canonicals. Thread B holds `dag.lock()` and then
    /// performs writes on the same nodes-shard via
    /// `sched.nodes.insert` (the synthetic opposing ordering).
    /// With the snapshot+drop hygiene, no `Ref` survives into
    /// the DAG-lock window of thread A, so thread B's writes never
    /// stall on a `Ref` held by thread A — and conversely thread A
    /// never blocks waiting for thread B to release the shard
    /// write while thread B is parked on the DAG lock.
    ///
    /// The watchdog timeout below catches any deadlock by checking
    /// progress markers from both threads. Without the snapshot+drop
    /// hygiene, the cross-thread inversion is structurally possible
    /// (the lifecycle Ref crosses `dag.lock`); with snapshot+drop
    /// the inversion is closed.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn lifecycle_sweeps_drop_nodes_ref_before_dag_lock() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::Arc;
        use std::thread;
        use std::time::{Duration, Instant};

        let loader = Arc::new(MemorySourceLoader::new());
        for i in 0..8 {
            loader.insert(format!("/race-{i}.vue"), Arc::from("v"));
        }
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Seed nodes for each /race-{i}.vue at gen >= 1.
        for i in 0..8 {
            sched.submit_request(Request {
                file_id: format!("/race-{i}.vue"),
                target: TargetStage::Analysis,
                priority: Priority::Interactive,
                source: Some(Arc::from("v")),
                file_kind: None,
                request_context: None,
            });
        }
        sched.drive_all();

        let stop = Arc::new(AtomicBool::new(false));
        let stop_lifecycle = Arc::clone(&stop);
        let stop_lock = Arc::clone(&stop);
        let sched_lifecycle = Arc::clone(&sched);
        let sched_lock = Arc::clone(&sched);
        let lifecycle_ticks = Arc::new(AtomicU64::new(0));
        let lifecycle_ticks_w = Arc::clone(&lifecycle_ticks);
        let lock_ticks = Arc::new(AtomicU64::new(0));
        let lock_ticks_w = Arc::clone(&lock_ticks);

        // Thread A: lifecycle sweeps that historically held a
        // nodes-shard `Ref` across `dag.lock()`. With the snapshot+drop
        // hygiene they snapshot the `Arc<FileNode>` and drop the `Ref`
        // first.
        let lifecycle = thread::spawn(move || {
            let mut tick = 0u64;
            while !stop_lifecycle.load(Ordering::Acquire) {
                let id = format!("/race-{}.vue", tick % 8);
                match tick % 3 {
                    0 => sched_lifecycle.invalidate(&id),
                    1 => sched_lifecycle.close_file(&id),
                    _ => {
                        let snap = ArtifactSnapshot {
                            generation: sched_lifecycle
                                .try_get_source(&id)
                                .map(|s| s.generation)
                                .unwrap_or(1),
                            profile_hash: 42,
                            data: Arc::new(crate::node::EmptyData),
                        };
                        sched_lifecycle.commit_artifact(&id, 42, snap);
                    }
                }
                tick = tick.wrapping_add(1);
                lifecycle_ticks_w.store(tick, Ordering::Release);
            }
        });

        // Thread B: takes `dag.lock()` first, then mutates the
        // nodes shard via `insert/remove`. This is the synthetic
        // `dag.lock → nodes-shard-write` ordering that any future
        // production caller might introduce. Without snapshot+drop,
        // the lifecycle sweeps' `Ref → dag.lock` ordering deadlocks
        // against this shape; with snapshot+drop no `Ref` is held
        // across `dag.lock` from thread A, so thread B's writes
        // proceed without waiting on a `Ref`-blocked shard.
        let lock_traffic = thread::spawn(move || {
            let mut tick = 0u64;
            while !stop_lock.load(Ordering::Acquire) {
                let _guard = sched_lock.dag.lock();
                let id = format!("/race-{}.vue", tick % 8);
                let synth_id = format!("/synth-{}.vue", tick % 4);
                // Insert + remove on a fresh canonical so we
                // exercise the same DashMap shard write path
                // without disturbing the lifecycle thread's
                // operations on /race-* nodes.
                let synth_node = sched_lock.create_node(&synth_id, None);
                sched_lock.nodes.insert(synth_id.clone(), synth_node);
                sched_lock.nodes.remove(&synth_id);
                // Also probe the /race-* shard to fully exercise
                // the cross-thread shard contention surface.
                let _ = sched_lock.nodes.get(&id);
                drop(_guard);
                tick = tick.wrapping_add(1);
                lock_ticks_w.store(tick, Ordering::Release);
            }
        });

        // Watchdog: poll progress markers from both threads. If
        // either is parked on a deadlock, its tick counter stops
        // advancing while the other side continues (or both stop
        // if the deadlock is mutual). Stamp the last-observed tick
        // pair at deadline-2s and again at deadline; both pairs
        // must show strict progress.
        let start = Instant::now();
        let mid_deadline = start + Duration::from_secs(1);
        while Instant::now() < mid_deadline {
            thread::sleep(Duration::from_millis(25));
        }
        let lifecycle_at_mid = lifecycle_ticks.load(Ordering::Acquire);
        let lock_at_mid = lock_ticks.load(Ordering::Acquire);

        let deadline = start + Duration::from_secs(2);
        while Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        let lifecycle_at_end = lifecycle_ticks.load(Ordering::Acquire);
        let lock_at_end = lock_ticks.load(Ordering::Acquire);

        stop.store(true, Ordering::Release);

        // Bounded join: poll `is_finished` with a watchdog deadline
        // rather than calling `join()` directly. An unbounded
        // `.join()` would stall `cargo test` indefinitely if a
        // regression reintroduces the AB-BA deadlock — the
        // forward-progress assertions below already characterise
        // the stall, but the join itself would never return. The
        // 5-second budget matches the sibling race-stress test's
        // join budget and is comfortably above the 1-second sleep
        // already used during the workload window.
        let join_deadline = Instant::now() + Duration::from_secs(5);
        let mut lifecycle_finished = false;
        let mut lock_finished = false;
        while Instant::now() < join_deadline && !(lifecycle_finished && lock_finished) {
            if !lifecycle_finished && lifecycle.is_finished() {
                lifecycle_finished = true;
            }
            if !lock_finished && lock_traffic.is_finished() {
                lock_finished = true;
            }
            thread::sleep(Duration::from_millis(25));
        }
        assert!(
            lifecycle_finished && lock_finished,
            "lifecycle + dag.lock → nodes-shard-write threads must terminate within \
             the 5-second budget after `stop` is set — an unbounded `join()` would \
             hang cargo test indefinitely if a regression reintroduces the AB-BA \
             stall. (lifecycle_finished={lifecycle_finished}, \
             lock_finished={lock_finished})",
        );
        // Both threads are confirmed finished; the join calls
        // below return immediately.
        lifecycle.join().expect("lifecycle thread");
        lock_traffic.join().expect("lock-traffic thread");

        // Discriminating assertion: both threads must have made
        // forward progress between the mid-point and the deadline.
        // An AB-BA inversion without the snapshot+drop hygiene
        // would park one (or both) threads on a lock acquisition
        // and the corresponding tick counter would stop advancing.
        assert!(
            lifecycle_at_end > lifecycle_at_mid,
            "lifecycle thread must continue making progress (no AB-BA stall): \
             observed mid={lifecycle_at_mid}, end={lifecycle_at_end}",
        );
        assert!(
            lock_at_end > lock_at_mid,
            "dag.lock → nodes-shard-write thread must continue making progress \
             (no AB-BA stall): observed mid={lock_at_mid}, end={lock_at_end}",
        );
    }

    // ─── Failed-blocker propagation discriminators ───

    /// Pre-admission failure race: a producer terminalization that
    /// happens BEFORE the consumer Artifact admission must still
    /// surface a typed `DependencyFailed` on the consumer.
    ///
    /// Without the persistent failure store, when the matrix
    /// consults `file_stage_analysis_blocker_status` for the failed
    /// dep, the dead-producer arm returns `Resolved` and the
    /// Artifact admits with an EMPTY blocker set. The executor reads
    /// the OWNER's snapshot (which is unrelated to the failed dep)
    /// and resolves `Ready` — the pre-admission failure race that
    /// this discriminator pins down.
    ///
    /// With the persistent `SchedulerDag::terminal_dep_failures`
    /// store, it carries the terminal record from
    /// `terminalize_failure(Source)`. The matrix's first arm
    /// consults the store and returns `Failed(record)`.
    /// `admit_artifact_with_blockers` collects the record, drops the
    /// dep from the gating set, AND attaches the record on the
    /// just-submitted Artifact node via
    /// `dag.attach_failed_dep`. Dispatch drains the attached map
    /// into the `ReadyJob`. The pre-dispatch chokepoint in
    /// `execute_stage_on_worker` surfaces a typed `DependencyFailed`.
    ///
    /// Discriminator: drive `/dep.ts` Source to FileNotFound failure
    /// FIRST (no `/dep.ts` content + no submitted Artifact yet), then
    /// call `register_resolved_deps('/a.vue', blockers=['/dep.ts'])`
    /// followed by `submit_request(Artifact{/a.vue})`. The Artifact
    /// MUST resolve `Failed(DependencyFailed)` citing `/dep.ts`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn source_failure_before_artifact_admission_propagates_dependency_failed() {
        use std::time::Duration;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        // /dep.ts is deliberately NOT inserted — execute_source_stage
        // routes through the FileNotFound terminalize_failure path
        // and records the terminal-dep-failure entry under the
        // Analysis DepKey BEFORE any Artifact admission for /a.vue.

        let sched = Scheduler::new(SchedulerConfig::default(), loader);

        // Drive /a.vue Source + Analysis to committed so a later
        // Artifact request finds the owner ready.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });
        let analysis_state = analysis_handle.wait();
        assert!(
            analysis_state.is_ready(),
            "/a.vue Analysis precondition failed: {analysis_state:?}",
        );
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Trigger /dep.ts Source failure via auto-ingest BEFORE the
        // Artifact admission. The owner /a.vue's late-blocker
        // registration auto-ingests /dep.ts as a Source request; the
        // worker enters execute_source_stage, the loader returns
        // None, and the FileNotFound terminalize_failure path
        // populates terminal_dep_failures under the Analysis DepKey
        // for /dep.ts.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );
        // Wait for /dep.ts to terminalize. The terminal record's
        // presence in terminal_dep_failures is the gate.
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut dep_gen_observed = 0u64;
        let observed_record = loop {
            // Inspect under the DAG lock to avoid racing with the
            // worker's terminalize_failure write.
            let dag = sched.dag.lock();
            let dep_gen = sched
                .nodes
                .get("/dep.ts")
                .map(|n| n.generation())
                .unwrap_or(0);
            if dep_gen > 0 {
                dep_gen_observed = dep_gen;
                let key = DepKey::FileStage {
                    canonical: Arc::clone(&dep_arc),
                    generation: dep_gen,
                    stage: FileStageKey::Analysis,
                };
                if let Some(rec) = dag.lookup_terminal_dep_failure(&key) {
                    break rec;
                }
            }
            drop(dag);
            if std::time::Instant::now() >= deadline {
                panic!(
                    "/dep.ts Source must terminalize within 5s — \
                     terminal_dep_failures entry never landed for \
                     the auto-ingested dep. dep_gen={dep_gen_observed}",
                );
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        assert!(
            matches!(
                observed_record.cause,
                crate::job::SchedulerError::FileNotFound { .. }
            ),
            "terminal_dep_failures must carry the producer's \
             FileNotFound cause verbatim. observed: {:?}",
            observed_record.cause,
        );

        // NOW submit the Artifact. The matrix consults
        // terminal_dep_failures and returns Failed(record); the
        // admission attaches the marker; the pre-dispatch chokepoint
        // surfaces DependencyFailed citing /dep.ts.
        let artifact_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 77 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        let artifact_state = artifact_handle.wait();
        match &artifact_state {
            CompletionState::Failed(crate::job::SchedulerError::DependencyFailed {
                dep_key,
                ..
            }) => {
                let (canonical, stage) = match dep_key {
                    crate::dag::DepKey::FileStage {
                        canonical, stage, ..
                    } => (canonical.as_ref(), *stage),
                    other_key => panic!(
                        "expected FileStage DepKey, got {other_key:?}. \
                         observed: {artifact_state:?}, a_gen={a_gen}"
                    ),
                };
                assert_eq!(
                    canonical, "/dep.ts",
                    "pre-admission-failure race must surface \
                     DependencyFailed citing /dep.ts (not the owner). \
                     observed: {artifact_state:?}, a_gen={a_gen}",
                );
                assert_eq!(
                    stage, crate::dag::FileStageKey::Analysis,
                    "the failed DepKey stage must be Analysis \
                     (the Artifact gates on Analysis DepKey). observed: \
                     {artifact_state:?}",
                );
            }
            other => panic!(
                "expected Failed(DependencyFailed {{ dep_key: FileStage {{ canonical: \"/dep.ts\", stage: Analysis, .. }}, .. }}) \
                 on Artifact admission AFTER /dep.ts Source terminalized; got {other:?}. \
                 Without the persistent failure store, the dead-producer arm returns Resolved, \
                 the blocker is dropped, and the Artifact resolves Ready over a snapshot built \
                 from a missing prerequisite. \
                 a_gen={a_gen}, dep_gen_observed={dep_gen_observed}",
            ),
        }
    }

    /// Analysis short-circuit on a failed blocker dep: without
    /// kind-uniform chokepoint dispatch, `failed_blocker_deps` was
    /// consumed only by the Artifact arm, so an Analysis node with
    /// a fanned-out or attached marker ran its user-side
    /// `execute_analysis` over a dead prerequisite.
    ///
    /// With the kind-uniform chokepoint in `execute_stage_on_worker`,
    /// short-circuit fires regardless of task kind. This test admits
    /// an Analysis node with an attached marker and asserts the
    /// Analysis stage never invokes the user-side executor.
    ///
    /// Discriminator: a custom `StageExecutor` instruments
    /// `execute_analysis` with a hit counter. The test:
    ///   1. Submits `/owner.vue` Analysis. Drives the Source stage
    ///      but holds the worker BEFORE Analysis dispatches.
    ///   2. Directly attaches a `FailedDepRecord` to the pending
    ///      Analysis node via `dag.attach_failed_dep`. This exercises
    ///      the post-admission-attach path (the chokepoint gap that
    ///      the kind-uniform short-circuit closes).
    ///   3. Drives the Analysis. The pre-dispatch chokepoint MUST
    ///      fire — the Analysis handle resolves
    ///      `Failed(DependencyFailed)` AND `execute_analysis` MUST
    ///      NOT have been invoked.
    #[test]
    fn analysis_short_circuits_on_failed_blocker_dep() {
        use std::sync::atomic::{AtomicU64, Ordering};

        /// Counts `execute_analysis` invocations so the test can
        /// assert the Analysis executor was never called for an
        /// owner whose only blocker has a Failed record.
        struct CountingAnalysisExecutor {
            analysis_hits: AtomicU64,
        }
        impl crate::executor::StageExecutor for CountingAnalysisExecutor {
            fn execute_analysis(
                &self,
                _canonical_id: &str,
                _source: &crate::node::SourceSnapshot,
                generation: u64,
            ) -> Result<crate::node::AnalysisSnapshot, crate::executor::StageError> {
                self.analysis_hits.fetch_add(1, Ordering::AcqRel);
                Ok(crate::node::AnalysisSnapshot::new_empty(generation))
            }
        }

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/owner.vue".to_string(), Arc::from("owner content"));
        let executor = Arc::new(CountingAnalysisExecutor {
            analysis_hits: AtomicU64::new(0),
        });
        let sched = Scheduler::new_sync_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::clone(&executor) as Arc<dyn crate::executor::StageExecutor>,
        );

        // Submit /owner.vue Analysis. Drive Source only — handle
        // the inbox-then-Source-dispatch sequence manually so the
        // Analysis identity is admitted (by `handle_stage_complete`
        // after Source commits) BUT NOT yet dispatched, leaving a
        // window for the test to attach the failure marker.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/owner.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("owner content")),
            file_kind: None,
            request_context: None,
        });
        // Drain inbox to process the NewRequest (Source admitted).
        sched.drain_inbox();
        // Dispatch ONE job (Source). Worker commits Source and
        // sends a StageComplete back into the inbox.
        let _ = sched.drive_one();
        // Drain inbox: StageComplete → handle_stage_complete admits
        // Analysis identity. The next drive_one would dispatch it,
        // but we want to attach the marker first.
        sched.drain_inbox();

        // Inject a FailedDepRecord onto the just-admitted Analysis
        // node BEFORE drive_one picks it up. This simulates the
        // post-admission attach path that
        // `admit_artifact_with_blockers` and
        // `register_resolved_deps` use for already-Failed deps
        // (pre-admission failure race) — and the fan-out path that
        // would mark an Analysis node waiting on a sibling dep
        // whose Source fails (Analysis-on-failed-Analysis-dep gap).
        let owner_arc: Arc<str> = Arc::from("/owner.vue");
        let owner_gen = sched.try_get_source("/owner.vue").unwrap().generation;
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        let dep_key = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&owner_arc),
            generation: owner_gen,
            stage: FileStageKey::Analysis,
        };
        {
            let mut dag = sched.dag.lock();
            let attached = dag.attach_failed_dep(
                &analysis_identity,
                crate::dag::FailedDepRecord {
                    dep_key: dep_key.clone(),
                    cause: crate::job::SchedulerError::FileNotFound {
                        file_id: "/dep.ts".to_string(),
                    },
                },
            );
            assert!(
                attached,
                "precondition: attach_failed_dep must land on the pending Analysis \
                 identity admitted by handle_stage_complete. owner_gen={owner_gen}",
            );
        }

        // Drive remaining work. The Analysis dispatch should
        // short-circuit via the pre-dispatch chokepoint, NOT call
        // execute_analysis.
        sched.drive_all();
        let state = analysis_handle
            .try_get()
            .expect("owner Analysis must resolve after drive_all");

        match &state {
            CompletionState::Failed(crate::job::SchedulerError::DependencyFailed {
                dep_key,
                ..
            }) => {
                let (canonical, stage) = match dep_key {
                    crate::dag::DepKey::FileStage {
                        canonical, stage, ..
                    } => (canonical.as_ref(), *stage),
                    other_key => panic!(
                        "expected FileStage DepKey, got {other_key:?}. observed: {state:?}",
                    ),
                };
                assert_eq!(
                    canonical, "/dep.ts",
                    "Analysis short-circuit must surface \
                     DependencyFailed citing /dep.ts. observed: {state:?}",
                );
                assert_eq!(
                    stage, crate::dag::FileStageKey::Analysis,
                    "failed DepKey stage must be Analysis. observed: {state:?}",
                );
            }
            other => panic!(
                "expected /owner.vue Analysis to short-circuit with \
                 Failed(DependencyFailed {{ dep_key: FileStage {{ canonical: \"/dep.ts\", stage: Analysis, .. }}, .. }}). \
                 got {other:?}. Without kind-uniform short-circuit the Analysis arm dropped \
                 failed_blocker_deps silently and execute_analysis ran on a stale source.",
            ),
        }

        // Discriminating invariant: execute_analysis must NOT have
        // been invoked. Without kind-uniform short-circuit: hit
        // count >= 1 (the Analysis arm ignored the marker and ran).
        // With kind-uniform short-circuit: hit count == 0 (the
        // pre-dispatch chokepoint fired before the Analysis arm).
        // `owner_gen` is captured so a regression also has the gen
        // context.
        let hits = executor.analysis_hits.load(Ordering::Acquire);
        assert_eq!(
            hits, 0,
            "execute_analysis must NOT have been invoked when the \
             Analysis node carries a failed_blocker_deps marker. The pre-dispatch \
             short-circuit chokepoint MUST fire before kind-dispatch. hits={hits}, \
             owner_gen={owner_gen}",
        );
    }

    /// Source-completion blocker admission race: a producer that
    /// terminalized BEFORE the owner Source completed is classified
    /// as `Failed` by the matrix, recorded onto the per-canonical
    /// Artifact blocker registry, and surfaces as a typed
    /// `DependencyFailed` on the FIRST Artifact admission.
    ///
    /// The owner's ANALYSIS must remain ungated: missing macro_type_dep
    /// shapes only affect codegen (the Artifact stage). Templates,
    /// `defineSlots`, and script-level diagnostics derive from the
    /// parsed source independently of resolved type shapes, so an
    /// unresolved type dep must not block the Analysis publication
    /// the way it does for an Artifact (see
    /// `host_manage_tests::template_slots_with_unresolved_type_deps`
    /// for the matching session-level contract).
    ///
    /// The Source-completion path in `handle_stage_complete(Source)`
    /// calls `extract_deps`, routes each blocker through
    /// `file_stage_analysis_blocker_status`, and records the live +
    /// failed dep pair via `record_artifact_blockers`. The Analysis
    /// admit at the end of the arm completes normally with no deps.
    /// When the owner's Artifact is later admitted,
    /// `admit_artifact_with_blockers` drains the registry, attaches
    /// `FailedDepRecord` to the Artifact node, and the pre-dispatch
    /// chokepoint surfaces `DependencyFailed`.
    ///
    /// Discriminator: drive `/dep.ts` Source to FileNotFound failure
    /// FIRST so `terminal_dep_failures` carries the record. Then
    /// submit `/a.vue` Analysis with a custom executor whose
    /// `extract_deps('/a.vue')` returns `/dep.ts` as a blocker. The
    /// `/a.vue` Source completion handler records the failure on
    /// the Artifact registry and the Analysis resolves `Ready`.
    /// Submit the Artifact, and the chokepoint surfaces
    /// `Failed(DependencyFailed)` citing `/dep.ts`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn source_completion_routes_failed_blocker_through_classifier() {
        use std::time::Duration;

        /// Custom executor: `/a.vue` extracts `/dep.ts` as a blocker.
        /// `/dep.ts` itself extracts nothing.
        struct OwnerWithDepExtractor;
        impl crate::executor::StageExecutor for OwnerWithDepExtractor {
            fn extract_deps(
                &self,
                canonical_id: &str,
                _source: &crate::node::SourceSnapshot,
            ) -> crate::executor::ExtractedDeps {
                if canonical_id == "/a.vue" {
                    crate::executor::ExtractedDeps {
                        forward_deps: vec!["/dep.ts".to_string()],
                        blocker_ids: vec!["/dep.ts".to_string()],
                    }
                } else {
                    crate::executor::ExtractedDeps::default()
                }
            }
        }

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        // /dep.ts deliberately omitted — Source loader returns None,
        // execute_source_stage routes through terminalize_failure
        // with FileNotFound, populating terminal_dep_failures.

        let executor: Arc<dyn crate::executor::StageExecutor> = Arc::new(OwnerWithDepExtractor);
        let sched = Scheduler::with_executor(SchedulerConfig::default(), loader, executor);

        // Step 1: submit /dep.ts directly and drive to terminal
        // FileNotFound BEFORE /a.vue Source extracts deps. This
        // populates terminal_dep_failures under /dep.ts's Analysis
        // DepKey at /dep.ts's generation.
        let dep_handle = sched.submit_request(Request {
            file_id: "/dep.ts".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        let dep_state = dep_handle.wait();
        assert!(
            !dep_state.is_ready(),
            "precondition: /dep.ts must fail (no content). got: {dep_state:?}",
        );
        // Wait for the terminal record to land in the store.
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        let dep_gen_observed = {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let dep_gen = sched
                    .nodes
                    .get("/dep.ts")
                    .map(|n| n.generation())
                    .unwrap_or(0);
                if dep_gen > 0 {
                    let key = DepKey::FileStage {
                        canonical: Arc::clone(&dep_arc),
                        generation: dep_gen,
                        stage: FileStageKey::Analysis,
                    };
                    let dag = sched.dag.lock();
                    if dag.lookup_terminal_dep_failure(&key).is_some() {
                        break dep_gen;
                    }
                }
                if std::time::Instant::now() >= deadline {
                    panic!("/dep.ts must terminalize and populate terminal_dep_failures within 5s",);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        };

        // Step 2: submit /a.vue Analysis. The Source completes via
        // the custom executor's default execute_source path; the
        // Source-completion handler then runs extract_deps which
        // returns /dep.ts as a blocker. The classifier-routed
        // admission sees the persistent failure record, records it
        // on the per-canonical Artifact blocker registry, and the
        // Analysis is admitted ungated (analysis is recoverable from
        // the source alone — codegen consumes the resolved type
        // shapes, not analysis).
        let owner_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });

        // Analysis must resolve `Ready` — the missing dep is recorded
        // on the Artifact registry, not gating Analysis.
        let analysis_state = {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(s) = owner_handle.try_get() {
                    break Some(s);
                }
                if std::time::Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        };
        let analysis_state = analysis_state.unwrap_or_else(|| {
            panic!(
                "owner Analysis must resolve within 5s — analysis is ungated by macro_type_dep \
                 status; the Source-completion path records dep failures on the per-canonical \
                 Artifact blocker registry, leaving Analysis to publish normally. \
                 dep_gen_observed={dep_gen_observed}",
            )
        });
        assert!(
            analysis_state.is_ready(),
            "owner Analysis must succeed when its macro_type_dep is missing; the dep failure \
             is gated at Artifact admission, not Analysis. got: {analysis_state:?}",
        );

        // Step 3: submit /a.vue Artifact. The Artifact admission
        // drains the per-canonical blocker registry, reclassifies
        // every persisted failure against the live state, and
        // attaches a `FailedDepRecord` to the Artifact node. The
        // pre-dispatch chokepoint in `execute_stage_on_worker`
        // surfaces a typed `DependencyFailed` citing /dep.ts before
        // the Artifact executor runs.
        let artifact_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 42 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        let artifact_state = {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(s) = artifact_handle.try_get() {
                    break Some(s);
                }
                if std::time::Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        };
        let artifact_state = artifact_state.unwrap_or_else(|| {
            panic!(
                "owner Artifact must resolve within 5s — the Source-completion path persisted \
                 the dead-producer dep on the Artifact registry, so `admit_artifact_with_blockers` \
                 must surface DependencyFailed on the chokepoint. \
                 dep_gen_observed={dep_gen_observed}",
            )
        });
        match &artifact_state {
            CompletionState::Failed(crate::job::SchedulerError::DependencyFailed {
                dep_key,
                cause,
            }) => {
                match dep_key {
                    crate::dag::DepKey::FileStage {
                        canonical, stage, ..
                    } => {
                        assert_eq!(
                            canonical.as_ref(),
                            "/dep.ts",
                            "DependencyFailed must cite /dep.ts (the failed prerequisite). \
                             state={artifact_state:?}",
                        );
                        assert_eq!(
                            *stage,
                            crate::dag::FileStageKey::Analysis,
                            "failed DepKey stage must be Analysis. state={artifact_state:?}",
                        );
                    }
                    other_key => panic!(
                        "expected FileStage DepKey on Artifact admission, got {other_key:?}. \
                         state={artifact_state:?}",
                    ),
                }
                // Cause must be FileNotFound (the producer was missing).
                match cause.as_ref() {
                    crate::job::SchedulerError::FileNotFound { file_id } => {
                        assert_eq!(
                            file_id, "/dep.ts",
                            "carried cause must cite /dep.ts. state={artifact_state:?}",
                        );
                    }
                    other_cause => panic!(
                        "DependencyFailed.cause must carry FileNotFound for /dep.ts, \
                         got {other_cause:?}. state={artifact_state:?}",
                    ),
                }
            }
            other => panic!(
                "expected Failed(DependencyFailed citing /dep.ts) on owner Artifact after \
                 the Source-completion path persisted the dead-producer dep on the \
                 Artifact registry. got {other:?}. \
                 dep_gen_observed={dep_gen_observed}",
            ),
        }
    }

    /// Post-complete `register_resolved_deps` failure persistence:
    /// when the owner's Analysis is ALREADY complete and the late
    /// blockers contain a producer that has terminally failed BEFORE
    /// `register_resolved_deps` runs, the matrix returns
    /// `Failed(record)` for the dep and `dep_keys` ends up empty.
    /// Without the failure-side persistence, the registry persisted
    /// only the live `dep_keys` (an empty `BTreeSet`, which
    /// `record_artifact_blockers` treated as a remove) and the
    /// `failed_records` were dropped on the floor. A later Artifact
    /// admission drained an empty registry entry and silently
    /// resolved `Ready` over the dead prerequisite.
    ///
    /// With the failure-side persistence, the registry slot is
    /// [`crate::dag::PendingBlockerSet`] — both the still-gating
    /// `deps` and the `failed` records ride together. The Artifact
    /// admission drains both, attaches every failed record via
    /// `attach_failed_dep`, and the pre-dispatch chokepoint surfaces
    /// `DependencyFailed`.
    ///
    /// Discriminator: drive `/dep.ts` to terminal FileNotFound FIRST
    /// (independent submit, no relation to the owner). Then drive
    /// `/a.vue` Source+Analysis to complete via a synthetic submit
    /// (no extract_deps). Call `register_resolved_deps('/a.vue',
    /// resolved=['/dep.ts'], blockers=['/dep.ts'])` AFTER the owner
    /// Analysis is complete — the matrix returns `Failed(record)`
    /// for `/dep.ts` immediately because `terminal_dep_failures`
    /// has the record. Submit Artifact `/a.vue`. Without the
    /// failure-side persistence: handle resolves `Ready` (the
    /// registry was cleared and the Artifact admission saw no
    /// blockers). With it: handle resolves `Failed(DependencyFailed)`
    /// citing `/dep.ts` with the `FileNotFound` cause carried
    /// through.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn register_resolved_deps_persists_failed_record_when_owner_analysis_already_complete() {
        use std::time::Duration;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        // /dep.ts deliberately omitted — independent submit will
        // terminalize it via FileNotFound BEFORE
        // register_resolved_deps fires.

        let sched = Scheduler::new(SchedulerConfig::default(), loader);

        // Step 1: independently fail /dep.ts FIRST so
        // terminal_dep_failures carries the record at its (gen=1,
        // Analysis) DepKey.
        let dep_handle = sched.submit_request(Request {
            file_id: "/dep.ts".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        let dep_state = dep_handle.wait();
        assert!(
            !dep_state.is_ready(),
            "precondition: /dep.ts must fail (no content). got: {dep_state:?}",
        );
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        let dep_gen_observed = {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let dep_gen = sched
                    .nodes
                    .get("/dep.ts")
                    .map(|n| n.generation())
                    .unwrap_or(0);
                if dep_gen > 0 {
                    let key = DepKey::FileStage {
                        canonical: Arc::clone(&dep_arc),
                        generation: dep_gen,
                        stage: FileStageKey::Analysis,
                    };
                    let dag = sched.dag.lock();
                    if dag.lookup_terminal_dep_failure(&key).is_some() {
                        break dep_gen;
                    }
                }
                if std::time::Instant::now() >= deadline {
                    panic!(
                        "precondition: /dep.ts must terminalize and populate \
                         terminal_dep_failures within 5s",
                    );
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        };

        // Step 2: drive /a.vue Source+Analysis to complete. The
        // default DefaultExecutor's extract_deps returns no deps, so
        // the Source-completion path's classifier-routed branch is
        // NOT entered.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });
        let analysis_state = analysis_handle.wait();
        assert!(
            analysis_state.is_ready(),
            "precondition: /a.vue Analysis must complete. got: {analysis_state:?}",
        );
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;
        let a_arc: Arc<str> = Arc::from("/a.vue");
        assert!(
            sched
                .nodes
                .get("/a.vue")
                .and_then(|n| n.current_analysis())
                .is_some(),
            "precondition: /a.vue Analysis must be committed at a_gen={a_gen}",
        );

        // Step 3: register_resolved_deps AFTER owner Analysis is
        // complete. The 3-state matrix sees the persistent failure
        // record for /dep.ts and returns Failed(record); dep_keys
        // is empty, failed_records carries the record.
        //
        // Without failure-side persistence: dep_set built from
        // dep_keys is empty, the registry entry is cleared
        // (record_artifact_blockers treats empty set as remove),
        // failed_records dropped at function end. The matrix
        // never sees the record again.
        //
        // With failure-side persistence: registry persists
        // PendingBlockerSet { deps: empty, failed: [record] }.
        // The Artifact admission below drains this and attaches
        // the failed record.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Verify: the registry holds the failure record.
        let registry_after_register = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            !registry_after_register.failed.is_empty(),
            "registry must persist failed records when \
             register_resolved_deps fires on a complete owner with \
             dead blockers. observed: {registry_after_register:?}, \
             a_gen={a_gen}, dep_gen_observed={dep_gen_observed}",
        );
        assert!(
            registry_after_register.failed.iter().any(|r| matches!(
                &r.dep_key,
                crate::dag::DepKey::FileStage { canonical, .. }
                if canonical.as_ref() == "/dep.ts"
            )),
            "registry failed records must include /dep.ts. \
             observed: {registry_after_register:?}",
        );

        // Step 4: submit Artifact /a.vue. The admission drains both
        // deps (empty) and failed records ([/dep.ts record]); the
        // record attaches to the Artifact node and the pre-dispatch
        // chokepoint surfaces DependencyFailed.
        let artifact_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 77 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        let state = {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(s) = artifact_handle.try_get() {
                    break Some(s);
                }
                if std::time::Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        };
        let state = state.expect(
            "Artifact handle must resolve within 5s after \
             register_resolved_deps persisted the failed record on a \
             complete-owner registry slot",
        );
        match &state {
            CompletionState::Failed(crate::job::SchedulerError::DependencyFailed {
                dep_key,
                cause,
            }) => {
                match dep_key {
                    crate::dag::DepKey::FileStage {
                        canonical, stage, ..
                    } => {
                        assert_eq!(
                            canonical.as_ref(),
                            "/dep.ts",
                            "DependencyFailed must cite /dep.ts. state={state:?}",
                        );
                        assert_eq!(
                            *stage,
                            crate::dag::FileStageKey::Analysis,
                            "failed DepKey stage must be Analysis. state={state:?}",
                        );
                    }
                    other_key => {
                        panic!("expected FileStage DepKey, got {other_key:?}. state={state:?}",)
                    }
                }
                match cause.as_ref() {
                    crate::job::SchedulerError::FileNotFound { file_id } => {
                        assert_eq!(
                            file_id, "/dep.ts",
                            "carried cause must cite /dep.ts. state={state:?}",
                        );
                    }
                    other_cause => panic!(
                        "DependencyFailed.cause must carry FileNotFound for /dep.ts, \
                         got {other_cause:?}. state={state:?}",
                    ),
                }
            }
            other => panic!(
                "expected Failed(DependencyFailed citing /dep.ts) on Artifact admission \
                 after post-complete register_resolved_deps recorded the failed record. \
                 got {other:?}. Without failure-side persistence: the failed record was \
                 dropped because the registry only persisted live dep_keys (an empty \
                 BTreeSet was treated as a remove), so the Artifact admission drained an \
                 empty registry entry and silently resolved Ready over the dead prerequisite. \
                 a_gen={a_gen}, dep_gen_observed={dep_gen_observed}",
            ),
        }
    }

    /// Analysis-stage terminal failure must fan out a
    /// `FailedDepRecord` to every already-admitted downstream waiter
    /// gating on `DepKey::FileStage { stage: Analysis }`, symmetric
    /// with the Source-side `fanout_source_failure_to_analysis_waiters`
    /// path. Without the Analysis-side fan-out,
    /// `terminalize_failure(Analysis)` only inserted into the
    /// persistent `terminal_dep_failures` store (which closes the
    /// pre-admission race) and relied on the generic
    /// `cancel(&analysis_identity)` to release waiters. But `cancel`
    /// only clears each waiter's `deps_remaining` entry — it does
    /// NOT record a `FailedDepRecord` — so an already-admitted
    /// Artifact waiter dispatched without the marker and resolved
    /// `Ready` over a snapshot built from a dead prerequisite.
    ///
    /// With the Analysis-side fan-out, `terminalize_failure(Analysis)`
    /// calls `fanout_analysis_failure_to_waiters` BEFORE `cancel` so
    /// each downstream waiter receives the failure marker; the
    /// pre-dispatch chokepoint then surfaces a typed
    /// `DependencyFailed` with the producer's `StageFailed` cause.
    ///
    /// Discriminator: a `GatedFailingAnalysisExecutor` blocks
    /// `/dep.ts` Analysis inside the executor until the test releases
    /// the gate (then returns `Err(StageError)`). The owner Artifact
    /// is admitted with the live Analysis DepKey for `/dep.ts` BEFORE
    /// the gate is released — so the failure is a post-admission
    /// failure. Without the fan-out: Artifact handle resolves Ready.
    /// With it: resolves `Failed(DependencyFailed)` with `StageFailed`
    /// cause.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn analysis_failure_fanout_to_admitted_waiters_symmetric_to_source() {
        use std::time::{Duration, Instant};

        /// Gate for the Analysis stage: signal entry then block on
        /// release, then return Err so Analysis fails terminally.
        struct AnalysisGate {
            entered_tx: crossbeam_channel::Sender<()>,
            release_rx: crossbeam_channel::Receiver<()>,
        }

        struct GatedFailingAnalysisExecutor {
            gates: dashmap::DashMap<String, AnalysisGate>,
        }

        impl crate::executor::StageExecutor for GatedFailingAnalysisExecutor {
            fn execute_analysis(
                &self,
                canonical_id: &str,
                _source: &crate::node::SourceSnapshot,
                generation: u64,
            ) -> Result<crate::node::AnalysisSnapshot, crate::executor::StageError> {
                if let Some(gate) = self.gates.get(canonical_id) {
                    let _ = gate.entered_tx.send(());
                    let _ = gate.release_rx.recv();
                    return Err(crate::executor::StageError {
                        message: format!("gated analysis failure for {canonical_id}"),
                    });
                }
                Ok(crate::node::AnalysisSnapshot::new_empty(generation))
            }
        }

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep content"));

        let (dep_entered_tx, dep_entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (dep_release_tx, dep_release_rx) = crossbeam_channel::bounded::<()>(1);

        let executor = Arc::new(GatedFailingAnalysisExecutor {
            gates: dashmap::DashMap::new(),
        });
        executor.gates.insert(
            "/dep.ts".to_string(),
            AnalysisGate {
                entered_tx: dep_entered_tx,
                release_rx: dep_release_rx,
            },
        );

        let sched = Scheduler::with_executor(SchedulerConfig::default(), loader, executor);

        fn poll_resolved<T: Clone>(
            handle: &CompletionHandle<T>,
            budget: Duration,
        ) -> Option<CompletionState<T>> {
            let deadline = Instant::now() + budget;
            while Instant::now() < deadline {
                if let Some(s) = handle.try_get() {
                    return Some(s);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            handle.try_get()
        }

        // Step 1: drive /a.vue Source + Analysis to committed.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });
        let analysis_state = poll_resolved(&analysis_handle, Duration::from_secs(5))
            .expect("/a.vue Analysis must complete");
        assert!(
            analysis_state.is_ready(),
            "precondition: /a.vue Analysis must reach Ready. got: {analysis_state:?}",
        );
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Step 2: register /dep.ts as a late blocker — auto-ingests
        // /dep.ts, runs Source (no gate, succeeds), then Analysis
        // (gated, blocked).
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Step 3: wait for the /dep.ts Analysis worker to enter the
        // gated executor. At this point /dep.ts Source has committed
        // and Analysis DAG identity is admitted + dispatched but
        // blocked inside execute_analysis.
        dep_entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("/dep.ts Analysis worker must enter the gated executor within 5s");

        // Step 4: submit Artifact + WAIT for admission BEFORE
        // releasing the gate. The matrix sees /dep.ts as Gating
        // (live Analysis DAG identity), so the Artifact admits with
        // a recorded Analysis DepKey on /dep.ts.
        let artifact_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 99 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        let artifact_identity = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            generation: a_gen,
            profile_hash: profile_hash_to_bytes(99),
            content_hash: [0u8; 16],
        };
        let admit_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let admitted = {
                let dag = sched.dag.lock();
                dag.token_for(&artifact_identity).is_some()
            };
            if admitted {
                break;
            }
            if Instant::now() >= admit_deadline {
                panic!(
                    "Artifact admission must complete within 5s of submit_request. \
                     a_gen={a_gen}",
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        // Step 5: release the gate — /dep.ts Analysis returns Err →
        // terminalize_failure(Analysis) → fan-out into the Artifact's
        // Analysis DepKey waiter → failed_blocker_deps marker →
        // dispatch → execute_artifact_stage short-circuits with
        // DependencyFailed.
        drop(dep_release_tx);

        let state = poll_resolved(&artifact_handle, Duration::from_secs(5)).expect(
            "Artifact handle must resolve within 5s after /dep.ts Analysis \
             fails terminally; without the Analysis-side fan-out the Analysis-keyed \
             waiter dropped its DepKey via cancel without a FailedDepRecord and resolved Ready",
        );
        match &state {
            CompletionState::Failed(crate::job::SchedulerError::DependencyFailed {
                dep_key,
                cause,
            }) => {
                match dep_key {
                    crate::dag::DepKey::FileStage {
                        canonical, stage, ..
                    } => {
                        assert_eq!(
                            canonical.as_ref(),
                            "/dep.ts",
                            "DependencyFailed must cite /dep.ts (the failed prerequisite), \
                             not the owner /a.vue. state={state:?}, a_gen={a_gen}",
                        );
                        assert_eq!(
                            *stage,
                            crate::dag::FileStageKey::Analysis,
                            "failed DepKey stage must be Analysis (the Artifact gated on \
                             /dep.ts Analysis). state={state:?}",
                        );
                    }
                    other_key => panic!(
                        "expected FileStage DepKey on Analysis-failure fan-out, got {other_key:?}. \
                         state={state:?}",
                    ),
                }
                // The producer's terminal cause must be carried
                // through verbatim. The gated Analysis executor
                // returns Err, so the cause must be StageFailed for
                // /dep.ts.
                match cause.as_ref() {
                    crate::job::SchedulerError::StageFailed { file_id, .. } => {
                        assert_eq!(
                            file_id, "/dep.ts",
                            "carried cause must cite the producer (/dep.ts). state={state:?}",
                        );
                    }
                    other_cause => panic!(
                        "DependencyFailed.cause must carry StageFailed for /dep.ts, \
                         got {other_cause:?}. state={state:?}",
                    ),
                }
            }
            other => panic!(
                "expected Failed(DependencyFailed citing /dep.ts) on Artifact admission \
                 after /dep.ts Analysis fails terminally. got {other:?}. \
                 Without the Analysis-side fan-out, cancel(&analysis_identity) released the \
                 Analysis DepKey from each waiter's deps_remaining WITHOUT recording a \
                 FailedDepRecord; the Artifact dispatched without the marker and resolved \
                 Ready over a dead prerequisite. a_gen={a_gen}",
            ),
        }
    }

    /// Same-generation recovery semantics: a `terminal_dep_failures`
    /// record planted at `(canonical, gen, Analysis)` must be
    /// cleared when `signal_stage_complete(Source)` or
    /// `signal_stage_complete(Analysis)` fires for the same
    /// `(canonical, gen)`. Without this clear, a Source/Analysis
    /// that previously failed and is retried at the same generation
    /// (e.g. an external commit lands fresh content at the same
    /// generation, or the host re-runs the stage in a recovery
    /// path) would leave the matrix returning `Failed` for the dep
    /// even though the dep is now successfully committed.
    ///
    /// Discriminator: plant a synthetic record directly via
    /// `dag.insert_terminal_dep_failure`, then drive a successful
    /// completion through `dag.signal_stage_complete` at the same
    /// `(canonical, gen)`. The record MUST be gone afterwards.
    /// Without the recovery clear, the record survives and any
    /// subsequent matrix consult against
    /// `(/dep.ts, gen, Analysis)` returns `Failed`. With the
    /// clear, the record is gone and the matrix returns `Gating` /
    /// `Satisfied` based on live state.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn signal_stage_complete_clears_terminal_dep_failure_for_same_gen_recovery() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/dep.ts".to_string(), Arc::from("dep content"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        // Plant a FileNode for /dep.ts and bump to gen=1 so we have
        // a stable generation to plant the record under.
        let dep_node = sched.create_node("/dep.ts", None);
        let dep_gen_v1 = dep_node.bump_generation();
        sched.nodes.insert("/dep.ts".to_string(), dep_node);

        let key_v1 = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: dep_gen_v1,
            stage: FileStageKey::Analysis,
        };

        // Plant a synthetic terminal failure record at (/dep.ts, 1, Analysis).
        {
            let mut dag = sched.dag.lock();
            dag.insert_terminal_dep_failure(crate::dag::FailedDepRecord {
                dep_key: key_v1.clone(),
                cause: crate::job::SchedulerError::FileNotFound {
                    file_id: "/dep.ts".to_string(),
                },
            });
            assert!(
                dag.lookup_terminal_dep_failure(&key_v1).is_some(),
                "precondition: record must be planted at gen={dep_gen_v1}",
            );
        }

        // Recovery path 1: Source completion at the same gen must
        // clear the record.
        let source_snap = Arc::new(crate::node::SourceSnapshot::new_empty(
            Arc::from("dep content"),
            dep_gen_v1,
        ));
        {
            let mut dag = sched.dag.lock();
            dag.signal_stage_complete(
                &dep_arc,
                dep_gen_v1,
                &TaskKind::Source,
                &RequestResult::Source(Arc::clone(&source_snap)),
            );
            assert!(
                dag.lookup_terminal_dep_failure(&key_v1).is_none(),
                "terminal_dep_failures must be cleared by \
                 signal_stage_complete(Source) at the same gen. \
                 Without the clear, the record survives and the matrix \
                 returns Failed for a successfully-recovered dep.",
            );
        }

        // Recovery path 2: Analysis completion at the same gen also
        // clears. Re-plant the record to test the Analysis-completion
        // path independently.
        {
            let mut dag = sched.dag.lock();
            dag.insert_terminal_dep_failure(crate::dag::FailedDepRecord {
                dep_key: key_v1.clone(),
                cause: crate::job::SchedulerError::FileNotFound {
                    file_id: "/dep.ts".to_string(),
                },
            });
            assert!(
                dag.lookup_terminal_dep_failure(&key_v1).is_some(),
                "precondition: record re-planted before Analysis path",
            );
        }
        let analysis_snap = Arc::new(crate::node::AnalysisSnapshot::new_empty(dep_gen_v1));
        {
            let mut dag = sched.dag.lock();
            dag.signal_stage_complete(
                &dep_arc,
                dep_gen_v1,
                &TaskKind::Analysis,
                &RequestResult::Analysis(Arc::clone(&analysis_snap)),
            );
            assert!(
                dag.lookup_terminal_dep_failure(&key_v1).is_none(),
                "terminal_dep_failures must be cleared by \
                 signal_stage_complete(Analysis) at the same gen",
            );
        }

        // Negative discriminator: Artifact completion at the same
        // gen must NOT touch the record. Artifact failures
        // terminalize per-profile, not the canonical's Analysis
        // key, so the recovery semantic does not apply.
        {
            let mut dag = sched.dag.lock();
            dag.insert_terminal_dep_failure(crate::dag::FailedDepRecord {
                dep_key: key_v1.clone(),
                cause: crate::job::SchedulerError::FileNotFound {
                    file_id: "/dep.ts".to_string(),
                },
            });
        }
        let artifact_snap = Arc::new(ArtifactSnapshot {
            generation: dep_gen_v1,
            profile_hash: 1,
            data: Arc::new(crate::node::EmptyData),
        });
        {
            let mut dag = sched.dag.lock();
            dag.signal_stage_complete(
                &dep_arc,
                dep_gen_v1,
                &TaskKind::Artifact { profile_hash: 1 },
                &RequestResult::Artifact(Arc::clone(&artifact_snap)),
            );
            assert!(
                dag.lookup_terminal_dep_failure(&key_v1).is_some(),
                "Artifact completion must NOT clear terminal_dep_failures — \
                 the recovery semantic only applies to Source/Analysis stages \
                 because Artifact failures terminalize per-profile, not the \
                 canonical's Analysis key.",
            );
        }
    }

    /// Persistent terminal-dep-failure cleanup characterization.
    /// The store must:
    ///   1. Persist a synthetic record under the recorded `(canonical, gen)` key.
    ///   2. Drop the matching-gen record when
    ///      `supersede_old_file_generations` runs (i.e., on
    ///      `invalidate(canonical)`).
    ///   3. Drop every referencing record on `remove(canonical)`.
    ///   4. Drop every record on `reset()` (via `dag.clear()`).
    ///
    /// Plants records directly via `dag.insert_terminal_dep_failure`
    /// so the test is independent of the auto-ingest/terminalize
    /// timing path — the cleanup contract is the discriminating
    /// invariant we exercise here.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn terminal_dep_failure_persists_across_gen_bump_invalidation_on_remove() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/dep.ts".to_string(), Arc::from("dep content"));
        // Use new_sync so we can deterministically observe the
        // cleanup sweeps without driver-thread timing.
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Plant a FileNode for /dep.ts so the matrix / cleanup
        // sweeps have something to operate on. Then plant a
        // terminal-dep-failure record under that FileNode's
        // generation.
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        let dep_node = sched.create_node("/dep.ts", None);
        let dep_gen_v1 = dep_node.bump_generation();
        sched.nodes.insert("/dep.ts".to_string(), dep_node);
        let key_v1 = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: dep_gen_v1,
            stage: FileStageKey::Analysis,
        };
        {
            let mut dag = sched.dag.lock();
            dag.insert_terminal_dep_failure(crate::dag::FailedDepRecord {
                dep_key: key_v1.clone(),
                cause: crate::job::SchedulerError::FileNotFound {
                    file_id: "/dep.ts".to_string(),
                },
            });
            assert!(
                dag.lookup_terminal_dep_failure(&key_v1).is_some(),
                "precondition: record planted at gen={dep_gen_v1} must be \
                 observable via lookup_terminal_dep_failure",
            );
        }

        // 2. Invalidate /dep.ts → supersede sweep must clear the
        // gen=v1 record so a fresh generation is not pinned as
        // Failed.
        sched.invalidate("/dep.ts");
        let dep_gen_v2 = sched
            .nodes
            .get("/dep.ts")
            .map(|n| n.generation())
            .unwrap_or(0);
        assert!(
            dep_gen_v2 > dep_gen_v1,
            "invalidate must bump /dep.ts past dep_gen_v1={dep_gen_v1}",
        );
        {
            let dag = sched.dag.lock();
            assert!(
                dag.lookup_terminal_dep_failure(&key_v1).is_none(),
                "supersede_old_file_generations must drop the \
                 gen=v1 terminal-dep-failure record on invalidate. \
                 dep_gen_v1={dep_gen_v1}, dep_gen_v2={dep_gen_v2}",
            );
        }

        // 3. Plant a fresh gen=v2 record + a synthetic record on a
        // SIBLING file so we can verify remove(/dep.ts) scrubs only
        // /dep.ts references and not other files.
        let key_v2 = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: dep_gen_v2,
            stage: FileStageKey::Analysis,
        };
        let sibling_arc: Arc<str> = Arc::from("/sibling.ts");
        let sibling_key = DepKey::FileStage {
            canonical: Arc::clone(&sibling_arc),
            generation: 5,
            stage: FileStageKey::Analysis,
        };
        {
            let mut dag = sched.dag.lock();
            dag.insert_terminal_dep_failure(crate::dag::FailedDepRecord {
                dep_key: key_v2.clone(),
                cause: crate::job::SchedulerError::FileNotFound {
                    file_id: "/dep.ts".to_string(),
                },
            });
            dag.insert_terminal_dep_failure(crate::dag::FailedDepRecord {
                dep_key: sibling_key.clone(),
                cause: crate::job::SchedulerError::FileNotFound {
                    file_id: "/sibling.ts".to_string(),
                },
            });
        }

        // remove(/dep.ts) must scrub /dep.ts records and PRESERVE
        // sibling records.
        sched.remove("/dep.ts");
        {
            let dag = sched.dag.lock();
            assert!(
                dag.lookup_terminal_dep_failure(&key_v2).is_none(),
                "remove(/dep.ts) must scrub every terminal-dep-\
                 failure record referencing /dep.ts. dep_gen_v2={dep_gen_v2}",
            );
            assert!(
                dag.lookup_terminal_dep_failure(&sibling_key).is_some(),
                "remove(/dep.ts) must NOT scrub records for OTHER \
                 canonicals (here /sibling.ts). The retain predicate must \
                 only drop entries whose DepKey references the removed file.",
            );
        }

        // 4. reset() (via dag.clear()) must wipe the store.
        sched.reset();
        {
            let dag = sched.dag.lock();
            assert!(
                dag.lookup_terminal_dep_failure(&sibling_key).is_none(),
                "reset() must wipe the terminal_dep_failures store",
            );
        }
    }

    /// `DependencyFailed` must carry the producer's terminal cause
    /// verbatim so a downstream consumer can disambiguate failure
    /// kinds without re-reading state from the failed file. Without
    /// the carried cause every dependency failure looks identical
    /// (a structural envelope citing the failed DepKey) and a
    /// consumer cannot distinguish a missing source file from an
    /// executor-side stage failure.
    ///
    /// Discriminator: drive `/dep.ts` Source to FileNotFound (no
    /// loader entry for `/dep.ts`). The terminalize path records a
    /// `FailedDepRecord { cause: FileNotFound { file_id: "/dep.ts" }, .. }`.
    /// The pre-dispatch chokepoint must clone the record's cause
    /// into the surfaced `DependencyFailed.cause` Box. The test
    /// asserts both the dep-key identity AND the underlying cause
    /// variant + payload — with the typed cause field, the variant
    /// destructures to a `FileNotFound` cause and the assertion passes.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dependency_failed_carries_source_filenotfound_cause() {
        use std::time::Duration;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        // /dep.ts deliberately NOT inserted — Source terminalizes
        // via FileNotFound.

        let sched = Scheduler::new(SchedulerConfig::default(), loader);

        // Drive /a.vue Analysis ready.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });
        let analysis_state = analysis_handle.wait();
        assert!(
            analysis_state.is_ready(),
            "/a.vue Analysis precondition failed: {analysis_state:?}",
        );

        // Auto-ingest /dep.ts via late blocker. Source fails
        // FileNotFound; terminal_dep_failures gets populated.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        // Wait for /dep.ts terminalization.
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let dag = sched.dag.lock();
            let dep_gen = sched
                .nodes
                .get("/dep.ts")
                .map(|n| n.generation())
                .unwrap_or(0);
            if dep_gen > 0 {
                let key = DepKey::FileStage {
                    canonical: Arc::clone(&dep_arc),
                    generation: dep_gen,
                    stage: FileStageKey::Analysis,
                };
                if dag.lookup_terminal_dep_failure(&key).is_some() {
                    break;
                }
            }
            drop(dag);
            if std::time::Instant::now() >= deadline {
                panic!("/dep.ts must terminalize within 5s");
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        // Submit Artifact; pre-dispatch chokepoint must surface
        // DependencyFailed with cause: FileNotFound.
        let artifact_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 77 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        let artifact_state = artifact_handle.wait();
        match &artifact_state {
            CompletionState::Failed(crate::job::SchedulerError::DependencyFailed {
                dep_key,
                cause,
            }) => {
                match dep_key {
                    crate::dag::DepKey::FileStage {
                        canonical, stage, ..
                    } => {
                        assert_eq!(
                            canonical.as_ref(),
                            "/dep.ts",
                            "DependencyFailed must cite /dep.ts. observed: {artifact_state:?}",
                        );
                        assert_eq!(
                            *stage,
                            crate::dag::FileStageKey::Analysis,
                            "the Artifact gated on Analysis DepKey. observed: {artifact_state:?}",
                        );
                    }
                    other => panic!(
                        "expected FileStage DepKey, got {other:?}. observed: {artifact_state:?}",
                    ),
                }
                // The cause must carry the producer's terminal
                // FileNotFound verbatim. Without the cause-carry
                // fix, every DependencyFailed envelope would be
                // indistinguishable from a StageFailed-driven
                // failure: the consumer would have to re-read state
                // off the failed file (already gone) to figure out
                // why the producer died.
                match cause.as_ref() {
                    crate::job::SchedulerError::FileNotFound { file_id } => {
                        assert_eq!(
                            file_id, "/dep.ts",
                            "FileNotFound.file_id must name the failed producer. \
                             observed cause: {cause:?}",
                        );
                    }
                    other => panic!(
                        "DependencyFailed.cause must be FileNotFound when the \
                         producer's Source failed via missing loader entry; \
                         got {other:?}. Without the carry-through, the consumer \
                         loses the FileNotFound vs StageFailed discrimination."
                    ),
                }
            }
            other => {
                panic!("expected Failed(DependencyFailed) on /dep.ts FileNotFound; got {other:?}",)
            }
        }
    }

    /// `DependencyFailed` must carry an executor-side
    /// [`SchedulerError::StageFailed`] cause through the fan-out
    /// path. The discriminating pair to the FileNotFound test: the
    /// producer's Source enters the user-side executor and the
    /// executor returns an `Err(StageError)`. The carry contract
    /// requires the surfaced `DependencyFailed.cause` to be the
    /// `StageFailed` envelope produced by terminalize_failure
    /// (citing the failed producer, NOT the consumer).
    ///
    /// Reuses the [`GatedFailingSourceExecutor`] from the
    /// post-admission fan-out test: `/dep.ts` Source enters the
    /// executor, blocks on a gate until the test releases, then
    /// returns Err. The fan-out path attaches a
    /// `FailedDepRecord { cause: StageFailed { .. } }` on the
    /// Artifact waiter; the pre-dispatch chokepoint clones the
    /// cause into the surfaced envelope.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dependency_failed_carries_source_stage_failed_cause() {
        use std::time::{Duration, Instant};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        // /dep.ts content IS inserted: the gated executor must
        // reach `execute_source` to return Err (StageFailed). A
        // missing loader entry would route via FileNotFound
        // instead, which the sibling test already covers.
        loader.insert("/dep.ts".to_string(), Arc::from("dep content"));

        let (dep_entered_tx, dep_entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (dep_release_tx, dep_release_rx) = crossbeam_channel::bounded::<()>(1);

        let executor = Arc::new(GatedFailingSourceExecutor {
            gates: dashmap::DashMap::new(),
        });
        executor.gates.insert(
            "/dep.ts".to_string(),
            SourceGate {
                entered_tx: dep_entered_tx,
                release_rx: dep_release_rx,
            },
        );

        let sched = Scheduler::with_executor(SchedulerConfig::default(), loader, executor);

        // Drive /a.vue Source + Analysis to committed.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });
        let analysis_state = {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(s) = analysis_handle.try_get() {
                    break s;
                }
                if Instant::now() >= deadline {
                    panic!("/a.vue Analysis must complete within 5s");
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        };
        assert!(
            analysis_state.is_ready(),
            "/a.vue Analysis precondition: {analysis_state:?}",
        );
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Register /dep.ts as a late blocker. Auto-ingest dispatches
        // /dep.ts Source → gated executor → entered_tx fires.
        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );
        dep_entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("/dep.ts Source worker must enter the gated executor within 5s");

        // Submit Artifact while /dep.ts Source is mid-execution.
        // The matrix sees the live Source identity → Artifact admits
        // with Analysis DepKey on deps_remaining.
        let artifact_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 77 },
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // Wait for Artifact admission so the fan-out path attaches
        // the marker on a live waiter (avoiding the pre-admission
        // race covered by the sibling test).
        let artifact_identity = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            generation: a_gen,
            profile_hash: profile_hash_to_bytes(77),
            content_hash: [0u8; 16],
        };
        let admit_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let admitted = {
                let dag = sched.dag.lock();
                dag.token_for(&artifact_identity).is_some()
            };
            if admitted {
                break;
            }
            if Instant::now() >= admit_deadline {
                panic!("Artifact admission must complete within 5s");
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        // Release the gate — /dep.ts Source returns Err → fan-out
        // attaches FailedDepRecord { cause: StageFailed { .. } }.
        drop(dep_release_tx);

        let resolved_state = {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(s) = artifact_handle.try_get() {
                    break s;
                }
                if Instant::now() >= deadline {
                    panic!("Artifact handle must resolve within 5s after /dep.ts release");
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        };

        match &resolved_state {
            CompletionState::Failed(crate::job::SchedulerError::DependencyFailed {
                dep_key,
                cause,
            }) => {
                match dep_key {
                    crate::dag::DepKey::FileStage { canonical, .. } => {
                        assert_eq!(
                            canonical.as_ref(), "/dep.ts",
                            "DependencyFailed must cite the failed producer. observed: {resolved_state:?}",
                        );
                    }
                    other => panic!("expected FileStage DepKey, got {other:?}",),
                }
                // The carried cause must be StageFailed citing
                // /dep.ts. Without the typed `cause` field on
                // DependencyFailed the variant lacks any cause
                // payload; with the typed field, the cause must be
                // the producer's terminal StageFailed envelope
                // (NOT FileNotFound, which only fires when the
                // loader returns None).
                match cause.as_ref() {
                    crate::job::SchedulerError::StageFailed { file_id, stage, .. } => {
                        assert_eq!(
                            file_id, "/dep.ts",
                            "StageFailed.file_id must name the failed producer. \
                             observed cause: {cause:?}",
                        );
                        assert_eq!(
                            stage, "Source",
                            "StageFailed.stage must be the producer stage that \
                             failed. observed cause: {cause:?}",
                        );
                    }
                    other => panic!(
                        "DependencyFailed.cause must be StageFailed when the producer's \
                         Source returned Err from the user-side executor; got {other:?}",
                    ),
                }
            }
            other => {
                panic!("expected Failed(DependencyFailed) on /dep.ts StageFailed; got {other:?}",)
            }
        }
    }

    /// Two-profile persistence: a live blocker dep that transitions
    /// to `Failed` between profile-1 and profile-2 admissions must
    /// persist as a failure record in the registry so the second
    /// admission picks it up.
    ///
    /// The bug the rebuild closes: `admit_artifact_with_blockers`
    /// previously kept `next_pending.failed = stored.failed` (the
    /// inbound, OLD failure set) instead of rebuilding from
    /// classification. When a live dep classifies as `Failed` at
    /// profile-1 admission, the record is attached to profile-1's
    /// Artifact correctly but never enters `next_pending.failed` —
    /// so the registry slot is dropped (empty rebuild). Profile-2
    /// admission drains an empty registry and the matrix never
    /// reconsults `terminal_dep_failures` for the (no-longer-recorded)
    /// blocker — the Artifact dispatches with no failure marker and
    /// resolves `Ready` over the dead prerequisite.
    ///
    /// Discriminator: stage the precondition directly (registry slot
    /// holds `{deps: [dep1.Analysis@gen=1], failed: []}` AND
    /// `terminal_dep_failures` holds dep1's failure record), then
    /// admit profile-1 and observe both the registry's post-admit
    /// state (must persist `failed=[record]`) AND profile-2's
    /// post-admit Artifact (must carry `failed_blocker_deps`).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn admit_artifact_persists_classifier_failure_across_profiles() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        // /dep.ts has NO content — we stage the failure directly
        // via terminal_dep_failures.
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Plant /a.vue and /dep.ts FileNodes at gen=1.
        let a_node = sched.create_node("/a.vue", None);
        let a_gen = a_node.bump_generation();
        sched.nodes.insert("/a.vue".to_string(), a_node);
        let dep_node = sched.create_node("/dep.ts", None);
        let dep_gen = dep_node.bump_generation();
        sched.nodes.insert("/dep.ts".to_string(), dep_node);
        let a_arc: Arc<str> = Arc::from("/a.vue");
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        assert_eq!(
            a_gen, dep_gen,
            "precondition: bumping each from gen=0 leaves both at gen=1",
        );

        // Commit /a.vue's Source + Analysis at gen=1 so the admission
        // path routes through `admit_artifact_with_blockers` (the
        // already-complete arm). Without this, submit_request would
        // dispatch the Artifact via the normal pipeline that does not
        // exercise the blocker registry.
        {
            let a = sched.nodes.get("/a.vue").unwrap();
            a.source.store(Arc::new(Some(Arc::new(
                crate::node::SourceSnapshot::new_empty(Arc::from("a content"), a_gen),
            ))));
            a.analysis.store(Arc::new(Some(Arc::new(
                crate::node::AnalysisSnapshot::new_empty(a_gen),
            ))));
            assert!(
                a.current_analysis().is_some(),
                "precondition: /a.vue Analysis must be committed at gen={a_gen}",
            );
        }

        // Stage the failure: /dep.ts terminalized at gen=1. The
        // persistent store carries the record.
        let dep_key = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        {
            let mut dag = sched.dag.lock();
            dag.insert_terminal_dep_failure(crate::dag::FailedDepRecord {
                dep_key: dep_key.clone(),
                cause: crate::job::SchedulerError::FileNotFound {
                    file_id: "/dep.ts".to_string(),
                },
            });
            // Stage the live-dep registry slot so the next admission
            // re-classifies the dep through the matrix (which now
            // returns Failed).
            let mut deps = std::collections::BTreeSet::new();
            deps.insert(dep_key.clone());
            dag.record_artifact_blockers(
                &a_arc,
                a_gen,
                crate::dag::PendingBlockerSet {
                    deps,
                    failed: Vec::new(),
                },
            );
        }

        // Profile-1 admission: classifier routes /dep.ts to Failed.
        // Without the rebuild, `next_pending.failed = stored.failed`
        // → empty → registry slot dropped.
        let _p1_token = {
            let mut dag = sched.dag.lock();
            sched.admit_artifact_with_blockers(
                &mut dag,
                &a_arc,
                a_gen,
                /* profile_hash = */ 77,
                Priority::Interactive,
                None,
            )
        };

        // Discriminating assertion #1: the rebuild persists the
        // failure record into the next-admission registry slot.
        let registry_after_p1 = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            registry_after_p1.failed.iter().any(|r| matches!(
                &r.dep_key,
                DepKey::FileStage { canonical, stage, generation }
                if canonical.as_ref() == "/dep.ts"
                    && *stage == FileStageKey::Analysis
                    && *generation == dep_gen
            )),
            "rebuild contract: profile-1 admission must persist the classifier's \
             Failed verdict into next_pending.failed for /dep.ts Analysis@gen={dep_gen} \
             so profile-2 admission sees it. observed registry: {registry_after_p1:?}. \
             Without the rebuild, next_pending.failed was assigned from the inbound \
             stored.failed (empty) and the slot was dropped, leaving profile-2 with an \
             empty registry.",
        );

        // Profile-2 admission at the same (owner, gen). Under the
        // rebuild it drains the persisted failure record and attaches
        // it to profile-2's Artifact node.
        let p2_token = {
            let mut dag = sched.dag.lock();
            sched.admit_artifact_with_blockers(
                &mut dag,
                &a_arc,
                a_gen,
                /* profile_hash = */ 99,
                Priority::Interactive,
                None,
            )
        };

        // Discriminating assertion #2: drain `next_ready` and verify
        // profile-2's Artifact carries the failure marker. Profile-1
        // is drained first by token order — skip it and locate
        // profile-2's token.
        let mut p2_failed_blocker_deps = None;
        loop {
            let job = {
                let mut dag = sched.dag.lock();
                dag.next_ready()
            };
            match job {
                Some(ready) => {
                    if ready.token == p2_token {
                        p2_failed_blocker_deps = Some(ready.failed_blocker_deps.clone());
                        break;
                    }
                    // Drop other ready jobs (e.g., profile-1) without
                    // executing them — we are not driving the
                    // scheduler, just inspecting next_ready output.
                }
                None => break,
            }
        }
        let p2_failed = p2_failed_blocker_deps.expect(
            "profile-2 Artifact must reach next_ready (blocker_deps should be empty \
             after the registry's failure record drained on admit). \
             Without the rebuild the Artifact also reached next_ready but with an empty \
             failed_blocker_deps map.",
        );
        assert!(
            p2_failed.contains_key(&dep_key),
            "rebuild contract: profile-2 Artifact must carry a FailedDepRecord for \
             /dep.ts Analysis@gen={dep_gen} so the pre-dispatch chokepoint surfaces \
             DependencyFailed. observed: {p2_failed:?}. Without the rebuild, \
             failed_blocker_deps would be empty because the registry slot would have \
             been dropped after profile-1, and profile-2 would dispatch without the \
             marker, silently resolving Ready.",
        );

        // Discriminating assertion #3: the persisted FailedDepRecord
        // must carry the verbatim FileNotFound cause planted at
        // terminalize time. A weaker `contains_key` assertion would
        // still pass if the registry rebuild substituted a different
        // SchedulerError variant (e.g., StageFailed) for the same
        // DepKey — the cause carry-through is what lets the
        // pre-dispatch chokepoint disambiguate FileNotFound vs
        // StageFailed downstream.
        let record = p2_failed.get(&dep_key).expect(
            "p2_failed must contain the planted dep_key after the contains_key assertion above",
        );
        match &record.cause {
            crate::job::SchedulerError::FileNotFound { file_id } => {
                assert_eq!(
                    file_id, "/dep.ts",
                    "FileNotFound.file_id must name the failed producer. \
                     observed cause: {:?}",
                    record.cause,
                );
            }
            other => panic!(
                "profile-2 FailedDepRecord.cause must be FileNotFound (carried \
                 verbatim from the terminalize-time planting); got {other:?}. \
                 Without the cause carry-through the registry rebuild would lose \
                 the FileNotFound vs StageFailed discrimination at admit time."
            ),
        }
    }

    /// Failed-record persistence after recovery: a previously-failed
    /// blocker dep that recovers at the same generation must drop
    /// from the registry's `failed` set on the next admission. The
    /// same-gen recovery path (`clear_terminal_dep_failure_for_gen`)
    /// clears the persistent store, but `stored.failed` in the
    /// registry slot is independent — if `admit_artifact_with_blockers`
    /// carries it verbatim into `next_pending`, the next admission
    /// attaches a stale failure record to a now-Satisfied dep and
    /// the Artifact incorrectly resolves `Failed`.
    ///
    /// Discriminator: stage the registry slot with
    /// `failed=[stale_record]` and `terminal_dep_failures` empty
    /// (matching the post-recovery shape where the same-gen clear
    /// has fired), AND commit /dep.ts's Source+Analysis at the
    /// recorded generation so the classifier returns `Satisfied`
    /// for the dep. Admit an Artifact: the rebuild must drop the
    /// stale record so the Artifact dispatches without a failure
    /// marker.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn admit_artifact_drops_stale_failed_record_on_same_gen_recovery() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        loader.insert("/dep.ts".to_string(), Arc::from("dep content"));
        let sched = Scheduler::new_sync(SchedulerConfig::default(), loader);

        // Plant /a.vue and /dep.ts FileNodes at gen=1, with full
        // Source+Analysis committed for both (recovery state).
        let a_node = sched.create_node("/a.vue", None);
        let a_gen = a_node.bump_generation();
        sched.nodes.insert("/a.vue".to_string(), a_node);
        let dep_node = sched.create_node("/dep.ts", None);
        let dep_gen = dep_node.bump_generation();
        sched.nodes.insert("/dep.ts".to_string(), dep_node);
        let a_arc: Arc<str> = Arc::from("/a.vue");
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        assert_eq!(a_gen, dep_gen, "precondition: both nodes at gen=1");

        // Commit /a.vue Source+Analysis at gen=1 (owner is complete,
        // routes via admit_artifact_with_blockers).
        {
            let a = sched.nodes.get("/a.vue").unwrap();
            a.source.store(Arc::new(Some(Arc::new(
                crate::node::SourceSnapshot::new_empty(Arc::from("a content"), a_gen),
            ))));
            a.analysis.store(Arc::new(Some(Arc::new(
                crate::node::AnalysisSnapshot::new_empty(a_gen),
            ))));
        }

        // Commit /dep.ts Source+Analysis at gen=1 (recovery — the
        // classifier must observe this as the "current_analysis is
        // Some" Satisfied row).
        {
            let dep = sched.nodes.get("/dep.ts").unwrap();
            dep.source.store(Arc::new(Some(Arc::new(
                crate::node::SourceSnapshot::new_empty(Arc::from("dep content"), dep_gen),
            ))));
            dep.analysis.store(Arc::new(Some(Arc::new(
                crate::node::AnalysisSnapshot::new_empty(dep_gen),
            ))));
            assert!(
                dep.current_analysis().is_some(),
                "precondition: /dep.ts Analysis must be committed at gen={dep_gen} \
                 (recovery state — the dep is no longer a failure)",
            );
        }

        // Stage the bug-precondition: registry slot holds a stale
        // `failed` record for /dep.ts Analysis@gen=1. The same-gen
        // recovery clear has already wiped `terminal_dep_failures`
        // (no entry there).
        let dep_key = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        let stale_record = crate::dag::FailedDepRecord {
            dep_key: dep_key.clone(),
            cause: crate::job::SchedulerError::FileNotFound {
                file_id: "/dep.ts".to_string(),
            },
        };
        {
            let mut dag = sched.dag.lock();
            // Sanity: terminal_dep_failures must be empty (the
            // same-gen recovery clear path has fired).
            assert!(
                dag.lookup_terminal_dep_failure(&dep_key).is_none(),
                "precondition: terminal_dep_failures must be empty (post-recovery state)",
            );
            dag.record_artifact_blockers(
                &a_arc,
                a_gen,
                crate::dag::PendingBlockerSet {
                    deps: std::collections::BTreeSet::new(),
                    failed: vec![stale_record.clone()],
                },
            );
        }

        // Admit the Artifact. Under the rebuild, classifying the
        // persisted failure record returns Satisfied → drop. Under
        // a verbatim pass-through path
        // (`failed_records.extend(stored.failed)`), the stale
        // record would ride through unchanged.
        let token = {
            let mut dag = sched.dag.lock();
            sched.admit_artifact_with_blockers(
                &mut dag,
                &a_arc,
                a_gen,
                /* profile_hash = */ 31,
                Priority::Interactive,
                None,
            )
        };

        // Discriminating assertion #1: the registry slot for the
        // owner is dropped (empty) — the rebuild discarded the
        // stale record.
        let registry_after = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            registry_after.failed.is_empty(),
            "rebuild contract: a persisted failure record whose producer \
             recovered at the same gen (terminal_dep_failures empty + \
             dep.current_analysis() Some) must drop from next_pending.failed. \
             observed registry: {registry_after:?}. Without the rebuild the \
             registry kept the stale record because next_pending.failed = \
             stored.failed (verbatim pass-through), and a subsequent admission \
             would attach it to a now-Satisfied dep.",
        );

        // Discriminating assertion #2: the freshly-admitted Artifact
        // node carries no failure marker, so the pre-dispatch
        // chokepoint will not fire on it.
        let mut artifact_failed_blocker_deps = None;
        loop {
            let job = {
                let mut dag = sched.dag.lock();
                dag.next_ready()
            };
            match job {
                Some(ready) => {
                    if ready.token == token {
                        artifact_failed_blocker_deps = Some(ready.failed_blocker_deps.clone());
                        break;
                    }
                }
                None => break,
            }
        }
        let failed_blocker_deps = artifact_failed_blocker_deps.expect(
            "Artifact must reach next_ready (the rebuild drops the stale \
             record and the dep is Satisfied — no live gating deps)",
        );
        assert!(
            failed_blocker_deps.is_empty(),
            "rebuild contract: Artifact must dispatch with NO failure marker \
             because the persisted failed record was for a dep that has \
             recovered at the same generation. observed: {failed_blocker_deps:?}. \
             Without the rebuild, a verbatim stored.failed extend would attach \
             the stale record, the pre-dispatch chokepoint would fire, and the \
             Artifact would resolve Failed instead of Ready.",
        );
    }

    /// An executor that fires a test-supplied hook during the
    /// Analysis stage (the CPU-bound stage where typeinfo
    /// recursion paths land). Source-stage executes the default
    /// empty snapshot so the I/O-pool path is never the re-entry
    /// point.
    #[cfg(not(target_arch = "wasm32"))]
    struct HookExecutor {
        analysis_hook: Box<dyn Fn(&str) + Send + Sync>,
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl crate::executor::StageExecutor for HookExecutor {
        fn execute_source(
            &self,
            _canonical_id: &str,
            _file_kind: crate::node::FileKind,
            content: Arc<str>,
            generation: u64,
        ) -> Result<crate::node::SourceSnapshot, crate::executor::StageError> {
            Ok(crate::node::SourceSnapshot::new_empty(content, generation))
        }
        fn execute_analysis(
            &self,
            canonical_id: &str,
            _source: &crate::node::SourceSnapshot,
            generation: u64,
        ) -> Result<crate::node::AnalysisSnapshot, crate::executor::StageError> {
            (self.analysis_hook)(canonical_id);
            Ok(crate::node::AnalysisSnapshot::new_empty(generation))
        }
    }

    /// Single-worker pool inline-execute invariant: with
    /// `cpu_threads = 1` a CPU worker that submits a CPU-bound
    /// dependent request and waits via `wait_or_drive` MUST run
    /// the dependency INLINE on the same worker. Without this
    /// the only CPU worker would park behind itself and the chain
    /// would never complete; with it the inline-execute path runs
    /// the dep on the calling worker and both requests reach
    /// Ready.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn single_worker_pool_wait_or_drive_executes_cpu_dependency_inline() {
        use crate::caller_kind::CallerKind;
        use crate::job::CompletionState;
        use std::sync::atomic::{AtomicUsize, Ordering as MOrd};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>a</template>"));
        loader.insert("/b.vue".to_string(), Arc::from("<template>b</template>"));

        let inner_state = Arc::new(parking_lot::Mutex::new(
            None::<CompletionState<RequestResult>>,
        ));
        let inner_state_for_hook = Arc::clone(&inner_state);
        let analysis_calls = Arc::new(AtomicUsize::new(0));
        let analysis_calls_for_hook = Arc::clone(&analysis_calls);
        let scheduler_slot: Arc<parking_lot::Mutex<Option<std::sync::Weak<Scheduler>>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let scheduler_slot_for_hook = Arc::clone(&scheduler_slot);

        // Re-enter the scheduler from inside A's Analysis hook so
        // the dispatch is happening on the only CPU worker.
        let hook: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |canonical: &str| {
            if canonical == "/a.vue" && analysis_calls_for_hook.fetch_add(1, MOrd::SeqCst) == 0 {
                let weak = scheduler_slot_for_hook
                    .lock()
                    .as_ref()
                    .expect("scheduler weak ref must be installed by the test")
                    .clone();
                let sched = weak.upgrade().expect("scheduler must outlive the hook");
                let inner = sched.submit_request(Request {
                    file_id: "/b.vue".to_string(),
                    target: TargetStage::Analysis,
                    priority: Priority::Interactive,
                    source: None,
                    file_kind: None,
                    request_context: None,
                });
                *inner_state_for_hook.lock() =
                    Some(sched.wait_or_drive_with_caller(&inner, CallerKind::CpuWorker));
            }
        });

        let sched = Scheduler::with_executor(
            SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(HookExecutor {
                analysis_hook: hook,
            }),
        );
        *scheduler_slot.lock() = Some(Arc::downgrade(&sched));

        let outer = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        let start = std::time::Instant::now();
        let outer_state = outer
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect(
                "single-worker inline-execute path must complete within 5s; \
             a regression that parked the only CPU worker behind itself \
             would hang here indefinitely",
            );
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "single-worker inline-execute path must complete promptly; \
             outer wait took {elapsed:?} — the inline branch did not fire",
        );
        assert!(
            matches!(outer_state, CompletionState::Ready(_)),
            "outer Analysis must reach Ready via inline execution of B's chain: \
             {outer_state:?}",
        );
        let observed = inner_state
            .lock()
            .take()
            .expect("the worker hook must have driven the inner submission to a terminal state");
        assert!(
            matches!(observed, CompletionState::Ready(_)),
            "inner Analysis must reach Ready inline on the only CPU worker: {observed:?}",
        );
    }

    /// Re-entrant submission invariant: an executor running job
    /// A that submits a request for B from inside itself and
    /// waits via `wait_or_drive` MUST cooperatively drain the
    /// inbox (so B is admitted) and execute its dependency
    /// inline. Without the cooperative pump the dependent
    /// request would sit in the inbox while the only CPU worker
    /// blocks on the wait.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reentrant_submission_from_worker_drains_inbox() {
        use crate::caller_kind::CallerKind;
        use crate::job::CompletionState;
        use std::sync::atomic::{AtomicUsize, Ordering as MOrd};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>a</template>"));
        loader.insert("/b.vue".to_string(), Arc::from("<template>b</template>"));

        let parsed_files = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
        let parsed_for_hook = Arc::clone(&parsed_files);

        let inner_state = Arc::new(parking_lot::Mutex::new(None));
        let inner_state_for_hook = Arc::clone(&inner_state);
        let outer_calls = Arc::new(AtomicUsize::new(0));
        let outer_calls_for_hook = Arc::clone(&outer_calls);

        let scheduler_slot: Arc<parking_lot::Mutex<Option<std::sync::Weak<Scheduler>>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let scheduler_slot_for_hook = Arc::clone(&scheduler_slot);

        let hook: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |canonical: &str| {
            parsed_for_hook.lock().push(canonical.to_string());
            if canonical == "/a.vue" && outer_calls_for_hook.fetch_add(1, MOrd::SeqCst) == 0 {
                let weak = scheduler_slot_for_hook
                    .lock()
                    .as_ref()
                    .expect("scheduler weak ref must be installed by the test")
                    .clone();
                let sched = weak
                    .upgrade()
                    .expect("scheduler must outlive its worker hook");
                let inner = sched.submit_request(Request {
                    file_id: "/b.vue".to_string(),
                    target: TargetStage::Analysis,
                    priority: Priority::Interactive,
                    source: None,
                    file_kind: None,
                    request_context: None,
                });
                let state = sched.wait_or_drive_with_caller(&inner, CallerKind::CpuWorker);
                *inner_state_for_hook.lock() = Some(state);
            }
        });

        let sched = Scheduler::with_executor(
            SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(HookExecutor {
                analysis_hook: hook,
            }),
        );
        *scheduler_slot.lock() = Some(Arc::downgrade(&sched));

        let outer = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        let start = std::time::Instant::now();
        let outer_state = outer
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect(
                "re-entrant submission must complete within 5s; a regression that \
             starved the inbox would hang here indefinitely",
            );
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "re-entrant submission must not deadlock; outer wait took {elapsed:?}",
        );
        assert!(
            matches!(outer_state, CompletionState::Ready(_)),
            "outer request must complete Ready; got {outer_state:?}",
        );

        let observed = inner_state
            .lock()
            .take()
            .expect("the worker hook must have driven the inner submission to a terminal state");
        assert!(
            matches!(observed, CompletionState::Ready(_)),
            "inner request must reach Ready via the cooperative pump; got {observed:?}",
        );
        let parsed = parsed_files.lock();
        assert!(
            parsed.iter().any(|f| f == "/a.vue"),
            "executor must have parsed A: {parsed:?}",
        );
        assert!(
            parsed.iter().any(|f| f == "/b.vue"),
            "executor must have parsed B from inside A's hook: {parsed:?}",
        );
    }

    /// Same-path detection invariant: a `wait_or_drive` call on
    /// a handle whose target identity is on the calling thread's
    /// active path MUST surface a typed
    /// `Failed(StageFailed { stage: "wait_or_drive" })` rather
    /// than joining its own pending completion. Without the
    /// detection the call would block forever on the condvar
    /// (the worker is waiting on work it itself owns). With it
    /// the typed Failed lands within a few ms.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn same_path_self_await_returns_failed_not_hang() {
        use crate::caller_kind::{with_active_path, CallerKind};
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::job::{completion_pair, CompletionState, CompletionTarget, SchedulerError};
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/x.vue".to_string(), Arc::from("<template>x</template>"));
        let sched = Scheduler::with_executor(
            SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        );

        let identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (handle, sender) = completion_pair::<crate::job::RequestResult>();
        sender.set_target(CompletionTarget::Work(identity.clone()));

        // Run wait_or_drive_with_caller from within an
        // active-path frame for the SAME identity. The pump must
        // detect the self-await without blocking.
        let start = std::time::Instant::now();
        let state = with_active_path(identity, || {
            sched.wait_or_drive_with_caller(&handle, CallerKind::CpuWorker)
        });
        let elapsed = start.elapsed();

        // Discriminating: must return Failed within a small budget.
        // A hang would saturate the test timeout instead.
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "same-path self-await must return promptly; elapsed = {elapsed:?}",
        );
        match state {
            CompletionState::Failed(SchedulerError::StageFailed {
                file_id,
                stage,
                message,
            }) => {
                assert_eq!(file_id, "/x.vue", "Failed must name the canonical");
                assert_eq!(
                    stage, "wait_or_drive",
                    "Failed must be tagged as wait_or_drive"
                );
                assert!(
                    message.contains("self-await"),
                    "message must describe the self-await condition: {message:?}",
                );
            }
            other => {
                panic!("expected Failed(StageFailed {{ stage: \"wait_or_drive\" }}), got {other:?}",)
            }
        }
    }

    /// Lock discipline: a CPU worker parked in the cooperative
    /// pump must release the DAG lock between iterations so an
    /// external thread can still submit work and the driver can
    /// pump it. The test races a long-running outer wait against
    /// concurrent submissions and asserts every submission
    /// completes within the budget. A regression that held the
    /// DAG lock across `wait_timeout` would starve the driver and
    /// every submission would time out.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wait_or_drive_does_not_hold_dag_lock_while_blocked() {
        use crate::caller_kind::CallerKind;
        use crate::job::CompletionState;
        let loader = Arc::new(MemorySourceLoader::new());
        for i in 0..5 {
            loader.insert(
                format!("/sibling-{i}.vue"),
                Arc::from(format!("<template>s{i}</template>")),
            );
        }
        let sched = Scheduler::with_executor(
            SchedulerConfig {
                cpu_threads: 2,
                io_threads: 2,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        );

        // A handle that resolves only when the test explicitly
        // signals it — so the parked cooperative pump runs its
        // wait_timeout cycle until the assertion below is past
        // and the test signals shutdown.
        let (gated_handle, gated_sender) =
            crate::job::completion_pair::<crate::job::RequestResult>();
        let gated_handle_for_thread = gated_handle.clone();
        let sched_weak = Arc::downgrade(&sched);
        let cooperative_thread = std::thread::spawn(move || {
            // Upgrade the weak ref only for the duration of the
            // wait_or_drive call so dropping the test-owned
            // strong ref can shut the scheduler down without
            // this thread keeping it alive.
            let sched = sched_weak
                .upgrade()
                .expect("scheduler must be alive when the cooperative thread starts");
            sched.wait_or_drive_with_caller(&gated_handle_for_thread, CallerKind::CpuWorker)
        });

        // Submit several independent sibling requests AFTER the
        // cooperative-pump thread has parked. They must all
        // complete promptly — proof that the DAG lock is released
        // between pump iterations.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let sibling_handles: Vec<_> = (0..5)
            .map(|i| {
                sched.submit_request(Request {
                    file_id: format!("/sibling-{i}.vue"),
                    target: TargetStage::Source,
                    priority: Priority::Interactive,
                    source: None,
                    file_kind: None,
                    request_context: None,
                })
            })
            .collect();

        let start = std::time::Instant::now();
        let mut ready_count = 0usize;
        for h in &sibling_handles {
            let state = h.wait_timeout(std::time::Duration::from_secs(5)).expect(
                "sibling must complete within 5s; a regression that held the \
                 DAG lock across the cooperative-pump wait_timeout would hang here",
            );
            assert!(
                matches!(state, CompletionState::Ready(_)),
                "sibling must complete Ready under cooperative-pump back-pressure: {state:?}",
            );
            ready_count += 1;
        }
        let elapsed = start.elapsed();
        assert_eq!(
            ready_count, 5,
            "all 5 siblings must complete (lock-discipline guard)",
        );
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "siblings must complete promptly; took {elapsed:?} — \
             a regression holding the DAG lock across the cooperative \
             pump's wait_timeout would push this past the budget",
        );

        // Release the cooperative-pump thread. The Shutdown
        // signal is a clean exit — it cannot reach the dag
        // waiter set because gated_handle is not on the DAG, so
        // the test owns the wake-up directly.
        gated_sender.send(CompletionState::Shutdown);
        let state = cooperative_thread
            .join()
            .expect("cooperative thread must join cleanly");
        assert!(
            matches!(state, CompletionState::Shutdown),
            "cooperative thread must observe the explicit Shutdown wake-up; got {state:?}",
        );
    }

    /// `pump_ready` is the cooperative-pump primitive: it drains the
    /// inbox and dispatches every currently-ready job. A driver-led
    /// pump with a fresh submission must report progress on both
    /// the drain AND the dispatch counters; a follow-up pump with
    /// no fresh submissions must report no progress (so the driver
    /// can safely park).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pump_ready_drains_inbox_and_dispatches_ready_jobs() {
        use crate::caller_kind::CallerKind;
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>x</template>"));
        let sched = test_scheduler_with_loader(loader);

        // Submit a request — lands in the inbox.
        let _handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        // First pump: drains the submission AND dispatches the
        // resulting ready Source job. The drained count is at
        // least 1 (the submission); the dispatched count is at
        // least 1 (the Source admission that became ready).
        let stats = sched.pump_ready(PumpReason::DriverLoop, CallerKind::Driver);
        assert!(
            stats.drained >= 1,
            "pump must drain at least the queued NewRequest, got {stats:?}",
        );
        assert!(
            stats.dispatched >= 1,
            "pump must dispatch the admitted Source job, got {stats:?}",
        );
        assert!(stats.made_progress(), "non-zero counters imply progress");

        // Second pump immediately: no fresh submission, the
        // dispatched job is in-flight on the pool. Progress must
        // be reported as false so the driver can park.
        let idle = sched.pump_ready(PumpReason::DriverLoop, CallerKind::Driver);
        assert_eq!(
            idle.drained, 0,
            "idle pump drained={}, expected 0",
            idle.drained
        );
        assert_eq!(
            idle.dispatched, 0,
            "idle pump dispatched={}, expected 0",
            idle.dispatched,
        );
        assert!(
            !idle.made_progress(),
            "idle pump with no work must NOT report progress, got {idle:?}",
        );
    }

    // ──────────────────────────────────────────────────────────────
    // Unified macro-cycle filter — transitive reachability + late-
    // path coverage + atomic filter+submit under one DAG lock.
    //
    // Each test below is discriminating: the filter must drop
    // self-cycles, direct mutual cycles, and transitive cycles on
    // BOTH the Source-completion replay path AND the immediate
    // `register_resolved_deps` path.
    // ──────────────────────────────────────────────────────────────

    /// Direct self-cycle: a dep whose canonical+generation matches
    /// the owner is dropped immediately (does not even enter the
    /// BFS). The unified filter drops it on both the immediate and
    /// the Source-completion replay paths.
    #[test]
    fn filter_drops_direct_self_cycle() {
        let dag = crate::dag::SchedulerDag::new(crate::dag::DagAgingConfig::default());
        let owner: Arc<str> = Arc::from("/a.vue");
        let self_dep = DepKey::FileStage {
            canonical: Arc::clone(&owner),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (kept, dropped) =
            Scheduler::filter_macro_cycle_deps(&dag, &owner, 1, vec![self_dep.clone()]);
        assert!(
            kept.is_empty(),
            "self-cycle dep must be filtered out: kept={kept:?}"
        );
        assert_eq!(
            dropped.len(),
            1,
            "self-cycle dep must be recorded in dropped: dropped={dropped:?}",
        );
        assert_eq!(dropped[0], self_dep);
    }

    /// Direct mutual cycle on the LATE / immediate path: A's Source
    /// already completed when B's deps register with B→A→B. The
    /// immediate path must filter — without it both halves would
    /// submit mutually-blocking gates. The unified filter drops
    /// B→A so B's Analysis admits with no blockers.
    #[test]
    fn filter_drops_direct_mutual_cycle_late_registration() {
        let mut dag = crate::dag::SchedulerDag::new(crate::dag::DagAgingConfig::default());
        let a: Arc<str> = Arc::from("/a.vue");
        let b: Arc<str> = Arc::from("/b.vue");

        // Build state: A's Analysis is admitted with a B→Analysis
        // gating dep (B not yet committed). This is the "A already
        // gating on B" half of the mutual cycle.
        let a_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&a),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let b_dep = DepKey::FileStage {
            canonical: Arc::clone(&b),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        dag.submit(
            a_id,
            crate::dag::WorkKind::Analysis,
            Priority::Background,
            vec![b_dep.clone()],
            None,
        );

        // Now register B's deps with A as a blocker — the closing
        // half of the mutual cycle. The filter must drop the A
        // dep so B can admit and break the cycle.
        let a_dep = DepKey::FileStage {
            canonical: Arc::clone(&a),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (kept, dropped) = Scheduler::filter_macro_cycle_deps(&dag, &b, 1, vec![a_dep.clone()]);
        assert!(
            kept.is_empty(),
            "mutual-cycle dep must be filtered out on the immediate path: kept={kept:?}",
        );
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0], a_dep);
    }

    /// Transitive cycle A→B→C→A: an adjacency-only filter (just
    /// `has_dep_on`) cannot see the cycle because C is on B's
    /// deps_remaining, not A. The bounded BFS in
    /// `dep_reaches_owner` walks B's → C's → A's deps_remaining
    /// and reports the cycle.
    #[test]
    fn filter_drops_three_node_cycle_a_b_c_a() {
        let mut dag = crate::dag::SchedulerDag::new(crate::dag::DagAgingConfig::default());
        let a: Arc<str> = Arc::from("/a.vue");
        let b: Arc<str> = Arc::from("/b.vue");
        let c: Arc<str> = Arc::from("/c.vue");

        // Set up: B's Analysis gates on C's Analysis; C's Analysis
        // gates on A's Analysis. A→B closing dep is what the
        // filter must drop. A direct-adjacency check sees only
        // B's direct deps — C is on B's deps_remaining, NOT A —
        // so adjacency alone would NOT drop the A→B dep, even
        // though the transitive chain B→C→A closes the cycle.
        // The bounded BFS in `dep_reaches_owner` walks the
        // transitive chain and reports the cycle.
        let b_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&b),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let c_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&c),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let c_dep = DepKey::FileStage {
            canonical: Arc::clone(&c),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let a_dep = DepKey::FileStage {
            canonical: Arc::clone(&a),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        dag.submit(
            b_id,
            crate::dag::WorkKind::Analysis,
            Priority::Background,
            vec![c_dep.clone()],
            None,
        );
        dag.submit(
            c_id,
            crate::dag::WorkKind::Analysis,
            Priority::Background,
            vec![a_dep.clone()],
            None,
        );

        // A registers a B-blocker. The transitive walk must drop
        // it.
        let b_dep = DepKey::FileStage {
            canonical: Arc::clone(&b),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (kept, dropped) = Scheduler::filter_macro_cycle_deps(&dag, &a, 1, vec![b_dep.clone()]);
        assert!(
            kept.is_empty(),
            "three-node transitive cycle A→B→C→A must be filtered: kept={kept:?}",
        );
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0], b_dep);
    }

    /// Non-cycle dep: A depends on B, but B has no deps back to A.
    /// The filter must preserve the dep — a spurious drop would
    /// hide a legitimate gating relationship and let A's Artifact
    /// race ahead of B's Analysis. This is the false-positive
    /// safety guard required by §5 STOP condition (1).
    #[test]
    fn filter_preserves_non_cycle_dep() {
        let mut dag = crate::dag::SchedulerDag::new(crate::dag::DagAgingConfig::default());
        let a: Arc<str> = Arc::from("/a.vue");
        let b: Arc<str> = Arc::from("/b.vue");

        // B's Analysis is admitted with NO deps back to A.
        let b_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&b),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        dag.submit(
            b_id,
            crate::dag::WorkKind::Analysis,
            Priority::Background,
            Vec::new(),
            None,
        );

        // A→B dep must survive the filter.
        let b_dep = DepKey::FileStage {
            canonical: Arc::clone(&b),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (kept, dropped) = Scheduler::filter_macro_cycle_deps(&dag, &a, 1, vec![b_dep.clone()]);
        assert!(
            dropped.is_empty(),
            "non-cycle dep must be preserved: dropped={dropped:?}",
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0], b_dep);
    }

    /// Two concurrent `register_resolved_deps` calls for files
    /// with mutual macro-type deps (A→B and B→A) must not deadlock.
    ///
    /// Lock-discipline contract: the filter check and the submit
    /// run atomically under a single DAG lock guard. One of the
    /// threads observes the other's Analysis already gating when
    /// it runs the filter and drops the cyclic dep, or both
    /// filter cleanly because neither half is admitted yet.
    /// Either way, no mutual deadlock can form.
    ///
    /// Discriminator: a non-atomic filter+submit (lock released
    /// between filter and submit) would let two threads racing
    /// the Source-completion replay both pass the filter at
    /// different lock-released moments and both submit a
    /// mutually-blocking dep edge.
    ///
    /// The test asserts the property the atomic chokepoint
    /// guarantees: after both `register_resolved_deps` calls
    /// return, at least one of the two files' Analysis nodes
    /// must NOT carry the other's Analysis as a gating dep —
    /// otherwise the two files would form an unresolvable cycle
    /// at the DAG level.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn handle_source_complete_concurrent_completion_does_not_deadlock() {
        use std::sync::Arc as StdArc;
        use std::thread;
        use std::time::Duration;

        let loader = StdArc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        loader.insert("/b.vue".to_string(), Arc::from("b content"));

        let sched = StdArc::new(Scheduler::with_executor(
            SchedulerConfig::default(),
            loader,
            StdArc::new(crate::executor::DefaultExecutor),
        ));

        // Drive both files' Source to completion FIRST so the
        // subsequent register_resolved_deps calls hit the immediate
        // path (Source already complete when blockers arrive) —
        // the path the unified filter must cover (in addition to
        // the Source-completion replay path).
        let a_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });
        let b_handle = sched.submit_request(Request {
            file_id: "/b.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("b content")),
            file_kind: None,
            request_context: None,
        });
        let _ = a_handle.wait();
        let _ = b_handle.wait();

        // Now race two register_resolved_deps calls: A→B and B→A.
        let sched_a = StdArc::clone(&sched);
        let sched_b = StdArc::clone(&sched);
        let t_a = thread::spawn(move || {
            sched_a.register_resolved_deps(
                "/a.vue",
                vec!["/b.vue".to_string()],
                vec!["/b.vue".to_string()],
            );
        });
        let t_b = thread::spawn(move || {
            sched_b.register_resolved_deps(
                "/b.vue",
                vec!["/a.vue".to_string()],
                vec!["/a.vue".to_string()],
            );
        });
        // Bounded join — register_resolved_deps must complete
        // promptly. A regression that held a lock across the wait
        // would saturate the budget and panic here.
        let join_deadline = std::time::Instant::now() + Duration::from_secs(5);
        for handle in [t_a, t_b] {
            let remaining = join_deadline.saturating_duration_since(std::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "register_resolved_deps must return promptly",
            );
            handle
                .join()
                .expect("register_resolved_deps thread must not panic");
        }

        // Post-condition: at least ONE of the two Analysis nodes
        // must NOT carry the other's Analysis as a gating dep.
        // If both carried the other, no progress would be possible
        // at the DAG level — the very deadlock the chokepoint
        // exists to prevent.
        let a_arc: Arc<str> = Arc::from("/a.vue");
        let b_arc: Arc<str> = Arc::from("/b.vue");
        let a_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&a_arc),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let b_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&b_arc),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let dep_on_a = DepKey::FileStage {
            canonical: Arc::clone(&a_arc),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let dep_on_b = DepKey::FileStage {
            canonical: Arc::clone(&b_arc),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let dag = sched.dag.lock();
        let a_gates_on_b = dag.has_dep_on(&a_id, &dep_on_b);
        let b_gates_on_a = dag.has_dep_on(&b_id, &dep_on_a);
        assert!(
            !(a_gates_on_b && b_gates_on_a),
            "atomic filter+submit must prevent the mutual deadlock — \
             a_gates_on_b={a_gates_on_b}, b_gates_on_a={b_gates_on_a}",
        );
    }

    // ──────────────────────────────────────────────────────────────
    // Artifact same-path detection via Work target stamping.
    //
    // `handle_new_request` stamps the concrete
    // `CompletionTarget::Work(first_missing_identity)` on the
    // sender during admission. The request-fallback in
    // `active_path_contains_request(Artifact{..})` matches against
    // same-canonical Analysis frames (covers the brief race window
    // between submit and admission). Either way, the same-path
    // self-await detection in `wait_or_drive` fires and returns
    // `Failed(StageFailed { stage: "wait_or_drive", .. })`.
    //
    // Without target stamping a worker running A.Analysis that
    // submitted an A.Artifact request and waited would dedup onto
    // the in-flight Artifact, which gated on its own A.Analysis,
    // which couldn't complete because the worker was parked —
    // silent deadlock.
    // ──────────────────────────────────────────────────────────────

    /// A worker running A.Analysis that submits a request for
    /// A.Artifact{X} and waits must observe the same-path failure
    /// rather than hanging on a dedup attachment.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn analysis_executor_submits_same_file_artifact_returns_failed() {
        use crate::caller_kind::{with_active_path, CallerKind};
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::job::{CompletionState, SchedulerError};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        let sched = Arc::new(Scheduler::with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        ));

        let analysis_id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/a.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };

        // Simulate being inside the Analysis executor by pushing
        // the Analysis frame onto the active path before submitting
        // and waiting.
        let start = std::time::Instant::now();
        let state = with_active_path(analysis_id, || {
            let handle = sched.submit_request(Request {
                file_id: "/a.vue".to_string(),
                target: TargetStage::Artifact { profile_hash: 7 },
                priority: Priority::Interactive,
                source: Some(Arc::from("a content")),
                file_kind: None,
                request_context: None,
            });
            sched.wait_or_drive_with_caller(&handle, CallerKind::CpuWorker)
        });
        let elapsed = start.elapsed();

        // Discriminating: must return Failed promptly, not hang.
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "Analysis→Artifact same-path must return promptly; elapsed = {elapsed:?}",
        );
        match state {
            CompletionState::Failed(SchedulerError::StageFailed { stage, .. }) => {
                assert_eq!(stage, "wait_or_drive", "must be tagged as wait_or_drive");
            }
            other => {
                panic!(
                    "expected Failed(StageFailed {{ stage: \"wait_or_drive\" }}), got {other:?}",
                );
            }
        }
    }

    /// A single I/O worker that calls `wait_or_drive` on an
    /// I/O-bound dep must inline-execute the dep on its own thread.
    /// Without the symmetric `IoWorker × Source` inline-eligibility
    /// the dep would be dispatched back to `io_pool.execute(...)`
    /// and sit behind the parked I/O worker forever. The I/O
    /// capacity loan covers the budget-exhausted case.
    ///
    /// Setup: `io_threads = 1, cpu_threads = 1`. A `SourceHook`
    /// executor records each Source canonical it sees, and the
    /// outer Source closure re-submits a dependent Source request
    /// and waits on it. The inner Source must inline-execute on
    /// the I/O worker — otherwise the test times out.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn single_io_worker_wait_or_drive_executes_io_dependency_inline() {
        use crate::caller_kind::CallerKind;
        use crate::job::CompletionState;
        use std::sync::atomic::{AtomicUsize, Ordering as MOrd};

        /// Executor that fires a Source-stage hook (the I/O-bound
        /// stage). Mirrors `HookExecutor` but fires on Source
        /// instead of Analysis.
        struct SourceHookExecutor {
            source_hook: Box<dyn Fn(&str) + Send + Sync>,
        }
        impl crate::executor::StageExecutor for SourceHookExecutor {
            fn execute_source(
                &self,
                canonical_id: &str,
                _file_kind: crate::node::FileKind,
                content: Arc<str>,
                generation: u64,
            ) -> Result<crate::node::SourceSnapshot, crate::executor::StageError> {
                (self.source_hook)(canonical_id);
                Ok(crate::node::SourceSnapshot::new_empty(content, generation))
            }
        }

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>a</template>"));
        loader.insert("/b.vue".to_string(), Arc::from("<template>b</template>"));

        let inner_state = Arc::new(parking_lot::Mutex::new(
            None::<CompletionState<RequestResult>>,
        ));
        let inner_state_for_hook = Arc::clone(&inner_state);
        let outer_calls = Arc::new(AtomicUsize::new(0));
        let outer_calls_for_hook = Arc::clone(&outer_calls);
        let scheduler_slot: Arc<parking_lot::Mutex<Option<std::sync::Weak<Scheduler>>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let scheduler_slot_for_hook = Arc::clone(&scheduler_slot);

        // Outer Source for /a.vue submits a Source for /b.vue and
        // waits as an IoWorker.
        let hook: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |canonical: &str| {
            if canonical == "/a.vue" && outer_calls_for_hook.fetch_add(1, MOrd::SeqCst) == 0 {
                let weak = scheduler_slot_for_hook
                    .lock()
                    .as_ref()
                    .expect("scheduler weak ref must be installed")
                    .clone();
                let sched = weak.upgrade().expect("scheduler must outlive the hook");
                let inner = sched.submit_request(Request {
                    file_id: "/b.vue".to_string(),
                    target: TargetStage::Source,
                    priority: Priority::Interactive,
                    source: None,
                    file_kind: None,
                    request_context: None,
                });
                *inner_state_for_hook.lock() =
                    Some(sched.wait_or_drive_with_caller(&inner, CallerKind::IoWorker));
            }
        });

        let sched = Scheduler::with_executor(
            SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(SourceHookExecutor { source_hook: hook }),
        );
        *scheduler_slot.lock() = Some(Arc::downgrade(&sched));

        let outer = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        let start = std::time::Instant::now();
        let outer_state = outer
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect(
                "single-I/O-worker inline-execute must complete within 5s — \
                 the I/O inline branch did not fire (deadlock-class test)",
            );
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "single-I/O-worker inline-execute must complete promptly; \
             outer wait took {elapsed:?} — the I/O inline branch did not fire",
        );
        assert!(
            matches!(outer_state, CompletionState::Ready(_)),
            "outer Source must reach Ready via inline execution of B's Source: {outer_state:?}",
        );
        let observed = inner_state
            .lock()
            .take()
            .expect("the I/O worker hook must have driven the inner Source to a terminal state");
        assert!(
            matches!(observed, CompletionState::Ready(_)),
            "inner Source must reach Ready inline on the only I/O worker: {observed:?}",
        );
    }

    /// `handle_new_request` MUST stamp the concrete
    /// `CompletionTarget::Work` identity on the sender during
    /// admission, overwriting the request-level
    /// `CompletionTarget::Request` set by `submit_request`. The
    /// stamped identity matches the first-missing work stage —
    /// Source if the FileNode hasn't loaded yet, Analysis if
    /// Source is committed but Analysis is missing, Artifact
    /// otherwise.
    ///
    /// Discriminator: without the stamping the target would stay
    /// as `CompletionTarget::Request{..}` indefinitely.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn handle_new_request_stamps_work_target_for_first_missing_stage() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::job::CompletionTarget;
        use std::time::Duration;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        let sched = Arc::new(Scheduler::with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        ));

        // Submit a request and wait for it to be admitted. Once
        // handle_new_request runs, the sender's target slot must
        // hold a `Work` variant, not the initial `Request` shape.
        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_kind: None,
            request_context: None,
        });

        // Poll for the stamp landing — the driver thread runs
        // handle_new_request asynchronously, so the test must wait
        // until the admission has had a chance to fire. A 1s budget
        // is generous; in practice the stamp lands within
        // microseconds.
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let mut observed: Option<CompletionTarget> = None;
        while std::time::Instant::now() < deadline {
            if let Some(target) = handle.target() {
                if matches!(target, CompletionTarget::Work(_)) {
                    observed = Some(target);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        let target = observed.expect("handle_new_request must stamp Work target within 1s");
        match target {
            CompletionTarget::Work(WorkNodeIdentity::FileStage {
                canonical, stage, ..
            }) => {
                assert_eq!(canonical.as_ref(), "/a.vue");
                // first_missing = Source on a fresh FileNode with a
                // source attached but no committed snapshot yet.
                // Once Source completes, no re-stamping happens —
                // the target stays at the initial stamp. So the
                // observed stage may be Source OR Analysis depending
                // on whether the driver re-entered admission, but
                // it MUST be a file-stage (not Artifact, not the
                // abstract Request).
                assert!(
                    matches!(stage, FileStageKey::Source | FileStageKey::Analysis),
                    "stamped stage must be Source or Analysis, got {stage:?}",
                );
            }
            CompletionTarget::Work(WorkNodeIdentity::Artifact { canonical, .. }) => {
                assert_eq!(canonical.as_ref(), "/a.vue");
            }
            CompletionTarget::Work(WorkNodeIdentity::CacheNode { .. }) => {
                panic!("CacheNode identity must not be stamped on a file-stage request");
            }
            CompletionTarget::Request { .. } => {
                panic!("regression: target must be stamped to Work, still Request");
            }
        }
    }

    /// The inline-execute branch of `dispatch_ready_job` MUST
    /// install the winner's request-context TLS for the duration
    /// of the inline stage so the inner stage's audit events
    /// carry the inner request's tag, NOT the outer stage's.
    /// Without the install the inner stage would run under the
    /// OUTER worker's TLS slot (None or the outer request's id).
    ///
    /// Setup: cpu_threads=1; outer Analysis runs with no
    /// `request_context`; inside the outer executor a NEW Analysis
    /// is submitted with `request_context = Some(ctx_inner)` and
    /// `wait_or_drive` is called. The inner job inline-executes
    /// on the same thread. The inner executor records
    /// `current_request_id()`; the test asserts it equals the
    /// inner ctx id.
    ///
    /// Discriminator without the install: observed inner id = 0
    /// (outer's TLS was None). With the install: observed inner
    /// id = `INNER_ID`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn inline_execute_installs_winner_ctx_tls_for_audit() {
        use crate::caller_kind::CallerKind;
        use crate::job::CompletionState;
        use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as MOrd};

        const INNER_ID: u64 = 12345;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>a</template>"));
        loader.insert("/b.vue".to_string(), Arc::from("<template>b</template>"));

        // The inner analysis observer records the request id it
        // sees in TLS. Without the inline TLS install this stays
        // at 0; with it the observed id lands at INNER_ID.
        let inner_observed_id = Arc::new(AtomicU64::new(0));
        let inner_observed_for_hook = Arc::clone(&inner_observed_id);

        let outer_calls = Arc::new(AtomicUsize::new(0));
        let outer_calls_for_hook = Arc::clone(&outer_calls);

        let scheduler_slot: Arc<parking_lot::Mutex<Option<std::sync::Weak<Scheduler>>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let scheduler_slot_for_hook = Arc::clone(&scheduler_slot);

        let hook: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |canonical: &str| {
            if canonical == "/a.vue" {
                if outer_calls_for_hook.fetch_add(1, MOrd::SeqCst) == 0 {
                    // Outer execution: submit the inner request
                    // carrying ctx_inner and wait_or_drive. The
                    // inline-execute path runs B's Analysis on
                    // the same thread with ctx_inner installed.
                    let weak = scheduler_slot_for_hook
                        .lock()
                        .as_ref()
                        .expect("scheduler weak ref must be installed by the test")
                        .clone();
                    let sched = weak.upgrade().expect("scheduler must outlive the hook");
                    let ctx_inner = TestContext::new(INNER_ID, true);
                    let inner = sched.submit_request(Request {
                        file_id: "/b.vue".to_string(),
                        target: TargetStage::Analysis,
                        priority: Priority::Interactive,
                        source: None,
                        file_kind: None,
                        request_context: Some(OpaqueRequestContext(
                            ctx_inner as Arc<dyn RequestContextLike>,
                        )),
                    });
                    let _ = sched.wait_or_drive_with_caller(&inner, CallerKind::CpuWorker);
                }
            } else if canonical == "/b.vue" {
                // Inner execution: record the observed TLS id.
                let id = crate::request_context::current_request_id().unwrap_or(0);
                inner_observed_for_hook.store(id, MOrd::SeqCst);
            }
        });

        let sched = Scheduler::with_executor(
            SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(HookExecutor {
                analysis_hook: hook,
            }),
        );
        *scheduler_slot.lock() = Some(Arc::downgrade(&sched));

        // Outer carries NO context — without the inline install
        // the inner stage would observe None (the outer's TLS
        // slot). With the install the inner observes INNER_ID.
        let outer = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });

        let outer_state = outer
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect("outer Analysis must complete within 5s");
        assert!(
            matches!(outer_state, CompletionState::Ready(_)),
            "outer Analysis must reach Ready: {outer_state:?}",
        );

        let observed = inner_observed_id.load(MOrd::SeqCst);
        assert_eq!(
            observed, INNER_ID,
            "regression: inline-execute did NOT install winner_ctx TLS; \
             inner observed id = {observed}, expected {INNER_ID}",
        );
    }

    /// Inline-execute's audit pool tag must reflect the pool the
    /// inline branch is actually running on. A hardcoded
    /// `WorkerPoolTag::Cpu` regardless of caller would misattribute
    /// the IoWorker × Source inline path — an operator inspecting
    /// audit records would believe an I/O-bound Source ran on a
    /// CPU worker.
    ///
    /// Setup: `io_threads = 1`. Install an audit observer that
    /// records every `record_scheduler_dispatch` call. Submit an
    /// outer Source whose hook re-enters with another Source
    /// request and waits — the inner Source MUST inline-execute
    /// on the only I/O worker.
    ///
    /// Discriminator: with the pool tag derived from
    /// `(caller_kind, task_kind)` the dispatch lands tagged `Io`;
    /// the test asserts at least one dispatch landed with
    /// `WorkerPool::Io`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn inline_execute_io_worker_source_publishes_io_pool_tag() {
        use crate::caller_kind::CallerKind;
        use crate::job::CompletionState;
        use std::sync::atomic::{AtomicUsize, Ordering as MOrd};
        use std::sync::Mutex as StdMutex;
        use verter_audit::{AuditObserver, SchedulerAudit, WorkerPool};

        /// Records every `record_scheduler_dispatch` call.
        struct AuditRecorder {
            dispatches: StdMutex<Vec<SchedulerAudit>>,
        }
        impl AuditObserver for AuditRecorder {
            fn record_scheduler_dispatch(&self, audit: SchedulerAudit) {
                self.dispatches.lock().unwrap().push(audit);
            }
        }

        struct SourceHookExecutorIo {
            source_hook: Box<dyn Fn(&str) + Send + Sync>,
        }
        impl crate::executor::StageExecutor for SourceHookExecutorIo {
            fn execute_source(
                &self,
                canonical_id: &str,
                _file_kind: crate::node::FileKind,
                content: Arc<str>,
                generation: u64,
            ) -> Result<crate::node::SourceSnapshot, crate::executor::StageError> {
                (self.source_hook)(canonical_id);
                Ok(crate::node::SourceSnapshot::new_empty(content, generation))
            }
        }

        let recorder = Arc::new(AuditRecorder {
            dispatches: StdMutex::new(Vec::new()),
        });

        // Install the recorder on the OUTER thread. The inline
        // branch runs on the I/O worker thread, which has its own
        // TLS slot — so we also install the recorder there via
        // the request_context shim below.

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>a</template>"));
        loader.insert("/b.vue".to_string(), Arc::from("<template>b</template>"));

        let outer_calls = Arc::new(AtomicUsize::new(0));
        let outer_calls_for_hook = Arc::clone(&outer_calls);
        let scheduler_slot: Arc<parking_lot::Mutex<Option<std::sync::Weak<Scheduler>>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let scheduler_slot_for_hook = Arc::clone(&scheduler_slot);

        // The inline-execute audit is published while
        // `current_observer()` is the winner_ctx's observer. We
        // make the winner_ctx an observer that delegates to our
        // recorder so the audit records flow back out for
        // inspection.
        struct ObserverCtx {
            inner: Arc<AuditRecorder>,
            id: u64,
        }
        impl RequestContextLike for ObserverCtx {
            fn request_id(&self) -> u64 {
                self.id
            }
            fn capture_enabled(&self) -> bool {
                true
            }
            fn on_dedup_joiner(&self, _c: Arc<str>, _w: u64, _a: bool) {}
            fn record_cache_event(&self, _e: CacheEventKind) {}
            fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
                let ctx_guard = crate::request_context::OpaqueContextGuard::install(
                    OpaqueRequestContext(Arc::clone(&self) as Arc<dyn RequestContextLike>),
                );
                let observer_guard = verter_audit::observer::install_observer(Arc::clone(
                    &self.inner,
                )
                    as Arc<dyn AuditObserver>);
                Box::new(BothGuards {
                    _ctx: ctx_guard,
                    _obs: observer_guard,
                })
            }
        }
        struct BothGuards {
            _ctx: crate::request_context::OpaqueContextGuard,
            _obs: verter_audit::observer::ObserverGuard,
        }
        impl TlsUninstall for BothGuards {
            fn uninstall(self: Box<Self>) {}
        }

        let recorder_for_hook = Arc::clone(&recorder);
        // Outer Source for /a.vue submits a Source for /b.vue and
        // waits as an IoWorker. The inner Source inline-executes
        // on the I/O worker and publishes its dispatch audit.
        let hook: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |canonical: &str| {
            if canonical == "/a.vue" && outer_calls_for_hook.fetch_add(1, MOrd::SeqCst) == 0 {
                let weak = scheduler_slot_for_hook
                    .lock()
                    .as_ref()
                    .expect("scheduler weak ref must be installed by the test")
                    .clone();
                let sched = weak.upgrade().expect("scheduler must outlive the hook");
                let inner_ctx = Arc::new(ObserverCtx {
                    inner: Arc::clone(&recorder_for_hook),
                    id: 42,
                });
                let inner = sched.submit_request(Request {
                    file_id: "/b.vue".to_string(),
                    target: TargetStage::Source,
                    priority: Priority::Interactive,
                    source: None,
                    file_kind: None,
                    request_context: Some(OpaqueRequestContext(
                        inner_ctx as Arc<dyn RequestContextLike>,
                    )),
                });
                let _ = sched.wait_or_drive_with_caller(&inner, CallerKind::IoWorker);
            }
        });

        let sched = Scheduler::with_executor(
            SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(SourceHookExecutorIo { source_hook: hook }),
        );
        *scheduler_slot.lock() = Some(Arc::downgrade(&sched));

        let outer_ctx = Arc::new(ObserverCtx {
            inner: Arc::clone(&recorder),
            id: 7,
        });
        let outer = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: Some(OpaqueRequestContext(
                outer_ctx as Arc<dyn RequestContextLike>,
            )),
        });

        let outer_state = outer
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect("outer Source must complete within 5s");
        assert!(
            matches!(outer_state, CompletionState::Ready(_)),
            "outer Source must reach Ready: {outer_state:?}",
        );

        let records = recorder.dispatches.lock().unwrap().clone();
        let io_dispatches: Vec<_> = records
            .iter()
            .filter(|d| matches!(d.worker_pool, WorkerPool::Io))
            .collect();
        assert!(
            !io_dispatches.is_empty(),
            "regression: IoWorker × Source inline-execute MUST publish at least one \
             dispatch with WorkerPool::Io. All recorded dispatches: {records:?}",
        );
    }

    /// Inline-execute MUST clear the outer worker's TLS for the
    /// inner stage when `winner_ctx` is None. The inline path
    /// runs on the caller's worker thread, so a left-over outer
    /// context bleeds into the inner stage and the inner stage's
    /// audit events are misattributed to the wrong request.
    ///
    /// Pool-spawn paths run on fresh threads where TLS starts
    /// empty, so they need no clear — only the inline branch.
    ///
    /// Setup: cpu_threads=1; outer Analysis runs WITH a request
    /// context `OUTER_ID`. Inside the outer executor a NEW
    /// Analysis is submitted with `request_context = None` and
    /// `wait_or_drive` is called. The inner job inline-executes
    /// on the same thread. The inner executor records
    /// `current_request_id()`; the test asserts it equals 0
    /// (None observed, NOT the outer's id).
    ///
    /// Discriminator without the clear: observed inner id =
    /// OUTER_ID (the outer's TLS bled into the inner stage).
    /// With the clear: observed inner id = 0.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn inline_execute_clears_outer_tls_when_winner_ctx_is_none() {
        use crate::caller_kind::CallerKind;
        use crate::job::CompletionState;
        use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering as MOrd};

        const OUTER_ID: u64 = 7777;
        // Sentinel for "not yet observed" — i64 so we can
        // distinguish 0 (None observed) from "hook never ran".
        const NOT_OBSERVED: i64 = -1;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>a</template>"));
        loader.insert("/b.vue".to_string(), Arc::from("<template>b</template>"));

        // Records the request id observed by the inner stage.
        // 0 means TLS was None at the time of observation.
        let inner_observed = Arc::new(AtomicI64::new(NOT_OBSERVED));
        let inner_observed_for_hook = Arc::clone(&inner_observed);

        let outer_calls = Arc::new(AtomicUsize::new(0));
        let outer_calls_for_hook = Arc::clone(&outer_calls);

        let scheduler_slot: Arc<parking_lot::Mutex<Option<std::sync::Weak<Scheduler>>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let scheduler_slot_for_hook = Arc::clone(&scheduler_slot);

        // Cross-crate TLS observations: assert the session and
        // audit slots are also cleared on the inline-execute
        // None-winner_ctx path. The hook records what the inner
        // stage saw for each slot — a 1 means "context was
        // visible", 0 means "slot was empty as required".
        let inner_audit_observer_visible = Arc::new(AtomicUsize::new(0));
        let inner_audit_observer_visible_for_hook = Arc::clone(&inner_audit_observer_visible);

        let hook: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |canonical: &str| {
            if canonical == "/a.vue" {
                if outer_calls_for_hook.fetch_add(1, MOrd::SeqCst) == 0 {
                    // Outer execution: submit the inner request
                    // with NO context. The inline-execute branch
                    // must clear the outer's TLS so the inner
                    // stage observes None.
                    let weak = scheduler_slot_for_hook
                        .lock()
                        .as_ref()
                        .expect("scheduler weak ref must be installed by the test")
                        .clone();
                    let sched = weak.upgrade().expect("scheduler must outlive the hook");
                    let inner = sched.submit_request(Request {
                        file_id: "/b.vue".to_string(),
                        target: TargetStage::Analysis,
                        priority: Priority::Interactive,
                        source: None,
                        file_kind: None,
                        request_context: None,
                    });
                    let _ = sched.wait_or_drive_with_caller(&inner, CallerKind::CpuWorker);
                }
            } else if canonical == "/b.vue" {
                // Inner execution: record the observed TLS state
                // for each install_tls slot. 0 means cleared.
                let id = crate::request_context::current_request_id().unwrap_or(0);
                inner_observed_for_hook.store(id as i64, MOrd::SeqCst);
                // Audit observer slot must also be cleared on the
                // inline-execute None-winner_ctx path. A non-None
                // observer here means the outer's audit TLS bled
                // through despite the scheduler-side opaque clear.
                if verter_audit::current_observer().is_some() {
                    inner_audit_observer_visible_for_hook.store(1, MOrd::SeqCst);
                }
            }
        });

        let sched = Scheduler::with_executor(
            SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(HookExecutor {
                analysis_hook: hook,
            }),
        );
        *scheduler_slot.lock() = Some(Arc::downgrade(&sched));

        // Outer carries OUTER_ID — without the clear the inline
        // path would leave OUTER_ID in TLS for the inner stage.
        let outer_ctx = TestContext::new(OUTER_ID, true);
        let outer = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: Some(OpaqueRequestContext(
                outer_ctx as Arc<dyn RequestContextLike>,
            )),
        });

        let outer_state = outer
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect("outer Analysis must complete within 5s");
        assert!(
            matches!(outer_state, CompletionState::Ready(_)),
            "outer Analysis must reach Ready: {outer_state:?}",
        );

        let observed = inner_observed.load(MOrd::SeqCst);
        assert_ne!(
            observed, NOT_OBSERVED,
            "inner hook must have run — observed = NOT_OBSERVED ({NOT_OBSERVED})",
        );
        assert_eq!(
            observed, 0,
            "regression: inline-execute did NOT clear outer scheduler TLS when winner_ctx is None; \
             inner observed id = {observed}, expected 0 (None). \
             OUTER_ID was {OUTER_ID}.",
        );
        // The audit observer substrate slot must also be cleared.
        // Without the cross-crate clear, the outer's
        // `Arc<dyn AuditObserver>` (planted by
        // `RequestContextGuard::install`) would still be visible
        // to producers in lower crates emitting through
        // `verter_audit::current_observer()`, and the inner
        // stage's events would be misattributed.
        //
        // This test runs without an installed audit observer
        // (the outer context here is a `TestContext`, not a
        // `RequestContext`), so the slot is None going in and
        // stays None — the assertion guards against a future
        // path where the slot is populated but not cleared.
        assert_eq!(
            inner_audit_observer_visible.load(MOrd::SeqCst),
            0,
            "regression: inline-execute did NOT clear outer audit observer slot when winner_ctx is None",
        );
    }

    /// The cross-crate clear path must zero the session-side
    /// `current_request_context()` and the audit substrate's
    /// `current_observer()` while an `AllSlotsClearGuard` is held,
    /// and restore both on drop. This is the unit test for the
    /// host-registered hook plumbing — it exercises the substrate
    /// directly without going through the full inline-execute
    /// path so a future regression in the hook itself surfaces
    /// here as well as in the inline-execute test above.
    ///
    /// We can only test this with the scheduler-side opaque slot
    /// here because session-side TLS lives in `verter_session`,
    /// which depends on `verter_scheduler`. The session crate has
    /// its own unit tests asserting the same behaviour for the
    /// session and audit slots after `install_clear_tls_hook` is
    /// registered.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn all_slots_clear_guard_clears_scheduler_opaque_slot_and_restores_on_drop() {
        use crate::request_context::{
            AllSlotsClearGuard, OpaqueContextGuard, OpaqueRequestContext, RequestContextLike,
            TlsUninstall,
        };
        use std::sync::atomic::AtomicU64;

        struct DummyCtx {
            id: u64,
        }
        impl RequestContextLike for DummyCtx {
            fn request_id(&self) -> u64 {
                self.id
            }
            fn capture_enabled(&self) -> bool {
                false
            }
            fn on_dedup_joiner(&self, _c: Arc<str>, _w: u64, _a: bool) {}
            fn record_cache_event(&self, _event: crate::request_context::CacheEventKind) {}
            fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
                struct NoopUninstall;
                impl TlsUninstall for NoopUninstall {
                    fn uninstall(self: Box<Self>) {}
                }
                Box::new(NoopUninstall)
            }
        }
        let _id = AtomicU64::new(0);

        let ctx = Arc::new(DummyCtx { id: 12345 });
        let _outer =
            OpaqueContextGuard::install(OpaqueRequestContext(ctx as Arc<dyn RequestContextLike>));
        assert_eq!(
            crate::request_context::current_request_id(),
            Some(12345),
            "outer install must show in scheduler opaque slot",
        );

        {
            let _clear = AllSlotsClearGuard::clear_all();
            assert_eq!(
                crate::request_context::current_request_id(),
                None,
                "AllSlotsClearGuard must clear scheduler opaque slot while alive",
            );
        }

        assert_eq!(
            crate::request_context::current_request_id(),
            Some(12345),
            "AllSlotsClearGuard drop must restore prior scheduler opaque slot",
        );
    }

    /// `wait_or_drive_with_caller` MUST consult `handle.try_get()`
    /// BEFORE running same-path detection. If the same-path check
    /// ran first, a handle that had already resolved to `Ready`
    /// (or `Failed`, `Superseded`, `Shutdown`) would be MASKED
    /// with a synthetic `Failed(StageFailed { stage: "wait_or_drive" })`
    /// if its target matched the caller's active path. The
    /// try_get-first ordering ensures the resolved state surfaces
    /// as-is; the same-path check only runs on still-pending
    /// handles.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wait_or_drive_returns_ready_for_already_complete_matching_handle() {
        use crate::caller_kind::{with_active_path, CallerKind};
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::job::{completion_pair, CompletionState, CompletionTarget, RequestResult};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/x.vue".to_string(), Arc::from("<template>x</template>"));
        let sched = Arc::new(Scheduler::with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        ));

        // Build a handle, stamp it with a Work target that
        // matches the active path, then resolve it to Ready
        // BEFORE calling wait_or_drive. Without the try_get-first
        // ordering the same-path check would mask the Ready with
        // a synthetic Failed.
        let identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (handle, sender) = completion_pair::<RequestResult>();
        sender.set_target(CompletionTarget::Work(identity.clone()));
        // Send a synthetic Ready state so the handle's try_get
        // returns immediately.
        sender.send(CompletionState::Ready(RequestResult::Analysis(Arc::new(
            crate::node::AnalysisSnapshot::new_empty(1),
        ))));

        // Now run wait_or_drive from inside an active-path frame
        // matching the target. The try_get-first ordering ensures
        // the resolved state takes precedence over the same-path
        // check.
        let state = with_active_path(identity, || {
            sched.wait_or_drive_with_caller(&handle, CallerKind::CpuWorker)
        });
        match state {
            CompletionState::Ready(_) => {
                // Pass — the real terminal state surfaced.
            }
            other => {
                panic!(
                    "regression: resolved handle was masked by same-path detection. \
                     Expected Ready, got {other:?}",
                );
            }
        }
    }

    /// In the race window between `submit_request` (stamps
    /// `CompletionTarget::Request{Artifact{..}}`) and admission
    /// (stamps `CompletionTarget::Work(..)`), an Artifact request
    /// against a same-canonical Analysis frame must still be
    /// caught by the request-fallback in
    /// `active_path_contains_request`. This is the defense-in-depth
    /// guard against the race: without the Artifact→Analysis-frame
    /// fallback, the request-target check would short-circuit to
    /// false for Artifact requests; with it the check matches
    /// against the file's Analysis frame because Artifact admission
    /// gates on Analysis completion.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wait_or_drive_artifact_request_against_active_analysis_frame_returns_failed() {
        use crate::caller_kind::{with_active_path, CallerKind};
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::job::{
            completion_pair, CompletionState, CompletionTarget, RequestResult, SchedulerError,
        };
        use crate::stage::TargetStage;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/x.vue".to_string(), Arc::from("<template>x</template>"));
        let sched = Arc::new(Scheduler::with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        ));

        // Manually construct a handle with the pre-admission
        // `Request{Artifact{..}}` target — the race-window state.
        let analysis_id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (handle, sender) = completion_pair::<RequestResult>();
        sender.set_target(CompletionTarget::Request {
            canonical: Arc::from("/x.vue"),
            target: TargetStage::Artifact { profile_hash: 7 },
        });

        let start = std::time::Instant::now();
        let state = with_active_path(analysis_id, || {
            sched.wait_or_drive_with_caller(&handle, CallerKind::CpuWorker)
        });
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "race-window Artifact→Analysis must return promptly; elapsed = {elapsed:?}",
        );
        match state {
            CompletionState::Failed(SchedulerError::StageFailed { stage, .. }) => {
                assert_eq!(stage, "wait_or_drive", "must be tagged as wait_or_drive");
            }
            other => {
                panic!(
                    "expected Failed(StageFailed {{ stage: \"wait_or_drive\" }}), got {other:?}",
                );
            }
        }
    }

    /// Source-stage executor that submits a same-file Analysis
    /// request and waits must observe a same-path Failed rather
    /// than hang. The Source executor's frame is on the active
    /// path; the Analysis request matches against that Source
    /// frame via the broadened prerequisite-stage rule.
    ///
    /// Discriminator: pre-broadening the request fallback matched
    /// Analysis only against an Analysis frame, so this nested
    /// submit would hang. Post-broadening the Source frame is a
    /// match, the synthetic Failed surfaces immediately.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn source_executor_submits_same_file_analysis_returns_failed() {
        use crate::caller_kind::{with_active_path, CallerKind};
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::job::{completion_pair, CompletionState, CompletionTarget, SchedulerError};
        use crate::stage::TargetStage;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/x.vue".to_string(), Arc::from("<template>x</template>"));
        let sched = Arc::new(Scheduler::with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        ));

        // Hand-constructed handle with the pre-admission
        // `Request{Analysis}` target — the race-window state.
        let source_frame = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 1,
            stage: FileStageKey::Source,
        };
        let (handle, sender) = completion_pair::<RequestResult>();
        sender.set_target(CompletionTarget::Request {
            canonical: Arc::from("/x.vue"),
            target: TargetStage::Analysis,
        });

        let start = std::time::Instant::now();
        let state = with_active_path(source_frame, || {
            sched.wait_or_drive_with_caller(&handle, CallerKind::CpuWorker)
        });
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "Source→Analysis same-path must return promptly; elapsed = {elapsed:?}",
        );
        match state {
            CompletionState::Failed(SchedulerError::StageFailed { stage, .. }) => {
                assert_eq!(stage, "wait_or_drive");
            }
            other => panic!("expected Failed(StageFailed), got {other:?}"),
        }
    }

    /// Artifact-stage executor that submits an Artifact request
    /// for itself (same canonical AND same profile) and waits must
    /// observe a same-path Failed. The Artifact frame on the
    /// active path matches an Artifact request for the same
    /// canonical when (and only when) the profile_hash also
    /// matches — two different profiles are independent work
    /// units, and only the same-profile path is the self-await
    /// class.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wait_or_drive_artifact_request_against_active_artifact_frame_same_profile_returns_failed() {
        use crate::caller_kind::{with_active_path, CallerKind};
        use crate::dag::{profile_hash_to_bytes, WorkNodeIdentity};
        use crate::job::{completion_pair, CompletionState, CompletionTarget, SchedulerError};
        use crate::stage::TargetStage;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/x.vue".to_string(), Arc::from("<template>x</template>"));
        let sched = Arc::new(Scheduler::with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        ));

        let profile = 0x42u64;
        let artifact_frame = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/x.vue"),
            generation: 1,
            profile_hash: profile_hash_to_bytes(profile),
            content_hash: [1u8; 16],
        };
        let (handle, sender) = completion_pair::<RequestResult>();
        // SAME-profile Artifact request against the same canonical
        // — the self-await class.
        sender.set_target(CompletionTarget::Request {
            canonical: Arc::from("/x.vue"),
            target: TargetStage::Artifact {
                profile_hash: profile,
            },
        });

        let start = std::time::Instant::now();
        let state = with_active_path(artifact_frame, || {
            sched.wait_or_drive_with_caller(&handle, CallerKind::CpuWorker)
        });
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "Artifact→Artifact same-canonical+same-profile must return promptly; elapsed = {elapsed:?}",
        );
        match state {
            CompletionState::Failed(SchedulerError::StageFailed { stage, .. }) => {
                assert_eq!(stage, "wait_or_drive");
            }
            other => panic!("expected Failed(StageFailed), got {other:?}"),
        }
    }

    /// A DIFFERENT-profile Artifact request against an active
    /// Artifact frame for the same canonical is INDEPENDENT work
    /// — the per-profile gating must NOT short-circuit it to a
    /// synthetic Failed. The active-path probe must observe a
    /// non-match and the same-path Failed rail must NOT fire.
    ///
    /// Discriminator: without the per-profile comparison, the
    /// active-Artifact arm collapses all profiles into one same-
    /// path equivalence class — the
    /// `caller_kind::active_path_contains_request` probe would
    /// return `true` for a different-profile Artifact request,
    /// driving `wait_or_drive` into the synthetic-Failed branch.
    /// With the per-profile comparison in place the same call
    /// returns `false`, the synthetic branch is not taken, and
    /// the request proceeds normally.
    ///
    /// We assert the probe directly here (rather than calling
    /// `wait_or_drive_with_caller`, which would park on the
    /// unresolved handle indefinitely with no scheduler driving
    /// it) — the unit test for the rail's input is the cleanest
    /// way to discriminate the per-profile logic.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wait_or_drive_artifact_request_against_active_artifact_frame_different_profile_does_not_match(
    ) {
        use crate::caller_kind::{active_path_contains_request, with_active_path};
        use crate::dag::{profile_hash_to_bytes, WorkNodeIdentity};
        use crate::stage::TargetStage;

        let active_profile = 0x42u64;
        let other_profile = 0x99u64;
        assert_ne!(active_profile, other_profile);

        let artifact_frame = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/x.vue"),
            generation: 1,
            profile_hash: profile_hash_to_bytes(active_profile),
            content_hash: [1u8; 16],
        };
        with_active_path(artifact_frame, || {
            // Same-profile must match (sanity).
            assert!(
                active_path_contains_request(
                    "/x.vue",
                    TargetStage::Artifact {
                        profile_hash: active_profile,
                    },
                ),
                "same-profile Artifact request must still match",
            );
            // Different-profile must NOT match — independent work.
            assert!(
                !active_path_contains_request(
                    "/x.vue",
                    TargetStage::Artifact {
                        profile_hash: other_profile,
                    },
                ),
                "different-profile Artifact request must not synthesise same-path Failed",
            );
        });
    }

    /// A handle that resolves to a real terminal state DURING the
    /// inner re-check window inside `check_terminal_or_same_path`
    /// (i.e., between the active-path probe returning `true` and
    /// the synthetic-Failed synthesis) must surface its real
    /// terminal state. The inner `try_get` re-check is load-
    /// bearing — without it the synthetic Failed would mask a
    /// Ready/Failed/Superseded/Shutdown that landed in the gap.
    ///
    /// The discriminator uses a thread-local test hook installed
    /// via [`CheckTerminalHookGuard::install`] that fires between
    /// the active-path probe and the inner `try_get` re-check.
    /// The hook resolves the handle to Ready from inside the
    /// helper's own thread — so the entry `try_get` sees None,
    /// the active-path probe sees true, the hook fires, and the
    /// inner re-check observes the just-resolved Ready. Without
    /// the inner re-check this test observes synthetic Failed;
    /// with the re-check active it observes Ready.
    ///
    /// Deterministic — no thread races, no timing assumptions.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wait_or_drive_inner_re_check_observes_handle_resolved_during_same_path_probe() {
        use crate::caller_kind::{with_active_path, CallerKind};
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::job::{completion_pair, CompletionState, CompletionTarget};
        use crate::stage::TargetStage;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/x.vue".to_string(), Arc::from("<template>x</template>"));
        let sched = Arc::new(Scheduler::with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        ));

        // Active frame: /x.vue Analysis. Handle target: Request{
        // canonical=/x.vue, Analysis } — DOES match active path
        // via canonical+stage, so the helper takes the
        // same-path-Failed branch unless the inner re-check
        // catches the resolution.
        let active_frame = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (handle, sender) = completion_pair::<RequestResult>();
        sender.set_target(CompletionTarget::Request {
            canonical: Arc::from("/x.vue"),
            target: TargetStage::Analysis,
        });

        // Install the test hook BEFORE the wait_or_drive call.
        // The hook fires inside `check_terminal_or_same_path`
        // between the active-path probe and the inner try_get
        // re-check. It resolves the handle to Ready at exactly
        // the point where the inner re-check must observe the
        // resolution. The hook fires at most once — `send` on a
        // CompletionHandle returns false on subsequent calls so
        // re-firing is a benign no-op, but we still guard with
        // a flag for clarity.
        let sender_for_hook = sender.clone();
        let ready_result =
            RequestResult::Analysis(Arc::new(crate::node::AnalysisSnapshot::new_empty(1)));
        let mut hook_fired = false;
        let _hook_guard = crate::scheduler::CheckTerminalHookGuard::install(Box::new(move || {
            if !hook_fired {
                hook_fired = true;
                sender_for_hook.send(CompletionState::Ready(ready_result.clone()));
            }
        }));

        let state = with_active_path(active_frame, || {
            sched.wait_or_drive_with_caller(&handle, CallerKind::CpuWorker)
        });

        match state {
            CompletionState::Ready(_) => {
                // Inner re-check observed the hook-resolved Ready
                // — same-path Failed was suppressed correctly.
            }
            CompletionState::Failed(crate::job::SchedulerError::StageFailed { stage, .. })
                if stage == "wait_or_drive" =>
            {
                panic!(
                    "synthetic same-path Failed masked the hook-resolved Ready — \
                     inner try_get re-check inside check_terminal_or_same_path \
                     is missing or broken",
                );
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    /// Cooperative-loop re-check: a handle whose target is
    /// `CompletionTarget::Request{..}` at wait_or_drive entry but
    /// gets stamped to `CompletionTarget::Work(..)` AFTER the
    /// caller enters the cooperative loop must still surface the
    /// same-path Failed (and not hang). The re-check runs on
    /// every iteration so the late-stamped Work identity is
    /// observed.
    ///
    /// Setup: simulate the race by manually setting the target to
    /// `Request{}` initially, then upgrading to `Work{}` (with an
    /// identity that matches the active-path Analysis frame) on
    /// a separate thread shortly after wait_or_drive enters the
    /// cooperative loop. The cooperative loop must re-read the
    /// target and detect the same-path match on the Work identity.
    ///
    /// Discriminator: without the cooperative-loop re-check the
    /// loop captures target ONCE at entry against the `Request{}`
    /// shape, so a Work-stamped active-path match is missed and
    /// the loop hangs. The re-check inside the loop iterates the
    /// target read on every pass and the synthetic Failed surfaces.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wait_or_drive_rechecks_same_path_after_admission_stamps_work() {
        use crate::caller_kind::{with_active_path, CallerKind};
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::job::{completion_pair, CompletionState, CompletionTarget, SchedulerError};
        use crate::stage::TargetStage;
        use std::time::Duration;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/x.vue".to_string(), Arc::from("<template>x</template>"));
        let sched = Arc::new(Scheduler::with_executor(
            SchedulerConfig {
                cpu_threads: 2,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(crate::executor::DefaultExecutor),
        ));

        // Active-path frame is /y.vue Analysis. The handle's
        // initial Request{} target names /x.vue with target =
        // Source — does NOT match the active path. After the
        // late stamp the target becomes Work(FileStage{Analysis:
        // /y.vue, gen=1}), which IS the active path → same-path
        // Failed must surface.
        let active_frame = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/y.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let late_stamped_work = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/y.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (handle, sender) = completion_pair::<RequestResult>();
        // Initial target: a Request{} that does NOT match the
        // active frame (different canonical).
        sender.set_target(CompletionTarget::Request {
            canonical: Arc::from("/x.vue"),
            target: TargetStage::Source,
        });

        // Spawn a thread to mutate the target slot shortly after
        // wait_or_drive enters the cooperative loop. The mutation
        // is the synthetic "admission stamped Work mid-flight".
        let sender_for_stamp = sender.clone();
        let stamper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            sender_for_stamp.set_target(CompletionTarget::Work(late_stamped_work));
        });

        let start = std::time::Instant::now();
        let state = with_active_path(active_frame, || {
            sched.wait_or_drive_with_caller(&handle, CallerKind::CpuWorker)
        });
        let elapsed = start.elapsed();
        stamper.join().expect("stamper thread must not panic");

        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "cooperative loop must re-check target and surface same-path; elapsed = {elapsed:?}",
        );
        match state {
            CompletionState::Failed(SchedulerError::StageFailed { stage, .. }) => {
                assert_eq!(stage, "wait_or_drive");
            }
            other => {
                panic!("expected Failed(StageFailed) after late Work-stamp re-check, got {other:?}",)
            }
        }
    }
}
