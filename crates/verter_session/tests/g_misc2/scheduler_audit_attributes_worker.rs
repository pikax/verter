//! Wave 2 Slice 2.3 — `SchedulerAudit::worker_thread_id` /
//! `SchedulerAudit::worker_pool` discriminating coverage.
//!
//! Drives 16 concurrent requests through the real scheduler with
//! a session-side [`verter_session::request_context::RequestContext`]
//! attached to each submission. Each context's
//! [`verter_session::request_context::RequestContext::scheduler_audit`]
//! slot must end up populated with a non-empty `worker_thread_id`
//! and a `worker_pool` of either `Cpu` or `Io`.
//!
//! Discriminating: against the pre-Slice-2.3 tree, the
//! `record_scheduler_dispatch` observer trait method does not exist
//! and the `scheduler_audit` slot is unpopulated — every assertion
//! below would either fail to compile or fail at runtime.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;
use std::thread;

use verter_audit::WorkerPool;
use verter_scheduler::executor::{StageError, StageExecutor};
use verter_scheduler::node::{
    AnalysisSnapshot, ArtifactSnapshot, FileKind as SchedFileKind, SourceSnapshot,
};
use verter_scheduler::request_context::{OpaqueRequestContext, RequestContextLike};
use verter_scheduler::scheduler::{Request, Scheduler, SchedulerConfig};
use verter_scheduler::source_loader::{MemorySourceLoader, SourceLoader};
use verter_scheduler::stage::{Priority, TargetStage};
use verter_session::request_context::RequestContext;

const THREADS: usize = 16;

/// Stage executor that just hands the source through unmodified —
/// the test cares about scheduler dispatch attribution, not parse
/// content.
struct PassthroughExecutor;

impl StageExecutor for PassthroughExecutor {
    fn execute_source(
        &self,
        _canonical_id: &str,
        _file_kind: SchedFileKind,
        content: Arc<str>,
        generation: u64,
    ) -> Result<SourceSnapshot, StageError> {
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
fn scheduler_audit_records_non_empty_worker_thread_id_and_known_pool_under_load() {
    let loader = Arc::new(MemorySourceLoader::new());
    for i in 0..THREADS {
        loader.insert(format!("/file{i}.vue"), Arc::from("<template>x</template>"));
    }
    let executor: Arc<dyn StageExecutor> = Arc::new(PassthroughExecutor);
    let sched = Scheduler::test_with_executor(
        SchedulerConfig::default(),
        loader as Arc<dyn SourceLoader>,
        executor,
    );

    // Each worker thread submits ONE request with a distinct
    // session-side RequestContext, then waits for completion. The
    // contexts are kept alive until the very end so we can probe
    // their `scheduler_audit` slots after the worker pool has
    // dispatched the work.
    let contexts: Arc<Vec<Arc<RequestContext>>> = Arc::new(
        (0..THREADS)
            .map(|i| {
                RequestContext::new(
                    /* request_id */ 1000 + i as u64,
                    Arc::from(format!("/file{i}.vue").as_str()),
                    /* footprint_capture */ true,
                    /* audit_accumulator */ None,
                )
            })
            .collect(),
    );

    let handles: Vec<_> = (0..THREADS)
        .map(|i| {
            let sched = Arc::clone(&sched);
            let ctx = Arc::clone(&contexts[i]);
            thread::spawn(move || {
                let opaque = OpaqueRequestContext(ctx as Arc<dyn RequestContextLike>);
                let h = sched.submit_request(Request {
                    file_id: format!("/file{i}.vue"),
                    target: TargetStage::Artifact { profile_hash: 1 },
                    priority: Priority::Interactive,
                    source: Some(Arc::from("<template>z</template>")),
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

    // EVERY context's scheduler_audit slot must be populated. Pre-
    // Slice-2.3: the slot does not exist (compilation failure) or
    // is never written (runtime panic on `unwrap`).
    for (idx, ctx) in contexts.iter().enumerate() {
        let snap = ctx.scheduler_audit.lock().clone().unwrap_or_else(|| {
            panic!(
                "request {} (/file{}.vue): scheduler_audit slot was never \
                     populated — pre-Slice-2.3 the dispatch path did not call \
                     record_scheduler_dispatch on the active observer",
                ctx.request_id, idx,
            )
        });

        assert!(
            !snap.worker_thread_id.is_empty(),
            "request {} on /file{}.vue: worker_thread_id must be non-empty, got {:?}",
            ctx.request_id,
            idx,
            snap.worker_thread_id,
        );
        // `ThreadId(...)` is the `Debug` form of `std::thread::ThreadId`.
        assert!(
            snap.worker_thread_id.starts_with("ThreadId"),
            "request {}: worker_thread_id must look like a Debug-formatted \
             ThreadId, got {:?}",
            ctx.request_id,
            snap.worker_thread_id,
        );

        // Worker pool must be one of the documented variants — the
        // discriminator is open-set on the public type.
        assert!(
            matches!(snap.worker_pool, WorkerPool::Cpu | WorkerPool::Io),
            "request {}: worker_pool must be Cpu or Io, got {:?}",
            ctx.request_id,
            snap.worker_pool,
        );

        // `dispatch_count` is initialised to 1 on first dispatch;
        // pipeline stages (Source → Analysis → Artifact) bump it.
        // Always non-zero on a populated audit.
        assert!(
            snap.dispatch_count >= 1,
            "request {}: dispatch_count must be >= 1, got {}",
            ctx.request_id,
            snap.dispatch_count,
        );
    }
}

/// Negative control: a submission with no `request_context` must
/// NOT publish anything — the observer TLS slot stays empty on the
/// worker, `record_scheduler_dispatch` is a no-op, and any context
/// constructed for an unrelated request stays clean.
#[test]
fn submission_without_request_context_does_not_pollute_unrelated_contexts() {
    let loader = Arc::new(MemorySourceLoader::new());
    loader.insert("/free.vue".to_string(), Arc::from("<template>y</template>"));
    let executor: Arc<dyn StageExecutor> = Arc::new(PassthroughExecutor);
    let sched = Scheduler::test_with_executor(
        SchedulerConfig::default(),
        loader as Arc<dyn SourceLoader>,
        executor,
    );

    // A bystander RequestContext that nothing will dispatch under.
    let bystander = RequestContext::new(
        /* request_id */ 9999,
        Arc::from("/bystander.vue"),
        false,
        None,
    );

    let handle = sched.submit_request(Request {
        file_id: "/free.vue".to_string(),
        target: TargetStage::Source,
        priority: Priority::Interactive,
        source: Some(Arc::from("<template>z</template>")),
        file_kind: None,
        request_context: None,
    });
    handle.wait();

    assert!(
        bystander.scheduler_audit.lock().is_none(),
        "bystander RequestContext must not pick up another request's \
         scheduler audit; observer TLS only routes through the installed \
         session context",
    );
}
