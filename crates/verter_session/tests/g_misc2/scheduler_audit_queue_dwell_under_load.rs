//! Wave 2 Slice 2.3 — `SchedulerAudit::queue_dwell_ms` discriminating
//! coverage.
//!
//! Drives 16 concurrent submissions through the real scheduler with
//! a session-side [`verter_session::request_context::RequestContext`]
//! attached to each. The rendezvous lives at the scheduler DISPATCH
//! site, so the assertion reflects real SCHEDULER-PRIORITY-QUEUE dwell
//! — not pool-channel time:
//!
//! 1. An ALL-SUBMITTED barrier guarantees every submitter has returned
//!    from `submit_request` (so all 16 submissions are at least in the
//!    inbox channel) before the test inspects anything.
//! 2. The scheduler's test-only dispatch pause (armed at
//!    `POOL_THREADS`) parks the driver after exactly `POOL_THREADS`
//!    source dispatches, BEFORE the next dequeue. While parked, the
//!    driver re-drains the inbox so every surplus submission provably
//!    lands in `job_index`.
//! 3. The test waits until the driver is parked, then polls
//!    `Scheduler::test_job_queue_depth` until the surplus
//!    (`REQUESTS - POOL_THREADS`) is provably SITTING in the scheduler
//!    queue, and only THEN releases both the dispatch pause and the
//!    source gate (which held the two dispatched workers stable during
//!    inspection).
//!
//! The surplus entries are dequeued only after the test held them in
//! the queue, so their first-dispatch (source-stage)
//! `queue_dwell_ms = dequeue_at - entry.enqueue_time` is strictly
//! positive BY CONSTRUCTION, not by timing luck. The dispatch site
//! captures that value and publishes it via
//! `AuditObserver::record_scheduler_dispatch`.
//!
//! Discriminating: pre-Slice-2.3 the `record_scheduler_dispatch`
//! observer hook does not exist; the per-request scheduler_audit
//! slot is never populated; the assertion below would fail because
//! every slot is `None`. A naive stub that filled the slot but
//! always wrote `0.0` would also fail because zero is not
//! strictly-positive.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use verter_scheduler::executor::{StageError, StageExecutor};
use verter_scheduler::node::{
    AnalysisSnapshot, ArtifactSnapshot, FileKind as SchedFileKind, SourceSnapshot,
};
use verter_scheduler::request_context::{OpaqueRequestContext, RequestContextLike};
use verter_scheduler::scheduler::{Request, Scheduler, SchedulerConfig};
use verter_scheduler::source_loader::{MemorySourceLoader, SourceLoader};
use verter_scheduler::stage::{Priority, TargetStage};
use verter_session::request_context::RequestContext;

const REQUESTS: usize = 16;
/// Both scheduler pools are sized to this in the test, so at most this
/// many source-stage jobs can run concurrently. Once this many workers
/// have entered the gated source stage, every remaining submitted
/// request is provably parked in the priority queue.
const POOL_THREADS: usize = 2;

/// Holds the source-stage workers that the driver dispatches before it
/// parks at the dispatch pause, so the scheduler state stays stable
/// while the test inspects the priority-queue depth.
///
/// Each source execution increments `entered` and then blocks until the
/// driver flips `released`. Crucially, this gate does NOT prove dwell on
/// its own — entering `execute_source` only proves a worker left the
/// queue, not that the surplus is parked IN the queue. The dwell
/// guarantee comes from the scheduler dispatch pause (see the module
/// doc): the gate's only remaining job is to keep the [`POOL_THREADS`]
/// dispatched workers from completing (and enqueuing follow-on analysis
/// jobs) while the test observes that the surplus
/// ([`REQUESTS`] − [`POOL_THREADS`]) is sitting in `job_index`. The gate
/// is released together with the dispatch pause once that observation is
/// made.
struct SourceGate {
    entered: AtomicUsize,
    released: Mutex<bool>,
    release_cv: Condvar,
}

impl SourceGate {
    fn new() -> Self {
        Self {
            entered: AtomicUsize::new(0),
            released: Mutex::new(false),
            release_cv: Condvar::new(),
        }
    }

    /// Worker side: record arrival, then block until the driver
    /// releases the gate.
    fn enter_and_wait(&self) {
        self.entered.fetch_add(1, Ordering::SeqCst);
        let mut released = self.released.lock().expect("source gate mutex");
        while !*released {
            released = self
                .release_cv
                .wait(released)
                .expect("source gate condvar wait");
        }
    }

