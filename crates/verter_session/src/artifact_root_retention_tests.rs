//! `HostStoreView` artifact-membership RETENTION LEASE.
//!
//! The invariant: an immutable root must both NAME state and KEEP it
//! reachable. A `HostStoreView` captures a
//! [`crate::file_artifact_store::FileArtifactRoot`] in its pre-build
//! read window, and that capture is a lease — while the view lives,
//! `FileArtifactStore` may not physically reclaim any artifact version,
//! canonical→keys index entry or augmentation-index entry the view's
//! world contained.
//!
//! These tests exercise the lease through the REAL host mutation paths
//! (`upsert`, `evict`, reachability GC), not through the store's unit
//! surface: the defect they characterize is a view that captured
//! `(canonical, content_hash)` identity and then could not reach the
//! artifact that identity named, because the host had already freed it.

use std::sync::Arc;

use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

const CANONICAL: &str = "/proj/leased.ts";

fn host_with_file(source: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, source);
    host
}

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

/// THE defect, at the `HostStoreView` boundary: a view that captured a
/// root must still resolve the artifact of its own world after the host
/// has superseded that content version.
///
/// Before the lease existed the host's publish drained the prior
/// version outright, so the view's captured
/// `(canonical, whole_hash)` named an artifact `FileArtifactStore` had
/// already freed — content-addressing established identity, never
/// lifetime.
#[test]
fn view_still_resolves_its_captured_artifact_after_the_host_supersedes_it() {
    let host = host_with_file("export interface A { x: number }\n");
    let v1 = host
        .ensure_indexed_ready(CANONICAL)
        .expect("v1 artifact must materialise");
    let v1_key = host
        .authoritative_current_artifact_key(CANONICAL)
        .expect("v1 runtime-authoritative key");

    let view = host.resolver_store_view_read().into_owned_view();
    let root = Arc::clone(
        view.artifact_root()
            .expect("a host-built view MUST carry an artifact-membership lease"),
    );

    // Host-side content mutation: a new content version publishes and
    // the prior one leaves the current root's membership.
    upsert(&host, "export interface A { x: number; y: string }\n");
    let v2 = host
        .ensure_indexed_ready(CANONICAL)
        .expect("v2 artifact must materialise");
    let v2_key = host
        .authoritative_current_artifact_key(CANONICAL)
        .expect("v2 runtime-authoritative key");
    assert_ne!(
        v1.whole_hash, v2.whole_hash,
        "the mutation must actually change the content version, else this proves nothing"
    );

    let store = host.project_type_store();
    assert!(
        host.exact_current_indexed_for_test(CANONICAL, v1.whole_hash)
            .is_none(),
        "the CURRENT root must no longer serve the superseded version"
    );

    let through_lease = store
        .indexed()
        .indexed_at_root(&root, &v1_key)
        .expect("the view's captured root MUST still reach its own world's artifact");
    assert_eq!(
        through_lease.whole_hash, v1.whole_hash,
        "the lease must resolve the version the view captured, not the current one"
    );
    assert!(
        store.indexed().indexed_at_root(&root, &v2_key).is_none(),
        "the view must never see a version published after its capture"
    );
}

/// The per-canonical eviction cascade is a retirement too: the view
/// keeps reaching the artifact its world contained.
#[test]
fn view_still_resolves_its_captured_artifact_after_the_canonical_cascade_evicts_it() {
    let host = host_with_file("export interface B { b: number }\n");
    let _ = host
        .ensure_indexed_ready(CANONICAL)
        .expect("artifact must materialise");
    let key = host
        .authoritative_current_artifact_key(CANONICAL)
        .expect("runtime-authoritative artifact key");

    let view = host.resolver_store_view_read().into_owned_view();
    let root = Arc::clone(view.artifact_root().expect("view carries a lease"));

    host.project_type_store().evict_canonical(CANONICAL);

    let store = host.project_type_store();
    assert!(
        store.indexed().get_artifacts(&key).is_none(),
        "the current root must no longer serve the evicted artifact"
    );
    assert!(
        store.indexed().artifacts_at_root(&root, &key).is_some(),
        "the view's lease must survive an eviction of its captured world"
    );
}

/// Reachability GC computes its live set from the CURRENT world and
/// knows nothing about captured roots. It must therefore only RETIRE:
/// a sweep that freed bytes from `live_publish_set` alone would revoke
/// the lease a live view holds.
#[test]
fn reachability_gc_does_not_free_a_version_a_live_view_still_leases() {
    let host = host_with_file("export interface C { c: number }\n");
    let _ = host
        .ensure_indexed_ready(CANONICAL)
        .expect("artifact must materialise");
    let key = host
        .authoritative_current_artifact_key(CANONICAL)
        .expect("runtime-authoritative artifact key");

    let view = host.resolver_store_view_read().into_owned_view();
    let root = Arc::clone(view.artifact_root().expect("view carries a lease"));

    // An EMPTY live set makes every artifact unreachable in the current
    // world, and the sweep ends with an explicit reclamation REQUEST.
    host.project_type_store().evict_unreachable_artifacts(
        &rustc_hash::FxHashSet::default(),
        false,
        0,
    );

    let store = host.project_type_store();
    assert!(
        store.indexed().get_artifacts(&key).is_none(),
        "the sweep must retire the unreachable version out of the current root"
    );
    assert!(
        store.indexed().artifacts_at_root(&root, &key).is_some(),
        "a GC request is not a reachability decision — the leased version must survive"
    );
    assert!(
        store.indexed().live_root_count() >= 1,
        "the live view's lease must be registered while the view lives"
    );

    // Releasing the view is what makes the version reclaimable — so the
    // retention above is a lease, not an unconditional leak.
    drop(root);
    drop(view);
    // The manager may still cache a base view built at the same epoch;
    // rebuild past it so no root pins the retired version.
    upsert(&host, "export interface C { c: number; d: string }\n");
    let _ = host.resolver_store_view_read().into_owned_view();
    let store = host.project_type_store();
    store.indexed().reclaim_retired_versions();
    let probe = store.indexed().capture_root();
    assert!(
        store.indexed().artifacts_at_root(&probe, &key).is_none(),
        "once no root addresses the retired version it is gone from every root"
    );
}

