//! Plan §6.12 Commit Q — cross-thread RequestContext propagation tests.
//!
//! Pre-Q: scheduler workers installed only scheduler-side TLS via
//! `OpaqueContextGuard::install`. Session-side `CURRENT_REQUEST_CONTEXT`
//! stayed `None` on worker threads. Session-level `record_*` helpers
//! found no accumulator and silently no-op'd, leaving audit counters
//! such as `dep_signature_merges` and `node_arena_lock_acquisitions`
//! at zero on hot paths that ran inside scheduler workers.
//!
//! Post-Q: the production worker dispatch closures route through
//! `RequestContextLike::install_tls`, which (for the session impl)
//! calls `RequestContextGuard::install` — populating BOTH TLS slots.
//! `OpaqueContextGuard::install` itself is unchanged (test fixtures
//! continue to call it directly when only scheduler-side TLS is needed).
//!
//! The IO-worker tests drive a single-worker [`SchedulerIoPool`] via the
//! `run_on_io_worker_with_context` harness, which mirrors the production
//! IO dispatch closure (`install_tls` guard around the work) over the
//! surviving nonblocking `try_submit` primitive — the retired
//! `IoPool::submit_with_context` / `IoHandle` surface is gone.
//!
//! Tests:
//!
//! 1. `scheduler_worker_directly_sees_session_request_context_via_install_tls`
//!    is the strict discriminator: run a closure on the IO worker with a
//!    session-side `RequestContext` wrapped in `OpaqueRequestContext`
//!    and assert that `current_request_context()` returns `Some` inside
//!    the worker closure — pre-Q this fails because only the
//!    scheduler-side slot was installed.
//! 2. `opaque_context_guard_install_does_not_recurse` regression-locks
//!    the rev-6 recursion bug — direct call to
//!    `OpaqueContextGuard::install` with a session-style `install_tls`
//!    impl in the trait must not stack-overflow.
//! 3. `scheduler_winner_thread_propagates_session_context_via_install_tls`
//!    is the end-to-end shape: drives an audited request that triggers
//!    cross-file resolution via the scheduler's worker pools, then
//!    asserts the audit record's per-context counters
//!    (`dep_signature_merges`, `node_arena_lock_acquisitions`) are
//!    non-zero. Pre-Q both stay 0 because the bumps happen on worker
//!    threads where session-side TLS is unset.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use verter_scheduler::pool::SchedulerIoPool;
use verter_scheduler::request_context::{
    CacheEventKind, OpaqueContextGuard, OpaqueRequestContext, RequestContextLike, TlsUninstall,
};
use verter_session::request_context::{
    current_request_context, RequestContext, RequestContextGuard,
};

/// Run `f` on a single-worker [`SchedulerIoPool`] with an optional
/// scheduler-facing context installed into TLS for the closure's
/// duration, blocking the calling thread until it returns. This mirrors
/// the production IO-worker dispatch closure (scheduler.rs
/// `dispatch_ready_job`): the worker installs the context via
/// `install_tls` (which, for the session impl, populates BOTH the
/// scheduler-side and session-side TLS slots) and drops the guard on
/// return. It replaces the retired `IoPool::submit_with_context` /
/// `IoHandle` harness with the surviving nonblocking `try_submit`
/// primitive plus a completion channel for the synchronous wait.
fn run_on_io_worker_with_context(
    context: Option<OpaqueRequestContext>,
    f: impl FnOnce() + Send + 'static,
) {
    let pool = SchedulerIoPool::new(1, 8);
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel::<()>(1);
    pool.try_submit(Box::new(move || {
        let _guard: Option<Box<dyn TlsUninstall + Send>> =
            context.map(|opaque| Arc::clone(&opaque.0).install_tls());
        f();
        let _ = done_tx.send(());
    }))
    .expect("single-worker IO pool accepts one task under capacity");
    done_rx.recv().expect("IO worker ran the task to completion");
}

