//! BWH2 cooperative load-seam discriminators: the production
//! `ensure_loaded` path submits and drives through the host's
//! cooperative adapter, and a constructed host can install an
//! embedding-runtime yield hook.
//!
//! The adapter-level cancel/yield points are proven by the unit
//! suite (`src/cooperative_scheduler_tests.rs`); these cases bind the
//! HOST seam — the exact `ensure_loaded` route production callers
//! take — so a regression that admits scheduler work on a cancelled
//! drive, or integrates a snapshot after a cancelled/withdrawn drive,
//! cannot hide behind adapter-only coverage.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use verter_session::cooperative_scheduler::{CooperativeYield, YieldDecision};
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace};

/// A standalone host whose ONLY source lives in the backing workspace:
/// nothing was upserted through the host, so the scheduler holds no
/// source snapshot and `ensure_loaded` must go through the
/// submit/drive seam instead of answering from the loaded fast path.
fn host_with_workspace_file(config: HostConfig, canonical: &str, source: &str) -> VerterHost {
    let workspace = MemoryWorkspace::new(MemoryOptions::default());
    workspace.inject_file(canonical.to_string(), Arc::from(source));
    VerterHost::new(config, Arc::new(workspace))
}

/// A yield hook that continues at every offer and records how many
/// cooperative points the drive consulted it at.
#[derive(Debug)]
struct CountingYield {
    offers: AtomicUsize,
}

impl CooperativeYield for CountingYield {
    fn yield_to_runtime(&self) -> YieldDecision {
        self.offers.fetch_add(1, Ordering::SeqCst);
        YieldDecision::Continue
    }
}

/// A yield hook that withdraws the worker at its first offer.
#[derive(Debug)]
struct WithdrawingYield {
    offers: AtomicUsize,
}

impl CooperativeYield for WithdrawingYield {
    fn yield_to_runtime(&self) -> YieldDecision {
        self.offers.fetch_add(1, Ordering::SeqCst);
        YieldDecision::Cancelled
    }
}

/// A yield hook that meets the test at its first offer — on the native
/// load path that is the drive's one pre-park cooperative point, reached
/// after the request was admitted and before the wait is handed to the
/// scheduler — and continues.
#[derive(Debug)]
struct RendezvousYield {
    checkpoint: std::sync::Barrier,
    offered: AtomicBool,
}

impl CooperativeYield for RendezvousYield {
    fn yield_to_runtime(&self) -> YieldDecision {
        if !self.offered.swap(true, Ordering::SeqCst) {
            self.checkpoint.wait();
        }
        YieldDecision::Continue
    }
}

fn submitted_requests(host: &VerterHost) -> u64 {
    host.scheduler()
        .counters()
        .submit_count
        .load(Ordering::SeqCst)
}

/// A cancelled host drive refuses `ensure_loaded` at its admission
/// point: no Analysis request enters the scheduler inbox at all
/// (nothing is admitted, no handle exists to strand) and no partial
/// load is published — the not-loaded answer is a refusal, not a
/// cancelled drive over admitted work.
#[test]
fn cancelled_drive_refuses_ensure_loaded_admission() {
    let host = host_with_workspace_file(
        HostConfig::default(),
        "/coop/seam/a.ts",
        "export const a = 1;\n",
    );
    assert!(
        host.scheduler_source("/coop/seam/a.ts").is_none(),
        "precondition: the scheduler must not hold the source yet, so \
         ensure_loaded has to go through the submit/drive seam"
    );
    let submits_before = submitted_requests(&host);

    host.cooperative_drive().cancel();
    assert!(
        !host.ensure_loaded("/coop/seam/a.ts"),
        "a cancelled drive must answer not-loaded"
    );

    assert_eq!(
        submitted_requests(&host),
        submits_before,
        "a cancelled drive must not admit scheduler work through ensure_loaded"
    );
    assert!(
        host.scheduler_source("/coop/seam/a.ts").is_none(),
        "no partial load may be published by the refused path"
    );
    let provenance = host.provenance_snapshot();
    assert_eq!(provenance.ensure_loaded_calls, 1);
    assert_eq!(
        provenance.ensure_loaded_work_ns, 0,
        "the refused load must not run the snapshot integrate step"
    );
}

