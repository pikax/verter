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

use std::sync::atomic::{AtomicUsize, Ordering};
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