    /// Driver side: block (bounded) until at least `n` workers have
    /// entered the gated source stage. Panics on timeout so a genuine
    /// stall fails loudly instead of hanging the suite.
    fn wait_until_entered(&self, n: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.entered.load(Ordering::SeqCst) < n {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {n} source-stage workers to enter the \
                 gate (only {} entered); the surplus requests cannot queue",
                self.entered.load(Ordering::SeqCst),
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Driver side: release every parked (and future) source execution.
    fn release(&self) {
        let mut released = self.released.lock().expect("source gate mutex");
        *released = true;
        self.release_cv.notify_all();
    }
}

/// Executor that gates the source stage on a [`SourceGate`] so the
/// driver can deterministically force a queue-dwell window. Analysis
/// and artifact stages are pass-through (they run after the gate opens
/// and are not the stage whose dwell the test inspects — the
/// per-request `scheduler_audit` slot captures the *first* dispatch,
/// which is the source stage).
struct GatedSourceExecutor {
    gate: Arc<SourceGate>,
}

impl StageExecutor for GatedSourceExecutor {
    fn execute_source(
        &self,
        _canonical_id: &str,
        _file_kind: SchedFileKind,
        content: Arc<str>,
        generation: u64,
    ) -> Result<SourceSnapshot, StageError> {
        self.gate.enter_and_wait();
        Ok(SourceSnapshot::new_empty(content, generation))
    }
    fn execute_analysis(
        &self,
        _canonical_id: &str,
        _source: &SourceSnapshot,
        generation: u64,
    ) -> Result<AnalysisSnapshot, StageError> {
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
        Ok(ArtifactSnapshot {
            generation,
            profile_hash,
            data: Arc::new(verter_scheduler::node::EmptyData),
        })
    }
}

/// Join `handle`, panicking if it does not complete within `timeout`.
///
/// The join runs on a helper thread that reports its outcome through a
/// rendezvous channel; the caller blocks on `recv_timeout`. A genuinely
/// stuck worker never reports, so the `recv_timeout` elapses and we
/// PANIC with `label` — a hang surfaces as a loud failure within the
/// deadline instead of blocking the suite forever (a bare
/// `handle.join()` would itself hang on a real deadlock).
fn join_within(handle: thread::JoinHandle<()>, timeout: Duration, label: &str) {
    let (tx, rx) = std::sync::mpsc::sync_channel::<thread::Result<()>>(1);
    thread::spawn(move || {
        // `send` fails only if the receiver was dropped (caller already
        // panicked on timeout); ignore so this helper thread does not
        // itself panic on a benign disconnect.
        let _ = tx.send(handle.join());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(())) => {}
        Ok(Err(_)) => panic!("{label} panicked"),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("{label} deadlocked (join did not complete within {timeout:?})")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{label} watchdog channel disconnected before reporting")
        }
    }
}

#[test]
fn at_least_one_concurrent_request_observes_non_zero_queue_dwell_ms() {
    let loader = Arc::new(MemorySourceLoader::new());
    for i in 0..REQUESTS {
        loader.insert(format!("/file{i}.vue"), Arc::from("<template>x</template>"));
    }
    let gate = Arc::new(SourceGate::new());
    let executor: Arc<dyn StageExecutor> = Arc::new(GatedSourceExecutor {
        gate: Arc::clone(&gate),
    });
    // Build a constrained scheduler so contention is high — small
    // pools mean entries queue up before the workers can pick them
    // up. Both pools are sized to POOL_THREADS so source-stage
    // concurrency is capped at POOL_THREADS regardless of pool routing.
    let config = SchedulerConfig {
        cpu_threads: POOL_THREADS,
        io_threads: POOL_THREADS,
        ..SchedulerConfig::default()
    };
    let sched = Scheduler::with_executor(config, loader as Arc<dyn SourceLoader>, executor);

    let contexts: Arc<Vec<Arc<RequestContext>>> = Arc::new(
        (0..REQUESTS)
            .map(|i| {
                RequestContext::new(
                    /* request_id */ 2000 + i as u64,
                    Arc::from(format!("/file{i}.vue").as_str()),
                    /* footprint_capture */ true,
                    None,
                )
            })
            .collect(),
    );

    // Arm the dispatch pause BEFORE submitting: the driver will park
    // after exactly POOL_THREADS source dispatches, before the next
    // dequeue, and re-drain the inbox so the surplus accrues real
    // scheduler-queue dwell.
    sched.test_arm_dispatch_pause(POOL_THREADS);

    // All-submitted barrier: REQUESTS submitter threads + this driver
    // thread. Every submitter signals after `submit_request` returns, so
    // once the barrier trips all REQUESTS submissions are at least in the
    // inbox channel — the re-drain inside the dispatch pause is then
    // guaranteed to pull every one of them into `job_index`.
    let all_submitted = Arc::new(Barrier::new(REQUESTS + 1));

    // Submit ALL requests in parallel. Each uses a fresh session
    // context so the `scheduler_audit` slot is independently observable.
    let handles: Vec<_> = (0..REQUESTS)
        .map(|i| {
            let sched = Arc::clone(&sched);
            let ctx = Arc::clone(&contexts[i]);
            let all_submitted = Arc::clone(&all_submitted);
            thread::spawn(move || {
                let opaque = OpaqueRequestContext(ctx as Arc<dyn RequestContextLike>);
                let h = sched.submit_request(Request {
                    file_id: format!("/file{i}.vue"),
                    target: TargetStage::Artifact { profile_hash: 1 },
                    priority: Priority::Interactive,
                    source: Some(Arc::from("<template>y</template>")),
                    file_kind: None,
                    request_context: Some(opaque),
                });
                // Signal that this submission has entered the inbox, then
                // block on completion (which only happens after the test
                // releases the dispatch pause + the source gate).
                all_submitted.wait();
                h.wait();
            })
        })
        .collect();

    // Step 1: wait until every submitter has returned from
    // `submit_request`.
    all_submitted.wait();

    // Step 2: confirm POOL_THREADS source jobs were actually dispatched
    // (their workers entered the gate) — a cross-check that the pause
    // fired after real source dispatches, not before any.
    gate.wait_until_entered(POOL_THREADS);

    // Step 3: wait until the driver has parked at the dispatch pause
    // (after POOL_THREADS dispatches, before the next dequeue). While
    // parked it re-drains the inbox so the surplus lands in `job_index`.
    sched.test_wait_until_dispatch_paused();

    // Step 4: confirm the surplus (REQUESTS - POOL_THREADS) is PROVABLY
    // sitting in the scheduler priority queue before releasing. Bounded
    // poll; panic on stall so a logic error fails loudly. This is the
    // soundness core: the assertion later only holds because these
    // entries demonstrably accrued scheduler-queue dwell during the
    // pause window.
    let want_surplus = REQUESTS - POOL_THREADS;
    let depth_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let depth = sched.test_job_queue_depth();
        if depth >= want_surplus {
            break;
        }
        assert!(
            Instant::now() < depth_deadline,
            "surplus never reached the scheduler priority queue: observed \
             depth {depth}, want >= {want_surplus} (REQUESTS {REQUESTS} - \
             POOL_THREADS {POOL_THREADS})",
        );
        thread::sleep(Duration::from_millis(1));
    }

