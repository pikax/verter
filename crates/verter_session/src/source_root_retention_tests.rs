//! `HostStoreView` scheduler-SOURCE retention lease.
//!
//! The artifact half of the snapshot contract landed with
//! `artifact_root_retention_tests`. This is the source half: a
//! `FileNode` holds only its CURRENT `ArcSwap` snapshots, so
//! `bump_generation` makes the prior source immediately unreachable and
//! `Scheduler::try_get_source` can never answer for a world that has
//! moved on.
//!
//! A `HostStoreView` captures a
//! [`verter_scheduler::source_root::SchedulerSourceRoot`] in the SAME
//! pre-build read window as every other dimension. That capture both
//! NAMES the source epoch the view was built at and KEEPS every source
//! version visible at it reachable for the view's whole life.
//!
//! These tests drive the REAL host mutation path (`upsert`), not the
//! directory's unit surface.

use std::sync::Arc;

use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_scheduler::source_root::SourceStateAt;

const CANONICAL: &str = "/proj/leased-source.ts";

fn upsert(host: &VerterHost, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: CANONICAL.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

/// THE defect at the `HostStoreView` boundary: a view must still be able
/// to name its own world's source version after the host has edited the
/// file out from under it.
#[test]
fn view_still_resolves_its_captured_source_after_the_host_supersedes_it() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, "export const a = 1;\n");

    let v1 = host
        .scheduler()
        .try_get_source(CANONICAL)
        .expect("v1 source must be committed")
        .whole_hash;

    let view = host.resolver_store_view_read().into_owned_view();
    let root = Arc::clone(
        view.source_root()
            .expect("a host-built view MUST carry a scheduler-source lease"),
    );
    assert_eq!(
        root.lookup(CANONICAL).whole_hash(),
        Some(v1),
        "the captured root must name the world the view was built against",
    );

    upsert(&host, "export const a = 1;\nexport const b = 2;\n");
    let v2 = host
        .scheduler()
        .try_get_source(CANONICAL)
        .expect("v2 source must be committed")
        .whole_hash;
    assert_ne!(
        v1, v2,
        "the edit must actually change the content version, else this proves nothing",
    );

    assert_eq!(
        root.lookup(CANONICAL).whole_hash(),
        Some(v1),
        "the leased root must STILL answer with its own world's source \
         version — `bump_generation` made that snapshot unreachable from \
         the live node, which is exactly the gap the root closes",
    );
    assert_eq!(
        host.scheduler()
            .capture_source_root()
            .lookup(CANONICAL)
            .whole_hash(),
        Some(v2),
        "a freshly captured root sees the current world",
    );
}

/// The lease is what keeps the version reachable — not the epoch value.
/// While the view lives the superseded version survives reclamation;
/// once the view drops, it is reclaimed.
#[test]
fn the_view_lease_gates_source_version_reclamation() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, "export const a = 1;\n");
    let v1 = host
        .scheduler()
        .try_get_source(CANONICAL)
        .expect("v1 source")
        .whole_hash;

    let view = host.resolver_store_view_read().into_owned_view();
    let directory = Arc::clone(host.scheduler().source_directory());
    assert!(
        directory.live_root_count() >= 1,
        "a live view must register a lease on the source directory",
    );

    upsert(&host, "export const a = 2;\n");
    // A sweep may legitimately free versions OLDER than the lease's
    // epoch — the contract is that the version the lease SELECTS
    // survives, not that nothing is freed.
    let _ = directory.reclaim_superseded_versions();
    assert_eq!(
        view.source_root()
            .expect("lease")
            .lookup(CANONICAL)
            .whole_hash(),
        Some(v1),
        "reclamation must never free the version a live lease selects",
    );
    let retained_while_leased = directory.retained_version_count(CANONICAL);
    assert!(
        retained_while_leased >= 2,
        "the superseded version must still be retained while leased, saw \
         {retained_while_leased}",
    );

    drop(view);
    // The host's own cached base view is a live root too, so drain it by
    // rebuilding past the dropped lease before sweeping.
    let _ = host.resolver_store_view_read().into_owned_view();
    let _ = directory.reclaim_superseded_versions();
    let _ = directory.reclaim_superseded_versions();
    assert!(
        directory.retained_version_count(CANONICAL) < retained_while_leased,
        "once the leasing view is gone the superseded versions drain",
    );
    assert!(
        matches!(
            host.scheduler().capture_source_root().lookup(CANONICAL),
            SourceStateAt::Present { .. }
        ),
        "reclamation never disturbs the current root's answer",
    );
}
