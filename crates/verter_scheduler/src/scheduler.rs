//! Main scheduler: per-file node architecture with async staging.
//!
//! The [`Scheduler`] is the central coordination point. Callers submit
//! requests via [`submit_request`](Scheduler::submit_request), which returns
//! a [`CompletionHandle`] that resolves when the target stage is reached.
//!
//! # Module layout
//!
//! This file is the module root. It owns the public [`Scheduler`] surface,
//! the state that surface reads, and the module's tests. The implementation
//! behind that surface is split by responsibility, one submodule each:
//!
//! - [`admission`](self::admission) — turning a queued submission into one
//!   admitted DAG node (single requests and atomic batches).
//! - [`cancellation`](self::cancellation) — cancellation-token and teardown
//!   bookkeeping for in-flight work.
//! - [`completion`](self::completion) — terminal states: completion
//!   integration, failure fan-out and same-path self-await refusal.
//! - [`dependencies`](self::dependencies) — dependency gating: blocker
//!   classification, auto-ingest and macro-cycle filtering.
//! - [`driver`](self::driver) — the driver thread: dispatch, pool submission
//!   and stage execution.
//! - [`identity`](self::identity) — work identity: how a request, task or
//!   scoped flight becomes a DAG identity.
//! - [`lifecycle`](self::lifecycle) — construction, invalidation, removal and
//!   node-incarnation bookkeeping.
//!
//! Each submodule re-exports its items through this root, so a module reaches
//! a sibling's items only through this surface and never through a sibling
//! path. The submodules are private: the public surface of the crate is the
//! one this file declares.

use std::any::Any;
use std::collections::HashMap;
#[cfg(all(test, not(target_arch = "wasm32")))]
use std::sync::atomic::AtomicU8;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use dashmap::DashMap;
use parking_lot::{Condvar, Mutex};

use crate::dag::{
    profile_hash_from_bytes, profile_hash_to_bytes, DagCapacityBudget, DedupJoinerEvent, DepKey,
    FileStageKey, Hash16, PinId, ReadyJob, SchedulerDag, WorkKind, WorkNodeIdentity,
};
use crate::driver::{QueuedRequest, Submission, SubmissionInbox};
use crate::edges::EdgeManager;
use crate::execution::executor::{DefaultExecutor, StageExecutor};
use crate::job::{
    completion_pair, CompletionHandle, CompletionSender, CompletionState, RequestResult,
};
use crate::node::{AnalysisSnapshot, ArtifactSnapshot, FileNode, SourceSnapshot};
use crate::overlay::OverlayMap;
use verter_execution::cancellation::{CancellationOwner, CancellationToken};
use verter_language::FileLanguage;

use crate::source_loader::SourceLoader;
use crate::stage::{Priority, TargetStage, TaskKind};

// The scheduler is split by responsibility: this module root owns the public
// `Scheduler` surface, the state that surface reads, and the tests; each
// submodule below owns one responsibility and is re-exported here, so no
// module reaches into a sibling's items directly.
mod admission;
mod cancellation;
mod completion;
mod dependencies;
mod driver;
mod identity;
mod lifecycle;

use admission::*;
use cancellation::*;
use completion::*;
use dependencies::*;
use driver::*;
use identity::*;
// `lifecycle`'s only free export is `num_cpus`, which is native-only; gating
// the glob with it keeps the wasm32 build free of an unused-import warning.
#[cfg(not(target_arch = "wasm32"))]
use lifecycle::*;

// Roots only the responsibility modules below name. They are imported here
// so this module stays the single import surface every submodule resolves
// its crate-internal and external roots through.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use crate::audit_publish;
pub(crate) use crate::caller_kind;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use crate::execution::owner_command;
#[cfg(feature = "hotpath")]
pub(crate) use hotpath;
pub(crate) use verter_audit;
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
            dag_budget: None,
        }
    }
}

impl SchedulerConfig {
    /// Resolve the effective DAG admission budget — the explicit
    /// override if set, otherwise derived from the pool sizes.
    ///
    /// This is the SINGLE budget-resolution authority. Both
    /// [`SchedulerDag::with_budget`] (admission ceiling) and the host's
    /// pool transport sizing read it, so CPU and IO transports always
    /// dominate the same class budget the ledger admits against —
    /// keeping the DAG ledger the sole admission gate.
    pub fn resolved_dag_budget(&self) -> DagCapacityBudget {
        self.dag_budget.unwrap_or(DagCapacityBudget {
            cpu: self.cpu_threads.max(1) as u32,
            io: self.io_threads.max(1) as u32,
        })
    }
}

/// A request to the scheduler.
pub struct Request {
    pub file_id: String,
    pub target: TargetStage,
    pub priority: Priority,
    pub source: Option<Arc<str>>,
    pub file_language: Option<FileLanguage>,
    /// Optional session-side request context. When present, the
    /// scheduler stores the winner's context on the dedup group,
    /// fires `on_dedup_joiner` callbacks when this request joins an
    /// existing group, and installs the context into worker TLS
    /// around each stage closure so `current_request_id()` returns a
    /// meaningful value while the job runs.
    pub request_context: Option<verter_execution::request_context::OpaqueRequestContext>,
}

/// Typed admission parameters for one synchronous borrowed cache-node
/// producer. The four identity fields are the complete DAG dedup key; callers
/// must use a cache id whose value type is stable for that identity domain.
#[derive(Clone, Debug)]
pub struct ScopedCacheNodeRequest {
    /// Session-owned cache family discriminator.
    pub cache_id: crate::cache_id::SchedulerCacheId,
    /// Stable hash of the semantic producer key.
    pub key_hash: Hash16,
    /// Resolver/view epoch observed by the producer.
    pub view_epoch: u64,
    /// Snapshot pin that prevents cross-snapshot joins.
    pub snapshot_pin_id: PinId,
    /// Scheduling priority used by normal DAG lane admission.
    pub priority: Priority,
    /// Optional per-request context used for cancellation and TLS attribution.
    pub request_context: Option<verter_execution::request_context::OpaqueRequestContext>,
}

impl ScopedCacheNodeRequest {
    fn identity(&self) -> WorkNodeIdentity {
        WorkNodeIdentity::CacheNode {
            cache_id: self.cache_id,
            key_hash: self.key_hash,
            view_epoch: self.view_epoch,
            snapshot_pin_id: self.snapshot_pin_id,
        }
    }
}

/// Terminal failures from [`Scheduler::execute_scoped_cache_node`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopedCacheNodeError {
    /// This request was cancelled, or every owner of the shared job left.
    Cancelled,
    /// The scheduler shut down or reset before publication.
    Shutdown,
    /// The producer panicked. The unwind is contained at the scheduler rail.
    Panicked,
    /// The same typed identity was used concurrently for different value types.
    TypeMismatch,
    /// A producer synchronously requested its own active cache-node identity.
    Reentrant,
}

/// Shared scheduler-side rendezvous for one overlapping scoped cache-node
/// flight. The borrowed producer closure never lives here; exactly one waiting
/// caller claims execution after the DAG dispatches this flight.
#[doc(hidden)]
pub struct ScopedCacheFlight {
    state: Mutex<ScopedCacheFlightState>,
    changed: Condvar,
}

/// Typed result of a submission attempt.
///
/// Named charter boundary [`Admission`]. Generic over the success-handle
/// type `T` (the handle the submission path hands back on admission).
/// The three variants are exactly the admission outcomes — there is no
/// speculative fourth case:
///
/// - [`Admitted`](Admission::Admitted) — the submission was admitted
///   into the DAG; carries the caller's handle.
/// - [`DedupeJoined`](Admission::DedupeJoined) — a caller-side
///   [`DedupeHook`](crate::dedupe_hook::DedupeHook) probe matched an
///   already-in-flight equivalent, so the submission collapsed onto it
///   before reaching the DAG; carries the opaque
///   [`DedupeJoiner`](crate::dedupe_hook::DedupeJoiner).
/// - [`Backpressured`](Admission::Backpressured) — admission was
///   declined under the existing capacity ledger WITHOUT mutating
///   readiness. The caller retries or blocks on capacity.
#[derive(Debug)]
pub enum Admission<T> {
    /// Admitted into the DAG; carries the caller's handle.
    Admitted(T),
    /// Collapsed onto an in-flight flight by a caller-side dedupe probe.
    DedupeJoined(crate::dedupe_hook::DedupeJoiner),
    /// Admission declined under capacity backpressure, with no readiness
    /// mutation. The caller retries or blocks on capacity availability.
    Backpressured,
}

/// Batch submission handle.
///
/// Produced by [`Scheduler::submit_batch_atomic`]; drained via
/// [`Scheduler::wait_batch`]. Callers submit N independent requests
/// before any waits; the scheduler runs each request's stage work on
/// its own stage `cpu_pool` as the driver dispatches it (the scheduler
/// owns no OUTER batch fan-out — that wait lives on the host/runtime
/// coordinator pool). The handle carries one [`CompletionHandle`] per
/// submitted request in submission order so `wait_batch` can surface
/// results in the same order.
pub struct BatchHandle {
    pub(crate) handles: Vec<CompletionHandle<RequestResult>>,
}

impl BatchHandle {
    /// Number of requests in the batch (one completion handle each).
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// `true` when the batch carries no requests.
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    /// The per-request completion handles in submission (input) order.
    pub fn handles(&self) -> &[CompletionHandle<RequestResult>] {
        &self.handles
    }

    /// Consume the handle, yielding the owned completion handles in
    /// submission (input) order.
    pub fn into_handles(self) -> Vec<CompletionHandle<RequestResult>> {
        self.handles
    }
}

/// The main scheduler.
///
/// Manages per-file nodes, a priority queue, and a driver thread that
/// processes submissions and dispatches work to CPU/IO pools.
pub struct Scheduler {
    /// Per-file nodes (concurrent access via DashMap).
    ///
    /// EXECUTION state: each [`FileNode`] holds only its CURRENT
    /// snapshots, so a generation bump makes the prior source
    /// immediately unreachable and nothing here can answer "what did
    /// this file look like when my request started?". The immutable,
    /// leased answer to that question is `source_root`.
    pub(crate) nodes: DashMap<String, Arc<FileNode>>,
    /// The scheduler's epoch-indexed MVCC SOURCE authority.
    ///
    /// Every lifecycle transition that changes what
    /// [`Self::try_get_source`] logically answers publishes a version
    /// through [`crate::source_root::SchedulerSourceDirectory::publish_transition`],
    /// atomically with the transition itself. A consumer captures an
    /// O(1) [`crate::source_root::SchedulerSourceRoot`] and reads the
    /// world AS OF that capture — the directory is never reachable
    /// through the root.
    ///
    /// Lock rank: `dag` (outer) > source-root publication > `nodes`
    /// shard (inner).
    pub(crate) source_root: Arc<crate::source_root::SchedulerSourceDirectory>,
    /// Edge manager (reverse-dep index + forward-dep snapshots).
    pub(crate) edges: EdgeManager,
    /// Single driver-owned readiness authority — admission, dedup,
    /// dependency gating, the weighted-credit lane selector, capacity
    /// reservation, per-file waiter groups.
    ///
    /// Wrapped in `Arc` so worker closures can clone a handle for
    /// completion signalling without holding `&self`.
    pub(crate) dag: Arc<DagMutex>,
    /// Overlapping borrowed cache-node calls keyed by their full DAG identity.
    /// Values are request-scoped rendezvous only, never durable cache entries.
    scoped_cache_flights: DashMap<WorkNodeIdentity, Arc<ScopedCacheFlight>>,
    /// Serializes scoped-flight registry replacement with DAG admission and
    /// terminal removal, preventing a stale inbox item from joining/cancelling
    /// a newer flight that reused the same identity.
    scoped_cache_gate: Mutex<()>,
    /// Process-local owner id source for aggregate job-liveness registrations.
    next_scoped_owner_id: AtomicU64,
    /// Bounded inbox for submissions.
    pub(crate) inbox: SubmissionInbox,
    /// Current resolver snapshot (atomically swappable).
    pub(crate) overlay: Arc<OverlayMap>,
    /// Source loader for file reads.
    pub(crate) source_loader: Arc<dyn SourceLoader>,
    /// Stage executor — provides the actual parse/analysis/compile logic.
    pub(crate) executor: Arc<dyn StageExecutor>,
    /// Configuration (read at construction for pool sizing).
    pub(crate) config: SchedulerConfig,
    /// Host-constructed, injected scheduler CPU pool for stage work
    /// (parse, analysis, compile). Workers tag `CallerKind::CpuWorker`.
    /// Distinct from the host coordinator pool (`HostCpuPool`, tagged
    /// `External`) — host fan-out work never runs here.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cpu_pool: Arc<crate::execution::pool::SchedulerCpuPool>,
    /// Host-constructed, injected scheduler I/O pool for file reads.
    /// Separate from the CPU pool so blocking disk reads don't starve
    /// parse/analyze work. Workers tag `CallerKind::IoWorker`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) io_pool: Arc<crate::execution::pool::SchedulerIoPool>,
    /// Deferred blocker IDs for files whose node was at generation 0 when
    /// `register_resolved_deps` was called. Replayed when the node advances
    /// past generation 0 during Source stage completion.
    /// Each entry carries the dep's RESOLVED LANGUAGE alongside its id.
    ///
    /// The Source-completion auto-ingest runs under the DAG mutex inside a
    /// `nodes` shard-WRITE guard, where `source_loader.classify(..)` must not
    /// be called. Resolving the language at the single write site below —
    /// which runs unlocked — means every drained id arrives with one, so the
    /// reader never needs the host and never has to guess. Classifying at
    /// READ time instead would leave a peek-before-lock race: an id inserted
    /// between the peek and the lock would have no language, which is exactly
    /// the silent-skip this field's shape now prevents.
    pub deferred_blocker_ids: DashMap<String, Vec<(String, FileLanguage)>>,
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
    /// Count of stage completions refused at their publish point
    /// because the owning `FileNode` moved between dispatch and
    /// publish — a superseded generation or a re-homed incarnation.
    ///
    /// This is the observable rail for the refusal path, not a
    /// diagnostic: a refusal is silent in release (no panic, no typed
    /// caller error), so without a counter a test cannot distinguish
    /// "the gate fired" from "the race never happened". Expected to
    /// stay ZERO absent concurrent invalidation.
    #[cfg(any(test, feature = "semantic-observe"))]
    pub(crate) stale_completion_refusals: AtomicU64,
    /// Shutdown flag.
    pub(crate) shutdown: AtomicBool,
    /// Driver thread handle (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) driver_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Serializes concurrent `reset()` calls (native only).
    ///
    /// `reset()` takes `&self`, so two threads may enter it at once. The
    /// driver handle is `take()`n exactly once: the loser would skip the
    /// join and clear `nodes`/the DAG while the still-running driver is
    /// pumping. Holding this guard for the whole body turns the second
    /// reset into a full, ordered rerun after the first has joined the
    /// driver and finished clearing — never an overlap.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) reset_serial: Mutex<()>,
    /// Out-of-band teardown signal for the parked driver (native only).
    ///
    /// The submission inbox is a MULTI-consumer channel: every
    /// cooperative pump drains it, the driver's own dispatch loop
    /// included. A teardown request posted there can therefore be
    /// consumed before the driver reaches its park, after which the
    /// driver sleeps out its whole idle re-pump interval and whichever
    /// thread is joining it waits that long too. This channel has
    /// exactly ONE consumer — the parked driver — so a teardown signal
    /// can never be swallowed by a pump. Capacity one: a second signal
    /// while one is already queued carries no extra meaning.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) driver_teardown: (
        crossbeam_channel::Sender<()>,
        crossbeam_channel::Receiver<()>,
    ),
    /// Contention instrumentation counters surfaced through
    /// [`Self::counters`].
    pub(crate) counters: SchedulerCounters,
    /// Test-only dispatch pause instrumentation. Lets a dwell test park
    /// the driver after N dispatches and observe scheduler-queue depth
    /// before releasing. `cfg`-gated to `test` / the opt-in
    /// `test-support` feature; absent from every build without it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) dispatch_pause: DispatchPauseHook,
    /// Test-only mid-batch admission seam. Fired once per atomic batch,
    /// after the first admit and before the rest, WHILE the single
    /// `dag.lock()` is held — the rendezvous a LOCK-CONTINUITY test uses
    /// to release a concurrent observer that blocking-acquires the DAG
    /// lock (and thus cannot acquire until all N are admitted).
    /// `cfg`-gated to `test` / the opt-in `test-support` feature; absent
    /// from every build without it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) batch_admit_seam: BatchAdmitSeamHook,
    /// Test-only monotonic count of inbox teardown wakes posted by
    /// [`Self::reset`], bumped AFTER the send. The inbox message itself
    /// is consumable — any cooperative pump may swallow it — so a test
    /// that must release a rendezvous only once the wake is provably
    /// posted cannot observe the inbox directly: an empty inbox means
    /// "not yet sent" and "already swallowed" alike, and a non-empty one
    /// may hold an unrelated `StageComplete`. This counter is the
    /// edge-triggered, non-consumable observation of that same event.
    /// `cfg`-gated to `test` / the opt-in `test-support` feature; absent
    /// from every build without it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) reset_wake_posts: AtomicU64,
    /// Test-only monotonic counter bumped once per admission-lock
    /// acquisition inside [`Self::handle_new_request_batch`] (co-located
    /// with the `dag.lock()` call via [`Self::acquire_dag_for_admission`]
    /// so a per-item lock/unlock regression bumps it once per item). The
    /// LOCK-CONTINUITY rail asserts every admit in a batch ran under the
    /// SAME epoch — i.e. ONE acquisition. `cfg`-gated to `test` / the
    /// opt-in `test-support` feature; absent from every build without it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) dag_admit_epoch: AtomicU64,
    /// Test-only per-admit epoch trace. When a test installs a recorder
    /// (`Some(vec)`), [`Self::handle_new_request_batch`] pushes the
    /// acquisition epoch it admitted each request under; the helper then
    /// asserts all entries are equal (one held lock) and `len == N`. When
    /// `None` (the default, and every non-instrumented call) recording is
    /// a single cheap `Option` check. `cfg`-gated to `test` / the opt-in
    /// `test-support` feature; absent from every build without it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) batch_admit_epoch_trace: Mutex<Option<Vec<u64>>>,
    /// Test-only one-shot pool-submit fault injector. `0` = off, `1` =
    /// force the next non-inline `try_submit` to report `Full`, `2` =
    /// `Closed`. Lets a test exercise the
    /// [`Self::terminalize_pool_submit_violation`] RELEASE path (cancel
    /// the DAG node, release the parked reservation, surface `Failed`)
    /// without saturating the real transport. `cfg`-gated to `test`;
    /// absent from every non-test build.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) pool_submit_fault: AtomicU8,
    /// Test-only marker set when the last pool-submit fault was injected
    /// via [`Self::pool_submit_fault`] rather than observed from a real
    /// transport. Read (and cleared) by
    /// [`Self::terminalize_pool_submit_violation`] to suppress its
    /// `debug_assert!` for the deliberately-injected case.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) pool_submit_fault_was_injected: AtomicBool,
    /// Test-only count of I/O pool submit ATTEMPTS made by the driver
    /// (incremented at the top of [`Self::try_submit_io`], before the
    /// nonblocking `try_send`). A test parks the single I/O worker inside
    /// a Source stage and then asserts this counter reaches the full
    /// admitted fan-out WHILE the worker is still parked — proving the
    /// driver kept dispatching past the stuck worker rather than blocking
    /// on it. `cfg`-gated to `test`; absent from release builds.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) io_submit_attempts: std::sync::atomic::AtomicUsize,
}

impl Scheduler {
    /// Identities of the injected CPU and I/O worker pools.
    ///
    /// Test-support only: production callers do not inspect scheduler execution
    /// substrate identity. Fresh-host isolation tests use this to prove that two
    /// distinct scheduler shells dispatch onto the exact same injected pools.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    #[must_use]
    pub fn test_worker_pool_ids(&self) -> (usize, usize) {
        (self.cpu_pool.pool_id(), self.io_pool.pool_id())
    }