/// A host constructed with an embedding-runtime yield hook installs it
/// on the cooperative drive the production load path pumps through:
/// `ensure_loaded` consults the hook at its cooperative points and,
/// when the hook continues, reaches the ordinary loaded outcome with
/// outcomes unchanged.
#[test]
fn constructed_host_installs_yield_hook_on_the_load_path() {
    let hook = Arc::new(CountingYield {
        offers: AtomicUsize::new(0),
    });
    let host = host_with_workspace_file(
        HostConfig {
            cooperative_yield: Some(hook.clone()),
            ..HostConfig::default()
        },
        "/coop/seam/b.ts",
        "export const b = 2;\n",
    );

    assert!(
        host.ensure_loaded("/coop/seam/b.ts"),
        "a continuing hook must leave the load outcome unchanged"
    );
    assert!(
        hook.offers.load(Ordering::SeqCst) >= 1,
        "the production load path must consult the installed yield hook"
    );
    assert!(
        host.scheduler_analysis("/coop/seam/b.ts").is_some(),
        "the continuing drive must reach the committed Analysis snapshot"
    );
}

/// A yield withdrawal on the load seam is a not-loaded outcome that
/// integrates nothing: the request was admitted (the withdrawal
/// happened at the drive's cooperative point, after submission), the
/// drive returned its typed stop, and the integrate step never ran —
/// so no half-committed snapshot can be presented as loaded.
#[test]
fn withdrawn_drive_returns_not_loaded_without_integration() {
    let hook = Arc::new(WithdrawingYield {
        offers: AtomicUsize::new(0),
    });
    let host = host_with_workspace_file(
        HostConfig {
            cooperative_yield: Some(Arc::clone(&hook) as Arc<dyn CooperativeYield>),
            ..HostConfig::default()
        },
        "/coop/seam/c.ts",
        "export const c = 3;\n",
    );
    let submits_before = submitted_requests(&host);

    assert!(
        !host.ensure_loaded("/coop/seam/c.ts"),
        "a withdrawn drive must answer not-loaded"
    );

    assert_eq!(
        submitted_requests(&host),
        submits_before + 1,
        "the withdrawal happened at the drive point: exactly the one \
         admitted load request, no retry"
    );
    assert_eq!(
        hook.offers.load(Ordering::SeqCst),
        1,
        "the hook withdraws at the drive's first cooperative point"
    );
    let provenance = host.provenance_snapshot();
    assert_eq!(provenance.ensure_loaded_calls, 1);
    assert_eq!(
        provenance.ensure_loaded_work_ns, 0,
        "a withdrawn drive must not run the snapshot integrate step — no \
         partial scheduler snapshot may be published as loaded"
    );
}

/// With the host scheduler's driver parked, `ensure_loaded` must not
/// inline-execute scheduler stages on the calling (host/External)
/// thread: the driver owns the pump under the dual-pool isolation
/// invariant (host-coordinator work never runs scheduler stage work).
/// A sacrificial dispatch parks the driver first, so the load's own
/// stages sit READY and undispatched — the load can only complete
/// once the driver is released.
#[test]
fn native_ensure_loaded_parks_on_the_driver_instead_of_inline_pumping() {
    use verter_scheduler::scheduler::Request;
    use verter_scheduler::stage::{Priority, TargetStage};

    let hook = Arc::new(RendezvousYield {
        checkpoint: std::sync::Barrier::new(2),
        offered: AtomicBool::new(false),
    });
    let host = host_with_workspace_file(
        HostConfig {
            cooperative_yield: Some(Arc::clone(&hook) as Arc<dyn CooperativeYield>),
            ..HostConfig::default()
        },
        "/coop/seam/e.ts",
        "export const e = 4;\n",
    );
    let scheduler = Arc::clone(host.scheduler());
    scheduler.test_arm_dispatch_pause(0);
    // The sacrificial request gives the driver its one dispatch before
    // it parks, so the load's own stages are never dispatched while it
    // is parked.
    let _sacrificial = scheduler.submit_request(Request {
        file_id: "/coop/seam/sacrificial.ts".to_string(),
        target: TargetStage::Source,
        priority: Priority::Interactive,
        source: Some(Arc::from("export const s = 0;\n")),
        file_language: None,
        request_context: None,
    });
    scheduler.test_wait_until_dispatch_paused();

    let completed = Arc::new(AtomicBool::new(false));
    let submits_before = submitted_requests(&host);
    let load_thread = {
        let completed = Arc::clone(&completed);
        std::thread::spawn(move || {
            let loaded = host.ensure_loaded("/coop/seam/e.ts");
            completed.store(true, Ordering::SeqCst);
            assert!(loaded, "the parked-on-driver load must still complete");
            assert!(
                host.scheduler_analysis("/coop/seam/e.ts").is_some(),
                "the released load must reach the committed Analysis snapshot"
            );
        })
    };
    // Meet the load thread at the native pre-park cooperative point: it
    // has admitted its request and is about to hand the wait to the
    // scheduler. Only from here does "still not completed" say anything
    // about who pumps the stages; a fixed sleep or a submission count
    // could be observed before the thread reached the drive.
    hook.checkpoint.wait();
    assert!(
        scheduler.counters().submit_count.load(Ordering::SeqCst) > submits_before,
        "the load thread admitted its request before the pre-park point"
    );
    assert!(
        !completed.load(Ordering::SeqCst),
        "with the driver parked and the load's stages ready but \
         undispatched, ensure_loaded must not inline-execute scheduler \
         stages on the host thread"
    );
    scheduler.test_release_dispatch_pause();
    load_thread.join().expect("the load thread must not panic");
}

