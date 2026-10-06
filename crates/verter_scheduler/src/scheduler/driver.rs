//! The driver — dispatch, pool submission and stage execution.
//!
//! Part of the `scheduler` module. The root re-exports this module, so
//! every name used here is reached through the root rather than through a
//! sibling module.

use super::*;

/// THE single routing point from a dequeued [`ReadyJob`] to the
/// [`StageExecutor`]. Both the native pump path
/// ([`Scheduler::dispatch_ready_job`]) and the inline path
/// ([`Scheduler::execute_stage_inline`]) call this — there is no second
/// adapter, and the `ReadyJob`'s own `(kind, identity)` is matched ONCE here.
///
/// Routing:
///
/// - [`WorkNodeIdentity::CacheNode`] → the cache-materialisation hook
///   [`StageExecutor::execute_cache_node`], handed the full cache identity
///   (`cache_id` + `key_hash` + `view_epoch` + `snapshot_pin_id`) directly from
///   the identity plus a cancellation token. The dispatched node's parked
///   capacity reservation is then released by marking the identity complete —
///   cache-node identities are never observed as [`DepKey`] prerequisites by
///   file-stage or artifact nodes, so completing here cannot strand a waiter.
/// - file-stage / artifact work → the owned [`TaskKind`] execution descriptor
///   is constructed inline from the same match and the work runs through the
///   file-stage executor chokepoint [`Scheduler::execute_stage_on_worker`].
///
/// File-stage work returns its terminal `StageComplete` (cache-node work
/// returns `None`); the caller owns delivery so a sole inbox consumer can
/// drain rather than park on its own full inbox.
///
/// `file_node` is the resolved [`FileNode`] for file-stage work (the caller
/// looks it up and applies the removed-node / generation-mismatch guards
/// first); it is `None` for cache-node work, which has no file node.
#[allow(clippy::too_many_arguments)]
#[must_use = "the terminal StageComplete must be delivered to the inbox"]
pub(super) fn dispatch_ready_job_to_executor(
    job: &ReadyJob,
    file_node: Option<&FileNode>,
    generation: u64,
    failed_blocker_deps: std::collections::BTreeMap<DepKey, crate::dag::FailedDepRecord>,
    executor: &dyn StageExecutor,
    source_loader: &dyn SourceLoader,
    inbox_sender: &crossbeam_channel::Sender<Submission>,
    dag: Arc<DagMutex>,
    source_root: Arc<crate::source_root::SchedulerSourceDirectory>,
    cancellation: &CancellationToken,
) -> Option<Submission> {
    let _cancellation_guard =
        verter_execution::cancellation::JobCancellationGuard::install(cancellation.clone());
    match (&job.kind, &job.identity) {
        // Cache-node work routes straight to the cache-materialisation hook,
        // taking the full identity directly: `cache_id` + `key_hash` ride the
        // owned `TaskKind::CacheNode` descriptor, while `view_epoch` /
        // `snapshot_pin_id` ride the identity and are handed to the executor
        // separately (mirroring how `profile_hash` is passed alongside an
        // `Artifact` task).
        (
            WorkKind::CacheNode,
            WorkNodeIdentity::CacheNode {
                cache_id,
                key_hash,
                view_epoch,
                snapshot_pin_id,
            },
        ) => {
            // Run the cache node. The executor's default returns a typed
            // "unsupported" error (a host that has not opted in fails loudly);
            // the host override performs the real materialisation. The call is
            // panic-guarded so the reservation release below always runs — the
            // capacity class never leaks the permit parked at dispatch, even if
            // a host override panics.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                executor.execute_cache_node(
                    *cache_id,
                    *key_hash,
                    *view_epoch,
                    *snapshot_pin_id,
                    cancellation,
                )
            }));
            // The outcome decides the terminal. ONLY a clean `Ok(Ok(()))`
            // completes the node as SUCCESS; a typed `Ok(Err(..))` (the
            // not-overridden default and any real materialisation failure) and
            // a panic (`Err(..)`) route through the failure path so the work is
            // never recorded as if it succeeded. Both terminals release the
            // parked capacity reservation exactly once (the by-value
            // `release(self)` consume in `complete`/`cancel`), so the capacity
            // class never leaks the permit on any arm. Cache-node identities
            // are never observed as `DepKey` prerequisites, so neither terminal
            // can strand a file-stage/artifact waiter — `complete` returns no
            // newly-ready tokens and `cancel` returns no stranded tokens.
            match outcome {
                Ok(Ok(())) => {
                    let newly_ready = dag.lock().complete(&job.identity);
                    verter_debug_assert!(
                        newly_ready.is_empty(),
                        "CacheNode completion must not strand DAG waiters: \
                         CacheNode identities are not used as DepKey prerequisites",
                    );
                }
                Ok(Err(_stage_error)) => {
                    // Typed cache-node failure. Cancel the identity to release
                    // the parked reservation and terminalize the node as
                    // FAILED — never complete-as-success. This is the same
                    // mechanism the cache-node submit-failure path uses.
                    let stranded = dag.lock().cancel(&job.identity);
                    verter_debug_assert!(
                        stranded.is_empty(),
                        "CacheNode failure-cancel must not strand DAG waiters: \
                         CacheNode identities are not used as DepKey prerequisites",
                    );
                }
                Err(_panic) => {
                    // The host override panicked. Cancel the identity to
                    // release the parked reservation and terminalize the node
                    // as FAILED — the panic must not be silently completed as
                    // success. The unwind payload is dropped (the panic was
                    // already reported by the default hook); the scheduler does
                    // not re-raise on its worker thread.
                    let stranded = dag.lock().cancel(&job.identity);
                    verter_debug_assert!(
                        stranded.is_empty(),
                        "CacheNode panic-cancel must not strand DAG waiters: \
                         CacheNode identities are not used as DepKey prerequisites",
                    );
                }
            }
            None
        }
        // File-stage / artifact work. The owned execution descriptor is built
        // from the same `(kind, identity)` match and handed to the file-stage
        // executor chokepoint.
        (WorkKind::Load, WorkNodeIdentity::FileStage { .. }) => run_file_stage(
            TaskKind::Load,
            job,
            file_node,
            generation,
            failed_blocker_deps,
            executor,
            source_loader,
            inbox_sender,
            dag,
            source_root,
        ),
        // `Parse` is NEVER admitted as a runnable DAG node: it is a label for
        // the future CPU split, not a request-target file stage, so `admit_work`
        // (the file-stage admission path) `unreachable!()`s on `TaskKind::Parse`
        // and there is no other site that produces a `(WorkKind::Parse,
        // FileStage)` ready job. Routing it into `run_file_stage` would only
        // hand it to `execute_stage_on_worker`, whose `TaskKind::Parse` arm
        // panics anyway — so the invariant is single-sourced here at the router
        // (no live Parse pipeline exists), and the executor's own
        // `TaskKind::Parse => unreachable!()` stays as defense-in-depth.
        (WorkKind::Parse, WorkNodeIdentity::FileStage { .. }) => {
            unreachable!(
                "Parse is never admitted as a runnable DAG node — it is a label \
                 for the future CPU split, not a request-target file stage. \
                 `admit_work` rejects `TaskKind::Parse` (unreachable!) and no site \
                 produces a `(WorkKind::Parse, FileStage)` ready job, so the router \
                 can never observe one.",
            )
        }
        (WorkKind::Analysis, WorkNodeIdentity::FileStage { .. }) => run_file_stage(
            TaskKind::Analysis,
            job,
            file_node,
            generation,
            failed_blocker_deps,
            executor,
            source_loader,
            inbox_sender,
            dag,
            source_root,
        ),
        (WorkKind::Artifact, WorkNodeIdentity::Artifact { profile_hash, .. }) => run_file_stage(
            TaskKind::Artifact {
                profile_hash: profile_hash_from_bytes(*profile_hash),
            },
            job,
            file_node,
            generation,
            failed_blocker_deps,
            executor,
            source_loader,
            inbox_sender,
            dag,
            source_root,
        ),
        // The DAG's admission paths only produce the `(kind, identity)`
        // pairings handled above; any other combination is a corrupt ready
        // job (e.g. a `Load` kind on an `Artifact` identity), which the
        // admission layer makes unrepresentable.
        (kind, identity) => unreachable!(
            "ready job carries an inconsistent (kind, identity) pairing: \
             {kind:?} / {identity:?}",
        ),
    }
}