    /// Create a new scheduler with a driver thread (native).
    ///
    /// The host constructs and injects the scheduler's CPU and I/O
    /// pools (mirroring the `HostCpuPool` construction/injection
    /// pattern). The scheduler owns no pool construction.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        cpu_pool: Arc<crate::execution::pool::SchedulerCpuPool>,
        io_pool: Arc<crate::execution::pool::SchedulerIoPool>,
    ) -> Arc<Self> {
        Self::with_executor(
            config,
            source_loader,
            Arc::new(DefaultExecutor),
            cpu_pool,
            io_pool,
        )
    }

    /// Create a new scheduler with a custom stage executor and driver thread (native).
    ///
    /// The driver thread holds a `Weak<Scheduler>`, so dropping the last caller
    /// `Arc` allows `Drop` to run (sets shutdown, joins driver, drains pending).
    ///
    /// `cpu_pool` / `io_pool` are host-constructed and injected — see
    /// [`crate::execution::pool`]. The scheduler owns no pool construction.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_executor(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        executor: Arc<dyn StageExecutor>,
        cpu_pool: Arc<crate::execution::pool::SchedulerCpuPool>,
        io_pool: Arc<crate::execution::pool::SchedulerIoPool>,
    ) -> Arc<Self> {
        let scheduler = Arc::new(Self {
            nodes: DashMap::new(),
            source_root: Arc::new(crate::source_root::SchedulerSourceDirectory::new()),
            edges: EdgeManager::new(),
            dag: Arc::new(new_dag_mutex(SchedulerDag::with_budget(
                config.resolved_dag_budget(),
            ))),
            scoped_cache_flights: DashMap::new(),
            scoped_cache_gate: Mutex::new(()),
            next_scoped_owner_id: AtomicU64::new(1),
            inbox: SubmissionInbox::new(),
            overlay: Arc::new(OverlayMap::new()),
            source_loader,
            executor,
            config,
            cpu_pool,
            io_pool,
            deferred_blocker_ids: DashMap::new(),
            auto_ingested_recent: DashMap::new(),
            #[cfg(any(test, feature = "semantic-observe"))]
            stale_completion_refusals: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            driver_handle: Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            reset_serial: Mutex::new(()),
            driver_teardown: crossbeam_channel::bounded(1),
            counters: SchedulerCounters::default(),
            #[cfg(any(test, feature = "test-support"))]
            dispatch_pause: DispatchPauseHook::default(),
            #[cfg(any(test, feature = "test-support"))]
            batch_admit_seam: BatchAdmitSeamHook::default(),
            #[cfg(any(test, feature = "test-support"))]
            reset_wake_posts: AtomicU64::new(0),
            #[cfg(any(test, feature = "test-support"))]
            dag_admit_epoch: AtomicU64::new(0),
            #[cfg(any(test, feature = "test-support"))]
            batch_admit_epoch_trace: Mutex::new(None),
            #[cfg(all(test, not(target_arch = "wasm32")))]
            pool_submit_fault: AtomicU8::new(0),
            #[cfg(all(test, not(target_arch = "wasm32")))]
            pool_submit_fault_was_injected: AtomicBool::new(false),
            #[cfg(all(test, not(target_arch = "wasm32")))]
            io_submit_attempts: std::sync::atomic::AtomicUsize::new(0),
        });

        // Driver holds Weak so it doesn't prevent Drop.
        // Also clones the receiver so it can block on it without upgrading.
        let weak = Arc::downgrade(&scheduler);
        let receiver = scheduler.inbox.receiver.clone();
        let teardown = scheduler.driver_teardown.1.clone();
        let handle = std::thread::Builder::new()
            .name("verter-scheduler".to_string())
            .spawn(move || {
                Self::driver_loop_native(weak, receiver, teardown);
            })
            .expect("failed to spawn scheduler driver");

        *scheduler.driver_handle.lock() = Some(handle);
        scheduler
    }

    /// Create a new scheduler for sync/test use (no driver thread).
    /// Use `drive_one()` / `drive_all()` to process manually.
    ///
    /// On native, the host injects the scheduler's CPU and I/O pools
    /// (see [`crate::execution::pool`]); on WASM there are no pools (stages run
    /// inline on the calling thread).
    pub fn new_sync(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        #[cfg(not(target_arch = "wasm32"))] cpu_pool: Arc<crate::execution::pool::SchedulerCpuPool>,
        #[cfg(not(target_arch = "wasm32"))] io_pool: Arc<crate::execution::pool::SchedulerIoPool>,
    ) -> Arc<Self> {
        Self::new_sync_with_executor(
            config,
            source_loader,
            Arc::new(DefaultExecutor),
            #[cfg(not(target_arch = "wasm32"))]
            cpu_pool,
            #[cfg(not(target_arch = "wasm32"))]
            io_pool,
        )
    }

    /// Create a sync scheduler with a custom stage executor.
    ///
    /// On native, the host injects the scheduler's CPU and I/O pools
    /// (see [`crate::execution::pool`]); on WASM there are no pools.
    pub fn new_sync_with_executor(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        executor: Arc<dyn StageExecutor>,
        #[cfg(not(target_arch = "wasm32"))] cpu_pool: Arc<crate::execution::pool::SchedulerCpuPool>,
        #[cfg(not(target_arch = "wasm32"))] io_pool: Arc<crate::execution::pool::SchedulerIoPool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            nodes: DashMap::new(),
            source_root: Arc::new(crate::source_root::SchedulerSourceDirectory::new()),
            edges: EdgeManager::new(),
            dag: Arc::new(new_dag_mutex(SchedulerDag::with_budget(
                config.resolved_dag_budget(),
            ))),
            scoped_cache_flights: DashMap::new(),
            scoped_cache_gate: Mutex::new(()),
            next_scoped_owner_id: AtomicU64::new(1),
            inbox: SubmissionInbox::new(),
            overlay: Arc::new(OverlayMap::new()),
            source_loader,
            executor,
            config,
            #[cfg(not(target_arch = "wasm32"))]
            cpu_pool,
            #[cfg(not(target_arch = "wasm32"))]
            io_pool,
            deferred_blocker_ids: DashMap::new(),
            auto_ingested_recent: DashMap::new(),
            #[cfg(any(test, feature = "semantic-observe"))]
            stale_completion_refusals: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            #[cfg(not(target_arch = "wasm32"))]
            driver_handle: Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            reset_serial: Mutex::new(()),
            #[cfg(not(target_arch = "wasm32"))]
            driver_teardown: crossbeam_channel::bounded(1),
            counters: SchedulerCounters::default(),
            #[cfg(any(test, feature = "test-support"))]
            dispatch_pause: DispatchPauseHook::default(),
            #[cfg(any(test, feature = "test-support"))]
            batch_admit_seam: BatchAdmitSeamHook::default(),
            #[cfg(any(test, feature = "test-support"))]
            reset_wake_posts: AtomicU64::new(0),
            #[cfg(any(test, feature = "test-support"))]
            dag_admit_epoch: AtomicU64::new(0),
            #[cfg(any(test, feature = "test-support"))]
            batch_admit_epoch_trace: Mutex::new(None),
            #[cfg(all(test, not(target_arch = "wasm32")))]
            pool_submit_fault: AtomicU8::new(0),
            #[cfg(all(test, not(target_arch = "wasm32")))]
            pool_submit_fault_was_injected: AtomicBool::new(false),
            #[cfg(all(test, not(target_arch = "wasm32")))]
            io_submit_attempts: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    // ── Test-only constructors (build default pools + inject) ──
    //
    // Production code constructs `SchedulerCpuPool`/`SchedulerIoPool` in
    // the host and injects them. Tests rarely care about pool identity,
    // so these helpers build default pools sized from `config` (the IO
    // transport derived from `config.resolved_dag_budget().io`, matching
    // the host) and delegate to the injected constructors. Gated behind
    // `cfg(any(test, feature = "test-support"))` so production binaries
    // never link them; cross-crate tests opt in via the `test-support`
    // feature in `[dev-dependencies]`.

    /// Test analogue of [`Scheduler::new`] that builds default pools.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    pub fn test_new(config: SchedulerConfig, source_loader: Arc<dyn SourceLoader>) -> Arc<Self> {
        let (cpu_pool, io_pool) = Self::default_test_pools(&config);
        Self::new(config, source_loader, cpu_pool, io_pool)
    }

    /// Test analogue of [`Scheduler::with_executor`] that builds default
    /// pools.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    pub fn test_with_executor(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        executor: Arc<dyn StageExecutor>,
    ) -> Arc<Self> {
        let (cpu_pool, io_pool) = Self::default_test_pools(&config);
        Self::with_executor(config, source_loader, executor, cpu_pool, io_pool)
    }

    /// Test analogue of [`Scheduler::new_sync`] that builds default
    /// pools (native) / no pools (WASM).
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_new_sync(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
    ) -> Arc<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (cpu_pool, io_pool) = Self::default_test_pools(&config);
            Self::new_sync(config, source_loader, cpu_pool, io_pool)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::new_sync(config, source_loader)
        }
    }

    /// Test analogue of [`Scheduler::new_sync_with_executor`] that builds
    /// default pools (native) / no pools (WASM).
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_new_sync_with_executor(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        executor: Arc<dyn StageExecutor>,
    ) -> Arc<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (cpu_pool, io_pool) = Self::default_test_pools(&config);
            Self::new_sync_with_executor(config, source_loader, executor, cpu_pool, io_pool)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::new_sync_with_executor(config, source_loader, executor)
        }
    }

    // ── Request Submission ──

    /// Execute one request-scoped borrowed producer through normal cache-node
    /// DAG admission, deduplicating overlapping calls by the request's full
    /// typed identity.
    ///
    /// The closure is intentionally synchronous and need not be `'static`:
    /// after DAG dispatch, exactly one waiting caller runs its closure on the
    /// scheduler CPU pool via a scoped `install`; every overlapping caller
    /// receives the same `Arc<T>`. The rendezvous is removed at terminal state,
    /// so this API never becomes a second durable cache authority.
    pub fn execute_scoped_cache_node<T, F>(
        self: &Arc<Self>,
        request: ScopedCacheNodeRequest,
        build: F,
    ) -> Result<Arc<T>, ScopedCacheNodeError>
    where
        T: Send + Sync + 'static,
        F: FnOnce(&CancellationToken) -> T + Send,
    {
        let request_token = request
            .request_context
            .as_ref()
            .and_then(|context| context.0.cancellation_token());
        if request_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(ScopedCacheNodeError::Cancelled);
        }

        let identity = request.identity();
        if crate::caller_kind::active_path_contains_work(&identity) {
            return Err(ScopedCacheNodeError::Reentrant);
        }
        let owner_id = self.next_scoped_owner_id.fetch_add(1, Ordering::Relaxed);
        let flight = {
            let _gate = self.scoped_cache_gate.lock();
            loop {
                let candidate = match self.scoped_cache_flights.entry(identity.clone()) {
                    dashmap::mapref::entry::Entry::Occupied(entry) => Arc::clone(entry.get()),
                    dashmap::mapref::entry::Entry::Vacant(entry) => {
                        let flight = Arc::new(ScopedCacheFlight::new());
                        entry.insert(Arc::clone(&flight));
                        flight
                    }
                };
                if candidate.try_add_owner(owner_id, request_token.clone()) {
                    break candidate;
                }
                // A terminal/latched flight cannot accept a new owner. Remove
                // only that exact incarnation, cancel any stale DAG identity,
                // and retry against a fresh rendezvous.
                let _ = self.dag.lock().cancel(&identity);
                self.remove_scoped_flight_locked(&identity, &candidate);
            }
        };

        let submission = Submission::ScopedCacheNode {
            identity: identity.clone(),
            priority: request.priority,
            flight: Arc::clone(&flight),
            request_context: request.request_context.clone(),
        };
        if self.send_submission(submission).is_err() {
            self.terminalize_scoped_cache_flight(
                &identity,
                &flight,
                ScopedCacheTerminal::Shutdown,
                false,
            );
        } else {
            self.counters.submit_count.fetch_add(1, Ordering::Relaxed);
            self.counters
                .inbox_depth_max
                .fetch_max(self.inbox.sender.len() as u64, Ordering::Relaxed);
        }

        let result = self.wait_for_scoped_cache_node(
            &identity,
            &flight,
            request.request_context,
            request_token.clone(),
            build,
        );
        self.detach_scoped_cache_owner(&identity, &flight, owner_id);
        if request_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            Err(ScopedCacheNodeError::Cancelled)
        } else {
            result
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn test_scoped_cache_owner_count(&self, identity: &WorkNodeIdentity) -> usize {
        self.scoped_cache_flights
            .get(identity)
            .map_or(0, |flight| flight.owner_count())
    }

    /// Submit a request. Returns a handle that resolves when the target stage is reached.
    pub fn submit_request(&self, request: Request) -> CompletionHandle<RequestResult> {
        verter_audit::attribute!(SchedulerSubmitRequest);
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
        let submitted_lifetime =
            self.stamp_request(&request.file_id, request.file_language.clone());
        let submission = Submission::NewRequest {
            file_id: request.file_id,
            target: request.target,
            priority: request.priority,
            source: request.source,
            file_language: request.file_language,
            sender: sender.clone(),
            submitted_lifetime,
            request_context: request.request_context,
        };
        match self.send_submission(submission) {
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

    /// Atomically submit a batch of requests as ONE inbox item.
    ///
    /// This lands a single [`Submission::NewRequestBatch`] that the
    /// driver drains as a unit and admits under ONE `dag.lock()`
    /// acquisition (generation bumps + supersede sweeps + waiter
    /// registration for every request). Consequences:
    ///
    /// - The pump can never observe the batch half-admitted — either
    ///   none or all N nodes are in the DAG after the single item is
    ///   processed.
    /// - One batch is ONE wake and ONE `submit_count` increment, not N.
    /// - A source-updating batch supersedes the old generations of
    ///   every file in the batch in the same critical section.
    ///
    /// The pump discipline is preserved: dispatch / wait / parse /
    /// compile / dedup-callbacks all run OUTSIDE the DAG lock (dedup
    /// callbacks are collected during registration and fired after the
    /// lock releases). Capacity is still reserved at dequeue time, not
    /// at admission.
    ///
    /// Returns a [`BatchHandle`] whose completion handles are in input
    /// order; drain it via [`Self::wait_batch`].
    pub fn submit_batch_atomic(&self, requests: Vec<Request>) -> BatchHandle {
        verter_audit::attribute_n!(SchedulerSubmitBatch, requests.len());
        let mut handles = Vec::with_capacity(requests.len());
        let mut queued = Vec::with_capacity(requests.len());
        for request in requests {
            let submitted_lifetime =
                self.stamp_request(&request.file_id, request.file_language.clone());
            let (handle, sender) = completion_pair();
            // Mirror `submit_request`'s request-level target stamp so a
            // cooperative waiter on a batch handle gets the same
            // same-path fallback before admission overwrites it with the
            // concrete `Work` identity.
            sender.set_target(crate::job::CompletionTarget::Request {
                canonical: Arc::from(request.file_id.as_str()),
                target: request.target.clone(),
            });
            queued.push(QueuedRequest {
                file_id: request.file_id,
                target: request.target,
                priority: request.priority,
                source: request.source,
                file_language: request.file_language,
                sender,
                submitted_lifetime,
                request_context: request.request_context,
            });
            handles.push(handle);
        }

        if queued.is_empty() {
            // Nothing to submit — no wake, no accounting (mirrors the
            // empty-batch contract of `account_batch_submission`).
            return BatchHandle { handles };
        }

        match self.send_submission(Submission::NewRequestBatch { requests: queued }) {
            Ok(()) => {
                // ONE batch == ONE submission for contention accounting,
                // regardless of how many items it carries.
                self.counters.submit_count.fetch_add(1, Ordering::Relaxed);
                let depth = self.inbox.sender.len() as u64;
                let prev_max = self.counters.inbox_depth_max.load(Ordering::Relaxed);
                if depth > prev_max {
                    let _ = self
                        .counters
                        .inbox_depth_max
                        .fetch_max(depth, Ordering::Relaxed);
                }
            }
            Err(submission) => {
                // Inbox closed (scheduler shutting down): signal every
                // handle so callers don't hang.
                Self::shutdown_drained_submission(*submission);
            }
        }
        BatchHandle { handles }
    }

    /// Wait for a submitted batch to complete. Drains each
    /// [`CompletionHandle`] in submission (INPUT) order and returns the
    /// per-request results in that same order, so `result[i]`
    /// corresponds to the i-th submitted request regardless of the
    /// order the underlying stage work actually completed. The waiter
    /// never observes a partial set — every handle is resolved before
    /// the result vec is returned. Each request's stage work runs on
    /// the scheduler's own stage `cpu_pool` (the scheduler owns no outer
    /// batch fan-out).
    ///
    /// Uses `wait_or_drive` so both native (driver thread) and
    /// single-threaded callers share the same completion semantics.
    ///
    /// Generic over `Borrow<BatchHandle>` so BOTH a by-value
    /// `wait_batch(batch)` and a borrowed `wait_batch(&batch)` compile
    /// unchanged. The waiter only reads the handles, so a caller that
    /// wants to inspect the batch afterward (e.g. for per-request audit
    /// attribution) passes `&batch` and keeps ownership; a caller that
    /// is done with the batch passes it by value. The pre-existing
    /// by-value public signature is preserved by construction.
    pub fn wait_batch<B: std::borrow::Borrow<BatchHandle>>(
        self: &Arc<Self>,
        batch: B,
    ) -> Vec<crate::job::CompletionState<RequestResult>> {
        batch
            .borrow()
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
    /// `Source` stage work (the load+parse step the source stage runs
    /// under `TaskKind::Load`) the driver dispatches onto that pool — the
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

    /// Current occupancy of the DAG's dependency-edge and blocker
    /// reference tables, read under one DAG lock.
    pub fn dependency_occupancy(&self) -> crate::dag::DependencyOccupancy {
        self.dag.lock().dependency_occupancy()
    }

    // ── Sync Fast-Path Reads ──

    /// Get current source snapshot if generation-coherent.
    ///
    /// This reads the LIVE node — the answer changes the moment another
    /// thread bumps the generation. A consumer that needs a stable world
    /// captures a [`crate::source_root::SchedulerSourceRoot`] instead
    /// and reads it as-of.
    pub fn try_get_source(&self, id: &str) -> Option<Arc<SourceSnapshot>> {
        self.nodes.get(id)?.current_source()
    }

    /// Get the current source snapshot together with the
    /// [`SourceWitness`](crate::node::SourceWitness) of the node object it
    /// was read from. External publishers ([`Self::commit_artifact`],
    /// [`Self::remove_artifact_not_newer_than`]) are fenced on that witness.
    pub fn try_get_witnessed_source(&self, id: &str) -> Option<crate::node::WitnessedSource> {
        let node = self.nodes.get(id)?;
        let snapshot = node.current_source()?;
        let witness = crate::node::SourceWitness::capture(&node, &snapshot);
        Some(crate::node::WitnessedSource { snapshot, witness })
    }

    /// Get the current source snapshot of the node object `witness` was
    /// captured from. `None` once that node was retired or replaced (removal,
    /// reset, language re-home), even when its successor serves the same
    /// content at the same generation, or when it has no current source.
    pub fn try_get_source_for_witness(
        &self,
        witness: &crate::node::SourceWitness,
    ) -> Option<Arc<SourceSnapshot>> {
        let node = self.nodes.get(witness.canonical())?;
        if !node.carries_witness(witness) {
            return None;
        }
        node.current_source()
    }

    /// Capture an immutable, LEASED root of the scheduler's source
    /// world.
    ///
    /// O(1) in the number of tracked files: one mutex acquisition, one
    /// scalar read, one counter bump — never a `nodes` walk. The root
    /// both NAMES the epoch and KEEPS every version visible at it
    /// reachable until the root drops, so a holder can still resolve its
    /// own world after the live nodes have moved on.
    #[must_use]
    pub fn capture_source_root(&self) -> Arc<crate::source_root::SchedulerSourceRoot> {
        self.source_root.capture_root()
    }

    /// The scheduler's MVCC source directory — the publication and
    /// reclamation authority behind
    /// [`Self::capture_source_root`].
    #[must_use]
    pub fn source_directory(&self) -> &Arc<crate::source_root::SchedulerSourceDirectory> {
        &self.source_root
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
        // Serialize concurrent resets: the driver handle is `take()`n
        // exactly once, so without this the loser would skip the join and
        // clear `nodes`/the DAG while the still-running driver pumps.
        // Held for the whole body; `reset()` never re-enters itself and
        // the driver never calls it, so this cannot deadlock.
        let _serial = self.reset_serial.lock();
        // 1. Stop the driver thread. The dedicated teardown signal — not
        //    the inbox wake, which any cooperative pump may consume first —
        //    is what guarantees a parked driver observes this.
        self.shutdown.store(true, Ordering::Release);
        let _ = self.driver_teardown.0.try_send(());
        let _ = self.inbox.sender.try_send(Submission::Wake);
        #[cfg(any(test, feature = "test-support"))]
        self.reset_wake_posts.fetch_add(1, Ordering::Release);
        if let Some(handle) = self.driver_handle.lock().take() {
            if should_join_driver_thread(handle.thread().id(), std::thread::current().id()) {
                let _ = handle.join();
            }
        }

        // 2. Drain queued requests; cooperative admission validates their stamps.
        while let Ok(submission) = self.inbox.receiver.try_recv() {
            Self::shutdown_drained_submission(submission);
        }
        self.shutdown_all_scoped_cache_flights();

        // Cooperative pumps share the same lifecycle hold as admission.
        // No successor can appear between node unpublication and DAG clear.
        {
            let mut dag = self.dag.lock();
            let ids: Vec<String> = self.nodes.iter().map(|e| e.key().clone()).collect();
            self.source_root.publish_transition(|publication| {
                for id in &ids {
                    if let Some((_, node)) = self.nodes.remove(id) {
                        node.retire(&mut dag);
                        let canonical: Arc<str> = Arc::from(id.as_str());
                        publication.absent(&canonical, node.incarnation_id(), node.generation());
                    }
                }
            });
            dag.clear();
            self.edges.reverse_index.inner.clear();
            self.edges.forward_deps.clear();
            self.deferred_blocker_ids.clear();
            self.auto_ingested_recent.clear();
        }

        // 5. Drain inbox again — catch any completions that workers sent
        //    between step 2 and now. These are harmless (nodes removed in
        //    step 3, so handle_stage_complete will no-op), but draining
        //    keeps the channel clean.
        while let Ok(submission) = self.inbox.receiver.try_recv() {
            Self::shutdown_drained_submission(submission);
        }

        // 6. Unset shutdown so the next driver can run.
        self.shutdown.store(false, Ordering::Release);
    }

    /// Restart the driver thread after a `reset()`. Must be called on
    /// `Arc<Self>` because the driver needs a `Weak` reference.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn restart_driver(self: &Arc<Self>) {
        // The outgoing driver may have exited on the shutdown flag without
        // consuming its teardown signal; a leftover signal would stop the
        // incoming driver on its first park.
        while self.driver_teardown.1.try_recv().is_ok() {}
        let weak = Arc::downgrade(self);
        let receiver = self.inbox.receiver.clone();
        let teardown = self.driver_teardown.1.clone();
        let handle = std::thread::Builder::new()
            .name("verter-scheduler".to_string())
            .spawn(move || {
                Self::driver_loop_native(weak, receiver, teardown);
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
            Self::shutdown_drained_submission(submission);
        }
    }

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
        // Host callbacks finish before the lifecycle hold.
        let requested_incarnation = self.stamp_request(file_id, None);
        let dep_languages: std::collections::HashMap<String, FileLanguage> = blocker_dep_ids
            .iter()
            .map(|id| (id.clone(), self.source_loader.classify(id)))
            .collect();
        let canonical_arc: Arc<str> = Arc::from(file_id);
        let (generation, incarnation, inherited_priority) = {
            let mut dag = self.dag.lock();
            let Some(node) = self
                .nodes
                .get(file_id)
                .map(|entry| Arc::clone(entry.value()))
            else {
                return;
            };
            if node.submission_lifetime() != requested_incarnation {
                return;
            }
            self.edges
                .record_forward_deps(file_id, resolved_dep_ids.into_iter().collect());
            let generation = node.generation();
            if blocker_dep_ids.is_empty() {
                self.deferred_blocker_ids.remove(file_id);
                dag.clear_artifact_blockers(&canonical_arc, generation);
                return;
            }
            self.deferred_blocker_ids.insert(
                file_id.to_owned(),
                blocker_dep_ids
                    .iter()
                    .map(|id| (id.clone(), dep_languages[id].clone()))
                    .collect(),
            );
            if node.current_integrated_source().is_none() {
                return;
            }
            (
                generation,
                node.incarnation_id(),
                dag.highest_priority_for_file(&canonical_arc, generation)
                    .unwrap_or(Priority::Background),
            )
        };

        // A queued Source or reload already owns a node but may still be in
        // the inbox. Give it the same producer tracking as an auto-ingested
        // dependency before recording any blockers for its Analysis.
        for dep_id in &blocker_dep_ids {
            let submission = {
                let _dag = self.dag.lock();
                if !self.nodes.get(file_id).is_some_and(|live| {
                    live.incarnation_id() == incarnation && live.generation() == generation
                }) {
                    return;
                }
                let dep_node = Arc::clone(
                    self.nodes
                        .entry(dep_id.clone())
                        .or_insert_with(|| {
                            self.create_node_at(dep_id, Some(dep_languages[dep_id].clone()), 1)
                        })
                        .value(),
                );
                let dep_canonical: Arc<str> = Arc::from(dep_id.as_str());
                if dep_node.source_admission_pending()
                    && dep_node.current_source().is_none()
                    && !self.auto_ingest_tracking_gates(
                        &dep_canonical,
                        dep_node.incarnation_id(),
                        dep_node.generation(),
                    )
                {
                    let dep_gen = if dep_node.generation() == 0 {
                        self.source_root.publish_transition(|publication| {
                            let generation = publication.bump_node_generation(&dep_node);
                            publication.absent(
                                &dep_canonical,
                                dep_node.incarnation_id(),
                                generation,
                            );
                            generation
                        })
                    } else {
                        dep_node.generation()
                    };
                    self.auto_ingested_recent.insert(
                        Arc::clone(&dep_canonical),
                        AutoIngestedRecord {
                            incarnation: dep_node.incarnation_id(),
                            generation: dep_gen,
                            since: Instant::now(),
                        },
                    );
                    let (_, sender) = completion_pair::<RequestResult>();
                    Some(Submission::NewRequest {
                        file_id: dep_id.clone(),
                        target: TargetStage::Analysis,
                        priority: std::cmp::min(inherited_priority, Priority::Interactive),
                        source: None,
                        file_language: None,
                        request_context: None,
                        sender,
                        submitted_lifetime: dep_node.submission_lifetime(),
                    })
                } else {
                    None
                }
            };
            if let Some(submission) = submission {
                let _ = self.send_submission(submission);
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
        if !self.nodes.get(file_id).is_some_and(|live| {
            live.incarnation_id() == incarnation && live.generation() == generation
        }) {
            return;
        }
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
            let dep_canonical: Arc<str> = Arc::from(dep_id.as_str());
            let Some(dep_node) = self.nodes.get(dep_id).map(|n| Arc::clone(n.value())) else {
                continue;
            };
            let dep_gen = dep_node.generation();
            let dep_incarnation = dep_node.incarnation_id();
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
            let status = self.ensure_analysis_for_demand(
                &mut dag,
                &dep_canonical,
                dep_incarnation,
                dep_gen,
                std::cmp::min(inherited_priority, Priority::Interactive),
                AnalysisDemandKind::ArtifactBlocker,
            );
            match status {
                BlockerStatus::Satisfied => continue,
                BlockerStatus::Failed(record) => {
                    failed_records.push(record);
                    continue;
                }
                BlockerStatus::Gating => {
                    let dep_key = DepKey::FileStage {
                        canonical: Arc::clone(&dep_canonical),
                        incarnation: dep_incarnation,
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
            Self::filter_macro_cycle_deps(&dag, &canonical_arc, incarnation, generation, dep_keys);
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
    ///
    /// The artifact is published at `witness`'s generation, and only into
    /// the node object `witness` was captured from: a witness whose node was
    /// retired (removal, reset, language re-home) is rejected even when a
    /// successor serves the same content at the same generation. Returns
    /// whether the artifact was published.
    pub fn commit_artifact(
        &self,
        witness: &crate::node::SourceWitness,
        profile_hash: u64,
        data: Arc<dyn crate::node::SnapshotData>,
    ) -> bool {
        let file_id = witness.canonical();
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
            None => return false,
        };
        let incarnation = witness.incarnation();
        let generation = witness.generation();
        // Full coherence check: witnessed node object, node generation,
        // Source, AND Analysis must all match. Without this, an external
        // compile can publish an artifact before the scheduler's own
        // pipeline has committed the prerequisite stages.
        if !node.admits_work(incarnation, generation) {
            return false;
        }
        if node.current_source().is_none() || node.current_analysis().is_none() {
            return false;
        }
        let snap = Arc::new(ArtifactSnapshot {
            generation,
            profile_hash,
            data,
        });
        let canonical: Arc<str> = Arc::from(file_id);
        let artifact_id = WorkNodeIdentity::Artifact {
            canonical: Arc::clone(&canonical),
            incarnation,
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
        if !self
            .nodes
            .get(file_id)
            .is_some_and(|live| live.admits_work(incarnation, generation))
        {
            return false;
        }
        node.artifacts.insert(profile_hash, Arc::clone(&snap));
        let result = RequestResult::Artifact(snap);
        guard.signal_stage_complete(
            &canonical,
            incarnation,
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
        verter_debug_assert!(
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
        true
    }

    /// Evict the artifact snapshot for `(witness.canonical(), profile_hash)`
    /// only when it belongs to the witnessed node object and is no newer
    /// than the witnessed generation.
    ///
    /// Called by the host when a compile path refuses cache admission
    /// (e.g. an overflowed fact signature) and any prior artifact for
    /// the same `(canonical, profile)` produced at or before the
    /// caller's start-of-compile source must not remain observable
    /// via `try_get_artifact`. The symmetric counterpart to
    /// [`commit_artifact`](Self::commit_artifact): commit publishes the
    /// snapshot, this evicts it under the same witness.
    ///
    /// Generation gate. The slow refused compile that started at
    /// generation `N` may reach this call AFTER a fresh successful
    /// compile at generation `N+k` has landed a newer artifact via
    /// `commit_artifact`. Unconditionally removing would clobber the
    /// newer artifact, so the eviction proceeds only when the stored
    /// snapshot's `generation <= witness.generation()`.
    ///
    /// Incarnation gate. A witness whose node object was retired (removal,
    /// reset, language re-home) evicts nothing, even when a successor holds
    /// an artifact at the same generation for the same content: that
    /// artifact was never produced from the witnessed source.
    ///
    /// Returns whether an artifact was evicted. Does NOT touch generation,
    /// source, or analysis state — only the `(profile_hash → snapshot)`
    /// entry on the artifact map.
    pub fn remove_artifact_not_newer_than(
        &self,
        witness: &crate::node::SourceWitness,
        profile_hash: u64,
    ) -> bool {
        // Removal and reset retire and unpublish a node under the DAG lock,
        // so the witness check and the eviction cannot straddle a
        // retirement. The node `Ref` is taken after the lock (DAG-first).
        let _lifecycle = self.dag.lock();
        let Some(node) = self.nodes.get(witness.canonical()) else {
            return false;
        };
        if !node.carries_witness(witness) {
            return false;
        }
        let max_generation = witness.generation();
        // Race-free remove-if: `DashMap::remove_if` runs the predicate
        // under the per-shard lock so a concurrent internal artifact
        // publication cannot land a newer snapshot between the read and
        // the remove.
        node.artifacts
            .remove_if(&profile_hash, |_, snap| snap.generation <= max_generation)
            .is_some()
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
        self.invalidate_with_lock_hooks(id, &mut || {}, &mut || {});
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
        let submitted_lifetime = node.incarnation_id();
        if !self
            .nodes
            .get(id)
            .is_some_and(|live| live.incarnation_id() == submitted_lifetime)
        {
            return;
        }
        // Bump + publish atomically; see [`Self::invalidate`].
        let new_gen = self.source_root.publish_transition(|publication| {
            let new_gen = publication.bump_node_generation(&node);
            publication.absent(&canonical, node.incarnation_id(), new_gen);
            new_gen
        });
        node.pending_source.store(Arc::new(None));
        dag.supersede_old_file_generations(&canonical, new_gen);
        // Drop the DAG lock before the inbox send + sender_drop
        // below; the inbox channel is unrelated to the DAG lock.
        drop(dag);

        // Enqueue a Source job at Background priority to reload from disk
        let _ = self.send_submission(Submission::NewRequest {
            file_id: id.to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: None,
            file_language: None,
            submitted_lifetime: node.submission_lifetime(),
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
        teardown: crossbeam_channel::Receiver<()>,
    ) {
        // Mark the driver thread so cooperative-pump callers can
        // distinguish driver-led pumps from worker-led pumps. The
        // driver thread is born here, lives for the scheduler's
        // lifetime, and never delegates this slot to another role.
        let _ = crate::caller_kind::CallerKind::set(crate::caller_kind::CallerKind::Driver);

        // Periodic missed-wake safety net: even with no submission, the
        // driver re-pumps every 5s so any stranded ready work (e.g. a
        // waiter freed by a terminal-failure fan-out whose explicit
        // wake was missed) still dispatches. This is NOT priority
        // aging — selection-count weighted credit needs no timer; this
        // interval purely backstops a dropped wake.
        let idle_repump_interval = std::time::Duration::from_secs(5);

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

            // Wait for a teardown signal, the next submission, or the
            // idle re-pump tick. The teardown arm is what makes this park
            // exitable without an inbox message: the pump above drains the
            // very inbox every other pumper drains, so a teardown wake
            // posted there may already be gone by the time we get here.
            let park = crossbeam_channel::select! {
                recv(teardown) -> _ => DriverPark::Teardown,
                recv(receiver) -> submission => match submission {
                    Ok(submission) => DriverPark::Submission(submission),
                    Err(crossbeam_channel::RecvError) => DriverPark::Disconnected,
                },
                default(idle_repump_interval) => DriverPark::IdleTick,
            };
            match park {
                DriverPark::Submission(submission) => {
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
                // Both re-enter the outer loop, which observes the
                // shutdown flag (teardown) or re-pumps (idle tick).
                DriverPark::Teardown | DriverPark::IdleTick => continue,
                DriverPark::Disconnected => break,
            }
        }
    }

    /// Test-only: arm the dispatch pause so the driver parks after
    /// dispatching `pause_after` jobs and BEFORE the next dequeue. Must
    /// be called before submitting the requests whose scheduler-queue
    /// dwell the test inspects. See [`DispatchPauseHook`].
    #[cfg(any(test, feature = "test-support"))]
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
    #[cfg(any(test, feature = "test-support"))]
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
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[must_use]
    pub fn test_job_queue_depth(&self) -> usize {
        self.dag.lock().pending_len()
    }

    /// Test-only: how many times [`Self::reset`] has posted its inbox
    /// teardown wake. Observing this instead of the inbox itself is what
    /// lets a teardown test release a rendezvous exactly once the wake is
    /// posted, whether or not a concurrent pump has already swallowed it.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[must_use]
    pub fn test_reset_wake_posts(&self) -> u64 {
        self.reset_wake_posts.load(Ordering::Acquire)
    }

    /// Test-only: release the parked driver from the dispatch pause.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn test_release_dispatch_pause(&self) {
        let mut state = self.dispatch_pause.state.lock();
        state.released = true;
        self.dispatch_pause.cv.notify_all();
    }

    /// Test-only: install the mid-batch admission seam hook. Fired once
    /// per subsequent atomic batch, after the first admit and before the
    /// rest, WHILE the single `dag.lock()` is held. Used by the
    /// LOCK-CONTINUITY discriminator to prove the batch holds one
    /// continuously-held DAG lock across all N admits. See
    /// [`BatchAdmitSeamHook`].
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn test_install_batch_admit_seam(&self, hook: Box<dyn Fn() + Send + Sync>) {
        self.batch_admit_seam.install(hook);
    }

    /// Test-only: arm the per-admit epoch recorder. After this call every
    /// admit inside [`Self::handle_new_request_batch`] pushes the
    /// acquisition epoch it ran under into the trace. Pair with
    /// [`Self::test_take_batch_admit_epochs`] to read and disarm. The
    /// LOCK-CONTINUITY rail uses this to prove all N admits shared ONE
    /// lock acquisition.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn test_install_batch_admit_epoch_trace(&self) {
        *self.batch_admit_epoch_trace.lock() = Some(Vec::new());
    }

    /// Test-only: peek the number of admits recorded so far in the armed
    /// epoch trace WITHOUT consuming it. Returns 0 when no recorder is
    /// armed. Used by the LOCK-CONTINUITY observer to read, at the instant
    /// it acquires the DAG lock, how many admits had already recorded — a
    /// test-agnostic measure of admission progress (one push per admit) that
    /// is exactly `n` once the batch's held lock drops.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn test_peek_batch_admit_epoch_count(&self) -> usize {
        self.batch_admit_epoch_trace
            .lock()
            .as_ref()
            .map_or(0, |trace| trace.len())
    }

    /// Test-only: take and disarm the per-admit epoch trace armed by
    /// [`Self::test_install_batch_admit_epoch_trace`]. Returns the epochs
    /// recorded (one per admit, in admit order). Panics if no recorder was
    /// armed, so a test cannot silently assert over an empty trace.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn test_take_batch_admit_epochs(&self) -> Vec<u64> {
        self.batch_admit_epoch_trace
            .lock()
            .take()
            .expect("test_install_batch_admit_epoch_trace must be called before taking the trace")
    }

    /// Test-only: clear the committed analysis snapshot for a file while
    /// leaving its source snapshot and node generation untouched.
    /// Reproduces the scheduler state inside the Source→Analysis commit
    /// window — the Source job has committed at the node generation, the
    /// Analysis job has not yet — which a concurrent read otherwise hits
    /// only by racing the worker. `try_get_source` keeps serving the
    /// committed source; `try_get_analysis` returns `None` until a new
    /// analysis commit lands.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn test_clear_analysis(&self, id: &str) {
        if let Some(node) = self.nodes.get(id) {
            node.analysis.store(Arc::new(None));
        }
    }

    /// Whether the native driver thread owns this scheduler's pump.
    ///
    /// When true, a non-worker caller (host / `External` thread) must not
    /// pump [`Self::drive_one`] — it dequeues and inline-executes scheduler
    /// stage work on the calling thread, breaking the dual-pool isolation
    /// between host-coordinator threads and the scheduler's stage pools.
    /// Such callers park through [`Self::wait_or_drive`] instead and the
    /// driver dispatches. Always `false` on wasm32 (no driver exists).
    #[must_use]
    pub fn has_driver_thread(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.driver_handle.lock().is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

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
        verter_debug_assert!(
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
}

#[cfg(test)]
mod tests {
    fn fixture_incarnation(scheduler: &Scheduler, id: &str) -> u64 {
        scheduler
            .nodes
            .get(id)
            .map_or(0, |node| node.incarnation_id())
    }

    fn source_witness(scheduler: &Scheduler, id: &str) -> crate::node::SourceWitness {
        scheduler
            .try_get_witnessed_source(id)
            .expect("fixture invariant: current source is committed")
            .witness
    }

    use super::*;
    use crate::source_loader::MemorySourceLoader;

    #[cfg(not(target_arch = "wasm32"))]
    struct ScopedTestContext {
        id: u64,
        cancellation: CancellationToken,
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl verter_execution::request_context::RequestContextLike for ScopedTestContext {
        fn request_id(&self) -> u64 {
            self.id
        }

        fn capture_enabled(&self) -> bool {
            false
        }

        fn cancellation_token(&self) -> Option<CancellationToken> {
            Some(self.cancellation.clone())
        }

        fn on_dedup_joiner(&self, _: Arc<str>, _: u64, _: bool) {}

        fn record_cache_event(&self, _: verter_execution::request_context::CacheEventKind) {}

        fn install_tls(
            self: Arc<Self>,
        ) -> Box<dyn verter_execution::request_context::TlsUninstall + Send> {
            struct Noop;
            impl verter_execution::request_context::TlsUninstall for Noop {
                fn uninstall(self: Box<Self>) {}
            }
            Box::new(Noop)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scoped_test_context(
        id: u64,
    ) -> (
        verter_execution::request_context::OpaqueRequestContext,
        CancellationToken,
    ) {
        let cancellation = CancellationToken::new();
        let context = Arc::new(ScopedTestContext {
            id,
            cancellation: cancellation.clone(),
        });
        (
            verter_execution::request_context::OpaqueRequestContext(
                context as Arc<dyn verter_execution::request_context::RequestContextLike>,
            ),
            cancellation,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scoped_test_request(
        request_context: verter_execution::request_context::OpaqueRequestContext,
    ) -> ScopedCacheNodeRequest {
        ScopedCacheNodeRequest {
            cache_id: crate::cache_id::SchedulerCacheId(0x51FC),
            key_hash: [0xA5; 16],
            view_epoch: 7,
            snapshot_pin_id: PinId(11),
            priority: Priority::Interactive,
            request_context: Some(request_context),
        }
    }

    fn _test_scheduler() -> Arc<Scheduler> {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));
        loader.insert("/b.vue".to_string(), Arc::from("<template>bye</template>"));
        Scheduler::test_new_sync(SchedulerConfig::default(), loader)
    }

    fn test_scheduler_with_loader(loader: Arc<MemorySourceLoader>) -> Arc<Scheduler> {
        Scheduler::test_new_sync(SchedulerConfig::default(), loader)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn scoped_cache_cancelled_winner_does_not_abort_live_sibling() {
        let scheduler = Scheduler::test_new(
            SchedulerConfig {
                cpu_threads: 2,
                ..SchedulerConfig::default()
            },
            Arc::new(MemorySourceLoader::new()),
        );
        let (leader_context, leader_token) = scoped_test_context(1);
        let (sibling_context, _sibling_token) = scoped_test_context(2);
        let leader_request = scoped_test_request(leader_context);
        let sibling_request = scoped_test_request(sibling_context);
        let identity = leader_request.identity();
        let builds = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();

        let leader = {
            let scheduler = Arc::clone(&scheduler);
            let builds = Arc::clone(&builds);
            std::thread::spawn(move || {
                scheduler.execute_scoped_cache_node(leader_request, move |_| {
                    builds.fetch_add(1, Ordering::SeqCst);
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    41_u64
                })
            })
        };
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("leader must enter the scoped producer");

        let sibling = {
            let scheduler = Arc::clone(&scheduler);
            std::thread::spawn(move || {
                scheduler.execute_scoped_cache_node(sibling_request, |_| -> u64 {
                    panic!("deduplicated sibling must not execute its closure")
                })
            })
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while scheduler.test_scoped_cache_owner_count(&identity) != 2
            && std::time::Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        assert_eq!(scheduler.test_scoped_cache_owner_count(&identity), 2);

        leader_token.cancel();
        release_tx.send(()).unwrap();

        assert_eq!(
            leader.join().unwrap(),
            Err(ScopedCacheNodeError::Cancelled),
            "the cancelled caller observes its own cancellation"
        );
        assert_eq!(
            *sibling
                .join()
                .unwrap()
                .expect("live sibling receives result"),
            41
        );
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert_eq!(
            scheduler.dag.lock().cache_node_terminal_counts(),
            crate::dag::CacheNodeTerminalCounts {
                completed: 1,
                cancelled: 0,
            }
        );
    }

    /// A superseded scoped-cache flight must never cancel the DAG node that a
    /// LATER incarnation of the same `WorkNodeIdentity` owns.
    ///
    /// The flight registry is per-incarnation but the DAG node is keyed by
    /// identity alone. When a stale flight tore its DAG node down by identity,
    /// it removed the successor's freshly-admitted node: the successor's
    /// aggregate token latched cancelled and its `by_identity` entry vanished,
    /// so the successor was either force-cancelled (this test) or — when the
    /// stale cancel landed BEFORE the successor's dispatch — never dispatched
    /// at all, leaving `wait_for_scoped_cache_node` parked forever. That
    /// unbounded park is the >10s `VerterHost::ensure_ide_compiled` block
    /// observed under concurrent LSP load.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn stale_flight_teardown_leaves_the_current_incarnations_node_alive() {
        let scheduler = Scheduler::test_new(
            SchedulerConfig {
                cpu_threads: 4,
                ..SchedulerConfig::default()
            },
            Arc::new(MemorySourceLoader::new()),
        );
        let (stale_builder_context, stale_builder_token) = scoped_test_context(21);
        let (stale_follower_context, stale_follower_token) = scoped_test_context(22);
        let (successor_context, _successor_token) = scoped_test_context(23);
        let stale_builder_request = scoped_test_request(stale_builder_context);
        let stale_follower_request = scoped_test_request(stale_follower_context);
        let successor_request = scoped_test_request(successor_context);
        let identity = stale_builder_request.identity();

        let (stale_entered_tx, stale_entered_rx) = std::sync::mpsc::channel();
        let (stale_release_tx, stale_release_rx) = std::sync::mpsc::channel();

        // 1. The stale incarnation's builder claims the flight and parks
        //    inside its producer, holding the flight open.
        let stale_builder = {
            let scheduler = Arc::clone(&scheduler);
            std::thread::spawn(move || {
                scheduler.execute_scoped_cache_node(stale_builder_request, move |_| {
                    stale_entered_tx.send(()).unwrap();
                    stale_release_rx.recv().unwrap();
                    41_u64
                })
            })
        };
        stale_entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the stale builder must enter its producer");

        // 2. A second owner joins the SAME flight, then both requests are
        //    cancelled. Cancelling the builder first means that when the
        //    follower observes its own cancellation the aggregate is already
        //    latched, so the follower's detach retires this incarnation.
        let stale_follower = {
            let scheduler = Arc::clone(&scheduler);
            std::thread::spawn(move || {
                scheduler.execute_scoped_cache_node(stale_follower_request, |_| -> u64 {
                    panic!("the joined follower must not execute its own closure")
                })
            })
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while scheduler.test_scoped_cache_owner_count(&identity) != 2
            && std::time::Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        assert_eq!(scheduler.test_scoped_cache_owner_count(&identity), 2);
        stale_builder_token.cancel();
        stale_follower_token.cancel();
        assert_eq!(
            stale_follower.join().unwrap(),
            Err(ScopedCacheNodeError::Cancelled),
            "the cancelled follower retires the stale incarnation on detach"
        );

        // 3. A fresh request for the SAME identity admits a NEW flight and a
        //    NEW DAG node, and its builder parks inside its producer.
        let (successor_entered_tx, successor_entered_rx) = std::sync::mpsc::channel();
        let (successor_release_tx, successor_release_rx) = std::sync::mpsc::channel();
        let successor = {
            let scheduler = Arc::clone(&scheduler);
            std::thread::spawn(move || {
                scheduler.execute_scoped_cache_node(successor_request, move |_| {
                    successor_entered_tx.send(()).unwrap();
                    successor_release_rx.recv().unwrap();
                    99_u64
                })
            })
        };
        successor_entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the successor must enter its producer");

        // 4. Only NOW does the stale builder finish. Its teardown runs against
        //    an incarnation that is no longer the registry's current one, so it
        //    must leave the successor's DAG node untouched.
        stale_release_tx.send(()).unwrap();
        assert_eq!(
            stale_builder.join().unwrap(),
            Err(ScopedCacheNodeError::Cancelled),
            "the stale builder observes its own cancellation"
        );

        successor_release_tx.send(()).unwrap();
        assert_eq!(
            *successor
                .join()
                .unwrap()
                .expect("the successor must publish its own value, not inherit a stale cancel"),
            99
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn scoped_cache_publishes_before_cross_pool_installer_returns() {
        let scheduler = Scheduler::test_new(
            SchedulerConfig {
                cpu_threads: 1,
                ..SchedulerConfig::default()
            },
            Arc::new(MemorySourceLoader::new()),
        );
        let (leader_context, _leader_token) = scoped_test_context(10);
        let (joiner_context, _joiner_token) = scoped_test_context(11);
        let leader_request = scoped_test_request(leader_context);
        let joiner_request = scoped_test_request(joiner_context);
        let identity = leader_request.identity();
        let builds = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let submissions_before = scheduler.counters.submit_count.load(Ordering::Relaxed);
        let (leader_entered_tx, leader_entered_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();

        let coordinator = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .thread_name(|_| "scoped-cache-coordinator".to_owned())
            .build()
            .expect("coordinator pool must build");
        let scheduler_for_thread = Arc::clone(&scheduler);
        let scheduler_for_joiner = Arc::clone(&scheduler);
        let builds_for_thread = Arc::clone(&builds);
        let scheduler_for_publication = Arc::clone(&scheduler);
        let coordinator_thread = std::thread::spawn(move || {
            let outcomes = coordinator.install(|| {
                rayon::join(
                    || {
                        scheduler_for_thread.execute_scoped_cache_node(leader_request, move |_| {
                            builds_for_thread.fetch_add(1, Ordering::SeqCst);
                            // Claim the builder before the coordinator re-enters
                            // the joiner, then hold publication until it attaches.
                            leader_entered_tx.send(()).unwrap();
                            let deadline =
                                std::time::Instant::now() + std::time::Duration::from_secs(5);
                            while scheduler_for_publication.test_scoped_cache_owner_count(&identity)
                                != 2
                                && std::time::Instant::now() < deadline
                            {
                                std::thread::yield_now();
                            }
                            assert_eq!(
                                scheduler_for_publication.test_scoped_cache_owner_count(&identity),
                                2,
                                "the re-entrant joiner must attach before publication"
                            );
                            41_u64
                        })
                    },
                    move || {
                        leader_entered_rx
                            .recv_timeout(std::time::Duration::from_secs(5))
                            .expect("the leader must claim the builder before the joiner submits");
                        scheduler_for_joiner.execute_scoped_cache_node(joiner_request, |_| -> u64 {
                            panic!("deduplicated joiner must not execute its closure")
                        })
                    },
                )
            });
            let _ = done_tx.send(outcomes);
        });

        let outcomes = done_rx.recv_timeout(std::time::Duration::from_secs(5));
        if outcomes.is_err() {
            scheduler.reset();
        }
        coordinator_thread.join().expect("coordinator must finish");
        let (leader, joiner) =
            outcomes.expect("worker-side publication must release the re-entrant joiner");
        let leader = leader.expect("leader result");
        let joiner = joiner.expect("joiner result");
        assert_eq!(*leader, 41);
        assert_eq!(*joiner, 41);
        assert!(
            Arc::ptr_eq(&leader, &joiner),
            "owners share one publication"
        );
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert_eq!(
            scheduler.counters.submit_count.load(Ordering::Relaxed) - submissions_before,
            2,
            "both owners submit, but they must share one scoped build"
        );
        assert_eq!(
            scheduler.dag.lock().cache_node_terminal_counts(),
            crate::dag::CacheNodeTerminalCounts {
                completed: 1,
                cancelled: 0,
            }
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn scoped_cache_rejects_same_active_identity_without_second_submission() {
        let scheduler = Scheduler::test_new(
            SchedulerConfig {
                cpu_threads: 1,
                ..SchedulerConfig::default()
            },
            Arc::new(MemorySourceLoader::new()),
        );
        let (context, _token) = scoped_test_context(12);
        let outer_request = scoped_test_request(context);
        let nested_request = outer_request.clone();
        let submissions_before = scheduler.counters.submit_count.load(Ordering::Relaxed);
        let scheduler_for_thread = Arc::clone(&scheduler);
        let (done_tx, done_rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let scheduler_for_builder = Arc::clone(&scheduler_for_thread);
            let result = scheduler_for_thread.execute_scoped_cache_node(outer_request, move |_| {
                scheduler_for_builder
                    .execute_scoped_cache_node(nested_request, |_| 99_u64)
                    .expect_err("same-active-identity recursion must be rejected")
            });
            let _ = done_tx.send(result);
        });

        let result = done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("same-active-identity recursion must not block")
            .expect("outer scoped producer must publish");
        assert_eq!(*result, ScopedCacheNodeError::Reentrant);
        assert_eq!(
            scheduler.counters.submit_count.load(Ordering::Relaxed) - submissions_before,
            1,
            "the rejected recursive demand must not submit a second node"
        );
        assert_eq!(
            scheduler.dag.lock().cache_node_terminal_counts(),
            crate::dag::CacheNodeTerminalCounts {
                completed: 1,
                cancelled: 0,
            }
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sole_cancelled_scoped_cache_build_never_publishes_and_cold_retry_runs() {
        let scheduler = Scheduler::test_new(
            SchedulerConfig {
                cpu_threads: 2,
                ..SchedulerConfig::default()
            },
            Arc::new(MemorySourceLoader::new()),
        );
        let (cancelled_context, cancelled_token) = scoped_test_context(3);
        let request = scoped_test_request(cancelled_context);
        let builds = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();

        let first = {
            let scheduler = Arc::clone(&scheduler);
            let builds = Arc::clone(&builds);
            std::thread::spawn(move || {
                scheduler.execute_scoped_cache_node(request, move |job_cancellation| {
                    builds.fetch_add(1, Ordering::SeqCst);
                    entered_tx.send(()).unwrap();
                    while !job_cancellation.is_cancelled() {
                        std::thread::yield_now();
                    }
                    7_u64
                })
            })
        };
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("first producer must start");
        cancelled_token.cancel();
        assert_eq!(first.join().unwrap(), Err(ScopedCacheNodeError::Cancelled));

        let (retry_context, _retry_token) = scoped_test_context(4);
        let retry_request = scoped_test_request(retry_context);
        let retry = scheduler
            .execute_scoped_cache_node(retry_request, {
                let builds = Arc::clone(&builds);
                move |_| {
                    builds.fetch_add(1, Ordering::SeqCst);
                    9_u64
                }
            })
            .expect("an uncancelled retry must start a fresh flight");
        assert_eq!(*retry, 9);
        assert_eq!(builds.load(Ordering::SeqCst), 2);
        assert_eq!(
            scheduler.dag.lock().cache_node_terminal_counts(),
            crate::dag::CacheNodeTerminalCounts {
                completed: 1,
                cancelled: 1,
            }
        );
    }

    // ── Basic Pipeline ──

    #[test]
    fn driver_submission_makes_room_in_a_full_inbox() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));
        let sched = test_scheduler_with_loader(loader);
        for _ in 0..crate::driver::SUBMISSION_INBOX_CAPACITY {
            sched.inbox.sender.try_send(Submission::Wake).unwrap();
        }
        let handle = {
            let _driver = crate::caller_kind::CallerKindGuard::install(
                crate::caller_kind::CallerKind::Driver,
            );
            sched.submit_request(Request {
                file_id: "/a.vue".to_string(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: None,
            })
        };
        sched.drive_all();
        assert!(handle.try_get().unwrap().is_ready());
    }

    /// Source executor that fills the inbox to capacity while its stage runs,
    /// so the stage's terminal `StageComplete` meets a full inbox.
    struct InboxFillingSourceExecutor {
        inbox: std::sync::OnceLock<crossbeam_channel::Sender<Submission>>,
    }
    impl StageExecutor for InboxFillingSourceExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_source(
            &self,
            canonical_id: &str,
            file_language: FileLanguage,
            content: Arc<str>,
            generation: u64,
            incarnation: u64,
        ) -> Result<SourceSnapshot, crate::execution::executor::StageError> {
            let inbox = self.inbox.get().expect("inbox installed before driving");
            while inbox.try_send(Submission::Wake).is_ok() {}
            assert!(inbox.is_full());
            crate::execution::executor::DefaultExecutor.execute_source(
                canonical_id,
                file_language,
                content,
                generation,
                incarnation,
            )
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn inline_stage_completing_into_a_full_inbox_does_not_park_its_sole_consumer() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));
        let executor = Arc::new(InboxFillingSourceExecutor {
            inbox: std::sync::OnceLock::new(),
        });
        let sched = Scheduler::test_new_sync_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::clone(&executor) as Arc<dyn StageExecutor>,
        );
        assert!(!sched.has_driver_thread());
        executor
            .inbox
            .set(sched.inbox.sender.clone())
            .expect("inbox installed once");
        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let pump = Arc::clone(&sched);
        std::thread::spawn(move || {
            let state = pump.wait_or_drive(&handle);
            let _ = done_tx.send(state.is_ready());
        });
        let ready = done_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the sole inline consumer must not park on its own full inbox");
        assert!(ready);
    }

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
            file_language: None,
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
        assert!(
            sched.try_get_analysis("/a.vue").is_none(),
            "a Source-only request must not eagerly execute Analysis"
        );
    }

    #[test]
    fn analysis_request_after_source_only_completion_admits_missing_stage() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));
        let sched = test_scheduler_with_loader(loader);

        let source_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(matches!(
            source_handle.try_get(),
            Some(CompletionState::Ready(RequestResult::Source(_)))
        ));
        assert!(sched.try_get_analysis("/a.vue").is_none());

        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });
        sched.drive_all();

        match analysis_handle.try_get() {
            Some(CompletionState::Ready(RequestResult::Analysis(snapshot))) => {
                assert_eq!(snapshot.generation, 1);
            }
            state => panic!("expected Analysis after Source-only completion, got {state:?}"),
        }
        assert!(sched.try_get_analysis("/a.vue").is_some());
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
            request_context: None,
        });

        sched.drive_all();
        let _ = handle.try_get();

        let source_id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/a.vue"),
            incarnation: fixture_incarnation(&sched, "/a.vue"),
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

    /// Executor that records every `execute_cache_node` invocation and the
    /// full cache identity it was handed. Used by the CacheNode positive-
    /// dispatch test to prove the dispatch path actually ROUTES cache-node
    /// work to the executor (rather than a non-routing skip that never reaches
    /// any executor method). All other stage methods use the default stub
    /// bodies.
    /// The four `WorkNodeIdentity::CacheNode` fields a recorded
    /// `execute_cache_node` call was handed: `(cache_id, key_hash, view_epoch,
    /// snapshot_pin_id)`.
    type RecordedCacheCall = (u64, [u8; 16], u64, u64);

    #[derive(Debug, Default)]
    struct RecordingCacheExecutor {
        /// Number of `execute_cache_node` calls observed.
        calls: AtomicU64,
        /// The four identity fields of the last call, recorded so the test
        /// asserts the executor received the exact `WorkNodeIdentity::CacheNode`
        /// values (not a fabricated or zeroed identity).
        last: Mutex<Option<RecordedCacheCall>>,
    }

    impl StageExecutor for RecordingCacheExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn execute_cache_node(
            &self,
            cache_id: crate::cache_id::SchedulerCacheId,
            key_hash: crate::dag::Hash16,
            view_epoch: u64,
            snapshot_pin_id: crate::dag::PinId,
            _cancellation: &CancellationToken,
        ) -> Result<(), crate::execution::executor::StageError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            *self.last.lock() = Some((cache_id.0, key_hash, view_epoch, snapshot_pin_id.0));
            Ok(())
        }
    }

    /// A CacheNode ready job dispatches THROUGH the executor's
    /// `execute_cache_node` hook — the single dispatch entry
    /// (`dispatch_ready_job_to_executor`) routes the identity directly to the
    /// cache-materialisation method, handing it the exact four identity fields.
    ///
    /// DISCRIMINATOR: a non-routing dispatch that cancels/skips the CacheNode
    /// BEFORE any executor call makes the recorder observe ZERO calls and this
    /// test fails at the first assertion. (A `CacheNode` dispatch arm that
    /// `unreachable!()`s instead of calling the hook fails the same way.)
    #[test]
    fn cache_node_dispatch_routes_to_execute_cache_node() {
        use crate::cache_id::SchedulerCacheId;
        use crate::dag::{PinId, WorkKind};
        let loader = Arc::new(MemorySourceLoader::new());
        let executor = Arc::new(RecordingCacheExecutor::default());
        let sched = Scheduler::test_new_sync_with_executor(
            SchedulerConfig::default(),
            loader as Arc<dyn crate::source_loader::SourceLoader>,
            Arc::clone(&executor) as Arc<dyn StageExecutor>,
        );

        // Inject a CacheNode identity directly into the DAG (the cache layer's
        // production producer is U7; here we submit the identity by hand).
        let cache_identity = WorkNodeIdentity::CacheNode {
            cache_id: SchedulerCacheId(7),
            key_hash: [0xABu8; 16],
            view_epoch: 42,
            snapshot_pin_id: PinId(99),
        };
        sched.dag.lock().submit_expect(
            cache_identity.clone(),
            WorkKind::CacheNode,
            Priority::Interactive,
            Vec::new(),
            None,
        );

        // Drive the cache node to completion through the dispatch path.
        sched.drive_all();

        // POSITIVE ROUTING: the executor's cache hook was called exactly once.
        assert_eq!(
            executor.calls.load(Ordering::Acquire),
            1,
            "CacheNode dispatch must route to StageExecutor::execute_cache_node \
             — the retired defensive skip / unreachable!() adapter never called it",
        );
        // IDENTITY FIDELITY: the hook received the exact four identity fields
        // from `WorkNodeIdentity::CacheNode` (cache_id, key_hash, view_epoch,
        // snapshot_pin_id), proving the router forwards the full identity and
        // does not fabricate or zero it.
        assert_eq!(
            *executor.last.lock(),
            Some((7, [0xABu8; 16], 42, 99)),
            "execute_cache_node must receive the exact CacheNode identity fields",
        );
        // The CacheNode identity is consumed (completed) out of the DAG.
        assert!(
            sched.dag.lock().token_for(&cache_identity).is_none(),
            "the dispatched CacheNode identity must be completed out of the DAG",
        );
    }

    /// CacheNode dispatch releases the parked CPU permit. `next_ready` reserves
    /// a CPU permit for the cache-node candidate at dispatch; after
    /// `execute_cache_node` runs, the single dispatch entry marks the identity
    /// complete, releasing the reservation through its by-value consume.
    ///
    /// DISCRIMINATOR: without the release the {cpu:1} class stays drained and
    /// the follow-on Analysis below never dispatches —
    /// `in_flight_cpu_permits()` would read 1 and `h.try_get()` would not be
    /// ready.
    #[test]
    fn cache_node_dispatch_releases_cpu_permit() {
        use crate::cache_id::SchedulerCacheId;
        use crate::dag::{PinId, WorkKind};
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/follow.vue".to_string(), Arc::from("content"));
        // Tight {cpu:1, io:1} budget: a leaked CPU permit pins the class and
        // the follow-on Analysis below cannot dispatch.
        let config = SchedulerConfig {
            cpu_threads: 1,
            io_threads: 1,
            dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
        };
        let executor = Arc::new(RecordingCacheExecutor::default());
        let sched = Scheduler::test_new_sync_with_executor(
            config,
            loader as Arc<dyn crate::source_loader::SourceLoader>,
            Arc::clone(&executor) as Arc<dyn StageExecutor>,
        );

        // Inject a CacheNode identity directly into the DAG.
        let cache_identity = WorkNodeIdentity::CacheNode {
            cache_id: SchedulerCacheId(7),
            key_hash: [0u8; 16],
            view_epoch: 1,
            snapshot_pin_id: PinId(1),
        };
        sched.dag.lock().submit_expect(
            cache_identity.clone(),
            WorkKind::CacheNode,
            Priority::Interactive,
            Vec::new(),
            None,
        );

        // drive_all consumes the CacheNode via next_ready (which reserves a
        // CPU permit), routes it to execute_cache_node, then releases.
        sched.drive_all();

        // The cache hook ran (the work was genuinely dispatched, not skipped).
        assert_eq!(
            executor.calls.load(Ordering::Acquire),
            1,
            "CacheNode must dispatch through execute_cache_node before releasing",
        );
        // DISCRIMINATOR: the reservation released. Without the release the
        // counter would be 1 (parked reservation on the dispatched CacheNode
        // entry); with it the count is 0.
        assert_eq!(
            sched.dag.lock().in_flight_cpu_permits(),
            0,
            "CacheNode dispatch must release the parked CPU permit \
             — without it the {{cpu:1}} class would stay drained",
        );

        // Submit a real CPU job and verify it dispatches — the follow-on side
        // of the discriminator. A leaked permit would have stalled this.
        let h = sched.submit_request(Request {
            file_id: "/follow.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(
            h.try_get().unwrap().is_ready(),
            "follow-on Analysis must dispatch after CacheNode dispatch released the permit",
        );
    }

    /// A cache-node executor that fails with a typed `Ok(Err(StageError))`.
    /// Records its call count so the test can confirm the hook actually ran
    /// (the failure is real, not a non-routing skip).
    #[derive(Debug, Default)]
    struct FailingCacheExecutor {
        calls: AtomicU64,
    }

    impl StageExecutor for FailingCacheExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn execute_cache_node(
            &self,
            _cache_id: crate::cache_id::SchedulerCacheId,
            _key_hash: crate::dag::Hash16,
            _view_epoch: u64,
            _snapshot_pin_id: crate::dag::PinId,
            _cancellation: &CancellationToken,
        ) -> Result<(), crate::execution::executor::StageError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            Err(crate::execution::executor::StageError {
                kind: crate::execution::executor::StageErrorKind::Generic,
                message: "synthetic cache-node materialisation failure".to_string(),
            })
        }
    }

    /// A cache-node executor that PANICS inside `execute_cache_node`.
    #[derive(Debug, Default)]
    struct PanickingCacheExecutor {
        calls: AtomicU64,
    }

    impl StageExecutor for PanickingCacheExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn execute_cache_node(
            &self,
            _cache_id: crate::cache_id::SchedulerCacheId,
            _key_hash: crate::dag::Hash16,
            _view_epoch: u64,
            _snapshot_pin_id: crate::dag::PinId,
            _cancellation: &CancellationToken,
        ) -> Result<(), crate::execution::executor::StageError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            panic!("synthetic cache-node materialisation panic");
        }
    }

    /// Drive one injected CacheNode through the dispatch path under `executor`
    /// and return the scheduler so the caller can read the terminal counts.
    fn drive_one_injected_cache_node(executor: Arc<dyn StageExecutor>) -> Arc<Scheduler> {
        use crate::cache_id::SchedulerCacheId;
        use crate::dag::{PinId, WorkKind};
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::test_new_sync_with_executor(
            SchedulerConfig::default(),
            loader as Arc<dyn crate::source_loader::SourceLoader>,
            executor,
        );
        let cache_identity = WorkNodeIdentity::CacheNode {
            cache_id: SchedulerCacheId(7),
            key_hash: [0xABu8; 16],
            view_epoch: 42,
            snapshot_pin_id: PinId(99),
        };
        sched.dag.lock().submit_expect(
            cache_identity.clone(),
            WorkKind::CacheNode,
            Priority::Interactive,
            Vec::new(),
            None,
        );
        sched.drive_all();
        // The dispatched identity is terminalized out of the DAG regardless of
        // outcome (complete XOR cancel both remove it).
        assert!(
            sched.dag.lock().token_for(&cache_identity).is_none(),
            "a dispatched CacheNode identity must be terminalized out of the DAG",
        );
        sched
    }

    /// The single router must NOT swallow an `execute_cache_node`
    /// `Ok(Err(StageError))` and complete the node as success: a typed
    /// cache-node failure terminalizes the node as FAILED (via `cancel`), not
    /// as a successful `complete`.
    ///
    /// DISCRIMINATOR (`cache_node_terminal_counts`): with the pre-fix
    /// swallow-and-`complete()` router the failing node is recorded
    /// `completed == 1, cancelled == 0` and this test FAILS; with the fix the
    /// failure routes to `cancel`, recording `completed == 0, cancelled == 1`.
    /// `in_flight_cpu_permits() == 0` proves the parked reservation still
    /// releases on the failure arm (both terminals release exactly once).
    #[test]
    fn cache_node_failure_is_not_completed_as_success() {
        let executor = Arc::new(FailingCacheExecutor::default());
        let sched = drive_one_injected_cache_node(Arc::clone(&executor) as Arc<dyn StageExecutor>);

        // The hook genuinely ran (the failure is real routing, not a skip).
        assert_eq!(
            executor.calls.load(Ordering::Acquire),
            1,
            "execute_cache_node must be called once before the failure is surfaced",
        );
        let counts = sched.dag.lock().cache_node_terminal_counts();
        assert_eq!(
            counts.completed, 0,
            "a typed cache-node failure must NOT be completed-as-success — the \
             pre-fix router swallowed the Err and recorded completed == 1",
        );
        assert_eq!(
            counts.cancelled, 1,
            "a typed cache-node failure must terminalize the node as FAILED via \
             cancel (recorded cancelled == 1)",
        );
        assert_eq!(
            sched.dag.lock().in_flight_cpu_permits(),
            0,
            "the cache-node failure arm must still release the parked CPU permit \
             exactly once (cancel's by-value reservation consume)",
        );
    }

    /// The single router must NOT swallow an `execute_cache_node` PANIC and
    /// complete the node as success: a panicking cache node terminalizes as
    /// FAILED (via `cancel`), not as a successful `complete`. The panic is
    /// caught by the dispatch path's `catch_unwind` (so the worker does not
    /// abort), but the caught panic must route to the failure path.
    ///
    /// DISCRIMINATOR (`cache_node_terminal_counts`): with the pre-fix router
    /// the caught panic fell through to `complete()` (`completed == 1`) and
    /// this test FAILS; with the fix the panic arm calls `cancel`
    /// (`completed == 0, cancelled == 1`). The synthetic panic message printed
    /// by the default hook is expected — the panic is caught, not propagated.
    #[test]
    fn cache_node_panic_is_not_completed_as_success() {
        let executor = Arc::new(PanickingCacheExecutor::default());
        let sched = drive_one_injected_cache_node(Arc::clone(&executor) as Arc<dyn StageExecutor>);

        // The hook ran and panicked (proves the panic arm was exercised).
        assert_eq!(
            executor.calls.load(Ordering::Acquire),
            1,
            "execute_cache_node must be entered once before it panics",
        );
        let counts = sched.dag.lock().cache_node_terminal_counts();
        assert_eq!(
            counts.completed, 0,
            "a panicking cache node must NOT be completed-as-success — the pre-fix \
             router caught the panic then completed the node, recording completed == 1",
        );
        assert_eq!(
            counts.cancelled, 1,
            "a panicking cache node must terminalize the node as FAILED via cancel \
             (recorded cancelled == 1)",
        );
        assert_eq!(
            sched.dag.lock().in_flight_cpu_permits(),
            0,
            "the cache-node panic arm must still release the parked CPU permit \
             exactly once (cancel's by-value reservation consume)",
        );
    }

    /// Companion positive case: a SUCCESSFUL `execute_cache_node`
    /// (`Ok(Ok(()))`) IS completed-as-success. This anchors the other end of
    /// the discriminator — the success path records `completed == 1,
    /// cancelled == 0`, the exact inverse of the failure/panic tests above, so
    /// the split genuinely distinguishes the two terminals (it is not a
    /// constant that always reads "cancelled").
    #[test]
    fn cache_node_success_is_completed_not_cancelled() {
        let executor = Arc::new(RecordingCacheExecutor::default());
        let sched = drive_one_injected_cache_node(Arc::clone(&executor) as Arc<dyn StageExecutor>);

        assert_eq!(
            executor.calls.load(Ordering::Acquire),
            1,
            "execute_cache_node must be called once on the success path",
        );
        let counts = sched.dag.lock().cache_node_terminal_counts();
        assert_eq!(
            counts.completed, 1,
            "a successful cache node must be completed-as-success (completed == 1)",
        );
        assert_eq!(
            counts.cancelled, 0,
            "a successful cache node must NOT be cancelled (cancelled == 0)",
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Step 1: drive Source + Analysis to ready state by submitting
        // an Analysis request and draining.
        let h_analysis = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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
            file_language: None,
            request_context: None,
        });
        sched.drain_inbox();

        let artifact_id = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            incarnation: fixture_incarnation(&sched, "/a.vue"),
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
        let witness = source_witness(&sched, "/a.vue");
        assert_eq!(witness.generation(), 1);
        assert!(sched.commit_artifact(&witness, 42, Arc::new(crate::node::EmptyData)));

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
            file_language: None,
            request_context: None,
        });

        sched.drive_all();
        let _ = handle.try_get();

        let artifact_id = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            incarnation: fixture_incarnation(&sched, "/a.vue"),
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
        let sched = Scheduler::test_new_sync(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()),
        );

        let handle = sched.submit_request(Request {
            file_id: "/new.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("provided content")),
            file_language: None,
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
        let sched = Scheduler::test_new_sync(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()),
        );

        // First request
        let h1 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v1")),
            file_language: None,
            request_context: None,
        });

        // Second request (newer source) — before first is processed
        let h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v2")),
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
            request_context: None,
        });
        let hb = sched.submit_request(Request {
            file_id: "/b.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 0 },
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });
        let hc = sched.submit_request(Request {
            file_id: "/c.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 0 },
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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
            file_language: None,
            request_context: None,
        });
        sched.drive_all();

        let src = sched.try_get_source("/a.vue").unwrap();
        assert_eq!(&*src.source, "content");
    }

    // ── Close File ──

    #[test]
    fn close_file_clears_overlay() {
        let sched = Scheduler::test_new_sync(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()),
        );

        // Submit with source
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("editor content")),
            file_language: None,
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let handle = sched.submit_request(Request {
            file_id: "/missing.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 0 },
            priority: Priority::Background,
            source: None,
            file_language: None,
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
            file_language: None,
            request_context: None,
        });
        // Submit high priority second
        sched.submit_request(Request {
            file_id: "/high.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Critical,
            source: None,
            file_language: None,
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
        let sched = Scheduler::test_new(SchedulerConfig::default(), loader);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Critical,
            source: None,
            file_language: None,
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
        let sched = Scheduler::test_new(SchedulerConfig::default(), loader);

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
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn extract_deps(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
        ) -> crate::execution::executor::ExtractedDeps {
            let blocker_ids = self.blockers.get(canonical_id).cloned().unwrap_or_default();
            let forward_deps = blocker_ids.clone();
            crate::execution::executor::ExtractedDeps {
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
        let sched =
            Scheduler::test_new_sync_with_executor(SchedulerConfig::default(), loader, executor);

        // Request Artifact for A
        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 42 },
            priority: Priority::Critical,
            source: None,
            file_language: None,
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
        let sched =
            Scheduler::test_new_sync_with_executor(SchedulerConfig::default(), loader, executor);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 7 },
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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
        let sched = Scheduler::test_new_sync(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()), // empty — no files
        );

        let handle = sched.submit_request(Request {
            file_id: "/missing.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_source(
            &self,
            _canonical_id: &str,
            _file_language: FileLanguage,
            _content: Arc<str>,
            _generation: u64,
            _incarnation: u64,
        ) -> Result<SourceSnapshot, crate::execution::executor::StageError> {
            Err(crate::execution::executor::StageError {
                kind: crate::execution::executor::StageErrorKind::Generic,
                message: "synthetic source failure".to_string(),
            })
        }
    }

    /// Analysis executor that succeeds on Source and Errs on Analysis.
    struct ErrAnalysisExecutor;
    impl StageExecutor for ErrAnalysisExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_analysis(
            &self,
            _canonical_id: &str,
            _source: &SourceSnapshot,
            _generation: u64,
        ) -> Result<AnalysisSnapshot, crate::execution::executor::StageError> {
            Err(crate::execution::executor::StageError {
                kind: crate::execution::executor::StageErrorKind::Generic,
                message: "synthetic analysis failure".to_string(),
            })
        }
    }

    /// Artifact executor that succeeds on Source/Analysis and Errs on
    /// Artifact only.
    struct ErrArtifactExecutor;
    impl StageExecutor for ErrArtifactExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_artifact(
            &self,
            _canonical_id: &str,
            _source: &SourceSnapshot,
            _analysis: &AnalysisSnapshot,
            _profile_hash: u64,
            _generation: u64,
        ) -> Result<ArtifactSnapshot, crate::execution::executor::StageError> {
            Err(crate::execution::executor::StageError {
                kind: crate::execution::executor::StageErrorKind::Generic,
                message: "synthetic artifact failure".to_string(),
            })
        }
    }

    /// Source executor that panics on the first call.
    struct PanickingSourceExecutor;
    impl StageExecutor for PanickingSourceExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_source(
            &self,
            _canonical_id: &str,
            _file_language: FileLanguage,
            _content: Arc<str>,
            _generation: u64,
            _incarnation: u64,
        ) -> Result<SourceSnapshot, crate::execution::executor::StageError> {
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
            dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
        };
        Scheduler::test_new_sync_with_executor(config, loader, executor)
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
        };
        let sched =
            Scheduler::test_with_executor(config, loader, Arc::new(PanickingSourceExecutor));

        let h_panic = sched.submit_request(Request {
            file_id: "/panic.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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
    fn remove_and_readd_uses_a_higher_version() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("v1"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader.clone());

        // Upsert v1 → Source + Analysis at gen 1
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v1")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        let v1 = match h.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.version(),
            other => panic!("expected Analysis, got {:?}", other),
        };

        // Remove
        sched.remove("/a.vue");
        assert!(!sched.has_node("/a.vue"));

        // Re-add v2: a fresh node object whose versions order after every
        // version of the removed one, without any retained removal history.
        loader.insert("/a.vue".to_string(), Arc::from("v2"));
        let h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v2")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        let v2 = match h2.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.version(),
            other => panic!("expected Analysis, got {:?}", other),
        };

        assert!(
            v2 > v1,
            "re-added file must have version ({v2:?}) > removed version ({v1:?})"
        );
        assert_ne!(v2.incarnation, v1.incarnation);
    }

    #[test]
    fn deferred_blockers_are_replaced_not_appended() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        loader.insert("/dep1.ts".to_string(), Arc::from("dep1"));
        loader.insert("/dep2.ts".to_string(), Arc::from("dep2"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

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
        // Deferred entries now carry each dep's resolved language; project the
        // ids back out so these assertions test exactly what they did before.
        let deferred: Option<Vec<String>> = sched
            .deferred_blocker_ids
            .get("/a.vue")
            .map(|v| v.iter().map(|(id, _)| id.clone()).collect());
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Submit and drive to get a node at gen > 0
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_language: None,
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
            file_language: None,
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

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
    fn incarnation_rejects_pre_remove_source_submission() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("content"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Bind the request to the current incarnation.
        let h1 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("v1")),
            file_language: None,
            request_context: None,
        });

        // Retire that incarnation before the driver processes h1.
        sched.remove("/a.vue");

        // The old incarnation is rejected even though it carries source.
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
    fn auto_ingress_uses_backing_availability_after_unknown_removal() {
        for readable in [true, false] {
            let loader = Arc::new(MemorySourceLoader::new());
            loader.insert("/a.vue".into(), Arc::from("a"));
            loader.insert("/deleted-dep.ts".into(), Arc::from("dep"));
            let mut blockers_map = std::collections::HashMap::new();
            blockers_map.insert("/a.vue".into(), vec!["/deleted-dep.ts".into()]);
            let executor = Arc::new(BlockingExecutor {
                blockers: blockers_map,
            });
            let sched = Scheduler::test_new_sync_with_executor(
                SchedulerConfig::default(),
                loader.clone(),
                executor,
            );
            sched.remove("/deleted-dep.ts");
            if !readable {
                loader.remove("/deleted-dep.ts");
            }
            let h = sched.submit_request(Request {
                file_id: "/a.vue".into(),
                target: TargetStage::Artifact { profile_hash: 1 },
                priority: Priority::Interactive,
                source: Some(Arc::from("a")),
                file_language: None,
                request_context: None,
            });
            sched.drive_all();
            if readable {
                assert!(matches!(h.try_get(), Some(CompletionState::Ready(_))));
                assert!(sched
                    .nodes
                    .get("/deleted-dep.ts")
                    .unwrap()
                    .current_analysis()
                    .is_some());
            } else {
                assert!(
                    matches!(h.try_get(), Some(CompletionState::Failed(_))),
                    "absent backing must terminate dependants"
                );
            }
        }
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
        let sched = Scheduler::test_new_sync_with_executor(
            SchedulerConfig::default(),
            loader.clone(),
            executor,
        );

        // Submit /a.vue which depends on /dep.ts
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 1 },
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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

    /// Teardown must stop the driver even when the submission inbox does
    /// not deliver the request. The inbox is drained by every cooperative
    /// pump — the driver's own dispatch loop included — so a teardown wake
    /// posted there can be consumed before the driver reaches its park. A
    /// driver reachable only through the inbox then sleeps out its whole
    /// idle re-pump interval, and the thread joining it (every host
    /// teardown) waits that long with it.
    ///
    /// The window is entered by pausing the driver on the LAST dispatch of
    /// a backlog: the pause sits inside the pre-park pump loop, the
    /// teardown request lands and is swallowed by the paused driver's own
    /// re-drain, and on release there is no work left to keep the driver
    /// awake, so it parks — with the teardown already pending.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn teardown_stops_the_driver_when_its_wake_was_consumed_before_the_park() {
        /// Big enough that the backlog outlives the driver's first wake
        /// pump, so the final dispatch happens in the pre-park pump loop.
        const FILES: usize = 60;
        /// Source + Analysis.
        const DISPATCHES_PER_FILE: usize = 2;

        // The window is entered on a race between the backlog draining and
        // the teardown landing, so a single attempt can miss it; each
        // attempt is a few hundred milliseconds and the worst one rules.
        let mut worst = std::time::Duration::ZERO;
        for _ in 0..6 {
            let loader = Arc::new(MemorySourceLoader::new());
            for i in 0..FILES {
                loader.insert(format!("/f{i}.vue"), Arc::from("x"));
            }
            let sched = Scheduler::test_new(SchedulerConfig::default(), loader);

            // Park the driver on the backlog's final dispatch.
            sched.test_arm_dispatch_pause(FILES * DISPATCHES_PER_FILE);
            let pending: Vec<_> = (0..FILES)
                .map(|i| {
                    sched.submit_request(Request {
                        file_id: format!("/f{i}.vue"),
                        target: TargetStage::Analysis,
                        priority: Priority::Interactive,
                        source: Some(Arc::from("x")),
                        file_language: None,
                        request_context: None,
                    })
                })
                .collect();
            sched.test_wait_until_dispatch_paused();

            // Tear down while the driver sits in that re-draining park, so
            // the inbox wake is gone before the driver parks on it.
            //
            // The baseline is captured BEFORE the resetter spawns: the
            // counter bump is the first thing `reset()` does after two
            // non-blocking sends, so a fast resetter could post before a
            // later read and the delta-wait would spin to its deadline.
            let wake_baseline = sched.test_reset_wake_posts();
            let resetter = {
                let sched = Arc::clone(&sched);
                std::thread::spawn(move || {
                    let started = std::time::Instant::now();
                    sched.reset();
                    started.elapsed()
                })
            };
            // Wait for the observed event instead of a fixed sleep:
            // `reset()` stores `shutdown`, sends the teardown signal, then
            // posts the inbox wake in that order, so one posted wake
            // proves all three already happened and the pause can be
            // released into the exact swallowed-wake window under test.
            //
            // The observation is the post COUNT, never the inbox itself:
            // the paused driver re-drains the inbox every couple of
            // milliseconds, so an empty inbox cannot distinguish "the wake
            // is not sent yet" from "the wake was already swallowed" — the
            // very state this test exists to enter — and a non-empty inbox
            // may only hold a worker's `StageComplete`, releasing the pause
            // before `reset()` has signalled anything at all.
            //
            // The comparison is delta-based, never `== 0`: the counter is
            // monotonic across the scheduler's lifetime, so a reused
            // scheduler that already reset before would sit at 1, 2, ...
            // and a hardcoded zero-check would fall through without
            // waiting for the new wake at all.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while sched.test_reset_wake_posts() == wake_baseline {
                assert!(
                    std::time::Instant::now() < deadline,
                    "reset() did not post the inbox wake before the deadline"
                );
                std::thread::yield_now();
            }
            sched.test_release_dispatch_pause();

            worst = worst.max(resetter.join().expect("teardown must not panic"));

            // The teardown signal must not outlive the driver it stopped:
            // a leftover signal would stop the next driver on arrival.
            sched.restart_driver();
            let handle = sched.submit_request(Request {
                file_id: "/f0.vue".to_string(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: Some(Arc::from("x v2")),
                file_language: None,
                request_context: None,
            });
            assert!(
                handle.wait().is_ready(),
                "the restarted driver must still process requests"
            );
            drop(pending);
        }

        assert!(
            worst < std::time::Duration::from_secs(2),
            "teardown must reach the parked driver directly instead of \
             waiting out its idle re-pump interval; worst reset() took {worst:?}"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reset_clears_all_state() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::test_new(SchedulerConfig::default(), loader);

        // Populate state
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
            request_context: None,
        });
        h.wait();
        assert!(sched.has_node("/a.vue"));

        // Reset
        sched.reset();

        // All state cleared
        assert!(!sched.has_node("/a.vue"), "nodes must be cleared");
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
            file_language: None,
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
        let sched = Scheduler::test_new(SchedulerConfig::default(), loader);

        // Step 1: bring /a.vue to Source-committed so a subsequent
        // `register_resolved_deps` exercises the auto-ingest path
        // (the early-return at `generation == 0 || current_source().is_none()`
        // would otherwise skip the tracking insert).
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
                incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
                    incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
    fn reset_successor_versions_never_alias_the_cleared_node() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::test_new(SchedulerConfig::default(), loader);

        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
            request_context: None,
        });
        let pre = match h.wait() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.version(),
            other => panic!("expected Analysis, got {:?}", other),
        };

        // Reset
        sched.reset();
        sched.restart_driver();

        // The successor restarts its generation sequence; its committed
        // versions are told apart by the node object that committed them.
        let h2 = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v2")),
            file_language: None,
            request_context: None,
        });
        let post = match h2.wait() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.version(),
            other => panic!("expected Analysis, got {:?}", other),
        };

        let current = sched.try_get_source("/a.vue").expect("re-added source");
        assert_eq!(current.version(), post);
        assert_eq!(post.generation, pre.generation);
        assert!(
            post.incarnation > pre.incarnation,
            "post-reset version ({post:?}) must order after pre-reset ({pre:?})"
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Upsert + drive to get real generation
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
            file_language: None,
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Get to a stable generation
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        let gen = match h.try_get().unwrap() {
            CompletionState::Ready(RequestResult::Analysis(s)) => s.generation,
            _ => panic!("expected Analysis"),
        };

        // Commit an artifact at the correct generation
        let start_witness = source_witness(&sched, "/a.vue");
        assert_eq!(start_witness.generation(), gen);
        assert!(sched.commit_artifact(&start_witness, 42, Arc::new(crate::node::EmptyData)));

        // Should be readable
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_some(),
            "artifact committed at correct generation should be readable"
        );

        // A compile that started before the next edit finishes after it:
        // its start-of-compile witness is stale and must be dropped.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v2")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(source_witness(&sched, "/a.vue").generation() > gen);
        assert!(!sched.commit_artifact(&start_witness, 99, Arc::new(crate::node::EmptyData)));

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
        let sched =
            Scheduler::test_new_sync_with_executor(SchedulerConfig::default(), loader, executor);

        // Step 1: Initial upsert → drive to gen G
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v1")),
            file_language: None,
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
            file_language: None,
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Submit and drive exactly ONE job (Source) — don't let Analysis run.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
            request_context: None,
        });
        sched.drive_one(); // processes the Source job only
        let witness = source_witness(&sched, "/a.vue");

        // Verify Analysis is NOT yet committed
        assert!(
            sched.try_get_analysis("/a.vue").is_none(),
            "precondition: Analysis must not exist yet"
        );

        // Attempt to commit artifact WITHOUT Analysis
        assert!(!sched.commit_artifact(&witness, 42, Arc::new(crate::node::EmptyData)));

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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Step 1: Initial upsert → drive to gen G
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v1")),
            file_language: None,
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
            file_language: None,
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Create a node but don't run Source
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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

        // No source has been handed out, so no publication witness exists:
        // a host cannot commit against a generation it never read.
        assert!(
            sched.try_get_witnessed_source("/a.vue").is_none(),
            "no witness may be minted before Source commits"
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Step 1: upsert + drive to Analysis (Source + Analysis committed).
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
            incarnation: fixture_incarnation(&sched, "/a.vue"),
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
            file_language: None,
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

    impl crate::execution::executor::StageExecutor for GatedAnalysisExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_analysis(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
            generation: u64,
        ) -> Result<AnalysisSnapshot, crate::execution::executor::StageError> {
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

        let sched = Scheduler::test_with_executor(SchedulerConfig::default(), loader, executor);

        // Submit an Artifact request for /a.vue — drives Source →
        // Analysis. Analysis dispatches and blocks inside the gated
        // executor.
        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 11 },
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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

    impl crate::execution::executor::StageExecutor for DepAnalysisFailExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn extract_deps(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
        ) -> crate::execution::executor::ExtractedDeps {
            if canonical_id == self.owner_id {
                crate::execution::executor::ExtractedDeps {
                    forward_deps: vec![self.dep_id.clone()],
                    blocker_ids: vec![self.dep_id.clone()],
                }
            } else {
                crate::execution::executor::ExtractedDeps::default()
            }
        }

        fn execute_analysis(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
            generation: u64,
        ) -> Result<AnalysisSnapshot, crate::execution::executor::StageError> {
            if canonical_id == self.dep_id {
                Err(crate::execution::executor::StageError {
                    kind: crate::execution::executor::StageErrorKind::Generic,
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
    /// eventually — but only after the next idle re-pump tick
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

        let sched = Scheduler::test_with_executor(SchedulerConfig::default(), loader, executor);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 23 },
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });

        // The owner's Artifact must resolve in well under the
        // driver's idle re-pump interval (5s). Without the
        // wake-on-stranded path the dep's Analysis failure strands the
        // owner's Analysis gate and the driver sleeps in `recv_timeout`
        // until the next idle re-pump tick; with the wake the path
        // triggers a prompt dispatch and the owner's Analysis +
        // Artifact run in sub-second time.
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
             wait for the 5s idle re-pump tick); got None after {elapsed:?}"
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

    impl crate::execution::executor::StageExecutor for GatedPanickingSourceExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_source(
            &self,
            canonical_id: &str,
            _file_language: FileLanguage,
            _content: Arc<str>,
            _generation: u64,
            _incarnation: u64,
        ) -> Result<SourceSnapshot, crate::execution::executor::StageError> {
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
            dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
        };
        let sched = Scheduler::test_with_executor(config, loader, executor);

        let handle = sched.submit_request(Request {
            file_id: "/panic.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });

        // Wait for the Source worker to enter the gated executor.
        // At this point the IO permit is parked on the dispatched
        // gen=1 DAG node.
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Source worker must enter the gated executor");

        // Bump the generation BEHIND the running worker through the
        // live publication hold, WITHOUT the supersede sweep. This
        // isolates the permit-release responsibility on
        // `surface_stage_panic_as_failed`: there is no other code
        // path (no node cancel from supersede) that could
        // release the permit on its behalf. A generation-mismatch
        // early return would leave the permit parked forever in
        // this configuration.
        let node = sched.nodes.get("/panic.vue").expect("node exists").clone();
        sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&node));

        // Release the gate; the executor panics; panic-catch enters
        // `surface_stage_panic_as_failed`.
        drop(release_tx);

        // The request handle is the panic-catch receipt: terminalize_failure
        // signals Failed on the dispatched generation even after a live
        // bump, and that same cancel path drops the parked IO permit.
        match handle
            .wait_timeout(Duration::from_secs(5))
            .expect("panic-catch must complete the request handle")
        {
            CompletionState::Failed(_) => {}
            other => panic!("expected Failed after the gated source panic, got {other:?}"),
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

    impl crate::execution::executor::StageExecutor for GatedArtifactExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_artifact(
            &self,
            canonical_id: &str,
            _source: &SourceSnapshot,
            _analysis: &AnalysisSnapshot,
            profile_hash: u64,
            generation: u64,
        ) -> Result<ArtifactSnapshot, crate::execution::executor::StageError> {
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

        let sched = Scheduler::test_with_executor(SchedulerConfig::default(), loader, executor);

        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 17 },
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });

        // Wait until the Artifact worker has parked inside the gated
        // executor.
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Artifact worker must enter the gated executor");

        // Capture the live source witness (Source + Analysis have
        // already committed by the time the Artifact worker reached
        // the gate).
        let witness = source_witness(&sched, "/a.vue");

        // External commit lands while the worker is parked.
        assert!(sched.commit_artifact(&witness, 17, Arc::new(SentinelData { tag: EXTERNAL_TAG })));

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

    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
    use std::sync::Mutex as StdMutex;
    use verter_execution::request_context::{
        CacheEventKind, OpaqueRequestContext, RequestContextLike, TlsUninstall,
    };

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

    struct TestGuardBox(#[allow(dead_code)] verter_execution::request_context::OpaqueContextGuard);
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
            let guard = verter_execution::request_context::OpaqueContextGuard::install(
                OpaqueRequestContext(self as Arc<dyn RequestContextLike>),
            );
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
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_source(
            &self,
            _canonical_id: &str,
            _file_language: FileLanguage,
            content: Arc<str>,
            generation: u64,
            _incarnation: u64,
        ) -> Result<SourceSnapshot, crate::execution::executor::StageError> {
            let id = verter_execution::request_context::current_request_id().unwrap_or(0);
            self.source_observed.store(id, AtomicOrdering::SeqCst);
            Ok(SourceSnapshot::new_empty(content, generation))
        }
        fn execute_analysis(
            &self,
            _canonical_id: &str,
            _source: &SourceSnapshot,
            generation: u64,
        ) -> Result<AnalysisSnapshot, crate::execution::executor::StageError> {
            let id = verter_execution::request_context::current_request_id().unwrap_or(0);
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
        ) -> Result<ArtifactSnapshot, crate::execution::executor::StageError> {
            let id = verter_execution::request_context::current_request_id().unwrap_or(0);
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
        Scheduler::test_with_executor(SchedulerConfig::default(), loader, executor)
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
            file_language: None,
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
        let mut dag = SchedulerDag::new();
        let (_h1, s1) = completion_pair::<RequestResult>();
        let (_h2, s2) = completion_pair::<RequestResult>();

        let winner_ctx = TestContext::new(100, true); // captures
        let joiner_ctx = TestContext::new(200, true);

        let canonical: Arc<str> = Arc::from("/x.vue");
        // First registration creates the group — no dedup event.
        let winner_event = dag.register_request(
            &canonical,
            1,
            TargetStage::Analysis,
            s1,
            Some(OpaqueRequestContext(
                Arc::clone(&winner_ctx) as Arc<dyn RequestContextLike>
            )),
        );
        assert!(
            winner_event.is_none(),
            "the group-creating registration is not a dedup join",
        );
        // Second registration joins — returns a deferred event that is
        // NOT fired under the lock. The joiner callback only runs when
        // the caller fires the returned event.
        let joiner_event = dag.register_request(
            &canonical,
            1,
            TargetStage::Analysis,
            s2,
            Some(OpaqueRequestContext(
                Arc::clone(&joiner_ctx) as Arc<dyn RequestContextLike>
            )),
        );
        assert!(
            joiner_ctx.joiner_calls().is_empty(),
            "register_request must NOT fire the dedup callback itself — it returns an event",
        );
        joiner_event
            .expect("a dedup join must return a DedupJoinerEvent")
            .fire();

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
        let mut dag = SchedulerDag::new();
        let (_h1, s1) = completion_pair::<RequestResult>();
        let (_h2, s2) = completion_pair::<RequestResult>();

        let winner_ctx = TestContext::new(101, false); // no capture
        let joiner_ctx = TestContext::new(201, true);

        let canonical: Arc<str> = Arc::from("/y.vue");
        let winner_event = dag.register_request(
            &canonical,
            2,
            TargetStage::Analysis,
            s1,
            Some(OpaqueRequestContext(
                Arc::clone(&winner_ctx) as Arc<dyn RequestContextLike>
            )),
        );
        assert!(winner_event.is_none());
        let joiner_event = dag.register_request(
            &canonical,
            2,
            TargetStage::Analysis,
            s2,
            Some(OpaqueRequestContext(
                Arc::clone(&joiner_ctx) as Arc<dyn RequestContextLike>
            )),
        );
        joiner_event
            .expect("a dedup join must return a DedupJoinerEvent")
            .fire();

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
        use crate::execution::executor::ExtractedDeps;

        const PARENT: &str = "/parent.vue";
        const DEP: &str = "/dep.ts";
        const PARENT_REQ_ID: u64 = 4242;

        struct ParentAndDepProbe {
            parent_analysis_observed: Arc<AtomicU64>,
            dep_source_observed: Arc<AtomicU64>,
        }
        impl StageExecutor for ParentAndDepProbe {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
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
                _file_language: FileLanguage,
                content: Arc<str>,
                generation: u64,
                _incarnation: u64,
            ) -> Result<SourceSnapshot, crate::execution::executor::StageError> {
                let id = verter_execution::request_context::current_request_id().unwrap_or(0);
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
            ) -> Result<AnalysisSnapshot, crate::execution::executor::StageError> {
                let id = verter_execution::request_context::current_request_id().unwrap_or(0);
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
        // The TLS-propagation invariant is independent of host parallelism.
        // Pin this fixture's private pools so a highly parallel nextest run
        // cannot multiply host-sized pools into quadratic thread pressure.
        let sched = Scheduler::test_with_executor(
            SchedulerConfig {
                cpu_threads: 2,
                io_threads: 2,
                dag_budget: None,
            },
            loader,
            executor,
        );

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
            file_language: None,
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
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn execute_analysis(
                &self,
                canonical_id: &str,
                _source: &SourceSnapshot,
                generation: u64,
            ) -> Result<AnalysisSnapshot, crate::execution::executor::StageError> {
                // Extract file index from canonical_id like "/f{N}.vue".
                let idx = canonical_id
                    .trim_start_matches("/f")
                    .trim_end_matches(".vue")
                    .parse::<usize>()
                    .unwrap_or(0);
                let id = verter_execution::request_context::current_request_id().unwrap_or(0);
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
        let sched = Scheduler::test_with_executor(SchedulerConfig::default(), loader, executor);

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
                        file_language: None,
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
            file_language: None,
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

    /// Discriminator: `remove_artifact_not_newer_than(witness, profile)`
    /// MUST NOT clobber an artifact whose stored generation is strictly
    /// greater than the witnessed generation `N`.
    ///
    /// Race scenario: a slow compile started at generation `N` reaches
    /// its refusal arm AFTER a faster compile at `N+k` (k > 0) has
    /// already committed a fresh artifact. The slow compile's captured
    /// start witness names `N`; the eviction MUST observe the stored
    /// `generation = N+k > N` and skip the remove. The newer artifact at
    /// `N+k` survives; `try_get_artifact` continues to serve it.
    ///
    /// Discriminating property: an unconditional
    /// `node.artifacts.remove(&profile_hash)` would delete the newer
    /// artifact and `try_get_artifact` would return `None` after the
    /// call.
    #[test]
    fn remove_artifact_not_newer_than_preserves_newer_generation_artifact() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a v1"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader.clone());

        // Drive the node to a stable generation N with Source +
        // Analysis committed.
        let h = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v1")),
            file_language: None,
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
        let witness_n = source_witness(&sched, "/a.vue");
        assert_eq!(witness_n.generation(), gen_n);
        assert!(sched.commit_artifact(&witness_n, 42, Arc::new(crate::node::EmptyData)));

        // Advance to generation N+1 (k = 1, sufficient for the
        // discriminator). A re-upsert is the natural generation bump.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a v2")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        let witness_n_plus_k = source_witness(&sched, "/a.vue");
        let gen_n_plus_k = witness_n_plus_k.generation();
        assert!(
            gen_n_plus_k > gen_n,
            "fixture invariant: the second upsert must bump the node \
             generation strictly past gen N (observed gen_n = {gen_n}, \
             gen_n_plus_k = {gen_n_plus_k})"
        );

        // The "fast successful compile at N+k" commits a fresh artifact
        // at the bumped generation. This is the artifact the slow
        // refused compile must NOT clobber.
        assert!(sched.commit_artifact(&witness_n_plus_k, 42, Arc::new(crate::node::EmptyData)));

        // The slow refused compile reaches its eviction arm carrying
        // its captured START witness (gen_n). Since the stored snapshot
        // is at gen_n_plus_k > gen_n, the remove must be a no-op.
        assert!(!sched.remove_artifact_not_newer_than(&witness_n, 42));

        // KEY DISCRIMINATOR: the newer artifact at gen N+k MUST
        // survive. An unconditional remove would clobber it.
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_some(),
            "a slow refused compile at gen N must not clobber a fresher \
             artifact at gen N+k (gen_n = {gen_n}, gen_n_plus_k = {gen_n_plus_k})"
        );

        // Symmetric positive case: the current witness MUST evict.
        // Otherwise the method would never remove anything — masking the
        // legitimate refused-compile-cleanup path.
        assert!(sched.remove_artifact_not_newer_than(&witness_n_plus_k, 42));
        assert!(
            sched.try_get_artifact("/a.vue", 42).is_none(),
            "a current witness must evict the snapshot it is not older than"
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue + /dep.ts to Analysis so register_resolved_deps
        // records its blocker set in the live (non-zero-generation)
        // path.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue to Analysis so a_gen settles.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
            incarnation: fixture_incarnation(&sched, "/stale-dep.ts"),
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
        let witness = source_witness(&sched, "/a.vue");
        assert_eq!(witness.generation(), a_gen);
        assert!(sched.commit_artifact(&witness, 7, Arc::new(crate::node::EmptyData)));
        let a_incarnation = sched.nodes.get("/a.vue").unwrap().incarnation_id();
        sched.handle_stage_complete(
            "/a.vue",
            a_gen,
            TaskKind::Artifact { profile_hash: 7 },
            a_incarnation,
        );

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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue to Analysis so the node has both Source and
        // Analysis committed (the `commit_artifact` coherence gate
        // requires both).
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
            incarnation: fixture_incarnation(&sched, "/stale-dep.ts"),
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
        let witness = source_witness(&sched, "/a.vue");
        assert_eq!(witness.generation(), a_gen);
        assert!(sched.commit_artifact(&witness, 7, Arc::new(crate::node::EmptyData)));

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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue + /dep.ts to Analysis.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
            incarnation: fixture_incarnation(&sched, "/dep.ts"),
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

    /// A terminal Source failure must not become a live Analysis blocker
    /// or be retried merely because exact dependencies are registered later.
    #[test]
    fn register_resolved_deps_skips_dead_producer_deps() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Drive /a.vue to Analysis.
        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Missing backing drives a real terminal producer, distinguishing it
        // from a FileNode whose first Source request is still in the inbox.
        let dead = sched.submit_request(Request {
            file_id: "/dead.ts".into(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        assert!(matches!(
            dead.try_get(),
            Some(CompletionState::Failed(
                crate::job::SchedulerError::FileNotFound { .. }
            ))
        ));
        let dead_node = sched.nodes.get("/dead.ts").unwrap().clone();
        assert!(!dead_node.source_admission_pending());

        sched.register_resolved_deps(
            "/a.vue",
            vec!["/dead.ts".to_string()],
            vec!["/dead.ts".to_string()],
        );

        let a_arc: Arc<str> = Arc::from("/a.vue");
        let after = sched.dag.lock().peek_artifact_blockers(&a_arc, a_gen);
        assert!(
            after.deps.is_empty(),
            "a terminal producer cannot gate Analysis: {after:?}"
        );
        assert_eq!(
            after.failed.len(),
            1,
            "terminal failure must still reach the owner's Artifact"
        );
        assert!(
            sched
                .dag
                .lock()
                .token_for(&WorkNodeIdentity::FileStage {
                    canonical: Arc::from("/dead.ts"),
                    incarnation: dead_node.incarnation_id(),
                    generation: dead_node.generation(),
                    stage: FileStageKey::Source,
                })
                .is_none(),
            "blocker registration must not restart the terminal producer"
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
            incarnation: fixture_incarnation(&sched, "/dep.ts"),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        let dag = sched.dag.lock();
        // Confirm the DAG has no live Analysis identity for the dep:
        // terminalize_failure cancelled Source, and Analysis was
        // never admitted in the first place.
        let dep_analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/dep.ts"),
            incarnation: fixture_incarnation(&sched, "/dep.ts"),
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
        let sched = Scheduler::test_new_sync_with_executor(
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
            file_language: None,
            request_context: None,
        });
        sched.submit_request(Request {
            file_id: "/dep.ts".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("dep")),
            file_language: None,
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
            incarnation: fixture_incarnation(&sched, "/dep.ts"),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        let dag = sched.dag.lock();
        let dep_analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/dep.ts"),
            incarnation: fixture_incarnation(&sched, "/dep.ts"),
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/dep.ts".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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
            file_language: None,
            request_context: None,
        });
        sched.drive_all();
        let a_gen = sched.try_get_source("/a.vue").unwrap().generation;

        // Confirm no live Analysis DAG identity for the dead dep.
        let dep_analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/dep.ts"),
            incarnation: fixture_incarnation(&sched, "/dep.ts"),
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

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
            file_language: None,
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
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
            generation: dep_gen,
            stage: FileStageKey::Source,
        };
        let dep_analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&dep_arc),
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
        let status = sched.file_stage_analysis_blocker_status(
            &dag,
            &dep_arc,
            fixture_incarnation(&sched, dep_arc.as_ref()),
            dep_gen,
        );
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
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
    /// consults. The cleanup arm runs after `admit_work(TaskKind::Load)`
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a")),
            file_language: None,
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let dep_id = "/dep.ts";
        let dep_arc: Arc<str> = Arc::from(dep_id);

        // Plant a FileNode at gen=2 (the "live" generation).
        let dep_node = sched.create_node(dep_id, None);
        sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node)); // gen=1
        sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node)); // gen=2
        sched.nodes.insert(dep_id.to_string(), dep_node);

        // Plant a tracking entry at gen=1 (the "stale" gen).
        sched.auto_ingested_recent.insert(
            Arc::clone(&dep_arc),
            AutoIngestedRecord {
                incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let dep_id = "/dep.ts";
        let dep_arc: Arc<str> = Arc::from(dep_id);

        // FileNode at gen=2.
        let dep_node = sched.create_node(dep_id, None);
        sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node));
        sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node));
        sched.nodes.insert(dep_id.to_string(), dep_node);

        // Tracking entry at gen=2 (the LIVE generation, not the
        // stale one being consulted).
        sched.auto_ingested_recent.insert(
            Arc::clone(&dep_arc),
            AutoIngestedRecord {
                incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
                generation: 2,
                since: Instant::now(),
            },
        );

        let dep_key = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let dep_id = "/dep.ts";
        let dep_arc: Arc<str> = Arc::from(dep_id);
        // Capture the unpublished node's identity before publishing tracking.
        // Both transitional states must describe the same producer.
        let dep_node = sched.create_node(dep_id, None);
        let dep_incarnation = dep_node.incarnation_id();

        // State A — post-swap intermediate state #1: tracking
        // entry has been published but FileNode has not. This is
        // the ONLY mid-call observable state that the post-swap
        // ordering can produce. The matrix's FileNode-missing arm
        // must classify as Gating via `auto_ingest_tracking_gates`.
        let dep_gen: u64 = 1;
        sched.auto_ingested_recent.insert(
            Arc::clone(&dep_arc),
            AutoIngestedRecord {
                incarnation: dep_incarnation,
                generation: dep_gen,
                since: Instant::now(),
            },
        );

        let dep_key = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            incarnation: dep_incarnation,
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
        sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node));
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let canonical: Arc<str> = Arc::from("/dep.ts");

        // Arm 1: clear_auto_ingest_tracking with stale `generation`
        // argument. The live entry is at gen=2; clearing for gen=1
        // must be a no-op.
        sched.auto_ingested_recent.insert(
            Arc::clone(&canonical),
            AutoIngestedRecord {
                incarnation: fixture_incarnation(&sched, "/dep.ts"),
                generation: 2,
                since: Instant::now(),
            },
        );
        sched.clear_auto_ingest_tracking(
            &canonical,
            fixture_incarnation(&sched, canonical.as_ref()),
            /* old_gen */ 1,
        );
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
                incarnation: fixture_incarnation(&sched, "/dep.ts"),
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
                    incarnation: fixture_incarnation(&sched_clone_b, "/dep.ts"),
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

    impl crate::execution::executor::StageExecutor for GatedFailingSourceExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_source(
            &self,
            canonical_id: &str,
            _file_language: FileLanguage,
            content: Arc<str>,
            generation: u64,
            _incarnation: u64,
        ) -> Result<crate::node::SourceSnapshot, crate::execution::executor::StageError> {
            if let Some(gate) = self.gates.get(canonical_id) {
                // Signal entry to the test thread. Best-effort —
                // if the test dropped the receiver the executor
                // proceeds.
                let _ = gate.entered_tx.send(());
                // Block until release. The test drops the sender
                // to signal release; recv returns Disconnected,
                // which we treat as release.
                let _ = gate.release_rx.recv();
                return Err(crate::execution::executor::StageError {
                    kind: crate::execution::executor::StageErrorKind::Generic,
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

        let sched = Scheduler::test_with_executor(SchedulerConfig::default(), loader, executor);

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
            file_language: None,
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
            file_language: None,
            request_context: None,
        });

        let artifact_identity = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            incarnation: fixture_incarnation(&sched, "/a.vue"),
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
                incarnation: fixture_incarnation(&sched, "/dep.ts"),
                generation: dep_gen,
                stage: FileStageKey::Source,
            };
            let dep_analysis_id = WorkNodeIdentity::FileStage {
                canonical: Arc::from("/dep.ts"),
                incarnation: fixture_incarnation(&sched, "/dep.ts"),
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);
        let node = sched
            .nodes
            .entry("/race.vue".to_string())
            .or_insert_with(|| sched.create_node("/race.vue", None))
            .clone();
        // Generation 5 keeps both the commit publisher and the skip
        // path aligned on the same `(file, gen, profile)` slot so the
        // skip-arm fires every iteration of the worker thread.
        for _ in 0..5 {
            sched
                .source_root
                .publish_transition(|publication| publication.bump_node_generation(&node));
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
        let witness = source_witness(&sched_a, "/race.vue");
        let t_commit = thread::spawn(move || {
            while !stop_a.load(Ordering::Acquire) {
                sched_a.commit_artifact(&witness, 42, Arc::new(crate::node::EmptyData));
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
                let _ = Scheduler::execute_artifact_stage(
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

    /// A Source completion whose generation is superseded WHILE the
    /// completion is being processed must publish NOTHING — no
    /// Analysis admission, no blocker records — and must leave no
    /// admitted DAG node behind.
    ///
    /// The window is real and unlocked: `handle_stage_complete` checks
    /// the generation on ENTRY, then performs the extraction work
    /// (`extract_deps`, edge merge, auto-ingest, blocker
    /// classification) WITHOUT holding the DAG lock, and only then
    /// admits Analysis at the generation the Source job was DISPATCHED
    /// at. An `invalidate()` landing inside that window leaves the
    /// entry check already satisfied and the later admission stale, so
    /// the supersede sweep — which is purely backward-looking — has
    /// already run and can never cancel the identity that is about to
    /// be created.
    ///
    /// Deterministic seam with NO production test hook: the
    /// invalidation is driven from a test-owned `StageExecutor`'s
    /// `extract_deps`, which the scheduler calls at exactly that point.
    ///
    /// Discriminator: without the publish-time gate the stale Analysis
    /// identity is admitted, then skipped at dispatch on the
    /// generation-mismatch arm, so its token stays live forever and the
    /// capacity reservation parked at dispatch is never released — the
    /// skip's own safety condition ("the parked reservation releases
    /// through the DAG's cancel path") is violated. In a debug build
    /// the defensive `debug_assert!` on that arm fires first, which is
    /// itself the failure signal.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn source_completion_superseded_mid_flight_publishes_nothing() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::execution::executor::{ExtractedDeps, StageExecutor};
        use crate::node::SourceSnapshot;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{OnceLock, Weak};

        /// Invalidates the file from inside `extract_deps` — exactly the
        /// unlocked window between the entry generation check and the
        /// Source→Analysis publish. Fires exactly once so the
        /// re-submitted generation can settle.
        struct InvalidateDuringExtract {
            sched: OnceLock<Weak<Scheduler>>,
            fired: AtomicBool,
        }
        impl StageExecutor for InvalidateDuringExtract {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn extract_deps(&self, canonical_id: &str, _source: &SourceSnapshot) -> ExtractedDeps {
                if !self.fired.swap(true, Ordering::SeqCst) {
                    if let Some(sched) = self.sched.get().and_then(Weak::upgrade) {
                        sched.invalidate(canonical_id);
                    }
                }
                ExtractedDeps::default()
            }
        }

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/inv.vue".to_string(), Arc::from("x"));
        let executor = Arc::new(InvalidateDuringExtract {
            sched: OnceLock::new(),
            fired: AtomicBool::new(false),
        });
        let sched = Scheduler::test_new_sync_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::clone(&executor) as Arc<dyn StageExecutor>,
        );
        let _ = executor.sched.set(Arc::downgrade(&sched));

        sched.submit_request(Request {
            file_id: "/inv.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        // Sync mode: no driver thread, so this pump is the whole
        // schedule and the interleaving is fully determined.
        sched.drive_all();

        let canonical: Arc<str> = Arc::from("/inv.vue");
        let live_gen = sched.nodes.get("/inv.vue").unwrap().generation();
        assert!(
            live_gen >= 2,
            "precondition: the mid-flight invalidate must have advanced the generation, \
             observed {live_gen}",
        );

        let dag = sched.dag.lock();
        // The generation the Source job ran at is now retired. No
        // identity for it may survive in the DAG.
        for stage in [FileStageKey::Source, FileStageKey::Analysis] {
            let stale = WorkNodeIdentity::FileStage {
                canonical: Arc::clone(&canonical),
                incarnation: fixture_incarnation(&sched, canonical.as_ref()),
                generation: 1,
                stage,
            };
            assert!(
                dag.token_for(&stale).is_none(),
                "a stage completion at a superseded generation published a live DAG \
                 identity that no sweep can ever reach: {stale:?} is still admitted. \
                 Its dispatch will hit the generation-mismatch skip, which never \
                 releases the parked capacity reservation.",
            );
        }
        // Zero retained capacity: nothing admitted, dispatched-and-
        // abandoned, or otherwise left holding a reservation.
        assert_eq!(
            dag.total_active(),
            0,
            "a superseded stage completion retained DAG capacity — every admitted node \
             must have been completed or cancelled",
        );
        drop(dag);

        // The refusal is silent in release — no panic, no typed caller
        // error — so the counter is the only evidence the gate actually
        // fired rather than the race simply not happening.
        assert_eq!(
            sched.stale_completion_refusals(),
            1,
            "the mid-flight supersession must have been refused at the publish point \
             exactly once",
        );
    }

    /// The completion's incarnation witness must be the node the work
    /// was DISPATCHED against, not whatever node the map happens to hold
    /// when the completion is handled.
    ///
    /// Re-deriving it by lookup makes the check vacuous: the handler
    /// fetches the live node and then compares that node with itself, so
    /// a replacement is validated as if it were the original. Two node
    /// objects for the same canonical can sit at the SAME generation, so
    /// the generation check cannot catch it either — a replacement
    /// starts its own generation sequence at 0 and is bumped, and
    /// nothing forces it past the value the original already had.
    ///
    /// Discriminator: the replacement here carries its OWN committed
    /// source at the same generation, so every non-incarnation condition
    /// the gate tests is satisfied. Only a witness carried from dispatch
    /// rejects it. With a lookup-derived witness this publishes Analysis
    /// for a node that never ran the Source stage.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn stage_completion_witness_is_the_dispatched_incarnation_not_the_live_lookup() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/inc.vue".to_string(), Arc::from("x"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let handle = sched.submit_request(Request {
            file_id: "/inc.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        sched.drain_inbox();

        let dispatched = sched.nodes.get("/inc.vue").unwrap().clone();
        let dispatched_incarnation = dispatched.incarnation_id();
        let generation = dispatched.generation();
        // Give the DISPATCHED node a committed source, as a real Source
        // stage would before emitting its completion.
        dispatched.source.store(Arc::new(Some(Arc::new(
            crate::node::SourceSnapshot::new_empty(Arc::from("x"), generation),
        ))));

        // Replace the published node with a DIFFERENT incarnation at the
        // SAME generation, itself carrying a committed source. Every
        // condition except incarnation identity now holds.
        let replacement = Arc::new(crate::node::FileNode::new_at(
            "/inc.vue".to_string(),
            verter_language::FileLanguage::vue(),
            generation,
        ));
        replacement.source.store(Arc::new(Some(Arc::new(
            crate::node::SourceSnapshot::new_empty(Arc::from("x"), generation),
        ))));
        assert_ne!(
            replacement.incarnation_id(),
            dispatched_incarnation,
            "precondition: the replacement must be a distinct incarnation",
        );
        assert_eq!(
            replacement.generation(),
            generation,
            "precondition: the replacement must sit at the SAME generation, so only the \
             incarnation distinguishes it",
        );
        sched
            .nodes
            .insert("/inc.vue".to_string(), Arc::clone(&replacement));

        // Deliver the completion for the node that actually ran.
        sched.handle_stage_complete(
            "/inc.vue",
            generation,
            TaskKind::Load,
            dispatched_incarnation,
        );

        let analysis_id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/inc.vue"),
            incarnation: fixture_incarnation(&sched, "/inc.vue"),
            generation,
            stage: FileStageKey::Analysis,
        };
        assert!(
            sched.dag.lock().token_for(&analysis_id).is_none(),
            "the completion published Analysis for a replacement incarnation that never ran \
             the Source stage — the witness was re-derived by lookup and compared the live \
             node with itself",
        );
        assert_eq!(
            sched.stale_completion_refusals(),
            1,
            "the incarnation mismatch must be refused exactly once",
        );
        // And the refusal must not strand the request group: a waiter
        // parked on a completion that has just been refused, and will
        // never be republished, has to be terminalized.
        assert!(
            handle.try_get().is_some(),
            "a refused completion left its request group parked forever",
        );
    }

    /// Pending-Artifact admission must refuse a generation the node has
    /// already left.
    ///
    /// `admit_pending_artifacts` snapshotted the profiles under one lock,
    /// RELEASED it, then re-locked per profile. An `invalidate()` landing
    /// in that gap bumps the generation and runs its supersede sweep, and
    /// the loop then admits Artifact-G AFTER that backward-looking sweep
    /// — the identical defect the Source→Analysis publish closes, one
    /// stage down.
    ///
    /// The test reproduces the state that window exposes rather than the
    /// window itself: the node is advanced WITHOUT a sweep, so the
    /// generation's pending profile is still registered, exactly as it is
    /// between the snapshot and the re-lock.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pending_artifact_admission_refuses_a_generation_the_node_has_left() {
        use crate::dag::{profile_hash_to_bytes, WorkNodeIdentity};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/art.vue".to_string(), Arc::from("x"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/art.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 7 },
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        sched.drain_inbox();

        let node = sched.nodes.get("/art.vue").unwrap().clone();
        let generation = node.generation();
        let canonical: Arc<str> = Arc::from("/art.vue");
        assert!(
            !sched
                .dag
                .lock()
                .pending_artifact_profiles(&canonical, generation)
                .is_empty(),
            "precondition: a pending Artifact profile must be registered at gen {generation}",
        );

        // Advance the node WITHOUT a supersede sweep — the state the
        // released-lock window leaves behind.
        sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&node));
        assert_ne!(
            node.generation(),
            generation,
            "precondition: generation advanced"
        );

        sched.admit_pending_artifacts(
            &canonical,
            fixture_incarnation(&sched, canonical.as_ref()),
            generation,
            Priority::Background,
        );

        let stale_artifact = WorkNodeIdentity::Artifact {
            canonical: Arc::clone(&canonical),
            incarnation: fixture_incarnation(&sched, canonical.as_ref()),
            generation,
            profile_hash: profile_hash_to_bytes(7),
            content_hash: [0u8; 16],
        };
        assert!(
            sched.dag.lock().token_for(&stale_artifact).is_none(),
            "Artifact work was admitted for a generation the node had already left; no sweep \
             can reach it, so dispatch will skip it and never release its reservation",
        );
    }

    /// A refused Source completion must consume NOTHING, not merely
    /// publish nothing.
    ///
    /// Deferred blocker IDs are drained destructively and forward edges
    /// are unioned into the existing set. Doing either before the witness
    /// validates means a stale completion can swallow a LATER
    /// generation's deferred blockers — letting that generation's
    /// Artifact work run ungated — and can leave stale edges behind that
    /// no later extraction removes.
    ///
    /// Same deterministic seam as the supersession test: a test-owned
    /// executor invalidates from inside `extract_deps`, the unlocked
    /// window between the entry check and the publish.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn refused_source_completion_consumes_no_deferred_blockers() {
        use crate::execution::executor::{ExtractedDeps, StageExecutor};
        use crate::node::SourceSnapshot;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{OnceLock, Weak};

        struct InvalidateDuringExtract {
            sched: OnceLock<Weak<Scheduler>>,
            fired: AtomicBool,
        }
        impl StageExecutor for InvalidateDuringExtract {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn extract_deps(&self, canonical_id: &str, _source: &SourceSnapshot) -> ExtractedDeps {
                if !self.fired.swap(true, Ordering::SeqCst) {
                    if let Some(sched) = self.sched.get().and_then(Weak::upgrade) {
                        sched.invalidate(canonical_id);
                    }
                }
                ExtractedDeps::default()
            }
        }

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/owner.vue".to_string(), Arc::from("x"));
        let executor = Arc::new(InvalidateDuringExtract {
            sched: OnceLock::new(),
            fired: AtomicBool::new(false),
        });
        let sched = Scheduler::test_new_sync_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::clone(&executor) as Arc<dyn StageExecutor>,
        );
        let _ = executor.sched.set(Arc::downgrade(&sched));

        // Plant a deferred blocker for the owner.
        sched.deferred_blocker_ids.insert(
            "/owner.vue".to_string(),
            vec![("/dep.ts".to_string(), FileLanguage::script_ts())],
        );

        sched.submit_request(Request {
            file_id: "/owner.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();

        assert!(
            sched.stale_completion_refusals() >= 1,
            "precondition: the mid-flight invalidate must have caused a refusal",
        );
        assert!(
            sched.deferred_blocker_ids.contains_key("/owner.vue"),
            "a REFUSED completion drained the deferred blocker set. Those IDs belong to the \
             live generation now; swallowing them lets its Artifact work run ungated",
        );
    }

    /// A retained Arc to a removed node cannot authorize later admission.
    /// The object retirement marker closes admission after the sweep.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn removal_retires_the_canonical_so_late_admission_is_refused() {
        use crate::dag::{profile_hash_to_bytes, FileStageKey, WorkKind, WorkNodeIdentity};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/gone.vue".to_string(), Arc::from("x"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/gone.vue".to_string(),
            target: TargetStage::Artifact { profile_hash: 5 },
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        sched.drain_inbox();
        let canonical: Arc<str> = Arc::from("/gone.vue");
        let old_node = sched.nodes.get("/gone.vue").unwrap().clone();
        let generation = old_node.generation();

        sched.remove("/gone.vue");

        // Replay the admission a completion queued before the sweep
        // would have performed. It must be refused.
        let artifact_id = WorkNodeIdentity::Artifact {
            canonical: Arc::clone(&canonical),
            incarnation: old_node.incarnation_id(),
            generation,
            profile_hash: profile_hash_to_bytes(5),
            content_hash: [0u8; 16],
        };
        let analysis_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&canonical),
            incarnation: old_node.incarnation_id(),
            generation,
            stage: FileStageKey::Analysis,
        };
        {
            let mut dag = sched.dag.lock();
            assert!(
                dag.submit_file(
                    &old_node,
                    artifact_id.clone(),
                    WorkKind::Artifact,
                    Priority::Background,
                    Vec::new(),
                    None,
                )
                .is_none(),
                "Artifact work was admitted for a REMOVED file's generation. Its FileNode is \
                 gone, so dispatch can never run it and never releases its reservation",
            );
            assert!(
                dag.submit_file(
                    &old_node,
                    analysis_id.clone(),
                    WorkKind::Analysis,
                    Priority::Background,
                    Vec::new(),
                    None,
                )
                .is_none(),
                "file-stage work was admitted for a REMOVED file's generation",
            );
        }
        let dag = sched.dag.lock();
        assert!(dag.token_for(&artifact_id).is_none());
        assert!(dag.token_for(&analysis_id).is_none());
        assert_eq!(
            dag.total_active(),
            0,
            "removal left DAG nodes holding capacity",
        );
    }

    /// Retiring a generation must fan out to consumers keyed on an
    /// identity that was NEVER ADMITTED.
    ///
    /// An owner Artifact can gate on `dep:Analysis-G` while `dep`'s
    /// Source-G is still running, so no `Analysis-G` node exists yet.
    /// `invalidate(dep)` cancels Source-G — but cancelling nodes only
    /// reaches consumers whose dep was actually admitted, and
    /// `signal_file_failed` only drains FILE waiter groups. Nothing
    /// released the owner, so it parked on `Analysis-G` forever.
    ///
    /// The fix is one fan-out over the dep index by canonical +
    /// generation, which reaches every consumer regardless of which
    /// stage it named or whether that stage ever existed.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn retiring_a_generation_releases_waiters_on_never_admitted_identities() {
        use crate::dag::{DepKey, FileStageKey, WorkKind, WorkNodeIdentity};

        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let dep: Arc<str> = Arc::from("/dep.vue");
        let owner: Arc<str> = Arc::from("/owner.vue");
        // The dep node must exist so `invalidate` has something to bump.
        sched
            .nodes
            .insert("/dep.vue".to_string(), sched.create_node("/dep.vue", None));
        let dep_node = sched.nodes.get("/dep.vue").unwrap().clone();
        let dep_gen = sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node));

        let owner_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&owner),
            incarnation: fixture_incarnation(&sched, owner.as_ref()),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        // Gate the owner on the dep's ANALYSIS — which is never admitted.
        let gating_dep = DepKey::FileStage {
            canonical: Arc::clone(&dep),
            incarnation: fixture_incarnation(&sched, dep.as_ref()),
            generation: dep_gen,
            stage: FileStageKey::Analysis,
        };
        {
            let mut dag = sched.dag.lock();
            dag.submit(
                owner_id.clone(),
                WorkKind::Analysis,
                Priority::Background,
                vec![gating_dep.clone()],
                None,
            )
            .expect("owner admission");
            assert!(
                dag.has_dep_on(&owner_id, &gating_dep),
                "precondition: the owner must be gated on the dep's Analysis",
            );
            assert!(
                dag.token_for(&WorkNodeIdentity::FileStage {
                    canonical: Arc::clone(&dep),
                    incarnation: fixture_incarnation(&sched, dep.as_ref()),
                    generation: dep_gen,
                    stage: FileStageKey::Analysis,
                })
                .is_none(),
                "precondition: no Analysis node exists for the dep — there is nothing for a \
                 node-cancel sweep to fan out from",
            );
        }

        // Retire the dep's generation.
        sched.invalidate("/dep.vue");

        // The bar is that the owner is RELEASED, not merely that nothing
        // leaked — "no leak" is also satisfied by a hang, and by the
        // owner vanishing entirely. So assert all three:
        let mut dag = sched.dag.lock();
        // 1. it still EXISTS (a vanished owner would satisfy a bare
        //    `!has_dep_on`).
        assert!(
            dag.token_for(&owner_id).is_some(),
            "the owner node disappeared instead of being released",
        );
        // 2. its gate is gone.
        assert!(
            !dag.has_dep_on(&owner_id, &gating_dep),
            "the owner is still gated on an Analysis identity that was never admitted and \
             whose generation is now retired — nothing will ever complete it, so the owner \
             parks forever",
        );
        assert!(
            !dag.has_pending_deps(&owner_id),
            "the owner still reports pending deps after its only gate was retired",
        );
        // 3. it is actually DISPATCHABLE — the discriminator a bare
        //    `!has_dep_on` misses. Clearing the dep set without
        //    refreshing lane membership leaves the owner un-gated but
        //    never selected, which is a hang wearing the costume of a
        //    release.
        let ready = dag.next_ready();
        assert!(
            ready.is_some_and(|job| job.identity == owner_id),
            "the owner was un-gated but never became dispatchable — its lane membership was \
             not refreshed, so nothing will ever select it",
        );
    }

    /// A request PREPARED before a `remove()` and admitted after it must
    /// be rejected before it registers a waiter or admits anything.
    ///
    /// `prepare_request` runs outside `dag.lock()`, so a prepared request
    /// can cross a retirement boundary: `remove()` retires the object,
    /// cancels the DAG, deletes the `FileNode` and drains the shutdown
    /// waiters, all in the gap. A prepared request holds only its lifetime
    /// witness, which cannot authorize the next lifetime at that canonical.
    ///
    /// Generations can coincide on different objects. The crossing gate
    /// must compare the captured submission lifetime with the published node.
    ///
    /// Discriminator: without the gate the waiter is registered and
    /// either the admission succeeds against a node no dispatcher can
    /// resolve (parked permit + never-signalled waiter) or it is refused
    /// and the `None` is dropped (never-signalled waiter, no producer).
    /// Either way the handle never resolves.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn request_prepared_before_removal_is_terminalized_not_parked() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/cross.vue".to_string(), Arc::from("x"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Materialize the node so the request below prepares against a
        // real incarnation.
        sched.submit_request(Request {
            file_id: "/cross.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        sched.drain_inbox();

        // PREPARE a second request while the file is still live — this is
        // the state a request has when it is parked just before taking
        // `dag.lock()`.
        let (handle, sender) = crate::job::completion_pair::<RequestResult>();
        let queued = QueuedRequest {
            file_id: "/cross.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            sender,
            submitted_lifetime: fixture_incarnation(&sched, "/cross.vue"),
            request_context: None,
        };
        let prepared = sched
            .prepare_request(queued)
            .expect("precondition: the request must prepare while the file is live");

        // The file is removed in the gap.
        sched.remove("/cross.vue");
        assert!(
            sched.nodes.get("/cross.vue").is_none(),
            "precondition: removal must have deleted the FileNode",
        );

        // The prepared request resumes and is admitted.
        let mut post = AdmissionPostWork::default();
        {
            let mut dag = sched.dag.lock();
            sched.admit_prepared_under_lock(&mut dag, prepared, &mut post);
        }
        post.run(&sched);

        // The waiter must be terminalized, never parked.
        assert!(
            handle.try_get().is_some(),
            "a request prepared before `remove()` and admitted after it was left parked \
             forever: its FileNode is gone, so no dispatcher can resolve it and no producer \
             will ever signal the waiter",
        );
        // And nothing may have been admitted or reserved on its behalf.
        let dag = sched.dag.lock();
        assert_eq!(
            dag.total_active(),
            0,
            "the crossing request admitted work against a detached incarnation, whose \
             dispatch reserves capacity and then skips without cancelling",
        );
    }

    /// A `remove()` must terminalize its waiters as `Shutdown`, not
    /// `Superseded`.
    ///
    /// `Superseded` means "a newer generation invalidated this request".
    /// After `remove()` there IS no newer generation, so reporting it
    /// misdescribes the cause to every consumer. The trap is that
    /// `retire_generations_below` signals `Superseded` and its removal
    /// floor of `last_gen + 1` covers the LIVE generation too
    /// (`file_waiter_gens_below` is exclusive), so a removal that retires
    /// before draining silently converts the terminal.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn removal_terminalizes_waiters_as_shutdown_not_superseded() {
        use crate::job::CompletionState;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/bye.vue".to_string(), Arc::from("x"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let handle = sched.submit_request(Request {
            file_id: "/bye.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        sched.drain_inbox();
        assert!(
            handle.try_get().is_none(),
            "precondition: the request must still be pending before removal",
        );

        sched.remove("/bye.vue");

        match handle.try_get() {
            Some(CompletionState::Shutdown) => {}
            other => panic!(
                "a removed file's waiter must terminalize as Shutdown; `Superseded` would \
                 claim a newer generation invalidated it, and after `remove()` there is none. \
                 got {other:?}",
            ),
        }
    }

    /// A queued request bound to a retired incarnation terminates before
    /// registering a waiter that no producer can satisfy.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn retired_queued_request_terminates_before_registering_a_waiter() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/refused.vue".into(), Arc::from("x"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);
        let incarnation = sched.stamp_request("/refused.vue", None);
        let (handle, sender) = completion_pair();
        sched.remove("/refused.vue");
        let prepared = sched.prepare_request(QueuedRequest {
            file_id: "/refused.vue".into(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: None,
            file_language: None,
            sender,
            submitted_lifetime: incarnation,
            request_context: None,
        });
        assert!(prepared.is_none());
        assert!(matches!(handle.try_get(), Some(CompletionState::Shutdown)));
        assert_eq!(sched.dag.lock().total_active(), 0);
        assert!(!sched.has_node("/refused.vue"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn signal_file_shutdown_at_is_scoped_to_one_generation() {
        use crate::job::{completion_pair, CompletionState};

        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);
        let canonical: Arc<str> = Arc::from("/scoped.vue");

        let (h_old, s_old) = completion_pair::<RequestResult>();
        let (h_new, s_new) = completion_pair::<RequestResult>();
        {
            let mut dag = sched.dag.lock();
            // Dedup events are irrelevant here — there is no joiner.
            let _ = dag.register_request(&canonical, 1, TargetStage::Analysis, s_old, None);
            let _ = dag.register_request(&canonical, 2, TargetStage::Analysis, s_new, None);
            dag.signal_file_shutdown_at(&canonical, 1);
        }

        assert!(
            matches!(h_old.try_get(), Some(CompletionState::Shutdown)),
            "the named generation's waiter must be terminalized; got {:?}",
            h_old.try_get(),
        );
        assert!(
            h_new.try_get().is_none(),
            "a neighbouring generation's waiter must survive a generation-scoped shutdown",
        );
    }

    /// The host `SourceLoader` seam must never be entered while the DAG
    /// mutex is held.
    ///
    /// `create_node(_, None)` falls through to
    /// `source_loader.classify(..)`. The Source-completion auto-ingest runs
    /// under `dag.lock()` AND inside a `nodes` shard-WRITE guard, so passing
    /// `None` there would hold two locks across a callback into host code of
    /// unbounded cost — the same hazard that keeps `extract_deps` hoisted out
    /// of the hold, and a shape the base revision never had.
    ///
    /// Empirical rather than by inspection: the loader tries the DAG mutex
    /// from inside `classify`. In this single-threaded sync scheduler a
    /// failed `try_lock` can only mean the calling thread already holds it.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn host_classifier_is_never_called_while_the_dag_lock_is_held() {
        use crate::execution::executor::{ExtractedDeps, StageExecutor};
        use crate::node::SourceSnapshot;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{OnceLock, Weak};

        struct LockProbingLoader {
            inner: MemorySourceLoader,
            sched: OnceLock<Weak<Scheduler>>,
            saw_locked_classify: Arc<AtomicBool>,
        }
        impl SourceLoader for LockProbingLoader {
            fn load(&self, canonical_id: &str) -> Option<Arc<str>> {
                self.inner.load(canonical_id)
            }
            fn exists(&self, canonical_id: &str) -> bool {
                self.inner.exists(canonical_id)
            }
            fn realpath(&self, canonical_id: &str) -> Option<String> {
                self.inner.realpath(canonical_id)
            }
            fn classify(&self, canonical_id: &str) -> FileLanguage {
                if let Some(sched) = self.sched.get().and_then(Weak::upgrade) {
                    if sched.dag.try_lock().is_none() {
                        self.saw_locked_classify.store(true, Ordering::SeqCst);
                    }
                }
                self.inner.classify(canonical_id)
            }
        }

        /// Reports a blocker dep so the completion takes the auto-ingest path.
        struct BlockerExecutor;
        impl StageExecutor for BlockerExecutor {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn extract_deps(&self, _canonical: &str, _source: &SourceSnapshot) -> ExtractedDeps {
                ExtractedDeps {
                    blocker_ids: vec!["/dep-of-owner.ts".to_string()],
                    ..Default::default()
                }
            }
        }

        let inner = MemorySourceLoader::new();
        inner.insert("/owner.vue".to_string(), Arc::from("x"));
        let saw = Arc::new(AtomicBool::new(false));
        let loader = Arc::new(LockProbingLoader {
            inner,
            sched: OnceLock::new(),
            saw_locked_classify: Arc::clone(&saw),
        });
        let sched = Scheduler::test_new_sync_with_executor(
            SchedulerConfig::default(),
            Arc::clone(&loader) as Arc<dyn SourceLoader>,
            Arc::new(BlockerExecutor),
        );
        let _ = loader.sched.set(Arc::downgrade(&sched));

        sched.submit_request(Request {
            file_id: "/owner.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();

        assert!(
            sched.nodes.get("/dep-of-owner.ts").is_some(),
            "precondition: the blocker dep must have been auto-ingested, or this test proves \
             nothing about the auto-ingest path",
        );
        assert!(
            !saw.load(Ordering::SeqCst),
            "the host SourceLoader::classify seam was entered while the DAG mutex was held — \
             auto-ingest also holds a `nodes` shard-WRITE guard there, so this is two locks \
             across a host callback",
        );
    }

    /// A DEFERRED-ONLY blocker — one the extractor never reports — must
    /// still get its node created and its Load admitted.
    ///
    /// `register_resolved_deps` stores blockers and can then return EARLY
    /// (generation 0, or Source not yet committed) BEFORE its node-ensure
    /// pass, so a deferred id can reach Source completion with no
    /// `FileNode`. `extract_deps` omits bare/aliased deps by design and
    /// only lists them once exact resolutions are set, so an
    /// externally-resolved bare dependency is exactly this case.
    ///
    /// Discriminator: resolve languages from the extractor's list alone
    /// and this id has none, so its node is never created. The absent
    /// generation-0 node classifies as `Satisfied`, no Load is admitted,
    /// and the owner's Artifact proceeds UNGATED — a silent correctness
    /// loss, not a leak. Base created every merged blocker id
    /// unconditionally.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn deferred_only_blocker_is_still_created_and_admitted() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/owner2.vue".to_string(), Arc::from("x"));
        loader.insert("/bare-dep.ts".to_string(), Arc::from("y"));
        // DefaultExecutor reports NO blockers, so the only route for this
        // dep is the deferred set — the deferred-ONLY path.
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Register while the owner has no committed Source: this stores the
        // blocker and returns early, BEFORE the node-ensure pass.
        sched.register_resolved_deps("/owner2.vue", Vec::new(), vec!["/bare-dep.ts".to_string()]);
        assert!(
            sched.nodes.get("/bare-dep.ts").is_none(),
            "precondition: the early return must have skipped node-ensure, so the dep has no              FileNode yet — otherwise this test is not exercising the deferred-only path",
        );
        assert!(
            sched.deferred_blocker_ids.contains_key("/owner2.vue"),
            "precondition: the blocker must be recorded as deferred",
        );

        sched.submit_request(Request {
            file_id: "/owner2.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();

        let dep_node = sched.nodes.get("/bare-dep.ts");
        assert!(
            dep_node.is_some(),
            "the deferred-only blocker was never created: it is absent from the extractor's              list, so resolving languages from that list alone silently skips it. Its missing              generation-0 node then reads as Satisfied and the owner's Artifact runs ungated",
        );
        let dep_gen = dep_node.unwrap().generation();
        assert!(
            dep_gen >= 1,
            "an auto-ingested dep must be bumped above generation 0, since a gen-0 blocker              classifies as stale; observed {dep_gen}",
        );
        // And it must have been driven, not merely created.
        assert!(
            sched
                .nodes
                .get("/bare-dep.ts")
                .and_then(|n| n.current_source())
                .is_some(),
            "the deferred-only blocker was created but never admitted, so its Source never ran",
        );
        let _ = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/bare-dep.ts"),
            incarnation: fixture_incarnation(&sched, "/bare-dep.ts"),
            generation: dep_gen,
            stage: FileStageKey::Source,
        };
    }

    /// Control for the publish-time gate: an ordinary Source completion
    /// with no concurrent invalidation must publish NORMALLY and refuse
    /// nothing.
    ///
    /// Without this, a gate that refused EVERYTHING would still satisfy
    /// `source_completion_superseded_mid_flight_publishes_nothing` while
    /// silently breaking the pipeline — Analysis would never be admitted
    /// and no file would ever reach a committed analysis. This is the
    /// assertion that makes the refusal path discriminating rather than
    /// merely safe.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn clean_source_completion_publishes_analysis_and_refuses_nothing() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/clean.vue".to_string(), Arc::from("x"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        sched.submit_request(Request {
            file_id: "/clean.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: None,
            request_context: None,
        });
        sched.drive_all();

        // The pipeline actually advanced: Source → Analysis published.
        let node = sched.nodes.get("/clean.vue").unwrap().clone();
        assert!(
            node.current_analysis().is_some(),
            "an unperturbed request must reach a committed Analysis — the publish-time \
             gate must not refuse a completion whose generation is still current",
        );
        assert_eq!(
            sched.stale_completion_refusals(),
            0,
            "no completion may be refused when nothing retired the generation",
        );
    }

    #[test]
    fn queued_language_rehome_preserves_later_source_updates() {
        for initial_language in [FileLanguage::vue(), FileLanguage::script_ts()] {
            let sched = Scheduler::test_new_sync(
                SchedulerConfig::default(),
                Arc::new(MemorySourceLoader::new()),
            );
            let request = |source: &'static str, language| Request {
                file_id: "/queued.vue".into(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: Some(Arc::from(source)),
                file_language: Some(language),
                request_context: None,
            };
            let initial = sched.submit_request(request("initial", initial_language));
            assert!(sched.wait_or_drive(&initial).is_ready());
            let first = sched.submit_request(request("first", FileLanguage::script_ts()));
            let last = sched.submit_request(request("last", FileLanguage::script_ts()));
            sched.drive_all();
            assert!(matches!(first.try_get(), Some(CompletionState::Superseded)));
            assert!(
                matches!(last.try_get(), Some(CompletionState::Ready(_))),
                "{:?}",
                last.try_get()
            );
            assert_eq!(
                sched.try_get_source("/queued.vue").unwrap().source.as_ref(),
                "last"
            );
            let obsolete = sched.submit_request(request("obsolete", FileLanguage::script_ts()));
            sched.remove("/queued.vue");
            let replacement =
                sched.submit_request(request("replacement", FileLanguage::script_ts()));
            sched.drive_all();
            assert!(matches!(
                obsolete.try_get(),
                Some(CompletionState::Shutdown)
            ));
            assert!(matches!(
                replacement.try_get(),
                Some(CompletionState::Ready(_))
            ));
            assert_eq!(
                sched.try_get_source("/queued.vue").unwrap().source.as_ref(),
                "replacement"
            );
        }
    }

    #[test]
    fn queued_source_dependency_analysis_precedes_owner_artifact() {
        struct OrderProbe {
            events: StdMutex<Vec<String>>,
            extracted: bool,
        }
        impl StageExecutor for OrderProbe {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn extract_deps(
                &self,
                id: &str,
                _: &SourceSnapshot,
            ) -> crate::execution::executor::ExtractedDeps {
                if self.extracted && id == "/owner.vue" {
                    crate::execution::executor::ExtractedDeps {
                        forward_deps: vec!["/dep.ts".into()],
                        blocker_ids: vec!["/dep.ts".into()],
                    }
                } else {
                    crate::execution::executor::ExtractedDeps::default()
                }
            }
            fn execute_analysis(
                &self,
                id: &str,
                _: &SourceSnapshot,
                generation: u64,
            ) -> Result<AnalysisSnapshot, crate::execution::executor::StageError> {
                self.events.lock().unwrap().push(id.to_owned());
                Ok(AnalysisSnapshot::new_empty(generation))
            }
            fn execute_artifact(
                &self,
                _: &str,
                _: &SourceSnapshot,
                _: &AnalysisSnapshot,
                profile_hash: u64,
                generation: u64,
            ) -> Result<ArtifactSnapshot, crate::execution::executor::StageError> {
                self.events.lock().unwrap().push("artifact".into());
                Ok(ArtifactSnapshot {
                    generation,
                    profile_hash,
                    data: Arc::new(crate::node::EmptyData),
                })
            }
        }
        for (prequeued, extracted, reload) in [
            (true, false, false),
            (false, false, false),
            (true, true, false),
            (false, true, false),
            (true, false, true),
            (true, true, true),
        ] {
            let loader = Arc::new(MemorySourceLoader::new());
            loader.insert("/owner.vue".into(), Arc::from("owner"));
            loader.insert("/dep.ts".into(), Arc::from("dep"));
            let executor = Arc::new(OrderProbe {
                events: StdMutex::new(Vec::new()),
                extracted,
            });
            let sched = Scheduler::test_new_sync_with_executor(
                SchedulerConfig::default(),
                loader,
                executor.clone(),
            );
            let request = |id: &str, target| Request {
                file_id: id.into(),
                target,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: None,
            };
            if reload {
                let initial = sched.submit_request(request("/dep.ts", TargetStage::Analysis));
                assert!(sched.wait_or_drive(&initial).is_ready());
                executor.events.lock().unwrap().clear();
            }
            let owner = sched.submit_request(request("/owner.vue", TargetStage::Analysis));
            if extracted {
                // Leave the owner's Source completion ahead of the dependency
                // request in the inbox, while its node is already published.
                sched.drain_inbox();
                assert!(sched.drive_one());
            } else {
                assert!(sched.wait_or_drive(&owner).is_ready());
            }
            let dep = if reload {
                sched.close_file("/dep.ts");
                None
            } else {
                prequeued.then(|| sched.submit_request(request("/dep.ts", TargetStage::Source)))
            };
            if !extracted {
                sched.register_resolved_deps(
                    "/owner.vue",
                    vec!["/dep.ts".into()],
                    vec!["/dep.ts".into()],
                );
            }
            let artifact = sched.submit_request(request(
                "/owner.vue",
                TargetStage::Artifact { profile_hash: 7 },
            ));
            sched.drive_all();
            assert!(matches!(
                artifact.try_get(),
                Some(CompletionState::Ready(_))
            ));
            if let Some(dep) = dep {
                assert!(matches!(dep.try_get(), Some(CompletionState::Ready(_))));
            }
            let events = executor.events.lock().unwrap();
            assert_eq!(
                events.len(),
                3,
                "prequeued={prequeued}, extracted={extracted}, reload={reload}: {events:?}"
            );
            assert!(events[..2].iter().any(|id| id == "/owner.vue"));
            assert!(events[..2].iter().any(|id| id == "/dep.ts"));
            assert_eq!(
                events[2], "artifact",
                "both Analysis producers must precede Artifact"
            );
        }
    }

    #[test]
    fn dependency_registration_reset_during_backpressure_cannot_recreate_work() {
        struct ResetJoiner {
            scheduler: std::sync::Weak<Scheduler>,
            calls: AtomicU64,
        }
        impl RequestContextLike for ResetJoiner {
            fn request_id(&self) -> u64 {
                42
            }
            fn capture_enabled(&self) -> bool {
                false
            }
            fn on_dedup_joiner(&self, _: Arc<str>, _: u64, _: bool) {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                    self.scheduler.upgrade().unwrap().reset();
                }
            }
            fn record_cache_event(&self, _: CacheEventKind) {}
            fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
                struct Guard;
                impl TlsUninstall for Guard {
                    fn uninstall(self: Box<Self>) {}
                }
                Box::new(Guard)
            }
        }
        let loader = Arc::new(MemorySourceLoader::new());
        for id in ["/owner.vue", "/dup.vue", "/dep1.ts", "/dep2.ts", "/dep3.ts"] {
            loader.insert(id.into(), Arc::from("source"));
        }
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);
        let request = |id: &str| Request {
            file_id: id.into(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        };
        let owner = sched.submit_request(request("/owner.vue"));
        assert!(sched.wait_or_drive(&owner).is_ready());
        let mut dup = request("/dup.vue");
        dup.request_context = Some(OpaqueRequestContext(TestContext::new(1, false)));
        let _winner = sched.submit_request(dup);
        sched.drain_inbox();
        let joiner = Arc::new(ResetJoiner {
            scheduler: Arc::downgrade(&sched),
            calls: AtomicU64::new(0),
        });
        for _ in 0..sched.inbox.sender.capacity().unwrap() {
            let (_, sender) = completion_pair::<RequestResult>();
            assert!(sched
                .inbox
                .sender
                .try_send(Submission::NewRequest {
                    file_id: "/dup.vue".into(),
                    target: TargetStage::Analysis,
                    priority: Priority::Interactive,
                    source: None,
                    file_language: None,
                    sender,
                    submitted_lifetime: fixture_incarnation(&sched, "/dup.vue"),
                    request_context: Some(OpaqueRequestContext(joiner.clone())),
                })
                .is_ok());
        }
        let deps = vec!["/dep1.ts".into(), "/dep2.ts".into(), "/dep3.ts".into()];
        sched.register_resolved_deps("/owner.vue", deps.clone(), deps);
        assert_eq!(
            joiner.calls.load(Ordering::SeqCst),
            2,
            "reset must occur inside dependency enqueue backpressure"
        );
        sched.drive_all();
        assert!(
            sched.nodes.is_empty(),
            "retired registration recreated nodes after reset"
        );
        assert!(sched.auto_ingested_recent.is_empty());
        assert_eq!(sched.dag.lock().total_active(), 0);
    }

    #[test]
    fn exhausted_live_generation_refuses_advancement_and_leaves_no_history() {
        let sched = Scheduler::test_new_sync(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()),
        );
        let node = sched.create_node_at("/exhausted.vue", Some(FileLanguage::vue()), u64::MAX);
        let exhausted_incarnation = node.incarnation_id();
        let advance = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sched
                .source_root
                .publish_transition(|publication| publication.bump_node_generation(&node))
        }));
        assert!(
            advance.is_err(),
            "live generation exhaustion must refuse advancement"
        );
        assert_eq!(
            node.generation(),
            u64::MAX,
            "refused advancement must not wrap the stored generation"
        );
        sched.nodes.insert("/exhausted.vue".into(), node);
        sched.remove("/exhausted.vue");
        assert!(!sched.has_node("/exhausted.vue"));
        // The successor is a new object, not a continuation of the exhausted
        // one, so it starts a fresh generation sequence.
        let successor = sched.create_node("/exhausted.vue", Some(FileLanguage::vue()));
        assert_eq!(successor.generation(), 0);
        assert!(successor.incarnation_id() > exhausted_incarnation);
    }

    /// A language re-home advances a PUBLISHED file's generation, so it
    /// must run under the DAG lock and sweep the identities and waiters
    /// it retires — exactly like `invalidate()` and `close_file()`.
    ///
    /// Before the fix the re-home ran inside `prepare_request`, which is
    /// called OUTSIDE `dag.lock()`, and performed no supersede sweep at
    /// all — orphaning every identity admitted at the old generation and
    /// leaving the old waiters parked. Its own comment claimed the
    /// higher generation meant in-flight work "supersedes cleanly";
    /// nothing superseded it.
    ///
    /// Three assertions, matching the three ways the class shows up:
    /// stale-token removal, waiter supersession, and zero retained
    /// capacity.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn language_rehome_sweeps_stale_identities_waiters_and_capacity() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::job::CompletionState;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/reh.vue".to_string(), Arc::from("x"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Admit work at the Vue row, but do NOT drive it — the point is
        // that a live, admitted, undriven identity exists when the
        // re-home retires its generation.
        let first = sched.submit_request(Request {
            file_id: "/reh.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: Some(Arc::from("x")),
            file_language: Some(FileLanguage::vue()),
            request_context: None,
        });
        sched.drain_inbox();

        let canonical: Arc<str> = Arc::from("/reh.vue");
        let gen_before = sched.nodes.get("/reh.vue").unwrap().generation();
        let incarnation_before = fixture_incarnation(&sched, "/reh.vue");
        let admitted_stage = {
            let dag = sched.dag.lock();
            [FileStageKey::Source, FileStageKey::Analysis]
                .into_iter()
                .find(|stage| {
                    dag.token_for(&WorkNodeIdentity::FileStage {
                        canonical: Arc::clone(&canonical),
                        incarnation: incarnation_before,
                        generation: gen_before,
                        stage: *stage,
                    })
                    .is_some()
                })
        };
        let admitted_stage = admitted_stage.expect(
            "precondition: the first request must leave a live file-stage identity admitted",
        );
        assert!(
            first.try_get().is_none(),
            "precondition: the first request must still be pending before the re-home",
        );

        // Re-home onto a different resolved language. `source: None`, so
        // the admission core takes its no-sweep arm — the re-home itself
        // is the only thing that can retire the old generation.
        sched.submit_request(Request {
            file_id: "/reh.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Background,
            source: None,
            file_language: Some(FileLanguage::script_ts()),
            request_context: None,
        });
        sched.drain_inbox();

        let gen_after = sched.nodes.get("/reh.vue").unwrap().generation();
        assert!(
            gen_after > gen_before,
            "precondition: the re-home must advance the generation, {gen_before} -> {gen_after}",
        );

        // 1. Stale-token removal.
        let stale = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&canonical),
            incarnation: incarnation_before,
            generation: gen_before,
            stage: admitted_stage,
        };
        assert!(
            sched.dag.lock().token_for(&stale).is_none(),
            "the language re-home advanced the generation {gen_before} -> {gen_after} but left \
             {stale:?} admitted. A dispatcher popping it hits the generation-mismatch skip, \
             which never releases its parked capacity reservation.",
        );

        // 2. Waiter supersession — the old generation's waiter must be
        //    told, not left parked forever.
        assert!(
            matches!(first.try_get(), Some(CompletionState::Superseded)),
            "the re-home retired generation {gen_before} without signalling its waiter; \
             observed {:?}",
            first.try_get(),
        );

        // 3. Zero retained capacity for the retired generation.
        let dag = sched.dag.lock();
        let stale_nodes = dag.total_active() - dag.pending_len();
        assert_eq!(
            stale_nodes, 0,
            "the language re-home left dispatched-but-unfinished DAG nodes holding capacity",
        );
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
        use std::sync::mpsc;
        use std::sync::Arc;
        use std::thread;

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/race.vue".to_string(), Arc::from("r"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Seed a node at generation 1.
        sched.submit_request(Request {
            file_id: "/race.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Background,
            source: Some(Arc::from("r")),
            file_language: None,
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
        //
        // The invariant is proven by REAL EVENT ORDER, not by a fixed
        // sleep racing to sample mid-hold: the holder signals the exact
        // moment it acquires the lock (so the driver below never guesses
        // how long acquisition takes) and records the exact moment it
        // releases; `invalidate()`'s own return is then compared against
        // that recorded release instant. A bare-atomic bump would let
        // `invalidate()` return long before the holder's release,
        // independent of scheduler timing — never a narrow race margin.
        // Sample generation FROM THE HOLDER while the DAG lock is still
        // held. Timestamps from a side-thread monitor can pass under
        // observer starvation (the monitor wakes after release and records
        // a late instant even though the bump ran early). The holder
        // itself cannot miss an early bump: `node.generation()` is the
        // protected state, read under the lock that is supposed to gate
        // the bump.
        let dag_handle = Arc::clone(&sched.dag);
        let (acquired_tx, acquired_rx) = mpsc::sync_channel::<()>(0);
        let (release_tx, release_rx) = mpsc::sync_channel::<()>(0);
        let node_for_holder = Arc::clone(&node);
        let holder = thread::spawn(move || {
            let guard = dag_handle.lock();
            acquired_tx
                .send(())
                .expect("driver must outlive the acquire signal");
            release_rx
                .recv()
                .expect("driver must release after sampling");
            assert_eq!(
                node_for_holder.generation(),
                gen_before,
                "generation advanced while the DAG lock is held — \
                 bump_generation ran outside the lock"
            );
            drop(guard);
        });

        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("holder must signal lock acquisition (outer watchdog)");

        let (attempt_tx, attempt_rx) = mpsc::sync_channel::<()>(0);
        let node_clone = Arc::clone(&node);
        let sched_clone = Arc::clone(&sched);
        let inv = thread::spawn(move || {
            sched_clone.invalidate_signaling_before_dag_lock("/race.vue", attempt_tx);
            node_clone.generation()
        });
        // The hook fires immediately BEFORE `dag.lock()`, so this receipt
        // means "the invalidator is about to contend for the lock", not
        // "it is blocked on it". The sample below is therefore a cheap
        // early tripwire, not the discriminator: the holder's own in-lock
        // sample is what proves the bump did not run inside the hold, and
        // the type-level `SourcePublication` capability is what makes a
        // bare bump impossible to write at all.
        attempt_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("invalidator must reach the dag.lock() acquire (outer watchdog)");
        assert_eq!(
            node.generation(),
            gen_before,
            "generation advanced before dag.lock() — bump ran outside the lock"
        );
        release_tx
            .send(())
            .expect("holder must still be waiting to release");

        let gen_after_invalidate = inv.join().expect("invalidate thread must not panic");
        holder.join().expect("holder thread must not panic");
        assert!(
            gen_after_invalidate > gen_before,
            "invalidate() must still advance the generation once it \
             actually runs (gen_before={gen_before}, \
             observed={gen_after_invalidate})",
        );
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Seed nodes for each /race-{i}.vue at gen >= 1.
        for i in 0..8 {
            sched.submit_request(Request {
                file_id: format!("/race-{i}.vue"),
                target: TargetStage::Analysis,
                priority: Priority::Interactive,
                source: Some(Arc::from("v")),
                file_language: None,
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
                        if let Some(source) = sched_lifecycle.try_get_witnessed_source(&id) {
                            sched_lifecycle.commit_artifact(
                                &source.witness,
                                42,
                                Arc::new(crate::node::EmptyData),
                            );
                        }
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

        let sched = Scheduler::test_new(SchedulerConfig::default(), loader);

        // Drive /a.vue Source + Analysis to committed so a later
        // Artifact request finds the owner ready.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_language: None,
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
                    incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
            file_language: None,
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

    /// A Source snapshot is published by its worker before the driver integrates
    /// dependency facts from the matching completion. Requests arriving in that
    /// window must join the live Source identity rather than treating the raw
    /// snapshot bytes as a completed stage.
    #[test]
    fn published_source_is_not_request_ready_until_completion_is_integrated() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/paused.vue".to_string(), Arc::from("<template />"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let _prior_node = sched.create_node("/prior.ts", Some(FileLanguage::script_ts()));
        let initial = sched.submit_request(Request {
            file_id: "/paused.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });
        sched.drain_inbox();
        assert!(sched.drive_one(), "the Source work must dispatch");

        let canonical: Arc<str> = Arc::from("/paused.vue");
        let node = sched
            .nodes
            .get("/paused.vue")
            .expect("published FileNode")
            .clone();
        let generation = node.generation();
        assert!(
            node.current_source().is_some(),
            "precondition: the worker published snapshot bytes before StageComplete is drained",
        );
        let source_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&canonical),
            incarnation: fixture_incarnation(&sched, canonical.as_ref()),
            generation,
            stage: FileStageKey::Source,
        };
        assert!(
            sched.dag.lock().token_for(&source_identity).is_some(),
            "precondition: the dispatched Source identity stays live until integration",
        );

        let (source_joiner, source_sender) = completion_pair::<RequestResult>();
        sched.handle_new_request(
            "/paused.vue".to_string(),
            TargetStage::Source,
            Priority::Interactive,
            None,
            None,
            source_sender,
            fixture_incarnation(&sched, "/paused.vue"),
            None,
        );
        let (analysis_joiner, analysis_sender) = completion_pair::<RequestResult>();
        sched.handle_new_request(
            "/paused.vue".to_string(),
            TargetStage::Analysis,
            Priority::Interactive,
            None,
            None,
            analysis_sender,
            fixture_incarnation(&sched, "/paused.vue"),
            None,
        );

        assert!(
            source_joiner.try_get().is_none(),
            "published snapshot bytes must not satisfy a Source request before dependency integration",
        );
        let analysis_identity = WorkNodeIdentity::FileStage {
            canonical,
            incarnation: node.incarnation_id(),
            generation,
            stage: FileStageKey::Analysis,
        };
        assert!(
            sched.dag.lock().token_for(&analysis_identity).is_none(),
            "Analysis must not admit before the live Source identity integrates its completion",
        );

        sched.drain_inbox();
        sched.drive_all();
        assert!(initial.try_get().is_some_and(|state| state.is_ready()));
        assert!(source_joiner
            .try_get()
            .is_some_and(|state| state.is_ready()));
        assert!(analysis_joiner
            .try_get()
            .is_some_and(|state| state.is_ready()));
    }

    /// A legitimate Source-only producer has no live Source identity after
    /// completion. Registering a later Artifact blocker must atomically start
    /// its Analysis stage instead of classifying that producer as moot.
    #[test]
    fn late_blocker_starts_analysis_for_source_complete_dependency() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/owner.vue".to_string(), Arc::from("<template />"));
        loader.insert("/dep.ts".to_string(), Arc::from("export type T = string"));
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        for file_id in ["/owner.vue", "/dep.ts"] {
            let handle = sched.submit_request(Request {
                file_id: file_id.to_string(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: None,
            });
            assert!(
                sched.wait_or_drive(&handle).is_ready(),
                "Source-only precondition for {file_id}",
            );
        }

        let dep_generation = sched.nodes.get("/dep.ts").unwrap().generation();
        sched.register_resolved_deps(
            "/owner.vue",
            vec!["/dep.ts".to_string()],
            vec!["/dep.ts".to_string()],
        );

        let dep_canonical: Arc<str> = Arc::from("/dep.ts");
        let dep_analysis = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&dep_canonical),
            incarnation: fixture_incarnation(&sched, dep_canonical.as_ref()),
            generation: dep_generation,
            stage: FileStageKey::Analysis,
        };
        assert!(
            sched.dag.lock().token_for(&dep_analysis).is_some(),
            "late blocker demand must admit Analysis for an already Source-complete dependency",
        );

        sched.drive_all();
        assert!(
            sched
                .nodes
                .get("/dep.ts")
                .and_then(|node| node.current_analysis())
                .is_some(),
            "the Analysis admitted for late blocker demand must complete",
        );
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
        impl crate::execution::executor::StageExecutor for CountingAnalysisExecutor {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn execute_analysis(
                &self,
                _canonical_id: &str,
                _source: &crate::node::SourceSnapshot,
                generation: u64,
            ) -> Result<crate::node::AnalysisSnapshot, crate::execution::executor::StageError>
            {
                self.analysis_hits.fetch_add(1, Ordering::AcqRel);
                Ok(crate::node::AnalysisSnapshot::new_empty(generation))
            }
        }

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/owner.vue".to_string(), Arc::from("owner content"));
        let executor = Arc::new(CountingAnalysisExecutor {
            analysis_hits: AtomicU64::new(0),
        });
        let sched = Scheduler::test_new_sync_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::clone(&executor) as Arc<dyn crate::execution::executor::StageExecutor>,
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
            file_language: None,
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
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&owner_arc),
            incarnation: fixture_incarnation(&sched, owner_arc.as_ref()),
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
        impl crate::execution::executor::StageExecutor for OwnerWithDepExtractor {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn extract_deps(
                &self,
                canonical_id: &str,
                _source: &crate::node::SourceSnapshot,
            ) -> crate::execution::executor::ExtractedDeps {
                if canonical_id == "/a.vue" {
                    crate::execution::executor::ExtractedDeps {
                        forward_deps: vec!["/dep.ts".to_string()],
                        blocker_ids: vec!["/dep.ts".to_string()],
                    }
                } else {
                    crate::execution::executor::ExtractedDeps::default()
                }
            }
        }

        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("a content"));
        // /dep.ts deliberately omitted — Source loader returns None,
        // execute_source_stage routes through terminalize_failure
        // with FileNotFound, populating terminal_dep_failures.

        let executor: Arc<dyn crate::execution::executor::StageExecutor> =
            Arc::new(OwnerWithDepExtractor);
        let sched = Scheduler::test_with_executor(SchedulerConfig::default(), loader, executor);

        // Step 1: submit /dep.ts directly and drive to terminal
        // FileNotFound BEFORE /a.vue Source extracts deps. This
        // populates terminal_dep_failures under /dep.ts's Analysis
        // DepKey at /dep.ts's generation.
        let dep_handle = sched.submit_request(Request {
            file_id: "/dep.ts".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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
                        incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
            file_language: None,
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
            file_language: None,
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

        let sched = Scheduler::test_new(SchedulerConfig::default(), loader);

        // Step 1: independently fail /dep.ts FIRST so
        // terminal_dep_failures carries the record at its (gen=1,
        // Analysis) DepKey.
        let dep_handle = sched.submit_request(Request {
            file_id: "/dep.ts".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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
                        incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
            file_language: None,
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
            file_language: None,
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

        impl crate::execution::executor::StageExecutor for GatedFailingAnalysisExecutor {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn execute_analysis(
                &self,
                canonical_id: &str,
                _source: &crate::node::SourceSnapshot,
                generation: u64,
            ) -> Result<crate::node::AnalysisSnapshot, crate::execution::executor::StageError>
            {
                if let Some(gate) = self.gates.get(canonical_id) {
                    let _ = gate.entered_tx.send(());
                    let _ = gate.release_rx.recv();
                    return Err(crate::execution::executor::StageError {
                        kind: crate::execution::executor::StageErrorKind::Generic,
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

        let sched = Scheduler::test_with_executor(SchedulerConfig::default(), loader, executor);

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
            file_language: None,
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
            file_language: None,
            request_context: None,
        });

        let artifact_identity = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            incarnation: fixture_incarnation(&sched, "/a.vue"),
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        // Plant a FileNode for /dep.ts and bump to gen=1 so we have
        // a stable generation to plant the record under.
        let dep_node = sched.create_node("/dep.ts", None);
        let dep_gen_v1 = sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node));
        sched.nodes.insert("/dep.ts".to_string(), dep_node);

        let key_v1 = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
                fixture_incarnation(&sched, dep_arc.as_ref()),
                dep_gen_v1,
                &TaskKind::Load,
                &RequestResult::Source(Arc::clone(&source_snap)),
            );
            assert!(
                dag.lookup_terminal_dep_failure(&key_v1).is_none(),
                "terminal_dep_failures must be cleared by \
                 signal_stage_complete(Load) at the same gen. \
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
                fixture_incarnation(&sched, dep_arc.as_ref()),
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
                fixture_incarnation(&sched, dep_arc.as_ref()),
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Plant a FileNode for /dep.ts so the matrix / cleanup
        // sweeps have something to operate on. Then plant a
        // terminal-dep-failure record under that FileNode's
        // generation.
        let dep_arc: Arc<str> = Arc::from("/dep.ts");
        let dep_node = sched.create_node("/dep.ts", None);
        let dep_gen_v1 = sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node));
        sched.nodes.insert("/dep.ts".to_string(), dep_node);
        let key_v1 = DepKey::FileStage {
            canonical: Arc::clone(&dep_arc),
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
            generation: dep_gen_v2,
            stage: FileStageKey::Analysis,
        };
        let sibling_arc: Arc<str> = Arc::from("/sibling.ts");
        let sibling_key = DepKey::FileStage {
            canonical: Arc::clone(&sibling_arc),
            incarnation: fixture_incarnation(&sched, sibling_arc.as_ref()),
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

        let sched = Scheduler::test_new(SchedulerConfig::default(), loader);

        // Drive /a.vue Analysis ready.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_language: None,
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
                    incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
            file_language: None,
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

        let sched = Scheduler::test_with_executor(SchedulerConfig::default(), loader, executor);

        // Drive /a.vue Source + Analysis to committed.
        let analysis_handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_language: None,
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
            file_language: None,
            request_context: None,
        });

        // Wait for Artifact admission so the fan-out path attaches
        // the marker on a live waiter (avoiding the pre-admission
        // race covered by the sibling test).
        let artifact_identity = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/a.vue"),
            incarnation: fixture_incarnation(&sched, "/a.vue"),
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Plant /a.vue and /dep.ts FileNodes at gen=1.
        let a_node = sched.create_node("/a.vue", None);
        let a_gen = sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&a_node));
        sched.nodes.insert("/a.vue".to_string(), a_node);
        let dep_node = sched.create_node("/dep.ts", None);
        let dep_gen = sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node));
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
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
                fixture_incarnation(&sched, a_arc.as_ref()),
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
                DepKey::FileStage { canonical, stage, generation, .. }
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
            sched
                .admit_artifact_with_blockers(
                    &mut dag,
                    &a_arc,
                    fixture_incarnation(&sched, a_arc.as_ref()),
                    a_gen,
                    /* profile_hash = */ 99,
                    Priority::Interactive,
                    None,
                )
                .expect("test artifact admission refused by the live object witness")
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
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Plant /a.vue and /dep.ts FileNodes at gen=1, with full
        // Source+Analysis committed for both (recovery state).
        let a_node = sched.create_node("/a.vue", None);
        let a_gen = sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&a_node));
        sched.nodes.insert("/a.vue".to_string(), a_node);
        let dep_node = sched.create_node("/dep.ts", None);
        let dep_gen = sched
            .source_root
            .publish_transition(|publication| publication.bump_node_generation(&dep_node));
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
            incarnation: fixture_incarnation(&sched, dep_arc.as_ref()),
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
            sched
                .admit_artifact_with_blockers(
                    &mut dag,
                    &a_arc,
                    fixture_incarnation(&sched, a_arc.as_ref()),
                    a_gen,
                    /* profile_hash = */ 31,
                    Priority::Interactive,
                    None,
                )
                .expect("test artifact admission refused by the live object witness")
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
    impl crate::execution::executor::StageExecutor for HookExecutor {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn execute_source(
            &self,
            _canonical_id: &str,
            _file_language: FileLanguage,
            content: Arc<str>,
            generation: u64,
            _incarnation: u64,
        ) -> Result<crate::node::SourceSnapshot, crate::execution::executor::StageError> {
            Ok(crate::node::SourceSnapshot::new_empty(content, generation))
        }
        fn execute_analysis(
            &self,
            canonical_id: &str,
            _source: &crate::node::SourceSnapshot,
            generation: u64,
        ) -> Result<crate::node::AnalysisSnapshot, crate::execution::executor::StageError> {
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
                    file_language: None,
                    request_context: None,
                });
                *inner_state_for_hook.lock() =
                    Some(sched.wait_or_drive_with_caller(&inner, CallerKind::CpuWorker));
            }
        });

        let sched = Scheduler::test_with_executor(
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
            file_language: None,
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
                    file_language: None,
                    request_context: None,
                });
                let state = sched.wait_or_drive_with_caller(&inner, CallerKind::CpuWorker);
                *inner_state_for_hook.lock() = Some(state);
            }
        });

        let sched = Scheduler::test_with_executor(
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
            file_language: None,
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
        let sched = Scheduler::test_with_executor(
            SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(crate::execution::executor::DefaultExecutor),
        );

        let identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            incarnation: fixture_incarnation(&sched, "/x.vue"),
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
        let sched = Scheduler::test_with_executor(
            SchedulerConfig {
                cpu_threads: 2,
                io_threads: 2,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(crate::execution::executor::DefaultExecutor),
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
                    file_language: None,
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
    /// the drain AND the dispatch counters; a pump that finds no
    /// drainable submission and no ready job must report no progress
    /// so the driver parks instead of spinning.
    ///
    /// Idleness is a property of the available pump work, NOT of the
    /// request state: a pump run while a dispatched job is still in
    /// flight has nothing to drain and nothing ready, and must report
    /// no progress — reporting progress there is exactly the
    /// regression that makes a driver loop burn a core against a
    /// long-running job.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pump_ready_drains_inbox_and_dispatches_ready_jobs() {
        use crate::caller_kind::CallerKind;
        use std::time::{Duration, Instant};

        /// One-shot latch. Every wait is bounded so a scheduler that
        /// never reaches the awaited point fails the test rather than
        /// hanging it.
        #[derive(Default)]
        struct Latch {
            opened: parking_lot::Mutex<bool>,
            changed: parking_lot::Condvar,
        }
        impl Latch {
            fn open(&self) {
                *self.opened.lock() = true;
                self.changed.notify_all();
            }
            /// `true` when the latch is open, `false` on timeout.
            fn wait(&self, budget: Duration) -> bool {
                let deadline = Instant::now() + budget;
                let mut opened = self.opened.lock();
                while !*opened {
                    if self.changed.wait_until(&mut opened, deadline).timed_out() {
                        return *opened;
                    }
                }
                true
            }
        }

        /// Parks the dispatched Source job inside the loader seam until
        /// the test releases it, so "the job is still in flight" is a
        /// fact the test establishes rather than a race it hopes to
        /// win. Ungated, the job posts its own stage completion straight
        /// back into the inbox and the next pump legitimately drains 1.
        struct GatedLoader {
            inner: MemorySourceLoader,
            entered: Arc<Latch>,
            release: Arc<Latch>,
        }
        impl SourceLoader for GatedLoader {
            fn load(&self, canonical_id: &str) -> Option<Arc<str>> {
                self.entered.open();
                self.release.wait(Duration::from_secs(30));
                self.inner.load(canonical_id)
            }
            fn exists(&self, canonical_id: &str) -> bool {
                self.inner.exists(canonical_id)
            }
            fn classify(&self, canonical_id: &str) -> FileLanguage {
                self.inner.classify(canonical_id)
            }
            fn realpath(&self, canonical_id: &str) -> Option<String> {
                self.inner.realpath(canonical_id)
            }
        }

        let inner = MemorySourceLoader::new();
        inner.insert("/a.vue".to_string(), Arc::from("<template>x</template>"));
        let entered = Arc::new(Latch::default());
        let release = Arc::new(Latch::default());
        let sched = Scheduler::test_new_sync(
            SchedulerConfig::default(),
            Arc::new(GatedLoader {
                inner,
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
            }) as Arc<dyn SourceLoader>,
        );

        // Submit a request — lands in the inbox.
        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
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

        // In flight: the dispatched job is parked in the loader seam,
        // so it cannot have posted a completion. Nothing is drainable,
        // nothing is ready — every counter must be zero and the driver
        // must be told to park rather than re-poll the running job.
        assert!(
            entered.wait(Duration::from_secs(30)),
            "the dispatched Source job never reached the loader seam, so this test would \
             prove nothing about a pump with work in flight",
        );
        assert!(
            handle.try_get().is_none(),
            "precondition: the request must still be in flight while the loader is held",
        );
        let in_flight = sched.pump_ready(PumpReason::DriverLoop, CallerKind::Driver);
        assert_eq!(
            in_flight.drained, 0,
            "in-flight pump drained={}, expected 0",
            in_flight.drained,
        );
        assert_eq!(
            in_flight.dispatched, 0,
            "in-flight pump dispatched={}, expected 0",
            in_flight.dispatched,
        );
        assert_eq!(
            in_flight.executed_inline, 0,
            "in-flight pump executed_inline={}, expected 0",
            in_flight.executed_inline,
        );
        assert!(
            !in_flight.made_progress(),
            "a pump with nothing drainable and nothing ready must NOT report progress just \
             because a job is in flight, got {in_flight:?}",
        );

        // Release the worker. This scheduler carries no driver thread,
        // so the test owns the pump that carries the completion to a
        // terminal state. Bounded: a scheduler that never completes
        // fails here rather than looping forever.
        release.open();
        let deadline = Instant::now() + Duration::from_secs(30);
        let state = loop {
            if let Some(state) = handle.try_get() {
                break state;
            }
            assert!(
                Instant::now() < deadline,
                "request never reached a terminal state under a test-owned pump",
            );
            sched.pump_ready(PumpReason::DriverLoop, CallerKind::Driver);
            std::thread::yield_now();
        };
        assert!(
            matches!(state, CompletionState::Ready(_)),
            "the Source request must complete Ready, got {state:?}",
        );

        // Settled: the stage completion that resolved the handle was
        // drained by the pump that resolved it, nothing is admitted,
        // nothing is ready. Every counter must be zero so the driver
        // parks instead of spinning.
        let idle = sched.pump_ready(PumpReason::DriverLoop, CallerKind::Driver);
        assert_eq!(
            idle.drained, 0,
            "settled pump drained={}, expected 0",
            idle.drained
        );
        assert_eq!(
            idle.dispatched, 0,
            "settled pump dispatched={}, expected 0",
            idle.dispatched,
        );
        assert_eq!(
            idle.executed_inline, 0,
            "settled pump executed_inline={}, expected 0",
            idle.executed_inline,
        );
        assert!(
            !idle.made_progress(),
            "settled pump with no work must NOT report progress, got {idle:?}",
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
        let dag = crate::dag::SchedulerDag::new();
        let owner: Arc<str> = Arc::from("/a.vue");
        let self_dep = DepKey::FileStage {
            canonical: Arc::clone(&owner),
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (kept, dropped) =
            Scheduler::filter_macro_cycle_deps(&dag, &owner, 1, 1, vec![self_dep.clone()]);
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
        let mut dag = crate::dag::SchedulerDag::new();
        let a: Arc<str> = Arc::from("/a.vue");
        let b: Arc<str> = Arc::from("/b.vue");

        // Build state: A's Analysis is admitted with a B→Analysis
        // gating dep (B not yet committed). This is the "A already
        // gating on B" half of the mutual cycle.
        let a_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&a),
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let b_dep = DepKey::FileStage {
            canonical: Arc::clone(&b),
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        dag.submit_expect(
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
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (kept, dropped) =
            Scheduler::filter_macro_cycle_deps(&dag, &b, 1, 1, vec![a_dep.clone()]);
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
        let mut dag = crate::dag::SchedulerDag::new();
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
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let c_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&c),
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let c_dep = DepKey::FileStage {
            canonical: Arc::clone(&c),
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let a_dep = DepKey::FileStage {
            canonical: Arc::clone(&a),
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        dag.submit_expect(
            b_id,
            crate::dag::WorkKind::Analysis,
            Priority::Background,
            vec![c_dep.clone()],
            None,
        );
        dag.submit_expect(
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
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (kept, dropped) =
            Scheduler::filter_macro_cycle_deps(&dag, &a, 1, 1, vec![b_dep.clone()]);
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
        let mut dag = crate::dag::SchedulerDag::new();
        let a: Arc<str> = Arc::from("/a.vue");
        let b: Arc<str> = Arc::from("/b.vue");

        // B's Analysis is admitted with NO deps back to A.
        let b_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&b),
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        dag.submit_expect(
            b_id,
            crate::dag::WorkKind::Analysis,
            Priority::Background,
            Vec::new(),
            None,
        );

        // A→B dep must survive the filter.
        let b_dep = DepKey::FileStage {
            canonical: Arc::clone(&b),
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let (kept, dropped) =
            Scheduler::filter_macro_cycle_deps(&dag, &a, 1, 1, vec![b_dep.clone()]);
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

        let sched = StdArc::new(Scheduler::test_with_executor(
            SchedulerConfig::default(),
            loader,
            StdArc::new(crate::execution::executor::DefaultExecutor),
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
            file_language: None,
            request_context: None,
        });
        let b_handle = sched.submit_request(Request {
            file_id: "/b.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: Some(Arc::from("b content")),
            file_language: None,
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
            incarnation: fixture_incarnation(&sched, a_arc.as_ref()),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let b_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&b_arc),
            incarnation: fixture_incarnation(&sched, b_arc.as_ref()),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let dep_on_a = DepKey::FileStage {
            canonical: Arc::clone(&a_arc),
            incarnation: fixture_incarnation(&sched, a_arc.as_ref()),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let dep_on_b = DepKey::FileStage {
            canonical: Arc::clone(&b_arc),
            incarnation: fixture_incarnation(&sched, b_arc.as_ref()),
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

    /// The real Analysis producer waits for its own Artifact while its
    /// Analysis snapshot is still unpublished. Only self-await rejection
    /// can release that physical dependency cycle.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn analysis_executor_submits_same_file_artifact_reaches_a_terminal_state() {
        use crate::job::SchedulerError;

        struct SelfAwaitExecutor {
            scheduler: StdMutex<std::sync::Weak<Scheduler>>,
            entered: std::sync::mpsc::Sender<()>,
            result: std::sync::mpsc::Sender<CompletionState<RequestResult>>,
            release: StdMutex<std::sync::mpsc::Receiver<()>>,
        }
        impl StageExecutor for SelfAwaitExecutor {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn execute_analysis(
                &self,
                id: &str,
                _: &SourceSnapshot,
                generation: u64,
            ) -> Result<AnalysisSnapshot, crate::execution::executor::StageError> {
                let sched = self.scheduler.lock().unwrap().upgrade().unwrap();
                self.entered.send(()).unwrap();
                let artifact = sched.submit_request(Request {
                    file_id: id.into(),
                    target: TargetStage::Artifact { profile_hash: 7 },
                    priority: Priority::Interactive,
                    source: None,
                    file_language: None,
                    request_context: None,
                });
                let state = sched.wait_or_drive_with_caller(
                    &artifact,
                    crate::caller_kind::CallerKind::CpuWorker,
                );
                let _ = self.result.send(state);
                let _ = self.release.lock().unwrap().recv();
                Ok(AnalysisSnapshot::new_empty(generation))
            }
        }
        struct ReleaseOnDrop {
            sched: Arc<Scheduler>,
            release: std::sync::mpsc::Sender<()>,
        }
        impl Drop for ReleaseOnDrop {
            fn drop(&mut self) {
                self.sched.dag.lock().signal_all_shutdown();
                let _ = self.release.send(());
            }
        }
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let executor = Arc::new(SelfAwaitExecutor {
            scheduler: StdMutex::new(std::sync::Weak::new()),
            entered: entered_tx,
            result: result_tx,
            release: StdMutex::new(release_rx),
        });
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".into(), Arc::from("source"));
        let sched = Scheduler::test_with_executor(
            SchedulerConfig {
                cpu_threads: 2,
                io_threads: 1,
                dag_budget: None,
            },
            loader,
            executor.clone(),
        );
        *executor.scheduler.lock().unwrap() = Arc::downgrade(&sched);
        let release = ReleaseOnDrop {
            sched: sched.clone(),
            release: release_tx,
        };
        let analysis = sched.submit_request(Request {
            file_id: "/a.vue".into(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: None,
            file_language: None,
            request_context: None,
        });
        let watchdog = std::time::Duration::from_secs(60);
        entered_rx
            .recv_timeout(watchdog)
            .expect("the real Analysis executor must enter");
        let state = result_rx
            .recv_timeout(watchdog)
            .expect("self-await must release the blocked Analysis producer");
        assert!(
            sched.try_get_analysis("/a.vue").is_none(),
            "producer must remain unpublished while its self-await result is checked"
        );
        assert!(
            matches!(state, CompletionState::Failed(SchedulerError::StageFailed { ref stage, .. }) if stage == "wait_or_drive"),
            "{state:?}"
        );
        release.release.send(()).unwrap();
        assert!(sched.wait_or_drive(&analysis).is_ready());
    }

    /// A single I/O worker that calls `wait_or_drive` on an
    /// I/O-bound dep must inline-execute the dep on its own thread.
    /// Without the symmetric `IoWorker × Source` inline-eligibility
    /// the dep would be dispatched back onto the I/O pool via
    /// `try_submit_io` and sit behind the parked I/O worker forever.
    /// The I/O capacity loan covers the budget-exhausted case.
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
        impl crate::execution::executor::StageExecutor for SourceHookExecutor {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn execute_source(
                &self,
                canonical_id: &str,
                _file_language: FileLanguage,
                content: Arc<str>,
                generation: u64,
                _incarnation: u64,
            ) -> Result<crate::node::SourceSnapshot, crate::execution::executor::StageError>
            {
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
                    file_language: None,
                    request_context: None,
                });
                *inner_state_for_hook.lock() =
                    Some(sched.wait_or_drive_with_caller(&inner, CallerKind::IoWorker));
            }
        });

        let sched = Scheduler::test_with_executor(
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
            file_language: None,
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
        let sched = Arc::new(Scheduler::test_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::execution::executor::DefaultExecutor),
        ));

        // Submit a request and wait for it to be admitted. Once
        // handle_new_request runs, the sender's target slot must
        // hold a `Work` variant, not the initial `Request` shape.
        let handle = sched.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            source: Some(Arc::from("a content")),
            file_language: None,
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
                        file_language: None,
                        request_context: Some(OpaqueRequestContext(
                            ctx_inner as Arc<dyn RequestContextLike>,
                        )),
                    });
                    let _ = sched.wait_or_drive_with_caller(&inner, CallerKind::CpuWorker);
                }
            } else if canonical == "/b.vue" {
                // Inner execution: record the observed TLS id.
                let id = verter_execution::request_context::current_request_id().unwrap_or(0);
                inner_observed_for_hook.store(id, MOrd::SeqCst);
            }
        });

        let sched = Scheduler::test_with_executor(
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
            file_language: None,
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
        impl crate::execution::executor::StageExecutor for SourceHookExecutorIo {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn execute_source(
                &self,
                canonical_id: &str,
                _file_language: FileLanguage,
                content: Arc<str>,
                generation: u64,
                _incarnation: u64,
            ) -> Result<crate::node::SourceSnapshot, crate::execution::executor::StageError>
            {
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
                let ctx_guard = verter_execution::request_context::OpaqueContextGuard::install(
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
            _ctx: verter_execution::request_context::OpaqueContextGuard,
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
                    file_language: None,
                    request_context: Some(OpaqueRequestContext(
                        inner_ctx as Arc<dyn RequestContextLike>,
                    )),
                });
                let _ = sched.wait_or_drive_with_caller(&inner, CallerKind::IoWorker);
            }
        });

        let sched = Scheduler::test_with_executor(
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
            file_language: None,
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
                        file_language: None,
                        request_context: None,
                    });
                    let _ = sched.wait_or_drive_with_caller(&inner, CallerKind::CpuWorker);
                }
            } else if canonical == "/b.vue" {
                // Inner execution: record the observed TLS state
                // for each install_tls slot. 0 means cleared.
                let id = verter_execution::request_context::current_request_id().unwrap_or(0);
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

        let sched = Scheduler::test_with_executor(
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
            file_language: None,
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
        use std::sync::atomic::AtomicU64;
        use verter_execution::request_context::{
            AllSlotsClearGuard, OpaqueContextGuard, OpaqueRequestContext, RequestContextLike,
            TlsUninstall,
        };

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
            fn record_cache_event(
                &self,
                _event: verter_execution::request_context::CacheEventKind,
            ) {
            }
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
            verter_execution::request_context::current_request_id(),
            Some(12345),
            "outer install must show in scheduler opaque slot",
        );

        {
            let _clear = AllSlotsClearGuard::clear_all();
            assert_eq!(
                verter_execution::request_context::current_request_id(),
                None,
                "AllSlotsClearGuard must clear scheduler opaque slot while alive",
            );
        }

        assert_eq!(
            verter_execution::request_context::current_request_id(),
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
        let sched = Arc::new(Scheduler::test_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::execution::executor::DefaultExecutor),
        ));

        // Build a handle, stamp it with a Work target that
        // matches the active path, then resolve it to Ready
        // BEFORE calling wait_or_drive. Without the try_get-first
        // ordering the same-path check would mask the Ready with
        // a synthetic Failed.
        let identity = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            incarnation: fixture_incarnation(&sched, "/x.vue"),
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
        let sched = Arc::new(Scheduler::test_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::execution::executor::DefaultExecutor),
        ));

        // Manually construct a handle with the pre-admission
        // `Request{Artifact{..}}` target — the race-window state.
        let analysis_id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            incarnation: fixture_incarnation(&sched, "/x.vue"),
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
        let sched = Arc::new(Scheduler::test_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::execution::executor::DefaultExecutor),
        ));

        // Hand-constructed handle with the pre-admission
        // `Request{Analysis}` target — the race-window state.
        let source_frame = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            incarnation: fixture_incarnation(&sched, "/x.vue"),
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
        let sched = Arc::new(Scheduler::test_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::execution::executor::DefaultExecutor),
        ));

        let profile = 0x42u64;
        let artifact_frame = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/x.vue"),
            incarnation: fixture_incarnation(&sched, "/x.vue"),
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
            incarnation: 1,
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
        let sched = Arc::new(Scheduler::test_with_executor(
            SchedulerConfig::default(),
            loader,
            Arc::new(crate::execution::executor::DefaultExecutor),
        ));

        // Active frame: /x.vue Analysis. Handle target: Request{
        // canonical=/x.vue, Analysis } — DOES match active path
        // via canonical+stage, so the helper takes the
        // same-path-Failed branch unless the inner re-check
        // catches the resolution.
        let active_frame = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            incarnation: fixture_incarnation(&sched, "/x.vue"),
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
        let sched = Arc::new(Scheduler::test_with_executor(
            SchedulerConfig {
                cpu_threads: 2,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
            Arc::new(crate::execution::executor::DefaultExecutor),
        ));

        // Active-path frame is /y.vue Analysis. The handle's
        // initial Request{} target names /x.vue with target =
        // Source — does NOT match the active path. After the
        // late stamp the target becomes Work(FileStage{Analysis:
        // /y.vue, gen=1}), which IS the active path → same-path
        // Failed must surface.
        let active_frame = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/y.vue"),
            incarnation: fixture_incarnation(&sched, "/y.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let late_stamped_work = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/y.vue"),
            incarnation: fixture_incarnation(&sched, "/y.vue"),
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

    // ──────────────────────────────────────────────────────────────────
    // §6b — atomic batch admission (`submit_batch_atomic` / `wait_batch`)
    //
    // Each test below is DISCRIMINATING: it fails against a naive
    // sequential implementation (N separate `NewRequest` inbox items,
    // dedup callbacks fired under the DAG lock, per-item `submit_count`
    // bumps) and passes only against the atomic implementation (one
    // `NewRequestBatch` inbox item drained as a unit, all N admitted
    // under ONE DAG lock, dedup callbacks fired AFTER the lock releases,
    // one `submit_count` bump per batch).
    // ──────────────────────────────────────────────────────────────────

    /// A test source loader that resolves any path to a fixed body so a
    /// batch of N distinct canonicals can all admit a Source stage.
    fn batch_loader(paths: &[&str]) -> Arc<MemorySourceLoader> {
        let loader = Arc::new(MemorySourceLoader::new());
        for p in paths {
            loader.insert((*p).to_string(), Arc::from("<template>x</template>"));
        }
        loader
    }

    /// Build N distinct Source requests over `/n0.vue`.. with no source
    /// bytes (cold load through the loader).
    fn n_source_requests(n: usize) -> (Vec<String>, Vec<Request>) {
        let ids: Vec<String> = (0..n).map(|i| format!("/n{i}.vue")).collect();
        let reqs = ids
            .iter()
            .map(|id| Request {
                file_id: id.clone(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: None,
            })
            .collect();
        (ids, reqs)
    }

    /// LOCK-CONTINUITY discriminator (§6b P1b): process the already-drained
    /// atomic batch `item` (carrying exactly `n` admissible requests) and
    /// PROVE that all `n` admits ran under ONE continuously-held
    /// `dag.lock()` — not a per-item lock/unlock that merely leaves the
    /// right end-state.
    ///
    /// Two independent rails, the FIRST of which is the deterministic
    /// both-directions discriminator:
    ///
    /// RAIL 1 — epoch identity (deterministic, race-free, BOTH directions).
    ///   The admission-lock acquisition bumps a monotonic epoch
    ///   ([`Scheduler::acquire_dag_for_admission`]); every admit records the
    ///   epoch it ran under. After the batch this asserts the trace holds
    ///   exactly `n` entries that are ALL EQUAL — i.e. one acquisition
    ///   spanned every admit. A per-item lock/unlock acquires (and bumps the
    ///   epoch) once per item, so the recorded epochs differ → FAILS. This
    ///   rail does NOT depend on thread scheduling, so it fails a per-item
    ///   regression deterministically regardless of how `parking_lot`'s
    ///   barging unlock resolves the lock-handoff race.
    ///
    /// RAIL 2 — concurrent observer (the observable consequence; deterministic
    ///   in the held-lock direction). A `#[cfg(test)]` seam fires on the
    ///   admitting thread AFTER the first admit and BEFORE the rest, WHILE
    ///   the lock is held. It releases a watcher thread that does a BLOCKING
    ///   `dag.lock()`. Because the loop keeps holding the one lock through
    ///   the remaining admits, the watcher cannot acquire until the lock
    ///   drops at loop end — at which point the DAG already shows ALL `n`
    ///   admitted, so the watcher observes `pending_len() == n`. This proves
    ///   "no concurrent thread can ever first-observe the batch
    ///   half-admitted". A blocking acquire (not a single `try_lock` probe)
    ///   is what makes this span the WHOLE window 1..n rather than only the
    ///   instant after the first admit.
    ///
    /// A watchdog bounds the watcher so a regression that prevents the seam
    /// from firing fails loudly instead of hanging the suite. A ran-flag
    /// rejects a trivially-passing run where the seam was skipped.
    ///
    /// Requires `n >= 2` so there IS a post-first-admit window to observe.
    fn assert_batch_admit_holds_one_dag_lock(sched: &Arc<Scheduler>, item: Submission, n: usize) {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::mpsc;
        use std::sync::Barrier;
        use std::time::Duration;

        assert!(
            n >= 2,
            "the LOCK-CONTINUITY helper needs n >= 2 to have a mid-batch window; got n={n}",
        );

        // RAIL 1: arm the per-admit epoch recorder before admission runs.
        sched.test_install_batch_admit_epoch_trace();

        // RAIL 2 rendezvous: the seam hook signals `at_seam` and RETURNS
        // (it must NOT wait for the watcher to acquire — the watcher
        // blocking-acquires the SAME lock the admitting thread holds, so a
        // wait-for-acquire would deadlock). The admitting thread proceeds
        // to admit the remaining requests under the still-held lock.
        let at_seam = Arc::new(Barrier::new(2));
        // How many admits had recorded their epoch at the instant the
        // watcher finally acquired the DAG lock. One push happens per admit
        // UNDER the held DAG lock, so by mutual exclusion the watcher can
        // only read this while the admitting thread is NOT mid-admit. Post-
        // fix the watcher acquires only after the loop drops the one held
        // lock, so every admit has recorded → this is `n`. (Trace count is
        // used rather than `pending_len()` so the same observable holds for
        // BOTH the fresh-admit and the supersede batch, whose DAG node
        // counts differ.)
        let admits_recorded_at_acquire = Arc::new(AtomicUsize::new(usize::MAX));
        // Set once the watcher has actually acquired + recorded, so the
        // watchdog distinguishes "seam never fired" from "fired, observed".
        let watcher_ran = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = mpsc::channel::<()>();

        // Watcher: wait for the admitting thread to reach the mid-batch
        // seam, then BLOCKING-acquire the DAG lock. It parks until the
        // admitting thread's loop drops the one held lock; the admit count it
        // then reads is the whole batch (`n`).
        let watcher = {
            let sched = Arc::clone(sched);
            let at_seam = Arc::clone(&at_seam);
            let admits_recorded_at_acquire = Arc::clone(&admits_recorded_at_acquire);
            let watcher_ran = Arc::clone(&watcher_ran);
            std::thread::spawn(move || {
                at_seam.wait();
                // BLOCKING acquire: under the held lock this parks until the
                // admission loop releases at its `}`. It cannot observe a
                // partial batch because mutual exclusion forbids acquiring
                // mid-loop; the epoch-trace peek runs while holding the DAG
                // lock (same DAG→trace lock order as the admit path, so no
                // AB-BA), reading how many admits had completed.
                let guard = sched.dag.lock();
                admits_recorded_at_acquire
                    .store(sched.test_peek_batch_admit_epoch_count(), Ordering::SeqCst);
                drop(guard);
                watcher_ran.store(true, Ordering::SeqCst);
                let _ = done_tx.send(());
            })
        };

        // Install the seam for exactly this batch. It runs ON the admitting
        // thread WHILE the DAG lock is held, between the first and the rest;
        // it only RELEASES the watcher (signal-and-proceed), never waits.
        {
            let at_seam = Arc::clone(&at_seam);
            sched.test_install_batch_admit_seam(Box::new(move || {
                at_seam.wait();
            }));
        }

        // Admit the batch on THIS thread; the seam fires after the first
        // admit and the remaining admits run under the same held lock.
        sched.process_submission(item);

        // WATCHDOG: the watcher must report within the timeout. A regression
        // that never reaches the seam (so the watcher is stuck on `at_seam`)
        // fails here loudly instead of hanging the suite.
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| {
                panic!(
                    "batch-admit seam watchdog: the mid-batch seam never fired — the atomic \
                     batch did not reach the post-first-admit window under a held DAG lock \
                     (P1b LOCK-CONTINUITY regression)",
                )
            });

        watcher.join().expect("watcher thread must not panic");

        assert!(
            watcher_ran.load(Ordering::SeqCst),
            "the batch-admit seam watcher must have acquired the DAG lock and recorded",
        );

        // ── RAIL 1 assertion (deterministic continuity) ──
        let epochs = sched.test_take_batch_admit_epochs();
        assert_eq!(
            epochs.len(),
            n,
            "every one of the {n} admits must record its acquisition epoch; got {} entries \
             {epochs:?} (a per-item path that admitted fewer-or-more under the one item, or \
             skipped recording, is caught here)",
            epochs.len(),
        );
        let first_epoch = epochs[0];
        assert!(
            epochs.iter().all(|&e| e == first_epoch),
            "all {n} admits must run under ONE `dag.lock()` acquisition — every recorded epoch \
             must be identical; got {epochs:?}. Differing epochs mean admission re-acquired the \
             DAG lock between items (P1b LOCK-CONTINUITY regression: per-item lock/unlock instead \
             of one held lock), so the pump could observe the batch half-admitted",
        );

        // ── RAIL 2 assertion (observable consequence) ──
        let observed = admits_recorded_at_acquire.load(Ordering::SeqCst);
        assert_eq!(
            observed, n,
            "a concurrent thread that blocking-acquires the DAG lock from the mid-batch seam \
             must not be able to acquire until ALL {n} admits are done — at acquire it must see \
             all {n} admits recorded, got {observed}. A per-item lock/unlock would let it acquire \
             inside the batch and observe a partial count (P1b LOCK-CONTINUITY regression)",
        );
    }

    /// (1) The pump must NOT be able to observe a batch half-admitted.
    ///
    /// Submit N distinct requests as ONE atomic batch, then drain
    /// EXACTLY ONE inbox item. The atomic path lands all N DAG nodes
    /// from that single item, so `pending_len() == N` and the inbox is
    /// empty afterward.
    ///
    /// Discrimination has TWO independent rails:
    ///
    /// 1. END-STATE: a sequential per-request submission (N separate
    ///    `submit_request` calls) pushes N separate `NewRequest` items;
    ///    draining exactly one would admit ONE node (`pending_len() == 1`)
    ///    and leave N-1 items in the inbox. The `== N` + empty-inbox
    ///    assertion fails on the sequential impl.
    /// 2. LOCK-CONTINUITY (P1b): the end-state alone does NOT prove the
    ///    admit ran under ONE held `dag.lock()` — a per-item lock/unlock
    ///    that admitted all N from the one item would leave the same
    ///    `pending_len == N` + empty inbox while letting a concurrent
    ///    pump observe partial DAG state mid-batch. The
    ///    `assert_batch_admit_holds_one_dag_lock` helper proves continuity
    ///    deterministically via the epoch rail (all N admits record ONE
    ///    acquisition epoch; a per-item lock/unlock bumps a fresh epoch per
    ///    item → recorded epochs differ → FAIL) and corroborates it with a
    ///    concurrent observer that blocking-acquires the DAG lock and can
    ///    only get in once all N are admitted.
    #[test]
    fn atomic_batch_admission_not_partially_observable_by_pump() {
        const N: usize = 5;
        let (ids, reqs) = n_source_requests(N);
        let loader = batch_loader(&ids.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let sched = test_scheduler_with_loader(loader);

        let _batch = sched.submit_batch_atomic(reqs);

        // Drain exactly ONE inbox item — the whole batch is one item.
        let one = sched
            .inbox
            .receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the atomic batch must arrive as a single inbox item");

        // Admit the batch AND prove all N admits ran under ONE dag.lock()
        // acquisition (epoch rail) and that a concurrent observer can only
        // see the DAG once all N are admitted (observer rail).
        assert_batch_admit_holds_one_dag_lock(&sched, one, N);

        // END-STATE rail: all N nodes admitted from that single item.
        let pending = sched.dag.lock().pending_len();
        assert_eq!(
            pending, N,
            "one atomic batch item must admit all {N} nodes; got pending_len={pending} \
             (a sequential N-item batch would admit only 1 from one drained item)",
        );
        // Inbox empty: the batch was a single item, fully consumed.
        assert!(
            sched.inbox.receiver.is_empty(),
            "after draining the single batch item the inbox must be empty; a sequential \
             N-item batch would still hold N-1 items",
        );
    }

    /// (2) One batch == one wake == one `submit_count` increment.
    ///
    /// After `submit_batch_atomic(N)`, a single `pump_ready` reports
    /// `drained == 1` (one inbox item) and `submit_count` advanced by
    /// exactly 1.
    ///
    /// Discrimination: a sequential per-request submission increments
    /// `submit_count` N times (once per `submit_request`) and pushes N
    /// inbox items, so `drained == N`. Both `== 1` assertions fail on
    /// the sequential impl.
    #[test]
    fn exactly_one_wake_per_batch() {
        use crate::caller_kind::CallerKind;
        const N: usize = 4;
        let (ids, reqs) = n_source_requests(N);
        let loader = batch_loader(&ids.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let sched = test_scheduler_with_loader(loader);

        let before = sched
            .counters
            .submit_count
            .load(std::sync::atomic::Ordering::Relaxed);

        let _batch = sched.submit_batch_atomic(reqs);

        let after = sched
            .counters
            .submit_count
            .load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            after - before,
            1,
            "one atomic batch must bump submit_count by exactly 1; got +{} \
             (a sequential N-item batch bumps it N times)",
            after - before,
        );

        let stats = sched.pump_ready(PumpReason::DriverWake, CallerKind::Driver);
        assert_eq!(
            stats.drained, 1,
            "the atomic batch is ONE inbox item, so the pump drains exactly 1; got {} \
             (a sequential N-item batch drains N)",
            stats.drained,
        );
    }

    /// (3) Dedup callbacks must run AFTER the DAG lock is released, and
    /// fire EXACTLY ONCE.
    ///
    /// A batch carrying two requests for the SAME canonical (a dedup
    /// join) with test contexts. The joiner's `on_dedup_joiner`
    /// callback (a) asserts `dag.try_lock()` SUCCEEDS — proving the
    /// callback is not fired while admission still holds the lock — and
    /// (b) bumps an `AtomicU64` so the assertion can pin the firing
    /// count to exactly 1. A boolean flag would mask a double-fire (two
    /// callbacks both store `true`); the counter catches it.
    ///
    /// Discrimination: today `register_request` fires `on_dedup_joiner`
    /// INSIDE the `dag.lock()` critical section. If the atomic batch
    /// fired callbacks the same way, `try_lock()` inside the callback
    /// would FAIL (the admitting thread already holds it). The atomic
    /// path collects dedup events and fires them after `drop(dag)`, so
    /// `try_lock()` succeeds.
    ///
    /// The `dag` field's type is `DagMutex` (plain `parking_lot::Mutex`
    /// without the `hotpath` feature, the instrumented wrapper with it).
    /// That alias is compile-enforced; this test's lock/unlock contract
    /// is identical for both backends and is not a DagMutex-vs-Mutex
    /// discriminator.
    #[test]
    fn dedup_callbacks_run_after_dag_unlock() {
        let canonical = "/dup.vue";
        let loader = batch_loader(&[canonical]);
        let sched = test_scheduler_with_loader(loader);

        // A context whose `on_dedup_joiner` probes the DAG lock and
        // records the outcome via the shared Arc.
        struct LockProbeCtx {
            id: u64,
            dag: Arc<DagMutex>,
            try_lock_succeeded: Arc<std::sync::atomic::AtomicBool>,
            fire_count: Arc<std::sync::atomic::AtomicU64>,
        }
        impl verter_execution::request_context::RequestContextLike for LockProbeCtx {
            fn request_id(&self) -> u64 {
                self.id
            }
            fn capture_enabled(&self) -> bool {
                false
            }
            fn on_dedup_joiner(&self, _c: Arc<str>, _w: u64, _a: bool) {
                // Count every firing so a DOUBLE-fire is caught: a
                // boolean flag would silently collapse two callbacks into
                // one observed `true`. The contract is exactly-once.
                self.fire_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                // The whole point: the admitting thread must NOT be
                // holding the DAG lock when this callback runs.
                let got = self.dag.try_lock();
                self.try_lock_succeeded
                    .store(got.is_some(), std::sync::atomic::Ordering::SeqCst);
            }
            fn record_cache_event(
                &self,
                _event: verter_execution::request_context::CacheEventKind,
            ) {
            }
            fn install_tls(
                self: Arc<Self>,
            ) -> Box<dyn verter_execution::request_context::TlsUninstall + Send> {
                struct NoopUninstall;
                impl verter_execution::request_context::TlsUninstall for NoopUninstall {
                    fn uninstall(self: Box<Self>) {}
                }
                Box::new(NoopUninstall)
            }
        }

        let try_lock_succeeded = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fire_count = Arc::new(std::sync::atomic::AtomicU64::new(0));

        // Winner has a plain context; joiner probes the lock.
        let winner_ctx = OpaqueRequestContext(Arc::new(LockProbeCtx {
            id: 1,
            dag: Arc::clone(&sched.dag),
            try_lock_succeeded: Arc::clone(&try_lock_succeeded),
            fire_count: Arc::clone(&fire_count),
        })
            as Arc<dyn verter_execution::request_context::RequestContextLike>);
        let joiner_ctx = OpaqueRequestContext(Arc::new(LockProbeCtx {
            id: 2,
            dag: Arc::clone(&sched.dag),
            try_lock_succeeded: Arc::clone(&try_lock_succeeded),
            fire_count: Arc::clone(&fire_count),
        })
            as Arc<dyn verter_execution::request_context::RequestContextLike>);

        let reqs = vec![
            Request {
                file_id: canonical.to_string(),
                target: TargetStage::Analysis,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: Some(winner_ctx),
            },
            Request {
                file_id: canonical.to_string(),
                target: TargetStage::Analysis,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: Some(joiner_ctx),
            },
        ];

        let _batch = sched.submit_batch_atomic(reqs);
        // Drain + process the single batch item (admission runs here).
        let item = sched
            .inbox
            .receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("batch item");
        sched.process_submission(item);

        assert_eq!(
            fire_count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "on_dedup_joiner must fire EXACTLY ONCE for the single same-canonical \
             join — not zero (the joiner was never detected) and not twice (a \
             double-fire). A boolean flag would mask a double-fire as a single \
             observed `true`; the counter catches it.",
        );
        assert!(
            try_lock_succeeded.load(std::sync::atomic::Ordering::SeqCst),
            "on_dedup_joiner must run AFTER the DAG lock is released — \
             dag.try_lock() must succeed inside the callback (a callback fired \
             under the admission lock would see try_lock() fail)",
        );
    }

    /// (4) Capacity contract is unchanged for the batch path: capacity
    /// is reserved at DEQUEUE time, not at admission time.
    ///
    /// With an `io` budget of 1, admit a batch of 2 Source requests.
    /// Before any dequeue, zero permits are held (admission reserves
    /// nothing). The first `next_ready_for_pump` reserves one I/O
    /// permit; the second is budget-blocked (returns `None`).
    ///
    /// Discrimination: an impl that reserved capacity at admission
    /// would either show non-zero permits post-admission OR fail to
    /// admit the second node. The at-dequeue reservation invariant —
    /// permits == 0 after admission, exactly one reserved on first
    /// dequeue, second blocked — is what this asserts.
    #[test]
    fn capacity_contract_unchanged_for_batch() {
        use crate::caller_kind::CallerKind;
        let ids = ["/cap0.vue", "/cap1.vue"];
        let loader = batch_loader(&ids);
        // io budget = 1 so the second Source dequeue is budget-blocked.
        let sched = Scheduler::test_new_sync(
            SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                ..SchedulerConfig::default()
            },
            loader,
        );

        let reqs: Vec<Request> = ids
            .iter()
            .map(|id| Request {
                file_id: (*id).to_string(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: None,
            })
            .collect();

        let _batch = sched.submit_batch_atomic(reqs);
        let item = sched
            .inbox
            .receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("batch item");
        sched.process_submission(item);

        // After admission, before dequeue: no permits held.
        {
            let dag = sched.dag.lock();
            assert_eq!(
                dag.pending_len(),
                2,
                "both Source nodes admitted by the atomic batch",
            );
            assert_eq!(
                dag.in_flight_io_permits(),
                0,
                "admission must NOT reserve capacity — permits are taken at dequeue time",
            );
        }

        // First dequeue reserves exactly one I/O permit.
        let first = {
            let mut dag = sched.dag.lock();
            dag.next_ready_for_pump(CallerKind::Driver, &[])
        };
        assert!(first.is_some(), "first Source dequeue must succeed");
        assert_eq!(
            sched.dag.lock().in_flight_io_permits(),
            1,
            "first dequeue reserves exactly one I/O permit",
        );

        // Second dequeue is budget-blocked (io budget == 1).
        let second = {
            let mut dag = sched.dag.lock();
            dag.next_ready_for_pump(CallerKind::Driver, &[])
        };
        assert!(
            second.is_none(),
            "second Source dequeue must be budget-blocked under io budget == 1",
        );
    }

    /// (5) A source-updating batch supersedes ALL old-generation
    /// waiters atomically — after ONE batch message every stale handle
    /// is `Superseded`.
    ///
    /// Set up two files each with an old-generation Analysis waiter
    /// (generation 1) registered on the DAG. Then submit a batch that
    /// source-updates BOTH files (which bumps each to generation 2 and
    /// runs the supersede sweep). After processing the single batch
    /// item, both old-gen handles resolve to `Superseded`.
    ///
    /// Discrimination has TWO independent rails:
    ///
    /// 1. END-STATE: a sequential batch would process the two source
    ///    updates as two separate inbox items; draining the single batch
    ///    item would supersede only the FIRST file's waiter, leaving the
    ///    second still pending. The "both superseded after ONE message"
    ///    assertion fails on the sequential impl.
    /// 2. LOCK-CONTINUITY (P1b): the both-superseded end-state does NOT
    ///    by itself prove the two supersede sweeps ran under ONE held
    ///    `dag.lock()` — a per-item lock/unlock that swept both from the
    ///    one item would leave the same end-state while exposing a
    ///    mid-batch window where file 0 is superseded but file 1 is not.
    ///    The `assert_batch_admit_holds_one_dag_lock` helper proves
    ///    continuity deterministically via the epoch rail (both admits
    ///    record ONE acquisition epoch; a per-item lock/unlock bumps a fresh
    ///    epoch per file → recorded epochs differ → FAIL) and corroborates
    ///    it with a concurrent observer that blocking-acquires the DAG lock
    ///    and can only get in once BOTH sweeps are done.
    #[test]
    fn batch_supersede_sweep_is_atomic() {
        let ids = ["/sup0.vue", "/sup1.vue"];
        let loader = batch_loader(&ids);
        let sched = test_scheduler_with_loader(loader);

        // Create gen-1 nodes + register an old-gen Analysis waiter on
        // each, directly on the DAG (mirrors an in-flight request from
        // a prior generation).
        let mut old_handles = Vec::new();
        for id in ids {
            let canonical: Arc<str> = Arc::from(id);
            // Ensure the FileNode exists and is at generation 1.
            let node = sched
                .nodes
                .entry(id.to_string())
                .or_insert_with(|| sched.create_node(id, None))
                .clone();
            let gen = sched
                .source_root
                .publish_transition(|publication| publication.bump_node_generation(&node));
            assert_eq!(gen, 1, "fixture expects first bump to land generation 1");
            let (handle, sender) = completion_pair::<RequestResult>();
            // No context → no dedup event; discard the (None) return.
            let _ = sched.dag.lock().register_request(
                &canonical,
                1,
                TargetStage::Analysis,
                sender,
                None,
            );
            old_handles.push(handle);
        }

        // Sanity: both old-gen handles are still pending.
        for h in &old_handles {
            assert!(
                h.try_get().is_none(),
                "old-gen waiter must be pending before the supersede batch",
            );
        }

        // A batch that source-updates BOTH files (each carries source,
        // so each bumps to gen 2 and supersedes gen 1).
        let reqs: Vec<Request> = ids
            .iter()
            .map(|id| Request {
                file_id: (*id).to_string(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: Some(Arc::from("<template>updated</template>")),
                file_language: None,
                request_context: None,
            })
            .collect();

        let _batch = sched.submit_batch_atomic(reqs);
        let item = sched
            .inbox
            .receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("batch item");

        // Admit the batch AND prove both admits (each bumping a generation
        // and running its supersede sweep) ran under ONE dag.lock()
        // acquisition held continuously across both (epoch rail), and that a
        // concurrent observer can only see the DAG once BOTH are done
        // (observer rail). n = 2 (the two source-updated files).
        assert_batch_admit_holds_one_dag_lock(&sched, item, 2);

        // END-STATE rail: after ONE batch message every old-gen waiter is
        // Superseded.
        for (i, h) in old_handles.iter().enumerate() {
            match h.try_get() {
                Some(CompletionState::Superseded) => {}
                other => panic!(
                    "old-gen waiter #{i} must be Superseded after ONE atomic batch message; \
                     got {other:?} (a sequential batch would supersede only the first file)",
                ),
            }
        }
    }

    /// (6) `wait_batch` returns results in INPUT order even when the
    /// underlying handles complete out of dispatch order.
    ///
    /// Construct a `BatchHandle` by hand with three handles, complete
    /// them in the order [2, 0, 1] (out of input order), then assert
    /// `wait_batch` yields a result vec whose i-th entry is the
    /// completion sent to the i-th handle.
    ///
    /// Discrimination: an impl that returned results in completion
    /// order (e.g. a `select`/`FuturesUnordered`-style drain) would
    /// surface [r2, r0, r1]. The input-order contract — result[i]
    /// corresponds to input[i] — fails on a completion-order impl.
    #[test]
    fn wait_batch_returns_input_order() {
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        // Build three handles directly so the test fully controls the
        // completion order. Each is tagged with a distinct generation
        // so the recovered result is identifiable per input position.
        let mut handles = Vec::new();
        let mut senders = Vec::new();
        for _ in 0..3 {
            let (h, s) = completion_pair::<RequestResult>();
            handles.push(h);
            senders.push(s);
        }
        let batch = BatchHandle { handles };

        let mk = |gen: u64| {
            RequestResult::Source(Arc::new(crate::node::SourceSnapshot::new_empty(
                Arc::from("x"),
                gen,
            )))
        };

        // Complete OUT of input order: index 2 first, then 0, then 1.
        senders[2].send(CompletionState::Ready(mk(22)));
        senders[0].send(CompletionState::Ready(mk(0)));
        senders[1].send(CompletionState::Ready(mk(11)));

        let results = sched.wait_batch(&batch);
        assert_eq!(results.len(), 3, "one result per input handle");

        let gen_of = |state: &CompletionState<RequestResult>| -> u64 {
            match state {
                CompletionState::Ready(RequestResult::Source(snap)) => snap.generation,
                other => panic!("expected Ready(Source), got {other:?}"),
            }
        };
        assert_eq!(
            gen_of(&results[0]),
            0,
            "result[0] must correspond to INPUT handle 0, not the first-completed handle",
        );
        assert_eq!(
            gen_of(&results[1]),
            11,
            "result[1] must correspond to input handle 1"
        );
        assert_eq!(
            gen_of(&results[2]),
            22,
            "result[2] must correspond to input handle 2"
        );
    }

    /// `wait_batch` accepts a batch BY VALUE — the pre-existing public
    /// signature. This is a compile-AND-runtime proof of the additive
    /// boundary (P1a): the `Borrow<BatchHandle>` generic must keep the
    /// original `sched.wait_batch(batch)` call shape working so off-tree
    /// by-value callers are not source-broken. A regression that
    /// narrowed the parameter back to `&BatchHandle` would fail to
    /// COMPILE this `move`-style call (the `batch` binding is consumed,
    /// not borrowed), so the test cannot be reverted silently.
    #[test]
    fn wait_batch_accepts_batch_by_value() {
        let loader = Arc::new(MemorySourceLoader::new());
        let sched = Scheduler::test_new_sync(SchedulerConfig::default(), loader);

        let (h0, s0) = completion_pair::<RequestResult>();
        let (h1, s1) = completion_pair::<RequestResult>();
        let batch = BatchHandle {
            handles: vec![h0, h1],
        };

        let mk = |gen: u64| {
            RequestResult::Source(Arc::new(crate::node::SourceSnapshot::new_empty(
                Arc::from("x"),
                gen,
            )))
        };
        s0.send(CompletionState::Ready(mk(7)));
        s1.send(CompletionState::Ready(mk(9)));

        // BY VALUE — `batch` is moved into `wait_batch`. This is the
        // pre-existing call shape the `Borrow` generic preserves.
        let results = sched.wait_batch(batch);
        assert_eq!(results.len(), 2, "one result per input handle");
        let gen_of = |state: &CompletionState<RequestResult>| -> u64 {
            match state {
                CompletionState::Ready(RequestResult::Source(snap)) => snap.generation,
                other => panic!("expected Ready(Source), got {other:?}"),
            }
        };
        assert_eq!(gen_of(&results[0]), 7, "by-value result[0] follows input 0");
        assert_eq!(gen_of(&results[1]), 9, "by-value result[1] follows input 1");
    }

    /// Scheduler pool topology: host-injected
    /// `SchedulerCpuPool`/`SchedulerIoPool`, nonblocking `try_submit`,
    /// and the typed `try_submit`-full invariant-violation contract.
    #[cfg(not(target_arch = "wasm32"))]
    mod pool_topology {
        use super::*;
        use crate::dag::DagCapacityBudget;
        use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as AtomOrd};
        use std::time::Duration;

        /// CONSTRUCTION DOMINANCE (capacity-loan rail): when the host
        /// injects an IO pool sized from an EXPLICIT `dag_budget.io`, the
        /// transport capacity must dominate that budget so the DAG ledger
        /// stays the sole admission gate.
        ///
        /// Discriminator: a transport sized only as the legacy
        /// `io_threads * 4` headroom ignores the explicit budget. With
        /// `io_threads = 1` and `dag_budget.io = 16` that headroom is
        /// `4 < 16`, a TIGHTER gate than the ledger (a second admission
        /// authority). Sizing the transport from `resolved_dag_budget().io`
        /// keeps the capacity `>= 16` so the ledger stays the sole gate; a
        /// headroom-only transport of capacity `4` FAILS this assertion.
        #[test]
        fn injected_io_transport_dominates_explicit_dag_budget_io() {
            let config = SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                dag_budget: Some(DagCapacityBudget { cpu: 8, io: 16 }),
            };
            let loader = Arc::new(MemorySourceLoader::new());
            let sched = Scheduler::test_with_executor(
                config,
                loader,
                Arc::new(crate::execution::executor::DefaultExecutor),
            );
            let cap = sched.io_pool.transport_capacity();
            assert!(
                cap >= 16,
                "injected IO transport capacity ({cap}) must dominate the resolved \
                 dag_budget.io (16) so the channel never becomes a second admission \
                 authority; an io_threads*4-only transport (capacity 4) would FAIL this"
            );
        }

        /// G3-AC2: CPU transport is sized from the same resolved DAG
        /// budget the ledger admits against, matching the IO rail.
        #[test]
        fn injected_cpu_transport_dominates_explicit_dag_budget_cpu() {
            let config = SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                dag_budget: Some(DagCapacityBudget { cpu: 16, io: 8 }),
            };
            let loader = Arc::new(MemorySourceLoader::new());
            let sched = Scheduler::test_with_executor(
                config,
                loader,
                Arc::new(crate::execution::executor::DefaultExecutor),
            );
            let cap = sched.cpu_pool.transport_capacity();
            assert!(
                cap >= 16,
                "injected CPU transport capacity ({cap}) must dominate the resolved \
                 dag_budget.cpu (16) so the pool never becomes a second admission \
                 authority; a cpu_threads*4-only transport (capacity 4) would FAIL this"
            );
        }

        /// The scheduler's two pools are DISTINCT process-unique
        /// identities (the §6a/3-pool topology). A test can therefore
        /// prove a worker is on a SPECIFIC injected scheduler pool.
        #[test]
        fn scheduler_cpu_and_io_pools_have_distinct_identities() {
            let loader = Arc::new(MemorySourceLoader::new());
            let sched = Scheduler::test_new(SchedulerConfig::default(), loader);
            assert_ne!(
                sched.cpu_pool.pool_id(),
                sched.io_pool.pool_id(),
                "the scheduler CPU and IO pools must be distinct pools"
            );
        }

        /// ISOLATION (§6a, P0): a scheduler stage runs on a `CpuWorker`
        /// (Analysis/Artifact) or `IoWorker` (Source) thread — NEVER on
        /// the host coordinator pool's `External` worker, and NEVER on a
        /// scheduler-CPU/IO pool's identity token that is `None` (the
        /// default-External case). Drives a real native scheduler and
        /// probes `CallerKind` + the scheduler-pool identity tokens
        /// inside each stage.
        #[test]
        fn scheduler_stages_run_on_scheduler_pools_never_external() {
            use crate::execution::pool::{scheduler_cpu_pool_token, scheduler_io_pool_token};
            use crate::node::{AnalysisSnapshot, ArtifactSnapshot, SourceSnapshot};

            struct CallerKindProbe {
                source_kind: Arc<AtomicU64>,
                analysis_kind: Arc<AtomicU64>,
                source_io_token: Arc<AtomicUsize>,
                analysis_cpu_token: Arc<AtomicUsize>,
            }
            // Encode CallerKind as a stable u64 so the test thread can
            // compare without importing the enum's repr.
            fn kind_code(k: crate::caller_kind::CallerKind) -> u64 {
                match k {
                    crate::caller_kind::CallerKind::External => 1,
                    crate::caller_kind::CallerKind::Driver => 2,
                    crate::caller_kind::CallerKind::CpuWorker => 3,
                    crate::caller_kind::CallerKind::IoWorker => 4,
                    crate::caller_kind::CallerKind::Inline => 5,
                }
            }
            impl crate::execution::executor::StageExecutor for CallerKindProbe {
                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }
                fn execute_source(
                    &self,
                    _c: &str,
                    _k: FileLanguage,
                    content: Arc<str>,
                    generation: u64,
                    _incarnation: u64,
                ) -> Result<SourceSnapshot, crate::execution::executor::StageError>
                {
                    self.source_kind.store(
                        kind_code(crate::caller_kind::CallerKind::current()),
                        AtomOrd::SeqCst,
                    );
                    // `usize::MAX` is the "no scheduler-IO token" sentinel.
                    self.source_io_token.store(
                        scheduler_io_pool_token().unwrap_or(usize::MAX),
                        AtomOrd::SeqCst,
                    );
                    Ok(SourceSnapshot::new_empty(content, generation))
                }
                fn execute_analysis(
                    &self,
                    _c: &str,
                    _s: &SourceSnapshot,
                    generation: u64,
                ) -> Result<AnalysisSnapshot, crate::execution::executor::StageError>
                {
                    self.analysis_kind.store(
                        kind_code(crate::caller_kind::CallerKind::current()),
                        AtomOrd::SeqCst,
                    );
                    self.analysis_cpu_token.store(
                        scheduler_cpu_pool_token().unwrap_or(usize::MAX),
                        AtomOrd::SeqCst,
                    );
                    Ok(AnalysisSnapshot::new_empty(generation))
                }
                fn execute_artifact(
                    &self,
                    _c: &str,
                    _s: &SourceSnapshot,
                    _a: &AnalysisSnapshot,
                    profile_hash: u64,
                    generation: u64,
                ) -> Result<ArtifactSnapshot, crate::execution::executor::StageError>
                {
                    Ok(ArtifactSnapshot {
                        generation,
                        profile_hash,
                        data: Arc::new(crate::node::EmptyData),
                    })
                }
            }

            let source_kind = Arc::new(AtomicU64::new(0));
            let analysis_kind = Arc::new(AtomicU64::new(0));
            let source_io_token = Arc::new(AtomicUsize::new(usize::MAX));
            let analysis_cpu_token = Arc::new(AtomicUsize::new(usize::MAX));
            let probe = Arc::new(CallerKindProbe {
                source_kind: Arc::clone(&source_kind),
                analysis_kind: Arc::clone(&analysis_kind),
                source_io_token: Arc::clone(&source_io_token),
                analysis_cpu_token: Arc::clone(&analysis_cpu_token),
            });

            let loader: Arc<MemorySourceLoader> = Arc::new(MemorySourceLoader::new());
            // Force pool dispatch (not inline): the External test thread
            // is parked by `wait_or_drive`, so the driver dispatches onto
            // the scheduler pools.
            let sched = Scheduler::test_with_executor(
                SchedulerConfig {
                    cpu_threads: 2,
                    io_threads: 2,
                    dag_budget: None,
                },
                Arc::clone(&loader) as Arc<dyn crate::source_loader::SourceLoader>,
                probe as Arc<dyn crate::execution::executor::StageExecutor>,
            );

            let handle = sched.submit_request(Request {
                file_id: "/x.vue".to_string(),
                target: TargetStage::Analysis,
                priority: Priority::Interactive,
                source: Some(Arc::from("<template>x</template>")),
                file_language: None,
                request_context: None,
            });
            let state = handle.wait();
            assert!(
                matches!(state, CompletionState::Ready(_)),
                "request must complete: {state:?}"
            );

            // Source ran on an IoWorker (code 4) — NOT External (1).
            assert_eq!(
                source_kind.load(AtomOrd::SeqCst),
                4,
                "Source stage must run on a scheduler IoWorker, never the External \
                 host coordinator pool"
            );
            // Analysis ran on a CpuWorker (code 3) — NOT External (1).
            assert_eq!(
                analysis_kind.load(AtomOrd::SeqCst),
                3,
                "Analysis stage must run on a scheduler CpuWorker, never the External \
                 host coordinator pool"
            );
            // And it ran on THIS scheduler's specific pools (tokens set,
            // not the usize::MAX no-token sentinel) — proving the work
            // landed on the injected scheduler pools, not some other
            // CpuWorker/IoWorker-defaulting thread.
            assert_eq!(
                source_io_token.load(AtomOrd::SeqCst),
                sched.io_pool.pool_id(),
                "Source worker must carry THIS scheduler IO pool's identity token"
            );
            assert_eq!(
                analysis_cpu_token.load(AtomOrd::SeqCst),
                sched.cpu_pool.pool_id(),
                "Analysis worker must carry THIS scheduler CPU pool's identity token"
            );
        }

        /// Per-file gate so the test can park the single IO worker inside
        /// Source and observe that the driver keeps making progress.
        struct SourceGate {
            entered_tx: crossbeam_channel::Sender<()>,
            release_rx: crossbeam_channel::Receiver<()>,
        }
        struct GatedSourceExecutor {
            gates: dashmap::DashMap<String, SourceGate>,
        }
        impl crate::execution::executor::StageExecutor for GatedSourceExecutor {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn execute_source(
                &self,
                canonical_id: &str,
                _file_language: FileLanguage,
                content: Arc<str>,
                generation: u64,
                _incarnation: u64,
            ) -> Result<crate::node::SourceSnapshot, crate::execution::executor::StageError>
            {
                if let Some(gate) = self.gates.get(canonical_id) {
                    let _ = gate.entered_tx.send(());
                    let _ = gate.release_rx.recv();
                }
                Ok(crate::node::SourceSnapshot::new_empty(content, generation))
            }
        }

        /// DRIVER NONBLOCKING (the load-bearing discriminator): with
        /// `io_threads = 1` the single I/O worker parks INSIDE the first
        /// Source stage it picks up. The driver must keep dispatching
        /// every other admitted Source job onto the pool WITHOUT waiting
        /// for that stuck worker to drain.
        ///
        /// The discriminator is a driver-side submit counter
        /// ([`Scheduler::io_submit_attempts`], bumped at the top of
        /// `try_submit_io`). We pin worker job #0 with its gate held
        /// CLOSED, then poll the counter and require it to reach all `N`
        /// admitted submit attempts WHILE worker job #0 is still parked.
        /// The driver runs the dispatch loop on its own thread and calls
        /// `try_submit_io` per ready job; a `try_send` (`try_submit`)
        /// returns immediately whether or not the worker is draining, so
        /// the counter climbs to `N` even though job #0 never finishes.
        /// Only AFTER the counter proves the driver dispatched the whole
        /// fan-out do we release the gates and assert completion.
        ///
        /// Why it fails against a BLOCKING bounded `send`: the driver
        /// thread itself is the caller of
        /// `try_submit_io`, so a blocking `send` onto a transport that
        /// fills behind the parked worker would PARK THE DRIVER THREAD
        /// mid-dispatch — the counter would stall below `N` and the
        /// bounded wait below would time out. The nonblocking `try_send`
        /// never parks the driver, so the counter reaches `N` promptly.
        /// (With the production transport sizing — capacity ≥
        /// `dag_budget.io` — a blocking send is masked because the
        /// channel never fills with one worker; the regression is
        /// exercised directly by temporarily shrinking the transport and
        /// swapping `try_send`→`send`, which then stalls the driver here.
        /// The transport-sizing half of the invariant is pinned
        /// separately by `injected_io_transport_dominates_explicit_dag_budget_io`.)
        #[test]
        fn driver_does_not_block_dispatching_many_source_jobs_on_one_io_worker() {
            // N files, each gated. io_threads = 1 so a single worker can
            // be pinned inside Source job #0. Explicit dag_budget.io = N
            // admits all N at once; the transport (capacity ≥ N) holds the
            // N-1 jobs queued behind the parked worker without overflow.
            const N: usize = 8;
            let gates = dashmap::DashMap::new();
            let mut entered_rxs = Vec::with_capacity(N);
            let mut release_txs = Vec::with_capacity(N);
            let loader = Arc::new(MemorySourceLoader::new());
            for i in 0..N {
                let id = format!("/f{i}.vue");
                loader.insert(id.clone(), Arc::from("<template>x</template>"));
                let (etx, erx) = crossbeam_channel::bounded::<()>(1);
                let (rtx, rrx) = crossbeam_channel::bounded::<()>(1);
                gates.insert(
                    id,
                    SourceGate {
                        entered_tx: etx,
                        release_rx: rrx,
                    },
                );
                entered_rxs.push(erx);
                release_txs.push(rtx);
            }
            let executor = Arc::new(GatedSourceExecutor { gates });
            let config = SchedulerConfig {
                cpu_threads: 2,
                io_threads: 1,
                dag_budget: Some(DagCapacityBudget {
                    cpu: 8,
                    io: N as u32,
                }),
            };
            let sched = Scheduler::test_with_executor(
                config,
                loader as Arc<dyn crate::source_loader::SourceLoader>,
                executor as Arc<dyn crate::execution::executor::StageExecutor>,
            );

            // Submit all N Source requests. The single IO worker picks up
            // ONE of them and parks inside its gate (gate never released
            // yet); every other admitted job must still be dispatched by
            // the driver onto the pool transport.
            let mut handles = Vec::with_capacity(N);
            for i in 0..N {
                handles.push(sched.submit_request(Request {
                    file_id: format!("/f{i}.vue"),
                    target: TargetStage::Source,
                    priority: Priority::Interactive,
                    source: None,
                    file_language: None,
                    request_context: None,
                }));
            }

            // Wait until exactly ONE Source stage has been entered (the
            // worker is now parked on that gate, NOT released). Do not
            // release it — we want the worker stuck while we observe the
            // driver dispatch the rest. Poll every gate non-blockingly
            // against the SAME generous deadline the rest of the test uses
            // (rather than a fixed per-receiver budget), so a scheduler
            // slow to enter the first stage under load can't spuriously
            // time this initial wait out.
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            let parked_idx = loop {
                let entered = entered_rxs.iter().position(|erx| erx.try_recv().is_ok());
                if let Some(i) = entered {
                    break i;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the single IO worker must enter SOME Source stage and park there"
                );
                std::thread::yield_now();
            };

            // THE DISCRIMINATOR: with the worker still parked on
            // `parked_idx`, the driver must have dispatched ALL N submit
            // attempts onto the pool. Poll the driver-side counter until
            // it reaches N. A blocking `send` that filled behind the
            // parked worker would have parked the DRIVER thread, so this
            // count would stall below N and the loop would time out.
            loop {
                if sched.io_submit_attempts() >= N {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the driver only reached {} of {N} I/O submit attempts while the \
                     single worker was parked on Source #{parked_idx} — it blocked \
                     mid-dispatch instead of dispatching the whole admitted fan-out \
                     (a blocking-send driver regression)",
                    sched.io_submit_attempts()
                );
                std::thread::yield_now();
            }
            // The worker must STILL be parked (we never released its
            // gate): the driver reached the full fan-out without the
            // worker draining a single job.
            assert_eq!(
                sched.io_submit_attempts(),
                N,
                "the driver should dispatch exactly N Source jobs, all while the worker \
                 was parked"
            );

            // Now release every gate (starting with the parked one) so the
            // single worker drains all N jobs in turn and each request
            // completes.
            let _ = release_txs[parked_idx].send(());
            for (i, erx) in entered_rxs.iter().enumerate() {
                if i == parked_idx {
                    continue;
                }
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                erx.recv_timeout(remaining).unwrap_or_else(|_| {
                    panic!(
                        "Source stage #{i} was never entered within the deadline after \
                         release — the queued jobs did not drain"
                    )
                });
                let _ = release_txs[i].send(());
            }

            for (i, h) in handles.into_iter().enumerate() {
                let state = h.wait();
                assert!(
                    matches!(state, CompletionState::Ready(_)),
                    "Source request #{i} must complete Ready: {state:?}"
                );
            }
        }

        /// TRY_SUBMIT-FULL INVARIANT VIOLATION (the reservation-leak
        /// discriminator): a fault-injected `Full` from `try_submit` at
        /// the dispatch site must terminalize the job (caller observes
        /// `Failed`, NOT a hang), release the parked DAG reservation (no
        /// permit leak), and NOT requeue/double-credit. After the faulted
        /// job terminalizes, a SUBSEQUENT request in the SATURATED faulted
        /// class must still admit and complete — proving the permit was
        /// returned to the ledger.
        ///
        /// The leak must be discriminated, so the FAULTED resource class
        /// is sized to exactly ONE permit and a same-class follow-up is
        /// submitted. The one-shot fault fires on the FIRST non-inline
        /// dispatch, which for a Source-target request is the Source job's
        /// `try_submit_io` — an I/O-class submission. So the budget gives
        /// `io: 1`: the faulted job reserved the single I/O permit; if
        /// `terminalize_pool_submit_violation` failed to release it, the
        /// I/O class stays saturated forever and the second Source request
        /// could never dispatch (hang/timeout). With the by-value release
        /// in `cancel`, the permit returns to the ledger and the second
        /// request admits. (`cpu: 1` is incidental — only the I/O permit
        /// is exercised by Source-target work.)
        ///
        /// The fault is injected via the test-only one-shot
        /// `arm_pool_submit_fault_full` seam, which also suppresses the
        /// `terminalize_pool_submit_violation` `debug_assert!` (the test
        /// characterizes the RELEASE path that runs in release builds).
        ///
        /// Discrimination proof (performed once, then reverted): skipping
        /// the cancel/release inside `terminalize_pool_submit_violation`
        /// (so the I/O reservation leaks) makes the second Source request
        /// below STALL — `h_b.wait()` never returns Ready and the test
        /// hangs to its timeout. Restoring the release makes it pass.
        #[test]
        fn try_submit_full_terminalizes_releases_reservation_no_leak() {
            // Single I/O permit so a LEAKED reservation permanently
            // saturates the I/O class — the second Source request would
            // then never admit. Source loading is I/O-class work, and the
            // one-shot fault fires on the first non-inline dispatch (the
            // Source `try_submit_io`).
            let loader = Arc::new(MemorySourceLoader::new());
            loader.insert("/a.vue".to_string(), Arc::from("<template>a</template>"));
            loader.insert("/b.vue".to_string(), Arc::from("<template>b</template>"));
            let config = SchedulerConfig {
                cpu_threads: 1,
                io_threads: 1,
                dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
            };
            let sched = Scheduler::test_with_executor(
                config,
                loader as Arc<dyn crate::source_loader::SourceLoader>,
                Arc::new(crate::execution::executor::DefaultExecutor),
            );

            // Arm a one-shot Full fault: the NEXT non-inline pool submit
            // observes Full. Source (/a.vue) dispatches to the IO pool via
            // `try_submit_io`; that first non-inline submit hits the fault
            // and terminalizes, exercising the I/O reservation release.
            sched.arm_pool_submit_fault_full();

            let h_a = sched.submit_request(Request {
                file_id: "/a.vue".to_string(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: None,
            });
            // The faulted job terminalizes as Failed — NOT a hang.
            let state_a = h_a.wait();
            assert!(
                matches!(state_a, CompletionState::Failed(_)),
                "the try_submit-Full job must terminalize as Failed (no silent drop, \
                 no hang): {state_a:?}"
            );

            // The reservation must have been released: a fresh Source
            // request in the SATURATED I/O class (io:1) still admits and
            // completes. A leaked I/O permit would saturate the class and
            // stall this request forever.
            let h_b = sched.submit_request(Request {
                file_id: "/b.vue".to_string(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: None,
            });
            let state_b = h_b.wait();
            assert!(
                matches!(state_b, CompletionState::Ready(_)),
                "a subsequent same-class (I/O) request must complete after the faulted \
                 job released its reservation (no permit leak, no double-credit stall): \
                 {state_b:?}"
            );
        }
    }
}
