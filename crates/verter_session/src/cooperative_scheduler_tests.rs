//! Cooperative drive-adapter discriminators.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use verter_scheduler::job::CompletionState;
use verter_scheduler::scheduler::Request;
use verter_scheduler::stage::{Priority, TargetStage};

use super::{
    CooperativeDrive, CooperativePoint, CooperativeSchedulerAdapter, CooperativeSubmit,
    CooperativeYield, YieldDecision,
};
use crate::types::HostConfig;
use crate::VerterHost;

/// A yield hook that cancels the adapter at its first yield point and
/// records how often the scheduler was consulted afterwards.
#[derive(Debug)]
struct CancellingYield {
    fired: AtomicUsize,
}

impl CancellingYield {
    fn fenced_calls(&self) -> usize {
        self.fired.load(Ordering::SeqCst)
    }
}

impl CooperativeYield for CancellingYield {
    fn yield_to_runtime(&self) -> YieldDecision {
        self.fired.fetch_add(1, Ordering::SeqCst);
        YieldDecision::Cancelled
    }
}

fn standalone_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn analysis_request(canonical: &str) -> Request {
    Request {
        file_id: canonical.to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: None,
        file_language: None,
        request_context: None,
    }
}

/// Register one TypeScript source and return its canonical id, so a
/// submitted Analysis request has real scheduler work to drive.
fn register_ts_source(host: &VerterHost, canonical: &str, source: &str) {
    use verter_language::FileLanguage;
    let _update = host
        .upsert(crate::types::UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: std::sync::Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("register source");
}

/// An uncancellable adapter with the inline yield hook drives the
/// existing execution contract to the same terminal state as driving
/// the scheduler directly — cooperative points do not rewrite query
/// outcomes.
#[test]
fn uncancellable_drive_preserves_scheduler_outcome() {
    let host = standalone_host();
    register_ts_source(&host, "/coop/a.ts", "export const a = 1;\n");

    let direct = host
        .scheduler
        .submit_request(analysis_request("/coop/a.ts"));
    let direct_state = host.scheduler.wait_or_drive(&direct);

    let adapter = CooperativeSchedulerAdapter::new();
    let cooperatively = adapter.submit(&host.scheduler, analysis_request("/coop/a.ts"));
    let CooperativeSubmit::Submitted(handle) = cooperatively else {
        panic!("uncancelled adapter must submit");
    };
    let adapted_state = match adapter.drive(&host.scheduler, &handle) {
        CooperativeDrive::Driven(state) => state,
        CooperativeDrive::Cancelled(stop) => panic!("uncancelled drive stopped at {stop:?}"),
    };

    let direct_ready = matches!(direct_state, CompletionState::Ready(_));
    assert!(
        direct_ready,
        "control drive must reach Ready (got {direct_state:?})"
    );
    assert!(
        matches!(adapted_state, CompletionState::Ready(_)),
        "cooperative drive must reach the same Ready terminal (got {adapted_state:?})"
    );
}

/// A cancel before submit refuses without consulting the scheduler:
/// nothing is admitted and no handle exists to leak.
#[test]
fn cancel_before_submit_refuses_without_admission() {
    let host = standalone_host();
    register_ts_source(&host, "/coop/b.ts", "export const b = 2;\n");

    let adapter = CooperativeSchedulerAdapter::new();
    adapter.cancel();
    match adapter.submit(&host.scheduler, analysis_request("/coop/b.ts")) {
        CooperativeSubmit::Refused(stop) => assert_eq!(stop.point, CooperativePoint::BeforeSubmit),
        CooperativeSubmit::Submitted(_) => {
            panic!("cancelled adapter must not admit new work")
        }
    }
}

/// A cancel after submit, before drive, returns the typed cancelled
/// outcome and leaves the handle pending — the drive contributed no
/// stages, so no partial result can be presented as an outcome.
#[test]
fn cancel_before_drive_returns_typed_stop_and_no_result() {
    let host = standalone_host();
    register_ts_source(&host, "/coop/c.ts", "export const c = 3;\n");

    let adapter = CooperativeSchedulerAdapter::new();
    let CooperativeSubmit::Submitted(handle) =
        adapter.submit(&host.scheduler, analysis_request("/coop/c.ts"))
    else {
        panic!("uncancelled adapter must submit");
    };
    adapter.cancel();
    match adapter.drive(&host.scheduler, &handle) {
        CooperativeDrive::Cancelled(stop) => {
            assert_eq!(stop.point, CooperativePoint::BeforeDrive);
        }
        CooperativeDrive::Driven(state) => {
            panic!("cancelled drive must not present an outcome (got {state:?})")
        }
    }
}

/// A yield point that withdraws the worker stops the drive before the
/// scheduler is consulted for it; the pending handle is released by
/// dropping it, and cancellation observed at a yield is reported at the
/// yield point — distinct from both submit- and drive-side stops.
#[test]
fn yield_point_withdrawal_stops_before_the_scheduler_is_consulted() {
    let host = standalone_host();
    register_ts_source(&host, "/coop/d.ts", "export const d = 4;\n");

    let hook = Arc::new(CancellingYield {
        fired: AtomicUsize::new(0),
    });
    let adapter = CooperativeSchedulerAdapter::with_yield_hook(hook.clone());
    let CooperativeSubmit::Submitted(handle) =
        adapter.submit(&host.scheduler, analysis_request("/coop/d.ts"))
    else {
        panic!("uncancelled adapter must submit");
    };

    let driven = adapter.drive(&host.scheduler, &handle);
    assert!(
        matches!(
            &driven,
            CooperativeDrive::Cancelled(stop) if stop.point == CooperativePoint::AtYield
        ),
        "yield withdrawal must stop at the yield point (got {driven:?})"
    );
    assert_eq!(hook.fenced_calls(), 1, "exactly one yield point per drive");
    assert!(
        !driven.driven_ready(),
        "a stopped drive is never a Ready outcome"
    );
    // Releasing the retained handle is the caller's drop; the adapter
    // keeps no second reference to it.
    drop(handle);
}

/// A cancelled drive cannot expose a half-committed snapshot: the
/// cancelled outcome carries no value at the type level, and the
/// committed input behind the request stays fence-coherent.
#[test]
fn cancelled_drive_exposes_no_half_committed_snapshot() {
    use crate::input_handoff::{AcquiredFile, CommittedInputHandoff};

    let host = standalone_host();
    register_ts_source(&host, "/coop/e.ts", "export const e = 5;\n");

    let handoff = CommittedInputHandoff::commit(
        [AcquiredFile {
            canonical: std::sync::Arc::from("/coop/e.ts"),
            content: std::sync::Arc::from("export const e = 5;\n"),
        }],
        [],
    )
    .expect("coherent wave");

    let adapter = CooperativeSchedulerAdapter::new();
    let CooperativeSubmit::Submitted(handle) =
        adapter.submit(&host.scheduler, analysis_request("/coop/e.ts"))
    else {
        panic!("uncancelled adapter must submit");
    };
    adapter.cancel();
    let stopped = adapter.drive(&host.scheduler, &handle);
    assert!(matches!(stopped, CooperativeDrive::Cancelled(_)));

    // The committed basis remains exactly the bound identity: nothing
    // the cancelled drive did (or did not) do tore the snapshot fence,
    // and observing it still answers the committed bytes.
    assert_eq!(handoff.binding().admit_publication(handoff.basis()), Ok(()));
    assert!(matches!(
        handoff.observe("/coop/e.ts"),
        crate::input_handoff::HandoffObserve::File { .. }
    ));
}

/// A yield hook that continues on its first offer (before the first
/// driven stage) and withdraws on its second (between stages).
#[derive(Debug)]
struct ContinueOnceYield {
    offers: AtomicUsize,
}

impl ContinueOnceYield {
    fn offers(&self) -> usize {
        self.offers.load(Ordering::SeqCst)
    }
}

impl CooperativeYield for ContinueOnceYield {
    fn yield_to_runtime(&self) -> YieldDecision {
        if self.offers.fetch_add(1, Ordering::SeqCst) == 0 {
            YieldDecision::Continue
        } else {
            YieldDecision::Cancelled
        }
    }
}

/// The cooperative points sit INSIDE the pump a nonthreaded worker
/// runs, not only in front of it. On a scheduler with no driver
/// thread (the wasm/test inline execution model) the adapter drives
/// one `Scheduler::drive_one` stage per iteration and offers the
/// yield hook between stages: a hook that withdraws on its second
/// offer stops the drive with the Source stage committed but Analysis
/// not yet driven — the worker withdrew mid-request, and the pending
/// handle is all a later drive call needs to resume it.
#[test]
fn yield_point_between_stages_stops_mid_pump() {
    use verter_scheduler::scheduler::Scheduler;
    use verter_scheduler::source_loader::MemorySourceLoader;

    let scheduler = Scheduler::test_new_sync(
        verter_scheduler::scheduler::SchedulerConfig::default(),
        Arc::new(MemorySourceLoader::new()),
    );

    let hook = Arc::new(ContinueOnceYield {
        offers: AtomicUsize::new(0),
    });
    let adapter = CooperativeSchedulerAdapter::with_yield_hook(hook.clone());
    let request = Request {
        file_id: "/coop/f.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        // Inline source: the Source stage commits without depending
        // on the default executor's reading behaviour.
        source: Some(Arc::from("export const f = 6;\n")),
        file_language: None,
        request_context: None,
    };
    let CooperativeSubmit::Submitted(handle) = adapter.submit(&scheduler, request) else {
        panic!("uncancelled adapter must submit");
    };

    let driven = adapter.drive(&scheduler, &handle);
    assert!(
        matches!(
            &driven,
            CooperativeDrive::Cancelled(stop) if stop.point == CooperativePoint::AtYield
        ),
        "the between-stages yield offer must withdraw the worker (got {driven:?})"
    );
    assert_eq!(
        hook.offers(),
        2,
        "one offer before the first stage, one between stages"
    );
    assert!(
        scheduler.try_get_source("/coop/f.ts").is_some(),
        "the Source stage must have been driven before the withdrawal"
    );
    assert!(
        scheduler.try_get_analysis("/coop/f.ts").is_none(),
        "the withdrawal must stop the pump before the Analysis stage is driven"
    );
    drop(handle);
}

/// Cancellation observed between driven stages stops the pump with the
/// typed between-stages stop: at least one stage ran, no result value
/// is carried, and a re-drive of the same handle through a fresh,
/// uncancellable adapter reaches the scheduler's own terminal state —
/// withdrawal never rewrote the request's outcome.
#[test]
fn cancel_between_stages_stops_the_pump_and_a_re_drive_resumes() {
    use verter_scheduler::scheduler::Scheduler;
    use verter_scheduler::source_loader::MemorySourceLoader;

    let scheduler = Scheduler::test_new_sync(
        verter_scheduler::scheduler::SchedulerConfig::default(),
        Arc::new(MemorySourceLoader::new()),
    );

    // Withdraw after the Source stage by cancelling the adapter's
    // token at the between-stages offer — the embedding runtime's
    // out-of-band withdrawal shape.
    let cancellation = verter_scheduler::cancellation::CancellationToken::new();
    let hook = Arc::new(CancelTokenAtSecondOffer {
        offers: AtomicUsize::new(0),
        cancellation: cancellation.clone(),
    });
    let adapter = CooperativeSchedulerAdapter::with_yield_hook_and_cancellation(hook, cancellation);
    let request = Request {
        file_id: "/coop/g.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: Some(Arc::from("export const g = 7;\n")),
        file_language: None,
        request_context: None,
    };
    let CooperativeSubmit::Submitted(handle) = adapter.submit(&scheduler, request) else {
        panic!("uncancelled adapter must submit");
    };

    let stopped = adapter.drive(&scheduler, &handle);
    assert!(
        matches!(
            &stopped,
            CooperativeDrive::Cancelled(stop) if stop.point == CooperativePoint::BetweenStages
        ),
        "cancellation observed after a driven stage stops at the \
         between-stages point (got {stopped:?})"
    );
    assert!(
        scheduler.try_get_source("/coop/g.ts").is_some(),
        "exactly the Source stage ran before the cancellation"
    );
    assert!(
        scheduler.try_get_analysis("/coop/g.ts").is_none(),
        "the cancelled pump drove no Analysis stage"
    );

    // The handle stays pending and carries no partial outcome: a
    // later drive (here through a fresh uncancellable adapter, the
    // shape an embedding runtime's resumed continuation takes) pumps
    // the same request to the scheduler's own terminal state.
    let resumed = CooperativeSchedulerAdapter::new();
    match resumed.drive(&scheduler, &handle) {
        CooperativeDrive::Driven(state) => assert!(
            !matches!(
                state,
                CompletionState::Superseded | CompletionState::Shutdown
            ),
            "the resumed drive must reach a real terminal state (got {state:?})"
        ),
        CooperativeDrive::Cancelled(stop) => {
            panic!("an uncancellable resumed drive must not stop (got {stop:?})")
        }
    }
    drop(handle);
}

/// Always continues, but trips the shared cancellation token on its
/// second offer, so the stop is observed at the between-stages point
/// rather than at a yield withdrawal.
#[derive(Debug)]
struct CancelTokenAtSecondOffer {
    offers: AtomicUsize,
    cancellation: verter_scheduler::cancellation::CancellationToken,
}

impl CooperativeYield for CancelTokenAtSecondOffer {
    fn yield_to_runtime(&self) -> YieldDecision {
        if self.offers.fetch_add(1, Ordering::SeqCst) > 0 {
            self.cancellation.cancel();
        }
        YieldDecision::Continue
    }
}

/// With a driver thread installed, the adapter's drive NEVER pumps
/// `Scheduler::drive_one` on the calling thread: the driver owns the
/// pump, and a host/External caller that dequeues and inline-executes
/// ready stages breaks the dual-pool isolation between host
/// coordinator threads and the scheduler's stage pools. The armed
/// dispatch pause parks the driver after one sacrificial dispatch, so
/// the driven request's stages sit READY and undispatched — an
/// uncancellable drive on this thread cannot complete the work itself
/// (it must park on the driver); only releasing the driver finishes
/// the request.
#[test]
fn threaded_drive_parks_on_the_driver_instead_of_inline_pumping() {
    use std::time::Duration;
    use verter_scheduler::scheduler::Scheduler;
    use verter_scheduler::source_loader::MemorySourceLoader;

    let scheduler = Scheduler::test_new(
        verter_scheduler::scheduler::SchedulerConfig::default(),
        Arc::new(MemorySourceLoader::new()),
    );
    assert!(
        scheduler.has_driver_thread(),
        "precondition: the threaded test scheduler must have a driver"
    );
    scheduler.test_arm_dispatch_pause(0);
    // The sacrificial request gives the driver its one dispatch before
    // it parks, so nothing else is dispatched while it is parked.
    let _sacrificial = scheduler.submit_request(Request {
        file_id: "/coop/h1.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: Some(Arc::from("export const h1 = 1;\n")),
        file_language: None,
        request_context: None,
    });
    scheduler.test_wait_until_dispatch_paused();

    let adapter = CooperativeSchedulerAdapter::new();
    let request = Request {
        file_id: "/coop/h2.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: Some(Arc::from("export const h2 = 2;\n")),
        file_language: None,
        request_context: None,
    };
    let CooperativeSubmit::Submitted(handle) = adapter.submit(&scheduler, request) else {
        panic!("uncancelled adapter must submit");
    };

    let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let drive_thread = {
        let scheduler = Arc::clone(&scheduler);
        let completed = Arc::clone(&completed);
        std::thread::spawn(move || {
            let driven = adapter.drive(&scheduler, &handle);
            completed.store(true, Ordering::SeqCst);
            assert!(
                matches!(driven, CooperativeDrive::Driven(CompletionState::Ready(_))),
                "the parked-on-driver drive must still reach Ready (got {driven:?})"
            );
        })
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !completed.load(Ordering::SeqCst),
        "with the driver parked and the request's stages ready but \
         undispatched, a threaded drive must not complete the work \
         itself — the calling thread is not the pump"
    );
    scheduler.test_release_dispatch_pause();
    drive_thread
        .join()
        .expect("the drive thread must not panic");
}
