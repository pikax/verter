//! Integration discriminator for the cross-crate inline-execute
//! None-`winner_ctx` clear path.
//!
//! The scheduler-side unit test
//! `inline_execute_clears_outer_tls_when_winner_ctx_is_none` uses a
//! scheduler-only `TestContext`, so the outer scope never installs
//! the session-side `CURRENT_REQUEST_CONTEXT`, `CURRENT_ACCUMULATOR`,
//! or `verter_audit::current_observer()` slots. That test passes
//! whether or not the inline-execute path invokes the registered
//! cross-crate clear hook — the cross-crate slots are None going in.
//!
//! This integration test installs a REAL session-side
//! `RequestContextGuard` in the outer scope (so all three TLS slots
//! are populated by the host's install path), then drives the
//! scheduler's inline-execute path with `winner_ctx = None` and
//! asserts the inner stage observes `None` on every install_tls
//! slot:
//!
//! - `verter_session::request_context::current_request_context()`
//! - `verter_session::request_context::current_accumulator()`
//! - `verter_audit::current_observer()`
//!
//! Discriminator: if the scheduler-side inline-execute path reverts
//! to clearing only the scheduler opaque slot (i.e., the cross-crate
//! hook stops firing), the outer's session + audit TLS bleeds into
//! the inner stage and the assertions below trip.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering as MOrd};
use std::sync::Arc;

use verter_audit::current_observer;
use verter_scheduler::caller_kind::CallerKind;
use verter_scheduler::executor::{StageError, StageExecutor};
use verter_scheduler::job::CompletionState;
use verter_scheduler::node::{AnalysisSnapshot, FileKind as SchedFileKind, SourceSnapshot};
use verter_scheduler::request_context::{OpaqueRequestContext, RequestContextLike};
use verter_scheduler::scheduler::{Request, Scheduler, SchedulerConfig};
use verter_scheduler::source_loader::{MemorySourceLoader, SourceLoader};
use verter_scheduler::stage::{Priority, TargetStage};
use verter_session::component_meta_audit::accumulator::RequestFootprintAccumulator;
use verter_session::request_context::{
    current_accumulator, current_request_context, install_clear_tls_hook, RequestContext,
    RequestContextGuard,
};

const OUTER_ID: u64 = 9091;

/// `StageExecutor` whose Analysis hook re-enters the scheduler from
/// the outer canonical and records TLS observations from the inner
/// canonical.
struct InlineReentryExecutor {
    analysis_hook: Box<dyn Fn(&str) + Send + Sync>,
}

impl StageExecutor for InlineReentryExecutor {
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
        canonical_id: &str,
        _source: &SourceSnapshot,
        generation: u64,
    ) -> Result<AnalysisSnapshot, StageError> {
        (self.analysis_hook)(canonical_id);
        Ok(AnalysisSnapshot::new_empty(generation))
    }
}

