//! Wave 2 Slice 2.3 — `SchedulerAudit::queue_dwell_ms` discriminating
//! coverage.
//!
//! Drives 16 concurrent submissions through the real scheduler with
//! a session-side [`verter_session::request_context::RequestContext`]
//! attached to each. A source-stage gate ([`SourceGate`]) holds all
//! source-capable workers until the surplus requests are provably
//! parked in the priority queue, so those entries SIT between enqueue
//! and dispatch deterministically (not by timing luck). The dispatch
//! site captures `(dequeue_at - entry.enqueue_time)` and publishes it
//! via `AuditObserver::record_scheduler_dispatch`. At least one
//! captured `queue_dwell_ms` must be strictly positive.
//!
//! Discriminating: pre-Slice-2.3 the `record_scheduler_dispatch`
//! observer hook does not exist; the per-request scheduler_audit
//! slot is never populated; the assertion below would fail because
//! every slot is `None`. A naive stub that filled the slot but
//! always wrote `0.0` would also fail because zero is not
//! strictly-positive.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
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

/// Deterministic rendezvous between the test driver and the
/// source-stage workers. Replaces the old fixed per-stage `sleep`s,
/// which only *probabilistically* created queue contention.
///
/// Each source execution increments `entered` and then blocks until the
/// driver flips `released`. The driver holds the gate closed until
/// [`POOL_THREADS`] workers are parked inside it — at which point all
/// source-capable workers are occupied and the surplus requests
/// ([`REQUESTS`] − [`POOL_THREADS`]) are guaranteed to be sitting in the
/// queue, accumulating real dwell. Releasing the gate then dispatches
/// the queued entries whose first-dispatch (source-stage) `queue_dwell_ms`
/// is therefore strictly positive *by construction*, not by timing luck.
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

    // Submit ALL requests in parallel. Each uses a fresh session
    // context so the `scheduler_audit` slot is independently observable.
    let handles: Vec<_> = (0..REQUESTS)
        .map(|i| {
            let sched = Arc::clone(&sched);
            let ctx = Arc::clone(&contexts[i]);
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
                h.wait();
            })
        })
        .collect();

    // Deterministically force the queue-dwell window: block until
    // POOL_THREADS source workers are parked inside the gate. At that
    // point every source-capable worker is occupied, so the remaining
    // REQUESTS - POOL_THREADS submissions are provably sitting in the
    // priority queue, accumulating real dwell. Only then release the
    // gate so those queued entries dispatch with a strictly-positive
    // source-stage `queue_dwell_ms`.
    gate.wait_until_entered(POOL_THREADS);
    gate.release();

    for h in handles {
        h.join().expect("worker joined");
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

    // At LEAST ONE captured dwell must be strictly positive. The gate
    // held all source-capable workers until the surplus requests were
    // provably queued, so their first-dispatch (source-stage)
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