/// Run a file-stage [`ReadyJob`] through the file-stage executor chokepoint.
///
/// Factored out of [`dispatch_ready_job_to_executor`] so each file-stage arm
/// shares the node-presence assertion and the single call into
/// [`Scheduler::execute_stage_on_worker`]. Cache-node work never reaches here.
#[allow(clippy::too_many_arguments)]
#[must_use = "the terminal StageComplete must be delivered to the inbox"]
pub(super) fn run_file_stage(
    task_kind: TaskKind,
    job: &ReadyJob,
    file_node: Option<&FileNode>,
    generation: u64,
    failed_blocker_deps: std::collections::BTreeMap<DepKey, crate::dag::FailedDepRecord>,
    executor: &dyn StageExecutor,
    source_loader: &dyn SourceLoader,
    inbox_sender: &crossbeam_channel::Sender<Submission>,
    dag: Arc<DagMutex>,
    source_root: Arc<crate::source_root::SchedulerSourceDirectory>,
) -> Option<Submission> {
    let node = file_node.unwrap_or_else(|| {
        unreachable!(
            "file-stage dispatch ({:?}) requires a resolved FileNode; the caller \
             looks the node up and applies the removed-node / generation guards \
             before routing",
            job.kind,
        )
    });
    Scheduler::execute_stage_on_worker(
        node,
        generation,
        &task_kind,
        failed_blocker_deps,
        executor,
        source_loader,
        inbox_sender,
        dag,
        source_root,
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn should_join_driver_thread(
    handle_thread_id: std::thread::ThreadId,
    current_thread_id: std::thread::ThreadId,
) -> bool {
    handle_thread_id != current_thread_id
}

/// What ended the native driver's park.
///
/// The park watches three things at once, so the outcome is named rather
/// than inferred from a channel error: a teardown signal delivered on the
/// driver's own private channel, a submission from the shared inbox, a
/// closed inbox, or the idle re-pump deadline.
#[cfg(not(target_arch = "wasm32"))]
pub(super) enum DriverPark {
    /// A teardown was requested. Re-enters the outer loop, which observes
    /// the shutdown flag and exits.
    Teardown,
    /// A submission arrived and must be processed into the DAG.
    Submission(Submission),
    /// The inbox is closed: no further submission can arrive.
    Disconnected,
    /// The idle re-pump deadline expired. Backstops a dropped wake for
    /// stranded ready work; not priority aging.
    IdleTick,
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
/// `cfg`-gated to `test` / the opt-in `test-support` feature; it and
/// its single call site are absent from every build that does not
/// enable it, so production dispatch is unchanged.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub(crate) struct DispatchPauseHook {
    pub(super) state: Mutex<DispatchPauseState>,
    pub(super) cv: parking_lot::Condvar,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub(super) struct DispatchPauseState {
    /// `true` once a test has armed the hook.
    pub(super) armed: bool,
    /// Number of dispatches after which the driver parks (cumulative
    /// across `dispatch_ready_work` invocations).
    pub(super) pause_after: usize,
    /// Cumulative count of jobs dispatched since the hook was armed.
    pub(super) dispatched: usize,
    /// `true` once the driver has reached the pause point and is parked.
    pub(super) paused: bool,
    /// `true` once the pause has fired; prevents re-pausing on later
    /// dispatch-loop iterations.
    pub(super) consumed: bool,
    /// `true` once the test has released the parked driver.
    pub(super) released: bool,
}

#[cfg(any(test, feature = "test-support"))]
impl DispatchPauseHook {
    /// Driver side: record one dispatch and, if the cumulative count has
    /// reached the armed `pause_after`, park before the next dequeue.
    ///
    /// While parked, `redrain` is invoked repeatedly so every
    /// still-in-flight submission is pulled into the scheduler DAG; the
    /// test observes the resulting queue depth before releasing. Bounded
    /// at ~10 s — a release that never arrives PANICS rather than hanging
    /// the driver forever.
    ///
    /// The single call site is in the native dispatch loop.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_dispatch_and_maybe_pause(&self, redrain: &dyn Fn()) {
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
///
/// `pump_ready` and every producer of these counters are native-only.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PumpStats {
    /// Submissions drained from the inbox into the DAG.
    pub drained: usize,
    /// Ready jobs handed off to a pool (native dispatch).
    pub dispatched: usize,
    /// Ready jobs executed inline on the caller's thread.
    pub executed_inline: usize,
}

#[cfg(not(target_arch = "wasm32"))]
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
    /// A scoped borrowed cache-node flight was selected by the DAG. The
    /// driver signalled its waiting callers; one of them executes the closure.
    DeferredScoped,
    /// The job was skipped (defensive cases: CacheNode, removed
    /// node, generation mismatch).
    Skipped,
}

/// The mutex type guarding the scheduler's central [`SchedulerDag`] — the
/// single most contended lock in the scheduler (every admission, dispatch,
/// and completion acquires it).
///
/// A dedicated alias rather than reusing the crate-wide `Mutex` import: under
/// the `hotpath` feature this becomes `hotpath::wrap::parking_lot::Mutex`,
/// hotpath 0.23's drop-in instrumented wrapper, so the DAG lock's wait/hold
/// duration is exactly the kind of bottleneck that instrumentation exists to
/// surface — but the wrapper doesn't implement `Debug`/`Default`, and
/// `scheduler.rs` has several OTHER, unrelated `Mutex<T>` fields on
/// `#[derive(Debug, Default)]` structs (dispatch-pause state, cache-call
/// recording) that would break if the crate-wide import were swapped
/// instead. Scoping the wrap to this one alias keeps every other lock in the
/// file byte-identical to the pre-instrumentation code regardless of the
/// feature.
#[cfg(feature = "hotpath")]
pub(super) type DagMutex = hotpath::wrap::parking_lot::Mutex<SchedulerDag>;

#[cfg(not(feature = "hotpath"))]
pub(super) type DagMutex = Mutex<SchedulerDag>;

/// Guard type returned by locking a [`DagMutex`]. See [`DagMutex`] for why
/// this is a dedicated alias rather than a bare `parking_lot::MutexGuard`.
// `#[allow(dead_code)]` on both branches: rustc's type-alias dead-code check
// does not credit `acquire_dag_for_admission`'s return-position reference to
// this alias — reproduces on EITHER branch (whichever is the active cfg), so
// it is a rustc false positive for a private lifetime-generic alias used
// only in return position, not an actually-unused alias.
#[cfg(feature = "hotpath")]
#[allow(dead_code)]
pub(super) type DagMutexGuard<'a> = hotpath::wrap::parking_lot::MutexGuard<'a, SchedulerDag>;

#[cfg(not(feature = "hotpath"))]
#[allow(dead_code)]
pub(super) type DagMutexGuard<'a> = parking_lot::MutexGuard<'a, SchedulerDag>;

/// Construct a [`DagMutex`]. Under `hotpath` this routes through
/// `hotpath::mutex!`; without it, this is a plain `parking_lot::Mutex::new`,
/// identical to the pre-instrumentation code.
#[cfg(feature = "hotpath")]
pub(super) fn new_dag_mutex(dag: SchedulerDag) -> DagMutex {
    hotpath::mutex!(parking_lot::Mutex::new(dag), label = "scheduler_dag")
}

#[cfg(not(feature = "hotpath"))]
pub(super) fn new_dag_mutex(dag: SchedulerDag) -> DagMutex {
    Mutex::new(dag)
}

impl Scheduler {
    /// Enqueue without parking the sole driver (or a single-threaded inline
    /// pump) behind its own full inbox. Other callers wait for bounded
    /// capacity; the driver can consume one older submission to free a slot.
    pub(super) fn send_submission(&self, submission: Submission) -> Result<(), Box<Submission>> {
        #[cfg(not(target_arch = "wasm32"))]
        use caller_kind::CallerKind;
        let mut submission = submission;
        loop {
            submission = match self.inbox.sender.try_send(submission) {
                Ok(()) => return Ok(()),
                Err(crossbeam_channel::TrySendError::Full(submission)) => submission,
                Err(crossbeam_channel::TrySendError::Disconnected(submission)) => {
                    return Err(Box::new(submission))
                }
            };
            if self.shutdown.load(Ordering::Acquire) {
                return Err(Box::new(submission));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !matches!(
                CallerKind::current(),
                CallerKind::Driver | CallerKind::Inline
            ) && self.driver_handle.lock().is_some()
            {
                return self
                    .inbox
                    .sender
                    .send(submission)
                    .map_err(|error| Box::new(error.0));
            }
            let mut select = crossbeam_channel::Select::new();
            let send = select.send(&self.inbox.sender);
            select.recv(&self.inbox.receiver);
            let operation = select.select();
            if operation.index() == send {
                return operation
                    .send(&self.inbox.sender, submission)
                    .map_err(|error| Box::new(error.0));
            }
            if let Ok(older) = operation.recv(&self.inbox.receiver) {
                self.process_submission(older);
            }
        }
    }

    /// Drain all available submissions from the inbox.
    ///
    /// Compatibility wrapper around [`Self::drain_inbox_for_pump`].
    /// The pump entry returns a count; this entry discards it for
    /// the existing call sites that don't track progress directly.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn drain_inbox(&self) {
        let _ = self.drain_inbox_for_pump();
    }

    /// WASM build's drain_inbox (no cooperative-pump infrastructure
    /// compiled on `wasm32`).
    #[cfg(target_arch = "wasm32")]
    pub(super) fn drain_inbox(&self) {
        while let Ok(submission) = self.inbox.receiver.try_recv() {
            self.process_submission(submission);
        }
    }

    /// Process a single submission.
    pub(super) fn process_submission(&self, submission: Submission) {
        match submission {
            Submission::Wake => {}
            Submission::NewRequest {
                file_id,
                target,
                priority,
                source,
                file_language,
                sender,
                submitted_lifetime,
                request_context,
            } => {
                self.handle_new_request(
                    file_id,
                    target,
                    priority,
                    source,
                    file_language,
                    sender,
                    submitted_lifetime,
                    request_context,
                );
            }
            Submission::NewRequestBatch { requests } => {
                self.handle_new_request_batch(requests);
            }
            Submission::ScopedCacheNode {
                identity,
                priority,
                flight,
                request_context,
            } => {
                self.handle_scoped_cache_node_submission(
                    identity,
                    priority,
                    flight,
                    request_context,
                );
            }
            Submission::StageComplete {
                file_id,
                generation,
                task_kind,
                incarnation,
            } => {
                self.handle_stage_complete(&file_id, generation, task_kind, incarnation);
            }
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
        caller_kind: caller_kind::CallerKind,
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
        caller_kind: caller_kind::CallerKind,
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
                DispatchOutcome::DeferredScoped => stats.dispatched += 1,
                DispatchOutcome::Skipped => {}
            }

            // Test-only: after each dispatch, record it and — once the
            // armed `pause_after` count is reached — park here (before the
            // next dequeue) until the test releases, re-draining the inbox
            // so the surplus provably accrues scheduler-queue dwell. No-op
            // unless a test has armed the hook; absent from release builds.
            #[cfg(any(test, feature = "test-support"))]
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
    /// I/O worker, external, inline) goes to the scheduler CPU pool via
    /// nonblocking `cpu_pool.try_submit`. Source work always goes to the I/O
    /// pool.
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
        caller_kind: caller_kind::CallerKind,
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
        // One dispatch = one executed task; the dwell is the time the entry
        // sat in the DAG, and the pre-dequeue depth is the queue high-water
        // mark this dispatch observed.
        verter_audit::attribute!(TaskExecute);
        verter_audit::attribute_n!(
            TaskWait,
            dequeue_at
                .saturating_duration_since(job.enqueue_time)
                .as_nanos()
                .min(u64::MAX as u128)
        );
        verter_audit::attribute_max!(QueueDepth, queue_depth_pre_dequeue);

        let inbox_sender = self.inbox.sender.clone();
        let executor = Arc::clone(&self.executor);
        let source_loader = Arc::clone(&self.source_loader);

        // Cache-node work has no file node and routes straight to the
        // executor's cache-materialisation hook. It is CPU-class work
        // (`WorkKind::CacheNode`), so it is spawned onto the CPU pool where
        // the single dispatch entry `dispatch_ready_job_to_executor` runs it
        // and releases the dispatched node's parked reservation. There is no
        // file-stage routing for it (no node lookup, no generation guard).
        if matches!(job.identity, WorkNodeIdentity::CacheNode { .. }) {
            if let Some(flight) = self
                .scoped_cache_flights
                .get(&job.identity)
                .map(|entry| Arc::clone(entry.value()))
            {
                if flight.mark_dispatched(job.cancellation.clone()) {
                    return DispatchOutcome::DeferredScoped;
                }
                self.terminalize_scoped_cache_flight(
                    &job.identity,
                    &flight,
                    ScopedCacheTerminal::Cancelled,
                    false,
                );
                return DispatchOutcome::Skipped;
            }
            let executor_for_cache = Arc::clone(&self.executor);
            let source_loader_for_cache = Arc::clone(&self.source_loader);
            let inbox_for_cache = inbox_sender.clone();
            let dag_for_cache = Arc::clone(&self.dag);
            let source_root_for_cache = Arc::clone(&self.source_root);
            // Snapshot the identity for the submit-failure release path before
            // `job` moves into the pool closure below.
            let cache_identity = job.identity.clone();
            let task: crate::execution::pool::SchedulerPoolTask = Box::new(move || {
                let job = job.started(&inbox_for_cache);
                let cancellation = job.cancellation.clone();
                let completion = dispatch_ready_job_to_executor(
                    &job,
                    None,
                    0,
                    std::collections::BTreeMap::new(),
                    executor_for_cache.as_ref(),
                    source_loader_for_cache.as_ref(),
                    &inbox_for_cache,
                    dag_for_cache,
                    source_root_for_cache,
                    &cancellation,
                );
                Self::deliver_stage_completion_from_pool(&inbox_for_cache, completion);
            });
            return match self.try_submit_cpu(task) {
                Ok(crate::execution::pool::SchedulerPoolSubmitResult::Submitted) => {
                    DispatchOutcome::SubmittedToPool
                }
                // A failed cache-node submit releases the parked reservation by
                // cancelling the identity — cache-node identities are never
                // DepKey prerequisites, so this cannot strand a waiter.
                Err(_err) => {
                    let stranded = self.dag.lock().cancel(&cache_identity);
                    verter_debug_assert!(
                        stranded.is_empty(),
                        "CacheNode submit-failure release must not strand DAG waiters: \
                         CacheNode identities are not used as DepKey prerequisites",
                    );
                    DispatchOutcome::Skipped
                }
            };
        }

        // File-stage / artifact work. Build the owned execution descriptor
        // once for pool routing and the cold panic/submit-violation paths; the
        // actual execution routes through the single dispatch entry
        // `dispatch_ready_job_to_executor` below. `CacheNode` was branched away
        // above, so this match never sees it.
        let task_kind = match (&job.kind, &job.identity) {
            (WorkKind::Load, WorkNodeIdentity::FileStage { .. }) => TaskKind::Load,
            // `Parse` is never admitted as a runnable DAG node (see the router
            // `dispatch_ready_job_to_executor` and `admit_work`), so this
            // descriptor builder never observes a `(WorkKind::Parse, FileStage)`
            // ready job either. Fabricating a live `TaskKind::Parse` here would
            // contradict the router, which `unreachable!()`s on the same pairing.
            (WorkKind::Parse, WorkNodeIdentity::FileStage { .. }) => unreachable!(
                "Parse is never admitted as a runnable DAG node — no site produces \
                 a `(WorkKind::Parse, FileStage)` ready job (admit_work rejects \
                 `TaskKind::Parse`), so the pool-routing descriptor builder cannot \
                 observe one.",
            ),
            (WorkKind::Analysis, WorkNodeIdentity::FileStage { .. }) => TaskKind::Analysis,
            (WorkKind::Artifact, WorkNodeIdentity::Artifact { profile_hash, .. }) => {
                TaskKind::Artifact {
                    profile_hash: profile_hash_from_bytes(*profile_hash),
                }
            }
            (kind, identity) => unreachable!(
                "ready job carries an inconsistent file-stage (kind, identity) pairing: \
                 {kind:?} / {identity:?}",
            ),
        };
        let (file_id, incarnation, generation) = match &job.identity {
            WorkNodeIdentity::FileStage {
                canonical,
                incarnation,
                generation,
                ..
            } => (canonical.to_string(), *incarnation, *generation),
            WorkNodeIdentity::Artifact {
                canonical,
                incarnation,
                generation,
                ..
            } => (canonical.to_string(), *incarnation, *generation),
            WorkNodeIdentity::CacheNode { .. } => {
                unreachable!(
                    "CacheNode identities are routed above and never reach file-stage \
                              dispatch"
                )
            }
        };

        let node = match self.nodes.get(&file_id) {
            Some(n) => n.clone(),
            None => {
                verter_debug_assert!(
                    self.dag.lock().token_for(&job.identity).is_none(),
                    "defensive dispatch skip: removed-FileNode case implies the prior \
                     `remove()` cancelled the DAG identity before clearing nodes"
                );
                return DispatchOutcome::Skipped;
            }
        };
        if node.incarnation_id() != incarnation || node.generation() != generation {
            verter_debug_assert!(
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
        let source_root_handle = Arc::clone(&self.source_root);
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
        // `Load` is the I/O-class source label; every other file-stage label
        // is CPU-class. A CPU worker inline-runs CPU-class work; an I/O worker
        // inline-runs the I/O-class `Load` (source) work.
        let inline_eligible = (matches!(caller_kind, caller_kind::CallerKind::CpuWorker)
            && !matches!(task_kind, TaskKind::Load))
            || (matches!(caller_kind, caller_kind::CallerKind::IoWorker)
                && matches!(task_kind, TaskKind::Load));
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
                None => InlineTlsGuard::ClearAll(
                    verter_execution::request_context::AllSlotsClearGuard::clear_all(),
                ),
            };
            // Audit pool tag mirrors the pool the inline branch is
            // running on: `IoWorker × Source` runs inline on the
            // I/O worker, so the dispatch audit must record `Io`;
            // `CpuWorker × non-Source` runs inline on the CPU
            // worker → `Cpu`. The inline_eligible gate above
            // already restricts to these two combinations.
            let inline_pool_tag = match (caller_kind, &task_kind) {
                (caller_kind::CallerKind::IoWorker, TaskKind::Load) => {
                    audit_publish::WorkerPoolTag::Io
                }
                _ => audit_publish::WorkerPoolTag::Cpu,
            };
            let identity_for_path = identity.clone();
            caller_kind::with_active_path(identity_for_path, || {
                Self::publish_scheduler_dispatch(
                    inline_pool_tag,
                    audit_publish::SchedulerDepthsSnapshot {
                        inbox: inbox_depth,
                        queue: queue_depth_pre_dequeue,
                    },
                    queue_dwell_ms,
                );
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let cancellation = job.cancellation.clone();
                    dispatch_ready_job_to_executor(
                        &job,
                        Some(&node),
                        generation,
                        failed_blocker_deps,
                        executor.as_ref(),
                        source_loader.as_ref(),
                        &inbox_sender,
                        Arc::clone(&dag_handle),
                        Arc::clone(&source_root_handle),
                        &cancellation,
                    )
                }));
                match result {
                    Ok(completion) => self.deliver_stage_completion(completion),
                    Err(_) => Self::surface_stage_panic_as_failed(
                        &node,
                        generation,
                        &task_kind,
                        &inbox_sender,
                        Arc::clone(&dag_handle),
                    ),
                }
            });
            return DispatchOutcome::ExecutedInline;
        }

        // Capture the terminalization inputs BEFORE moving the worker
        // payload into the closure. If `try_submit` reports
        // `Full`/`Closed` the closure is consumed without running, so
        // the invariant-violation path needs its own copies of
        // `(canonical, dag, inbox_sender)` to release the parked
        // reservation through the normal DAG cancel path.
        let canonical_for_violation: Arc<str> = Arc::from(node.canonical_id.as_str());
        let dag_for_violation = Arc::clone(&dag_handle);
        let inbox_for_violation = inbox_sender.clone();

        if matches!(task_kind, TaskKind::Load) {
            // Source (`Load`) jobs: I/O pool loads content; the parse step is
            // intrinsic to the source-stage execution path (no separate node).
            let node_for_panic = Arc::clone(&node);
            let dag_for_panic = Arc::clone(&dag_handle);
            let task_kind_for_panic = task_kind.clone();
            let task: crate::execution::pool::SchedulerPoolTask = Box::new(move || {
                let job = job.started(&inbox_sender);
                let _guard: Option<
                    Box<dyn verter_execution::request_context::TlsUninstall + Send>,
                > = winner_ctx.map(|opaque| Arc::clone(&opaque.0).install_tls());
                Self::publish_scheduler_dispatch(
                    audit_publish::WorkerPoolTag::Io,
                    audit_publish::SchedulerDepthsSnapshot {
                        inbox: inbox_depth,
                        queue: queue_depth_pre_dequeue,
                    },
                    queue_dwell_ms,
                );
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    caller_kind::with_active_path(identity, || {
                        let cancellation = job.cancellation.clone();
                        dispatch_ready_job_to_executor(
                            &job,
                            Some(&node),
                            generation,
                            failed_blocker_deps,
                            executor.as_ref(),
                            source_loader.as_ref(),
                            &inbox_sender,
                            Arc::clone(&dag_handle),
                            Arc::clone(&source_root_handle),
                            &cancellation,
                        )
                    })
                }));
                match result {
                    Ok(completion) => {
                        Self::deliver_stage_completion_from_pool(&inbox_sender, completion)
                    }
                    Err(_) => Self::surface_stage_panic_as_failed(
                        &node_for_panic,
                        generation,
                        &task_kind_for_panic,
                        &inbox_sender,
                        dag_for_panic,
                    ),
                }
            });
            match self.try_submit_io(task) {
                Ok(crate::execution::pool::SchedulerPoolSubmitResult::Submitted) => {
                    DispatchOutcome::SubmittedToPool
                }
                Err(err) => self.terminalize_pool_submit_violation(
                    err,
                    &dag_for_violation,
                    &canonical_for_violation,
                    incarnation,
                    generation,
                    &task_kind,
                    &inbox_for_violation,
                ),
            }
        } else {
            // Analysis/Artifact jobs: pure CPU work.
            let node_for_panic = Arc::clone(&node);
            let dag_for_panic = Arc::clone(&dag_handle);
            let task_kind_for_panic = task_kind.clone();
            let task: crate::execution::pool::SchedulerPoolTask = Box::new(move || {
                let job = job.started(&inbox_sender);
                let _guard: Option<
                    Box<dyn verter_execution::request_context::TlsUninstall + Send>,
                > = winner_ctx.map(|opaque| Arc::clone(&opaque.0).install_tls());
                Self::publish_scheduler_dispatch(
                    audit_publish::WorkerPoolTag::Cpu,
                    audit_publish::SchedulerDepthsSnapshot {
                        inbox: inbox_depth,
                        queue: queue_depth_pre_dequeue,
                    },
                    queue_dwell_ms,
                );
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    caller_kind::with_active_path(identity, || {
                        let cancellation = job.cancellation.clone();
                        dispatch_ready_job_to_executor(
                            &job,
                            Some(&node),
                            generation,
                            failed_blocker_deps,
                            executor.as_ref(),
                            source_loader.as_ref(),
                            &inbox_sender,
                            Arc::clone(&dag_handle),
                            Arc::clone(&source_root_handle),
                            &cancellation,
                        )
                    })
                }));
                match result {
                    Ok(completion) => {
                        Self::deliver_stage_completion_from_pool(&inbox_sender, completion)
                    }
                    Err(_) => Self::surface_stage_panic_as_failed(
                        &node_for_panic,
                        generation,
                        &task_kind_for_panic,
                        &inbox_sender,
                        dag_for_panic,
                    ),
                }
            });
            match self.try_submit_cpu(task) {
                Ok(crate::execution::pool::SchedulerPoolSubmitResult::Submitted) => {
                    DispatchOutcome::SubmittedToPool
                }
                Err(err) => self.terminalize_pool_submit_violation(
                    err,
                    &dag_for_violation,
                    &canonical_for_violation,
                    incarnation,
                    generation,
                    &task_kind,
                    &inbox_for_violation,
                ),
            }
        }
    }

    /// Submit a Source task to the injected I/O pool. A test-only
    /// fault-injection seam ([`Self::pool_submit_fault`]) may force a
    /// `Full`/`Closed` result here so the invariant-violation RELEASE
    /// path can be characterized without the real transport ever being
    /// saturated. Production builds compile the seam out and call
    /// `try_submit` directly.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn try_submit_io(
        &self,
        task: crate::execution::pool::SchedulerPoolTask,
    ) -> Result<
        crate::execution::pool::SchedulerPoolSubmitResult,
        crate::execution::pool::SchedulerPoolSubmitError,
    > {
        // Record the submit ATTEMPT before doing anything else: a test
        // that parks the single I/O worker asserts the driver reaches
        // every admitted submit attempt while the worker is stuck. The
        // count is incremented for the attempt itself, independent of
        // whether the nonblocking `try_send` below succeeds.
        #[cfg(test)]
        self.io_submit_attempts.fetch_add(1, Ordering::AcqRel);
        #[cfg(test)]
        if let Some(err) = self.injected_pool_submit_fault() {
            // Drop `task` without running it (mirrors a genuine
            // Full/Closed where the transport consumed nothing).
            drop(task);
            return Err(err);
        }
        self.io_pool
            .try_submit(owner_command::OwnerCommand::io(task))
    }

    /// Submit an Analysis/Artifact task to the injected CPU pool. See
    /// [`Self::try_submit_io`] for the test-only fault seam.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn try_submit_cpu(
        &self,
        task: crate::execution::pool::SchedulerPoolTask,
    ) -> Result<
        crate::execution::pool::SchedulerPoolSubmitResult,
        crate::execution::pool::SchedulerPoolSubmitError,
    > {
        #[cfg(test)]
        if let Some(err) = self.injected_pool_submit_fault() {
            drop(task);
            return Err(err);
        }
        self.cpu_pool
            .try_submit(owner_command::OwnerCommand::cpu(task))
    }

    /// Read + clear the one-shot test-only pool-submit fault. Returns
    /// the forced error exactly once per arming (so a single armed fault
    /// terminalizes one job, not every subsequent dispatch). `0` = off,
    /// `1` = force `Full`, `2` = force `Closed`. On a hit it records that
    /// the fault was test-injected so the downstream
    /// `terminalize_pool_submit_violation` suppresses its `debug_assert!`
    /// (the test is characterizing the release-build RELEASE path).
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(super) fn injected_pool_submit_fault(
        &self,
    ) -> Option<crate::execution::pool::SchedulerPoolSubmitError> {
        let err = match self.pool_submit_fault.swap(0, Ordering::AcqRel) {
            1 => Some(crate::execution::pool::SchedulerPoolSubmitError::Full),
            2 => Some(crate::execution::pool::SchedulerPoolSubmitError::Closed),
            _ => None,
        };
        if err.is_some() {
            self.pool_submit_fault_was_injected
                .store(true, Ordering::Release);
        }
        err
    }

    /// Test-only: arm a one-shot pool-submit fault so the NEXT non-inline
    /// dispatch observes a forced `Full` from `try_submit`, exercising
    /// the invariant-violation terminalize/release path without ever
    /// saturating the real transport. Consumed once (auto-clears).
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn arm_pool_submit_fault_full(&self) {
        self.pool_submit_fault.store(1, Ordering::Release);
    }

    /// Test-only: number of I/O pool submit attempts the driver has made
    /// so far (see [`Self::io_submit_attempts`]). A test parks the single
    /// I/O worker and polls this until it reaches the full admitted
    /// fan-out, proving the driver kept dispatching past the stuck worker.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn io_submit_attempts(&self) -> usize {
        self.io_submit_attempts.load(Ordering::Acquire)
    }

    /// Handle a nonblocking pool-submit failure at the dispatch site.
    ///
    /// Under the DAG capacity-ledger invariant the pool is NOT genuinely
    /// full when `dispatch_ready_job` runs: `next_ready_for_pump`
    /// reserves the CPU/IO permit (dag.rs:1762) before producing the
    /// `ReadyJob`, credit commits only after the reservation succeeds
    /// (dag.rs:1775), and the reservation is parked on the node
    /// (dag.rs:1801). Inline-eligible loans (`IoWorker`×Source,
    /// `CpuWorker`×CPU) run on the caller thread and never enter a pool
    /// transport, so non-inline submissions are bounded by the resolved
    /// `dag_budget` — and the host sizes the IO transport to dominate
    /// `dag_budget.io`. A `Full`/`Closed` result is therefore an
    /// INVARIANT VIOLATION, not backpressure.
    ///
    /// The violation is `debug_assert!`ed (so debug/test builds fault
    /// loudly), then the job is terminalized via the SAME path the
    /// worker-panic recovery uses ([`Self::terminalize_failure`] +
    /// [`Self::requeue_terminalize_stranded`]): the DAG node is
    /// cancelled, releasing the parked reservation through the by-value
    /// `release(self)` consume — no permit leak. Credit was committed
    /// exactly once at admission and is NOT re-committed (the job is not
    /// requeued or re-admitted). The job is surfaced `Failed` with a
    /// typed [`crate::job::SchedulerError::StageFailed`] so the caller
    /// observes a terminal failure rather than a hang. No silent drop,
    /// no requeue, no double-credit.
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn terminalize_pool_submit_violation(
        self: &Arc<Self>,
        err: crate::execution::pool::SchedulerPoolSubmitError,
        dag: &DagMutex,
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
        task_kind: &TaskKind,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
    ) -> DispatchOutcome {
        // The `debug_assert!` fires for GENUINE invariant violations
        // (a real `Full`/`Closed` from the pool transport). It is
        // suppressed only when a test deliberately injected the fault
        // via the `pool_submit_fault` seam — that test is exercising the
        // RELEASE path that runs in release builds (where the assert is
        // compiled out), so faulting on the injected error would defeat
        // the test rather than catch a bug. Production / debug builds
        // with a real transport failure still fault loudly.
        let test_injected = {
            #[cfg(all(test, not(target_arch = "wasm32")))]
            {
                self.pool_submit_fault_was_injected
                    .swap(false, Ordering::AcqRel)
            }
            #[cfg(not(all(test, not(target_arch = "wasm32"))))]
            {
                false
            }
        };
        verter_debug_assert!(
            test_injected,
            "scheduler pool submit returned {err:?} at the dispatch site: the DAG \
             capacity ledger reserves the {task_kind:?} permit in next_ready_for_pump \
             before producing the ReadyJob, and each transport is sized to dominate \
             its matching dag_budget (CPU dominates dag_budget.cpu, IO dominates \
             dag_budget.io). Full/Closed is an invariant violation (fail-closed), \
             not backpressure"
        );
        let error = crate::job::SchedulerError::StageFailed {
            file_id: canonical.to_string(),
            stage: format!("{task_kind:?}"),
            message: format!("scheduler pool submit failed: {err:?}"),
        };
        let stranded =
            Self::terminalize_failure(dag, canonical, incarnation, generation, task_kind, error);
        Self::requeue_terminalize_stranded(inbox_sender, &stranded);
        DispatchOutcome::Skipped
    }

    /// Publish a single scheduler-dispatch fact through the audit
    /// observer TLS slot if one is installed. The session-side
    /// `RequestContext` impl writes the supplied facts into its
    /// per-request scheduler-audit slot; non-audit callers are a
    /// no-op via [`verter_audit::observer::AuditObserver`]'s default
    /// implementation. Static so it can be called from worker
    /// closures without holding `&self`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn publish_scheduler_dispatch(
        pool: audit_publish::WorkerPoolTag,
        depths: audit_publish::SchedulerDepthsSnapshot,
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
    pub(super) fn execute_stage_inline(&self, job: ReadyJob) {
        // Cache-node work has no file node: run it inline through the single
        // dispatch entry, which routes to the executor's cache hook and
        // releases the dispatched node's parked reservation. No file-stage
        // node lookup or generation guard applies.
        if matches!(job.identity, WorkNodeIdentity::CacheNode { .. }) {
            if let Some(flight) = self
                .scoped_cache_flights
                .get(&job.identity)
                .map(|entry| Arc::clone(entry.value()))
            {
                if !flight.mark_dispatched(job.cancellation.clone()) {
                    self.terminalize_scoped_cache_flight(
                        &job.identity,
                        &flight,
                        ScopedCacheTerminal::Cancelled,
                        false,
                    );
                }
                return;
            }
            let identity = job.identity.clone();
            let completion = caller_kind::with_active_path(identity, || {
                let cancellation = job.cancellation.clone();
                dispatch_ready_job_to_executor(
                    &job,
                    None,
                    0,
                    std::collections::BTreeMap::new(),
                    self.executor.as_ref(),
                    self.source_loader.as_ref(),
                    &self.inbox.sender,
                    self.dag.clone(),
                    self.source_root.clone(),
                    &cancellation,
                )
            });
            self.deliver_stage_completion(completion);
            return;
        }
        let (file_id, incarnation, generation) = match &job.identity {
            WorkNodeIdentity::FileStage {
                canonical,
                incarnation,
                generation,
                ..
            } => (canonical.to_string(), *incarnation, *generation),
            WorkNodeIdentity::Artifact {
                canonical,
                incarnation,
                generation,
                ..
            } => (canonical.to_string(), *incarnation, *generation),
            WorkNodeIdentity::CacheNode { .. } => {
                unreachable!(
                    "CacheNode identities are routed above and never reach file-stage \
                              inline dispatch"
                )
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
                verter_debug_assert!(
                    self.dag.lock().token_for(&job.identity).is_none(),
                    "defensive inline dispatch skip: removed-FileNode case implies the prior \
                     `remove()` cancelled the DAG identity before clearing nodes"
                );
                return;
            }
        };

        if node.incarnation_id() != incarnation || node.generation() != generation {
            verter_debug_assert!(
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
        // its own pending completion. The owned execution descriptor is
        // constructed by the single dispatch entry from `job`'s own
        // `(kind, identity)`.
        let identity = job.identity.clone();
        let failed_blocker_deps = job.failed_blocker_deps.clone();
        // The terminal `StageComplete` is delivered after the stage returns
        // and off the active path: this thread may be the inbox's sole
        // consumer, so a full inbox must be drained, never waited on.
        let completion = caller_kind::with_active_path(identity, || {
            let cancellation = job.cancellation.clone();
            dispatch_ready_job_to_executor(
                &job,
                Some(&node),
                generation,
                failed_blocker_deps,
                self.executor.as_ref(),
                self.source_loader.as_ref(),
                &self.inbox.sender,
                self.dag.clone(),
                self.source_root.clone(),
                &cancellation,
            )
        });
        self.deliver_stage_completion(completion);
    }

    /// Execute a stage on a worker (rayon thread or inline).
    ///
    /// This is a static method so it can be called from owner-affine CPU
    /// pool tasks without holding a reference to &self. All shared state
    /// is passed explicitly.
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
    ///
    /// Returns the stage's terminal `StageComplete`; the caller delivers it
    /// with the discipline its thread requires (see
    /// [`Scheduler::deliver_stage_completion`]).
    #[allow(clippy::too_many_arguments)]
    #[must_use = "the terminal StageComplete must be delivered to the inbox"]
    pub(super) fn execute_stage_on_worker(
        node: &FileNode,
        generation: u64,
        task_kind: &TaskKind,
        failed_blocker_deps: std::collections::BTreeMap<
            crate::dag::DepKey,
            crate::dag::FailedDepRecord,
        >,
        executor: &dyn StageExecutor,
        source_loader: &dyn SourceLoader,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        dag: Arc<DagMutex>,
        source_root: Arc<crate::source_root::SchedulerSourceDirectory>,
    ) -> Option<Submission> {
        let incarnation = node.incarnation_id();
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
            verter_debug_assert!(
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
                incarnation,
                generation,
                task_kind,
                SchedulerError::DependencyFailed {
                    dep_key: first_record.dep_key.clone(),
                    cause: Box::new(first_record.cause.clone()),
                },
            );
            Self::requeue_terminalize_stranded(inbox_sender, &stranded);
            return None;
        }
        match task_kind {
            // The source stage runs under the `Load` label (the live
            // `FileStage{Source}` node maps to `Load`); the load+parse work
            // runs in this one source-stage execution path.
            TaskKind::Load => {
                verter_debug_assert!(
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
                    source_root,
                )
            }
            TaskKind::Analysis => {
                verter_debug_assert!(
                    failed_blocker_deps.is_empty(),
                    "Analysis stage received failed_blocker_deps — pre-dispatch \
                     short-circuit must consume the marker before kind-dispatch \
                     (fan-out target invariant violated)",
                );
                Self::execute_analysis_stage(node, generation, executor, inbox_sender, dag)
            }
            TaskKind::Artifact { profile_hash } => {
                verter_debug_assert!(
                    failed_blocker_deps.is_empty(),
                    "Artifact stage received failed_blocker_deps — pre-dispatch \
                     short-circuit must consume the marker before kind-dispatch \
                     (fan-out target invariant violated)",
                );
                Self::execute_artifact_stage(
                    node,
                    generation,
                    *profile_hash,
                    executor,
                    inbox_sender,
                    dag,
                )
            }
            // `execute_stage_on_worker` is the file-stage executor chokepoint.
            // `Parse` is intrinsic to the source stage (never dispatched as a
            // standalone file stage in the single-`FileStage{Source}`-node
            // model), and `CacheNode` is routed directly to
            // `StageExecutor::execute_cache_node` by
            // [`Self::dispatch_ready_job_to_executor`] — it never reaches this
            // file-node-centric executor (it carries no `FileNode`).
            TaskKind::Parse | TaskKind::CacheNode { .. } => unreachable!(
                "execute_stage_on_worker runs file-stage work (Load/Analysis/Artifact); \
                 Parse is intrinsic to the source stage and CacheNode routes through \
                 dispatch_ready_job_to_executor to execute_cache_node"
            ),
        }
    }

    /// Execute the Source stage: load content, run executor, commit.
    /// Returns the terminal `StageComplete` for the caller to deliver.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    #[must_use = "the terminal StageComplete must be delivered to the inbox"]
    pub(super) fn execute_source_stage(
        node: &FileNode,
        generation: u64,
        executor: &dyn StageExecutor,
        source_loader: &dyn SourceLoader,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        dag: Arc<DagMutex>,
        source_root: Arc<crate::source_root::SchedulerSourceDirectory>,
    ) -> Option<Submission> {
        let incarnation = node.incarnation_id();
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
                    incarnation,
                    generation,
                    &TaskKind::Load,
                    SchedulerError::FileNotFound {
                        file_id: node.canonical_id.clone(),
                    },
                );
                Self::requeue_terminalize_stranded(inbox_sender, &stranded);
                return None;
            }
        };

        let snapshot = match executor.execute_source(
            &node.canonical_id,
            node.file_language.clone(),
            content,
            generation,
        ) {
            Ok(snap) => Arc::new(snap),
            Err(e) => {
                // Preserve the executor's typed failure discriminant:
                // a known-but-unsupported framework language surfaces
                // as the typed `UnsupportedLanguage` error, everything
                // else as `StageFailed`.
                let error = match e.kind {
                    crate::execution::executor::StageErrorKind::UnsupportedLanguage {
                        adapter_id,
                    } => SchedulerError::UnsupportedLanguage {
                        file_id: node.canonical_id.clone(),
                        adapter_id,
                    },
                    crate::execution::executor::StageErrorKind::StackUnavailable { needed } => {
                        SchedulerError::StackUnavailable {
                            file_id: node.canonical_id.clone(),
                            needed,
                        }
                    }
                    crate::execution::executor::StageErrorKind::Generic => {
                        SchedulerError::StageFailed {
                            file_id: node.canonical_id.clone(),
                            stage: "Source".to_string(),
                            message: e.message,
                        }
                    }
                };
                let stranded = Self::terminalize_failure(
                    &dag,
                    &canonical,
                    incarnation,
                    generation,
                    &TaskKind::Load,
                    error,
                );
                Self::requeue_terminalize_stranded(inbox_sender, &stranded);
                return None;
            }
        };

        // Commit the snapshot and publish the resulting `Present`
        // source version under ONE publication hold. The
        // generation-coherence check moves inside the hold too: a
        // concurrent `invalidate` publishes its `Absent` version under
        // the same lock, so the two can no longer interleave into a
        // root history that ends `Present` at a superseded generation.
        //
        // The `pending_source` clear, the DAG signal and the inbox send
        // stay OUTSIDE the hold — the publication lock is inner to the
        // DAG lock and must never be held across it.
        let identity =
            Self::dag_identity_for_task(&canonical, incarnation, generation, &TaskKind::Load);
        let guard = dag.lock();
        guard.token_for(&identity)?;
        let committed = source_root.publish_transition(|publication| {
            if node.generation() != generation {
                return false;
            }
            node.source.store(Arc::new(Some(Arc::clone(&snapshot))));
            publication.present(
                &canonical,
                node.incarnation_id(),
                generation,
                snapshot.whole_hash,
            );
            true
        });
        drop(guard);
        if committed {
            let pending = node.pending_source.load();
            if let Some((gen, _)) = pending.as_ref() {
                if *gen == generation {
                    node.pending_source.store(Arc::new(None));
                }
            }

            Some(Submission::StageComplete {
                file_id: node.canonical_id.clone(),
                generation,
                task_kind: TaskKind::Load,
                incarnation: node.incarnation_id(),
            })
        } else {
            None
        }
    }

    /// Execute the Analysis stage via the executor.
    /// Returns the terminal `StageComplete` for the caller to deliver.
    #[must_use = "the terminal StageComplete must be delivered to the inbox"]
    pub(super) fn execute_analysis_stage(
        node: &FileNode,
        generation: u64,
        executor: &dyn StageExecutor,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        dag: Arc<DagMutex>,
    ) -> Option<Submission> {
        let incarnation = node.incarnation_id();
        use crate::job::SchedulerError;

        let canonical: Arc<str> = Arc::from(node.canonical_id.as_str());

        // Source not ready — will be retried after Source completes.
        let source = node.current_source()?;

        let snapshot = match executor.execute_analysis(&node.canonical_id, &source, generation) {
            Ok(snap) => Arc::new(snap),
            Err(e) => {
                let stranded = Self::terminalize_failure(
                    &dag,
                    &canonical,
                    incarnation,
                    generation,
                    &TaskKind::Analysis,
                    SchedulerError::StageFailed {
                        file_id: node.canonical_id.clone(),
                        stage: "Analysis".to_string(),
                        message: e.message,
                    },
                );
                Self::requeue_terminalize_stranded(inbox_sender, &stranded);
                return None;
            }
        };

        let identity =
            Self::dag_identity_for_task(&canonical, incarnation, generation, &TaskKind::Analysis);
        let mut guard = dag.lock();
        guard.token_for(&identity)?;
        if node.generation() == generation {
            node.analysis.store(Arc::new(Some(Arc::clone(&snapshot))));

            let result = RequestResult::Analysis(snapshot);
            guard.signal_stage_complete(
                &canonical,
                incarnation,
                generation,
                &TaskKind::Analysis,
                &result,
            );

            Some(Submission::StageComplete {
                file_id: node.canonical_id.clone(),
                generation,
                task_kind: TaskKind::Analysis,
                incarnation: node.incarnation_id(),
            })
        } else {
            None
        }
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
    ///
    /// Returns the terminal `StageComplete` for the caller to deliver.
    #[must_use = "the terminal StageComplete must be delivered to the inbox"]
    pub(super) fn execute_artifact_stage(
        node: &FileNode,
        generation: u64,
        profile_hash: u64,
        executor: &dyn StageExecutor,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        dag: Arc<DagMutex>,
    ) -> Option<Submission> {
        let incarnation = node.incarnation_id();
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
                incarnation,
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
            verter_debug_assert!(
                stranded.is_empty(),
                "race-safe pre-executor skip must not strand DAG waiters: \
                 Artifact identities are graph leaves"
            );
            return None;
        }

        let source = node.current_source()?;
        let analysis = node.current_analysis()?;

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
                    incarnation,
                    generation,
                    &TaskKind::Artifact { profile_hash },
                    SchedulerError::StageFailed {
                        file_id: node.canonical_id.clone(),
                        stage: "Artifact".to_string(),
                        message: e.message,
                    },
                );
                Self::requeue_terminalize_stranded(inbox_sender, &stranded);
                return None;
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
            let identity = Self::dag_identity_for_task(
                &canonical,
                incarnation,
                generation,
                &TaskKind::Artifact { profile_hash },
            );
            guard.token_for(&identity)?;
            if let Some(existing) = node.artifacts.get(&profile_hash) {
                if existing.generation == generation {
                    drop(guard);
                    return None;
                }
            }
            node.artifacts.insert(profile_hash, Arc::clone(&snapshot));
            let result = RequestResult::Artifact(snapshot);
            guard.signal_stage_complete(
                &canonical,
                incarnation,
                generation,
                &TaskKind::Artifact { profile_hash },
                &result,
            );
            drop(guard);

            // Notify the driver loop that the Artifact stage is
            // terminal so handle_stage_complete can release the
            // DAG identity (and its capacity permit) via
            // dag.complete(&artifact_id).
            Some(Submission::StageComplete {
                file_id: node.canonical_id.clone(),
                generation,
                task_kind: TaskKind::Artifact { profile_hash },
                incarnation: node.incarnation_id(),
            })
        } else {
            None
        }
    }

    // ── Test/WASM Driver Control ──

    /// Cooperative pump path for CPU / I/O worker threads with a
    /// live driver. Each iteration runs the pump under the
    /// active-path filter so the DAG never returns the work the
    /// caller is parked on, then waits on the handle with a short
    /// timeout before re-pumping. The condvar wake-up bounds the
    /// worst-case latency between the driver's dispatch and the
    /// worker observing its handle resolved.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn wait_or_drive_cooperative<T: Clone>(
        self: &Arc<Self>,
        handle: &crate::job::CompletionHandle<T>,
        caller_kind: caller_kind::CallerKind,
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
            let active_path = caller_kind::snapshot_active_path();
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
    pub(super) fn wait_or_drive_inline<T: Clone>(
        self: &Arc<Self>,
        handle: &crate::job::CompletionHandle<T>,
    ) -> crate::job::CompletionState<T> {
        let _scope = caller_kind::CallerKindGuard::install(caller_kind::CallerKind::Inline);
        let mut idle_iterations = 0u32;
        loop {
            if let Some(state) = handle.try_get() {
                return state;
            }
            self.drain_inbox();
            let job = {
                let active_path = caller_kind::snapshot_active_path();
                let mut dag = self.dag.lock();
                dag.next_ready_for_pump(caller_kind::CallerKind::Inline, &active_path)
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