/// Outer Analysis is dispatched with a real
/// `verter_session::RequestContext` installed in TLS via
/// `RequestContextGuard::install`. Inside the outer hook the test
/// submits a NEW inner Analysis with `request_context: None` and
/// calls `wait_or_drive_with_caller(.., CpuWorker)`. The scheduler
/// inline-executes the inner stage on the same CPU worker, taking
/// the None-`winner_ctx` branch through `AllSlotsClearGuard`.
///
/// The inner hook records what each install_tls slot held at the
/// moment of execution. Every slot must be empty — if any slot
/// shows the outer's payload, the cross-crate clear path is broken.
#[test]
fn inline_execute_none_winner_ctx_clears_session_and_audit_slots_too() {
    // Idempotent — host construction normally does this, but this
    // test bypasses `VerterHost` so we register the hook directly.
    install_clear_tls_hook();

    let loader = Arc::new(MemorySourceLoader::new());
    loader.insert("/a.vue".to_string(), Arc::from("<template>a</template>"));
    loader.insert("/b.vue".to_string(), Arc::from("<template>b</template>"));

    // Set up per-slot inner observations. `inner_hook_ran` proves
    // the inner stage actually executed (otherwise vacuous-pass
    // would mask a regression).
    let inner_hook_ran = Arc::new(AtomicBool::new(false));
    let inner_session_ctx_id = Arc::new(AtomicI64::new(-1));
    let inner_accumulator_present = Arc::new(AtomicUsize::new(0));
    let inner_audit_observer_present = Arc::new(AtomicUsize::new(0));

    let outer_calls = Arc::new(AtomicUsize::new(0));

    // Slots cloned for the closure.
    let inner_hook_ran_for_hook = Arc::clone(&inner_hook_ran);
    let inner_session_ctx_id_for_hook = Arc::clone(&inner_session_ctx_id);
    let inner_accumulator_present_for_hook = Arc::clone(&inner_accumulator_present);
    let inner_audit_observer_present_for_hook = Arc::clone(&inner_audit_observer_present);
    let outer_calls_for_hook = Arc::clone(&outer_calls);

    let scheduler_slot: Arc<parking_lot::Mutex<Option<std::sync::Weak<Scheduler>>>> =
        Arc::new(parking_lot::Mutex::new(None));
    let scheduler_slot_for_hook = Arc::clone(&scheduler_slot);

    let hook: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |canonical: &str| {
        if canonical == "/a.vue" {
            // First (and only) outer execution: re-enter with
            // `request_context: None` so the scheduler's inline-execute
            // path takes the `AllSlotsClearGuard::clear_all()` branch.
            if outer_calls_for_hook.fetch_add(1, MOrd::SeqCst) == 0 {
                let weak = scheduler_slot_for_hook
                    .lock()
                    .as_ref()
                    .expect("scheduler weak ref installed by test")
                    .clone();
                let sched = weak
                    .upgrade()
                    .expect("scheduler must outlive the outer hook");
                let inner = sched.submit_request(Request {
                    file_id: "/b.vue".to_string(),
                    target: TargetStage::Analysis,
                    priority: Priority::Interactive,
                    source: None,
                    file_kind: None,
                    // CRITICAL — the None here forces the inline
                    // path to the `AllSlotsClearGuard` branch.
                    request_context: None,
                });
                let _ = sched.wait_or_drive_with_caller(&inner, CallerKind::CpuWorker);
            }
        } else if canonical == "/b.vue" {
            // Inner execution: record per-slot TLS visibility.
            inner_hook_ran_for_hook.store(true, MOrd::SeqCst);

            // 1) Session request-context slot.
            if let Some(ctx) = current_request_context() {
                inner_session_ctx_id_for_hook.store(ctx.request_id as i64, MOrd::SeqCst);
            }
            // 2) Session accumulator slot.
            if current_accumulator().is_some() {
                inner_accumulator_present_for_hook.store(1, MOrd::SeqCst);
            }
            // 3) Audit observer substrate slot.
            if current_observer().is_some() {
                inner_audit_observer_present_for_hook.store(1, MOrd::SeqCst);
            }
        }
    });

    let executor: Arc<dyn StageExecutor> = Arc::new(InlineReentryExecutor {
        analysis_hook: hook,
    });
    let sched = Scheduler::test_with_executor(
        SchedulerConfig {
            // Single CPU worker — forces inline-execute (no other
            // worker can pick up the inner Analysis).
            cpu_threads: 1,
            io_threads: 1,
            ..SchedulerConfig::default()
        },
        loader as Arc<dyn SourceLoader>,
        executor,
    );
    *scheduler_slot.lock() = Some(Arc::downgrade(&sched));

    // Build a REAL session-side RequestContext with footprint
    // capture enabled AND a real accumulator (so the CURRENT_ACCUMULATOR
    // TLS slot is actually populated by install — otherwise the inner
    // `current_accumulator().is_none()` assertion is vacuous because the
    // slot was never populated to begin with). Install it on the test
    // thread. The scheduler worker's `install_tls` will plant the same
    // context into every install_tls slot before executing the outer
    // Analysis.
    let outer_accumulator = Arc::new(RequestFootprintAccumulator::new());
    let outer_ctx = RequestContext::new(
        /* request_id */ OUTER_ID,
        Arc::from("/a.vue"),
        /* footprint_capture */ true,
        /* audit_accumulator */ Some(Arc::clone(&outer_accumulator)),
    );

    // Sanity: pre-install on the TEST thread, all three slots are
    // empty (this test must not depend on prior contamination).
    assert!(
        current_request_context().is_none(),
        "pre-state: test thread session slot must be empty",
    );
    assert!(
        current_accumulator().is_none(),
        "pre-state: test thread accumulator slot must be empty",
    );
    assert!(
        current_observer().is_none(),
        "pre-state: test thread audit slot must be empty",
    );

    let _guard = RequestContextGuard::install(Arc::clone(&outer_ctx));
    // Confirm the test thread now sees the installed slots (proves
    // `RequestContextGuard::install` actually populated them — and in
    // particular that CURRENT_ACCUMULATOR is now Some(outer accumulator),
    // which is the precondition that makes the inner None-assertion
    // discriminating rather than vacuous).
    assert!(
        current_request_context().is_some(),
        "post-install: session slot must be populated by RequestContextGuard",
    );
    assert!(
        current_accumulator().is_some(),
        "post-install: accumulator slot must be populated by RequestContextGuard \
         (precondition for the inner discriminating assertion — if this fires the \
         inner `is_none()` check on the inline-cleared accumulator slot is vacuous)",
    );
    assert!(
        current_observer().is_some(),
        "post-install: audit observer slot must be populated by RequestContextGuard",
    );

    // Submit the outer request and wait for completion. `wait()`
    // blocks on the scheduler's condvar — if the inline path hangs
    // (e.g., the inner stage parks forever), the test surfaces as
    // a CI-level test timeout rather than a panic message; that is
    // acceptable here because the assertions below only run after
    // outer completion and a hang in the inline path IS itself a
    // bug worth surfacing.
    let outer_handle = sched.submit_request(Request {
        file_id: "/a.vue".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: None,
        file_kind: None,
        request_context: Some(OpaqueRequestContext(
            Arc::clone(&outer_ctx) as Arc<dyn RequestContextLike>
        )),
    });
    let outer_state = outer_handle.wait();
    assert!(
        matches!(outer_state, CompletionState::Ready(_)),
        "outer Analysis must reach Ready: {outer_state:?}",
    );

    // The inner hook must have run — otherwise the assertions
    // below would pass vacuously.
    assert!(
        inner_hook_ran.load(MOrd::SeqCst),
        "inner hook must have run; nothing observed",
    );

    // Discriminating assertions: every install_tls slot the host
    // populates in the outer scope must be EMPTY when observed
    // from the inline-executed inner stage.
    //
    // Pre-fix (clearing only scheduler opaque): the
    // `current_request_context()` slot would still carry the
    // outer's `OUTER_ID`, and `current_observer()` would still
    // return the outer ctx as `Arc<dyn AuditObserver>` — both
    // assertions would fail.
    //
    // Post-fix (cross-crate hook fires): all three slots clear.
    assert_eq!(
        inner_session_ctx_id.load(MOrd::SeqCst),
        -1,
        "regression: session CURRENT_REQUEST_CONTEXT slot bled outer ctx \
         into inner stage; observed request_id != sentinel -1. \
         OUTER_ID was {OUTER_ID}.",
    );
    assert_eq!(
        inner_accumulator_present.load(MOrd::SeqCst),
        0,
        "regression: session CURRENT_ACCUMULATOR slot bled outer accumulator \
         into inner stage; cross-crate clear hook did not fire",
    );
    assert_eq!(
        inner_audit_observer_present.load(MOrd::SeqCst),
        0,
        "regression: verter_audit::current_observer() slot bled outer observer \
         into inner stage; cross-crate clear hook did not fire",
    );

    // Post-completion: outer guard must still see its slots
    // restored (drop hasn't happened yet — `_guard` is alive).
    assert!(
        current_request_context().is_some(),
        "post-outer: outer guard still alive — session slot must be restored",
    );
    assert!(
        current_accumulator().is_some(),
        "post-outer: outer guard still alive — accumulator slot must be restored",
    );
    assert!(
        current_observer().is_some(),
        "post-outer: outer guard still alive — audit slot must be restored",
    );
}