/// Source-without-Analysis must never answer the loaded fast path. A
/// drive that stopped between stages (a withdrawn cooperative drive)
/// leaves exactly that committed pair — Source published, Analysis
/// not, the dropped handle cancelling nothing. The deterministic
/// threaded producer of the same pair is a Source-target request
/// through the host's scheduler; the next `ensure_loaded` must not
/// report loaded from the leftover Source: it goes through the
/// submit/drive seam again and answers true only after Analysis
/// commits and the integrate step runs.
#[test]
fn source_without_analysis_never_answers_the_loaded_fast_path() {
    use verter_scheduler::scheduler::Request;
    use verter_scheduler::stage::{Priority, TargetStage};

    let host = host_with_workspace_file(
        HostConfig::default(),
        "/coop/seam/f.ts",
        "export const f = 5;\n",
    );
    assert!(
        host.scheduler_source("/coop/seam/f.ts").is_none(),
        "precondition: the scheduler must not hold the source yet, so \
         ensure_loaded has to go through the submit/drive seam"
    );

    // Commit exactly the Source stage: the between-stages snapshot pair.
    let handle = host.scheduler().submit_request(Request {
        file_id: "/coop/seam/f.ts".to_string(),
        target: TargetStage::Source,
        priority: Priority::Interactive,
        source: None,
        file_language: None,
        request_context: None,
    });
    host.scheduler().wait_or_drive(&handle);
    assert!(
        host.scheduler_source("/coop/seam/f.ts").is_some(),
        "precondition: the Source stage committed"
    );
    assert!(
        host.scheduler_analysis("/coop/seam/f.ts").is_none(),
        "precondition: the Analysis stage has not run"
    );

    let submits_before = submitted_requests(&host);
    assert!(
        host.ensure_loaded("/coop/seam/f.ts"),
        "the resumed load must complete through Analysis and integrate"
    );
    assert_eq!(
        submitted_requests(&host),
        submits_before + 1,
        "Source-without-Analysis must go through the submit/drive seam, \
         not answer loaded from the leftover Source"
    );
    assert!(
        host.scheduler_analysis("/coop/seam/f.ts").is_some(),
        "ensure_loaded may answer true only after the Analysis snapshot \
         commits"
    );
}

/// Committed scheduler snapshots are not a loaded file. A cancelled
/// drive skips the integrate step while the driver can still commit
/// Analysis afterwards; the deterministic producer of that pair is an
/// Analysis-target request driven straight through the scheduler. The
/// next `ensure_loaded` must not answer from those snapshots: it goes
/// through the submit/drive/integrate seam and answers true only once
/// the host-side dependency state exists.
#[test]
fn scheduler_snapshots_without_integration_never_answer_the_loaded_fast_path() {
    use verter_scheduler::scheduler::Request;
    use verter_scheduler::stage::{Priority, TargetStage};

    let host = host_with_workspace_file(
        HostConfig::default(),
        "/coop/seam/g.ts",
        "export const g = 6;\n",
    );

    let handle = host.scheduler().submit_request(Request {
        file_id: "/coop/seam/g.ts".to_string(),
        target: TargetStage::Analysis,
        priority: Priority::Interactive,
        source: None,
        file_language: None,
        request_context: None,
    });
    host.scheduler().wait_or_drive(&handle);
    assert!(
        host.scheduler_source("/coop/seam/g.ts").is_some()
            && host.scheduler_analysis("/coop/seam/g.ts").is_some(),
        "precondition: Source and Analysis both committed in the scheduler"
    );
    assert_eq!(
        host.provenance_snapshot().ensure_loaded_work_ns,
        0,
        "precondition: the host never ran the integrate step"
    );

    let submits_before = submitted_requests(&host);
    assert!(
        host.ensure_loaded("/coop/seam/g.ts"),
        "the load must complete through the seam"
    );
    assert_eq!(
        submitted_requests(&host),
        submits_before + 1,
        "committed snapshots without host integration must go through the \
         submit/drive/integrate seam, not answer loaded from the fast path"
    );
    // Integration is proven by stable state, not by elapsed time: the
    // integrated snapshot now satisfies the loaded fast path, so a second
    // call admits no further request.
    let submits_after_integration = submitted_requests(&host);
    assert!(host.ensure_loaded("/coop/seam/g.ts"));
    assert_eq!(
        submitted_requests(&host),
        submits_after_integration,
        "the integrated snapshot must answer the loaded fast path"
    );
}
