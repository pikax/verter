//! The host's readers own their freshness evidence in the live workspace's
//! content-transition history.
//!
//! Retiring history raises a floor every unleased canonical answers. A
//! reader that compares a canonical's answer against a generation it
//! captured — a request view clamping artifact-only answers, a stored
//! artifact's build generation, a held revision — must own that evidence in
//! the workspace it reads, or unrelated churn makes it falsely stale.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess, WorkspaceRead};

use crate::project_type_store::IndexedReady;
use crate::resolver_core::{CanonicalCompletionOverlay, RequestStoreView};
use crate::{HostConfig, VerterHost};

fn churn_records(tag: &str) -> Vec<(String, Arc<str>)> {
    (0..verter_workspace::freshness::DEFAULT_RETIRE_TRIGGER + 64)
        .map(|index| (format!("/churn/{tag}/{index}.ts"), Arc::from("export {};")))
        .collect()
}

fn churn(host: &VerterHost, tag: &str) {
    host.ws().notify_upsert_many(&churn_records(tag));
}

#[test]
fn a_request_view_caps_freshness_retirement_at_its_captured_generation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let base = host.resolver_store_view_read().into_owned_view();
    let captured = host.ws().content_generation();
    let leases_before = host.ws().resource_snapshot().freshness_history.view_leases;

    let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
    assert_eq!(
        host.ws().resource_snapshot().freshness_history.view_leases,
        leases_before + 1
    );
    churn(&host, "during");
    assert!(
        host.ws()
            .last_content_transition_generation("/untouched.ts")
            <= captured,
        "retirement must not pass the live request's captured generation"
    );

    drop(view);
    assert_eq!(
        host.ws().resource_snapshot().freshness_history.view_leases,
        leases_before
    );
    assert!(
        host.ws()
            .last_content_transition_generation("/untouched.ts")
            > captured,
        "once the request leaves, retirement may raise the floor"
    );
}

/// A revision a consumer captured and later compares for equality stays
/// equal across unrelated retirement while the consumer holds the lease,
/// and still moves when its own canonical transitions.
#[test]
fn a_held_revision_survives_unrelated_retirement_and_still_sees_its_own_edit() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let held = "/src/held.vue";
    let unheld = "/src/unheld.vue";
    host.notify_upsert(held, Arc::from("<script>let a = 1;</script>"));

    let evidence = host.lease_content_transition(held);
    assert!(evidence.is_some(), "the live workspace keeps a history");
    let revision = host.last_content_transition_generation(held);
    let unheld_before = host.last_content_transition_generation(unheld);

    churn(&host, "unrelated");
    assert!(
        host.last_content_transition_generation(unheld) > unheld_before,
        "the churn must have retired history past the trigger for this test \
         to prove anything"
    );
    assert_eq!(
        host.last_content_transition_generation(held),
        revision,
        "unrelated retirement must not read as a transition of a held canonical"
    );

    host.notify_upsert(held, Arc::from("<script>let a = 2;</script>"));
    assert!(host.last_content_transition_generation(held) > revision);
    drop(evidence);
}

/// Two overlapping `set_workspace` calls: the first is held open while it
/// installs its workspace's history, and the second is started meanwhile.
/// Whatever the interleaving, the workspace that ends up live is the one
/// whose history newly stored artifacts lease.
#[test]
fn overlapping_workspace_swaps_install_the_live_workspace_history() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let first = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let second = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));

    let (paused_tx, paused_rx) = mpsc::channel::<()>();
    let (resume_tx, resume_rx) = mpsc::channel::<()>();
    host.project_type_store
        .indexed()
        .pause_next_freshness_install(Box::new(move || {
            paused_tx
                .send(())
                .expect("the test is waiting for the pause");
            resume_rx.recv().expect("the test resumes the first swap");
        }));

    std::thread::scope(|scope| {
        let first_swap = {
            let workspace: Arc<dyn WorkspaceAccess> = first.clone();
            let host = &host;
            scope.spawn(move || host.set_workspace(workspace))
        };
        paused_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the first swap reaches its history installation");

        let (done_tx, done_rx) = mpsc::channel::<()>();
        let second_swap = {
            let workspace: Arc<dyn WorkspaceAccess> = second.clone();
            let host = &host;
            scope.spawn(move || {
                host.set_workspace(workspace);
                let _ = done_tx.send(());
            })
        };
        // A swap that publishes its workspace apart from its history runs
        // the second swap to completion here; one that publishes them
        // together blocks it until the first is resumed.
        let _ = done_rx.recv_timeout(Duration::from_millis(500));
        resume_tx.send(()).expect("the first swap is paused");
        first_swap.join().expect("first swap");
        second_swap.join().expect("second swap");
    });

    let live = host.ws();
    let live_is_second = std::ptr::addr_eq(Arc::as_ptr(&live), Arc::as_ptr(&second));
    let (live_ws, other_ws) = if live_is_second {
        (&second, &first)
    } else {
        (&first, &second)
    };

    let canonical = "/pkg/stored.d.ts";
    live_ws.notify_upsert(canonical, Arc::from("export {};"));
    let built_at = live_ws.content_generation();
    host.project_type_store.indexed().insert(
        Arc::from(canonical),
        Arc::new(IndexedReady::new_for_test([7; 16])),
    );
    assert_eq!(
        live_ws.resource_snapshot().freshness_history.leased_entries,
        1,
        "a newly stored artifact must lease its canonical in the live workspace"
    );
    assert_eq!(
        other_ws
            .resource_snapshot()
            .freshness_history
            .leased_entries,
        0,
        "a newly stored artifact must not lease in a swapped-out workspace"
    );

    live_ws.notify_upsert_many(&churn_records("live"));
    assert!(
        live_ws.last_content_transition_generation("/pkg/unstored.d.ts") > built_at,
        "the churn must have retired history past the trigger"
    );
    assert!(
        live_ws.last_content_transition_generation(canonical) <= built_at,
        "unrelated retirement in the live workspace must not stale the stored artifact"
    );
}