/// Cloning a view (and overlaying it) shares ONE lease — the
/// registration is a property of the captured root, not of each holder.
#[test]
fn cloned_views_share_one_lease_and_release_it_once() {
    let host = host_with_file("export interface D { d: number }\n");
    let _ = host.ensure_indexed_ready(CANONICAL);
    let store = host.project_type_store();

    let baseline = store.indexed().live_root_count();
    let view = host.resolver_store_view_read().into_owned_view();
    let after_build = store.indexed().live_root_count();
    assert!(
        after_build > baseline,
        "building a view MUST take a lease (baseline {baseline}, after {after_build})"
    );

    let clones: Vec<_> = (0..4).map(|_| view.clone()).collect();
    assert_eq!(
        store.indexed().live_root_count(),
        after_build,
        "cloning a view is a refcount bump on ONE lease, never a new registration"
    );

    drop(clones);
    drop(view);
    assert!(
        store.indexed().live_root_count() <= after_build,
        "releasing every holder must release the lease exactly once"
    );
}

/// The MEASURED memory bound: `current retained working set + versions
/// reachable from live view roots`, and nothing else.
///
/// The bound is per LIVE ROOT, not "everything born after the oldest live
/// root". A root at epoch `E` selects exactly ONE version per membership
/// entry — the one visible at `E` — so the versions born after it and
/// superseded before now are reachable from nothing and must be freed
/// while the root is still alive.
///
/// Three shapes:
///
/// - **steady state** (a view rebuilt per edit, the LSP shape): retention
///   is FLAT — each rebuild moves past the previous root, so 200 edits
///   retain the same handful of versions 200 000 would;
/// - **pinned** (ONE view held across every edit — the manager's own
///   cached base view is exactly this shape, and it survives an upsert):
///   retention is flat too, and INDEPENDENT of the edit count. It is
///   measured at two edit counts precisely because a single count cannot
///   tell "bounded" from "linear";
/// - **drain**: once the view AND the manager's cached view have moved
///   on, the retained set goes to ZERO.
///
/// Growth with the edit count under a pinned view is the OOM shape: an
/// unattended editor session holds one cached base view and every
/// keystroke would add a permanently-unreachable version.
#[test]
fn retention_is_flat_under_a_pinned_view_and_drains_after_it() {
    const EDITS: usize = 200;

    // Steady state: the view is rebuilt after each edit.
    let steady = host_with_file("export interface P { p: number }\n");
    for i in 0..EDITS {
        upsert(&steady, &format!("export interface P {{ p{i}: number }}\n"));
        let _ = steady.ensure_indexed_ready(CANONICAL);
        let _view = steady.resolver_store_view_read().into_owned_view();
    }
    let steady_retained = steady
        .project_type_store()
        .indexed()
        .retained_retired_version_count();
    assert_eq!(
        steady.project_type_store().indexed().len(),
        1,
        "one live artifact version per canonical"
    );
    assert!(
        steady_retained <= 32,
        "steady-state retention MUST stay flat and independent of edit count, \
         not grow with it — {EDITS} edits retained {steady_retained} versions"
    );

    // Pinned: ONE view leased across every edit, measured at two edit
    // counts on separate hosts.
    let pinned_retained = |edits: usize| {
        let host = host_with_file("export interface N { n: number }\n");
        let _ = host.ensure_indexed_ready(CANONICAL);
        let held = host.resolver_store_view_read().into_owned_view();
        for i in 0..edits {
            upsert(&host, &format!("export interface N {{ n{i}: number }}\n"));
            let _ = host.ensure_indexed_ready(CANONICAL);
        }
        host.project_type_store()
            .indexed()
            .reclaim_retired_versions();
        let retained = host
            .project_type_store()
            .indexed()
            .retained_retired_version_count();
        // The lease still holds: the pinned view must resolve its own
        // world, so the flat number above is retention, not eviction.
        assert!(
            held.whole_hash_for_tests(CANONICAL).is_some(),
            "the pinned view must still resolve its own captured world"
        );
        drop(held);
        retained
    };
    let at_small = pinned_retained(EDITS);
    let at_large = pinned_retained(EDITS * 4);
    assert!(
        at_large <= at_small.max(32),
        "retention under a PINNED view must not grow with the edit count — \
         {EDITS} edits retained {at_small}, {} edits retained {at_large}",
        EDITS * 4
    );

    // Drain: release the view, then let the manager rebuild past it.
    let pinned = host_with_file("export interface N { n: number }\n");
    let _ = pinned.ensure_indexed_ready(CANONICAL);
    let held = pinned.resolver_store_view_read().into_owned_view();
    for i in 0..EDITS {
        upsert(&pinned, &format!("export interface N {{ n{i}: number }}\n"));
        let _ = pinned.ensure_indexed_ready(CANONICAL);
    }
    let store = pinned.project_type_store();
    drop(held);
    let _fresh = pinned.resolver_store_view_read().into_owned_view();
    store.indexed().reclaim_retired_versions();
    assert_eq!(
        store.indexed().retained_retired_version_count(),
        0,
        "once no root can reach the retired versions they are ALL freed — \
         retention is a lease, never an unconditional leak"
    );
}