/// Plan §6.12 sub-task 2 test 1 — strict discriminator.
///
/// Run a closure on the IO worker (mirroring the production IO dispatch
/// closure's `install_tls` step) wrapping a session-side
/// `RequestContext` as an `OpaqueRequestContext`. Inside the worker,
/// assert that `verter_session::request_context::current_request_context()`
/// returns `Some` carrying the same `request_id`.
///
/// Pre-Q: the worker installed only the scheduler-side TLS slot via
/// `OpaqueContextGuard::install` — session-side stayed `None`,
/// `current_request_context()` returned `None`, and the assertion would
/// fail.
///
/// Post-Q: the worker calls `Arc::clone(&opaque.0).install_tls()`. For
/// the session `RequestContext` impl this routes through
/// `RequestContextGuard::install`, which populates both slots.
#[test]
fn scheduler_worker_directly_sees_session_request_context_via_install_tls() {
    let session_ctx: Arc<RequestContext> = RequestContext::new(
        /* request_id */ 42,
        /* canonical_id */ Arc::from("/x.vue"),
        /* footprint_capture */ true,
        /* audit_accumulator */ None,
    );

    // Wrap as the scheduler-facing opaque carrier.
    let opaque = OpaqueRequestContext(Arc::clone(&session_ctx) as Arc<dyn RequestContextLike>);

    let observed_some = Arc::new(AtomicBool::new(false));
    let observed_id = Arc::new(AtomicU64::new(0));
    let observed_some_clone = Arc::clone(&observed_some);
    let observed_id_clone = Arc::clone(&observed_id);

    // Single-worker SchedulerIoPool — exercises the IO-worker
    // install_tls bridging the production dispatch closure performs.
    run_on_io_worker_with_context(Some(opaque), move || {
        // Inside the worker thread: session-side TLS must be populated
        // because the worker routes the context through `install_tls`.
        if let Some(ctx) = current_request_context() {
            observed_some_clone.store(true, Ordering::SeqCst);
            observed_id_clone.store(ctx.request_id, Ordering::SeqCst);
        }
    });

    assert!(
        observed_some.load(Ordering::SeqCst),
        "Worker thread must see session RequestContext via install_tls bridging on the IO pool"
    );
    assert_eq!(
        observed_id.load(Ordering::SeqCst),
        42,
        "session-side current_request_context must carry the wrapping RequestContext's request_id"
    );
}

/// Plan §6.12 sub-task 2 test 2 — regression for the rev-6 recursion bug.
///
/// `OpaqueContextGuard::install` must NOT route through `install_tls`
/// itself. The bidirectional chain is:
///
///   `RequestContextGuard::install` → `OpaqueContextGuard::install`
///
/// Modifying `OpaqueContextGuard::install` to chain back into
/// `install_tls` would create infinite recursion via the trait's
/// session-side impl. Option A keeps `OpaqueContextGuard::install`
/// unchanged.
///
/// This test installs an opaque context whose `install_tls` impl
/// directly delegates to `OpaqueContextGuard::install` (the scheduler
/// trait's TestCtx pattern). If `OpaqueContextGuard::install` had been
/// changed to recurse, this would stack-overflow before the assertion.
#[test]
fn opaque_context_guard_install_does_not_recurse() {
    struct TestCtx {
        id: u64,
        captures: bool,
        joined: AtomicU64,
    }

    impl RequestContextLike for TestCtx {
        fn request_id(&self) -> u64 {
            self.id
        }
        fn capture_enabled(&self) -> bool {
            self.captures
        }
        fn on_dedup_joiner(&self, _c: Arc<str>, _w: u64, _a: bool) {
            self.joined.fetch_add(1, Ordering::Relaxed);
        }
        fn record_cache_event(&self, _event: CacheEventKind) {}
        fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
            let guard = OpaqueContextGuard::install(OpaqueRequestContext(
                self as Arc<dyn RequestContextLike>,
            ));
            Box::new(GuardBox(guard))
        }
    }

    struct GuardBox(#[allow(dead_code)] OpaqueContextGuard);
    impl TlsUninstall for GuardBox {
        fn uninstall(self: Box<Self>) {}
    }

    let ctx = Arc::new(TestCtx {
        id: 84,
        captures: false,
        joined: AtomicU64::new(0),
    });

    // Direct OpaqueContextGuard::install call — must not recurse.
    let _guard = OpaqueContextGuard::install(OpaqueRequestContext(
        Arc::clone(&ctx) as Arc<dyn RequestContextLike>
    ));

    // If install had recursed, control would never reach this point.
    assert_eq!(
        verter_scheduler::request_context::current_request_id(),
        Some(84),
        "scheduler-side TLS slot must hold the installed context's request id",
    );
}

