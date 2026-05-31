//! Wave 2 Slice 2.3 — `SchedulerAudit::queue_dwell_ms` discriminating
//! coverage.
//!
//! Drives 16 concurrent submissions through the real scheduler with
//! a session-side [`verter_session::request_context::RequestContext`]
//! attached to each. Under contention some entries SIT in the
//! priority queue between enqueue and dispatch; the dispatch site
//! captures `(dequeue_at - entry.enqueue_time)` and publishes it via
//! `AuditObserver::record_scheduler_dispatch`. At least one
//! captured `queue_dwell_ms` must be strictly positive.
//!
//! Discriminating: pre-Slice-2.3 the `record_scheduler_dispatch`
//! observer hook does not exist; the per-request scheduler_audit
//! slot is never populated; the assertion below would fail because
//! every slot is `None`. A naive stub that filled the slot but
//! always wrote `0.0` would also fail because zero is not
//! strictly-positive.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;
use std::thread;

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

/// Slow-on-purpose executor: every stage sleeps a few millis so the
/// driver's dispatch loop has to enqueue multiple jobs before the
/// pool drains them. That guarantees at least one entry observes a
/// non-zero queue dwell.
struct SlowExecutor;

impl StageExecutor for SlowExecutor {
    fn execute_source(
        &self,
        _canonical_id: &str,
        _file_kind: SchedFileKind,
        content: Arc<str>,
        generation: u64,
    ) -> Result<SourceSnapshot, StageError> {
        std::thread::sleep(std::time::Duration::from_millis(15));
        Ok(SourceSnapshot::new_empty(content, generation))
    }
    fn execute_analysis(
        &self,
        _canonical_id: &str,
        _source: &SourceSnapshot,
        generation: u64,
    ) -> Result<AnalysisSnapshot, StageError> {
        std::thread::sleep(std::time::Duration::from_millis(5));
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
        std::thread::sleep(std::time::Duration::from_millis(2));
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
    let executor: Arc<dyn StageExecutor> = Arc::new(SlowExecutor);
    // Build a constrained scheduler so contention is high — small
    // pools mean entries queue up before the workers can pick them
    // up.
    let config = SchedulerConfig {
        cpu_threads: 2,
        io_threads: 2,
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

    // At LEAST ONE captured dwell must be strictly positive. With a
    // 2-thread CPU pool and 16 requests sleeping 15ms in source +
    // 5ms in analysis, contention is guaranteed.
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
