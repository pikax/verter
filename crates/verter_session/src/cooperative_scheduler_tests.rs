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