/// Plan §6.12 sub-task 2 test 3 — end-to-end through the scheduler.
///
/// Drives a `Scheduler::submit_request` directly with a session-side
/// `RequestContext` wrapped as `OpaqueRequestContext`. The scheduler
/// dispatches the source-load stage on its `SchedulerIoPool` and the
/// analysis/artifact stages on its `SchedulerCpuPool`. Each worker
/// thread runs the dispatch closure's `install_tls` call, which routes
/// through the session trait impl and populates BOTH TLS slots.
///
/// A custom `StageExecutor` probes `verter_session::request_context::
/// current_request_context()` inside each stage and records the
/// observed `request_id` into shared atomics. The test thread then
/// asserts every stage observed the expected id (non-zero).
///
/// Pre-Q: stages observed `None` for session-side TLS because
/// `OpaqueContextGuard::install` only populated the scheduler-side
/// slot. Post-Q: every stage observes `Some(7)` matching the wrapping
/// session `RequestContext.request_id`.
#[test]
fn scheduler_winner_thread_propagates_session_context_via_install_tls() {
    use verter_scheduler::executor::{StageError, StageExecutor};
    use verter_scheduler::node::{
        AnalysisSnapshot, ArtifactSnapshot, FileKind as SchedFileKind, SourceSnapshot,
    };
    use verter_scheduler::scheduler::{Request, Scheduler, SchedulerConfig};
    use verter_scheduler::source_loader::{MemorySourceLoader, SourceLoader};
    use verter_scheduler::stage::{Priority, TargetStage};

    /// Probe executor that records `current_request_context()`'s
    /// `request_id` in shared atomics — proving session-side TLS is
    /// populated on the worker thread running each stage.
    struct SessionProbeExecutor {
        source_observed: Arc<AtomicU64>,
        analysis_observed: Arc<AtomicU64>,
        artifact_observed: Arc<AtomicU64>,
    }

    impl StageExecutor for SessionProbeExecutor {
        fn execute_source(
            &self,
            _canonical_id: &str,
            _file_kind: SchedFileKind,
            content: Arc<str>,
            generation: u64,
        ) -> Result<SourceSnapshot, StageError> {
            let id = current_request_context().map_or(0, |ctx| ctx.request_id);
            self.source_observed.store(id, Ordering::SeqCst);
            Ok(SourceSnapshot::new_empty(content, generation))
        }

        fn execute_analysis(
            &self,
            _canonical_id: &str,
            _source: &SourceSnapshot,
            generation: u64,
        ) -> Result<AnalysisSnapshot, StageError> {
            let id = current_request_context().map_or(0, |ctx| ctx.request_id);
            self.analysis_observed.store(id, Ordering::SeqCst);
            Ok(AnalysisSnapshot::new_empty(generation))
        }

        fn execute_artifact(
            &self,
            _canonical_id: &str,
            _source: &SourceSnapshot,
            _analysis: &AnalysisSnapshot,
            profile_hash: u64,
            generation: u64,
        ) -> Result<ArtifactSnapshot, StageError> {
            let id = current_request_context().map_or(0, |ctx| ctx.request_id);
            self.artifact_observed.store(id, Ordering::SeqCst);
            Ok(ArtifactSnapshot {
                generation,
                profile_hash,
                data: Arc::new(verter_scheduler::node::EmptyData),
            })
        }
    }

    let source_observed = Arc::new(AtomicU64::new(0));
    let analysis_observed = Arc::new(AtomicU64::new(0));
    let artifact_observed = Arc::new(AtomicU64::new(0));
    let probe = Arc::new(SessionProbeExecutor {
        source_observed: Arc::clone(&source_observed),
        analysis_observed: Arc::clone(&analysis_observed),
        artifact_observed: Arc::clone(&artifact_observed),
    });

    // Wire a real `Scheduler` with worker threads — exercises the
    // production winner-thread dispatch sites at scheduler.rs:1262/1293.
    let loader: Arc<dyn SourceLoader> = Arc::new(MemorySourceLoader::new());
    let sched = Arc::new(Scheduler::test_with_executor(
        SchedulerConfig::default(),
        loader,
        probe as Arc<dyn StageExecutor>,
    ));

    // Build a session-side RequestContext and wrap as the scheduler-
    // facing opaque carrier.
    let session_ctx: Arc<RequestContext> =
        RequestContext::new(7, Arc::from("/probe.vue"), true, None);
    let opaque = OpaqueRequestContext(Arc::clone(&session_ctx) as Arc<dyn RequestContextLike>);

    // Submit a request that drives source → analysis → artifact through
    // the worker pools.
    let handle = sched.submit_request(Request {
        file_id: "/probe.vue".to_string(),
        target: TargetStage::Artifact { profile_hash: 1 },
        priority: Priority::Interactive,
        source: Some(Arc::from("<template>x</template>")),
        file_kind: None,
        request_context: Some(opaque),
    });
    handle.wait();

    // Strict discriminating assertions: pre-Q every observed value
    // would be 0 because session-side TLS was not installed on the
    // worker. Post-Q all three stages see the wrapping context's id.
    assert_eq!(
        source_observed.load(Ordering::SeqCst),
        7,
        "execute_source worker thread must see session TLS via install_tls bridging at scheduler.rs:1262",
    );
    assert_eq!(
        analysis_observed.load(Ordering::SeqCst),
        7,
        "execute_analysis worker thread must see session TLS via install_tls bridging at scheduler.rs:1293",
    );
    assert_eq!(
        artifact_observed.load(Ordering::SeqCst),
        7,
        "execute_artifact worker thread must see session TLS via install_tls bridging at scheduler.rs:1293",
    );
}