    // Step 5: release the gate FIRST (so the dispatched + surplus source
    // workers can run freely), then release the dispatch pause so the
    // driver dequeues the surplus — each with a strictly-positive
    // source-stage `queue_dwell_ms` because it sat in the queue for the
    // whole pause window.
    gate.release();
    sched.test_release_dispatch_pause();

    // Bounded join: each submitter completes once its request finishes
    // (which requires the pause + gate to have been released). A genuine
    // deadlock PANICS within ~10s rather than hanging the suite — this is
    // also what makes the FIX-2 bound provable: with the release removed,
    // the submitters never complete and this join surfaces the stall.
    for (idx, h) in handles.into_iter().enumerate() {
        join_within(
            h,
            std::time::Duration::from_secs(10),
            &format!("submitter {idx}"),
        );
    }

    // EVERY context's scheduler_audit slot must be populated.
    for (idx, ctx) in contexts.iter().enumerate() {
        assert!(
            ctx.scheduler_audit.lock().is_some(),
            "request {} on /file{}.vue: scheduler_audit slot must be populated, \
             pre-Slice-2.3 the slot does not exist or is never written",
            ctx.request_id,
            idx,
        );
    }

    // At LEAST ONE captured dwell must be strictly positive. The
    // dispatch pause held the surplus requests in the scheduler priority
    // queue (confirmed via test_job_queue_depth) for the whole pause
    // window before releasing, so their first-dispatch (source-stage)
    // queue_dwell_ms is positive by construction — not by timing luck.
    let max_dwell = contexts
        .iter()
        .filter_map(|ctx| ctx.scheduler_audit.lock().clone())
        .map(|s| s.queue_dwell_ms)
        .fold(0.0_f64, f64::max);

    assert!(
        max_dwell > 0.0,
        "at least one of {} concurrent requests must observe a strictly \
         positive queue_dwell_ms; max observed = {} ms. \
         Pre-Slice-2.3 the field does not exist; a stub that always wrote \
         0.0 would also fail this assertion.",
        REQUESTS,
        max_dwell,
    );

    // Sanity: dwell must never be negative (Instant::saturating_sub
    // floors at zero).
    for (idx, ctx) in contexts.iter().enumerate() {
        if let Some(snap) = ctx.scheduler_audit.lock().clone() {
            assert!(
                snap.queue_dwell_ms >= 0.0,
                "request {} on /file{}.vue: queue_dwell_ms must be non-negative, got {}",
                ctx.request_id,
                idx,
                snap.queue_dwell_ms,
            );
        }
    }
}