/// Negative-control companion to test 1: when the IO worker runs
/// WITHOUT a context, session-side `current_request_context()` must be
/// `None`. Confirms the install_tls bridging is opt-in via the
/// `Some(opaque)` argument and not a side-effect of running on the pool.
#[test]
fn scheduler_worker_without_context_observes_no_session_context() {
    let observed_some = Arc::new(AtomicBool::new(false));
    let observed_some_clone = Arc::clone(&observed_some);

    run_on_io_worker_with_context(None, move || {
        observed_some_clone.store(current_request_context().is_some(), Ordering::SeqCst);
    });

    assert!(
        !observed_some.load(Ordering::SeqCst),
        "running on the IO worker with no context must NOT install a session context",
    );
}

/// Outer-thread → worker propagation: install a session-side
/// `RequestContextGuard` on the test thread, capture the active
/// scheduler-side `OpaqueRequestContext` via
/// `verter_scheduler::request_context::current_context()`, hand it to
/// the worker via the `run_on_io_worker_with_context` harness, and
/// assert the worker sees the SAME `request_id` on both the
/// scheduler-side and session-side TLS slots. Mirrors the production
/// scheduler dispatch pattern (winner_ctx is fetched via
/// `current_context()` at submission time and re-installed inside the
/// worker via the install_tls path).
#[test]
fn outer_session_guard_propagates_through_pool_submission() {
    let outer_ctx: Arc<RequestContext> = RequestContext::new(7, Arc::from("/o.vue"), true, None);
    let _outer_guard = RequestContextGuard::install(Arc::clone(&outer_ctx));

    // Capture the now-active scheduler-side opaque context, just like
    // the scheduler's `winner_ctx.or_else(...)` plumbing does.
    let opaque = verter_scheduler::request_context::current_context()
        .expect("RequestContextGuard::install populates the scheduler TLS slot");

    let observed_session_id = Arc::new(AtomicU64::new(0));
    let observed_scheduler_id = Arc::new(AtomicU64::new(0));
    let session_clone = Arc::clone(&observed_session_id);
    let scheduler_clone = Arc::clone(&observed_scheduler_id);

    run_on_io_worker_with_context(Some(opaque), move || {
        if let Some(ctx) = current_request_context() {
            session_clone.store(ctx.request_id, Ordering::SeqCst);
        }
        if let Some(id) = verter_scheduler::request_context::current_request_id() {
            scheduler_clone.store(id, Ordering::SeqCst);
        }
    });

    assert_eq!(
        observed_session_id.load(Ordering::SeqCst),
        7,
        "session-side current_request_context() must observe the outer guard's id on the worker",
    );
    assert_eq!(
        observed_scheduler_id.load(Ordering::SeqCst),
        7,
        "scheduler-side current_request_id() must observe the outer guard's id on the worker",
    );
}
