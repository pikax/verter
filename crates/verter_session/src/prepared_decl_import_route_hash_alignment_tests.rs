//! Discriminating regression test for the unification of the
//! ImportRoute hash source in `materialize_prepared_decl_bundle`.
//!
//! ## Why this test exists
//!
//! Before this fix, the two cold-materialise paths in
//! `crates/verter_session/src/host_manage/prepared_decl.rs` admitted
//! their bundles with DIFFERENT `ImportRoute` derived-fact hashes for
//! the same canonical:
//!
//! - The route-owned-shallow path
//!   (`materialize_prepared_decl_bundle_from_route_owned_shallow`) uses
//!   `self.generation_current_import_route_hash(canonical)` — re-resolves
//!   known-miss specifiers against the current workspace generation.
//! - The standard path (`materialize_prepared_decl_bundle`) used the
//!   static `facts.import_route_hash` — captured at `IndexedReady`
//!   materialisation time and never re-resolved.
//!
//! The view-side validator (`HostStoreView::build` line 684 +
//! `snapshot_tracked_import_route_hashes`) uses
//! `generation_current_import_route_hash` UNIFORMLY for both loops,
//! so the standard path's static-hash bundles silently mismatched
//! every warm-read once a known-miss specifier became resolvable —
//! producing a permanent re-materialise loop and inflating the
//! `PreparedDeclBundleRejectImportRoute*` counters.
//!
//! The fix unifies both paths on the dynamic hash. The discriminator
//! below catches the regression by constructing a fixture where the
//! static hash provably diverges from the dynamic hash, then asserting
//! the bundle warm-hits on the second call.
//!
//! ## Discriminating property
//!
//! 1. Upsert an owner whose import `./late_dep` is unresolvable at
//!    index time — `IndexedReady.import_routes` snapshots it as a
//!    known-miss with `resolved_canonical_id = None`.
//! 2. Upsert the late dep AFTER the owner is indexed — the owner's
//!    content does not change, so its content-pinned
//!    `IndexedReady.import_route_hash` stays static (pinned to the
//!    miss). Meanwhile `generation_current_import_route_hash`
//!    re-resolves the miss against the now-current workspace and
//!    yields a DIFFERENT hash (the late dep now resolves positively).
//! 3. Build a `HostStoreView` (which uses the dynamic hash).
//! 4. Cold-materialise the bundle via
//!    `prepared_decl_bundle_with_store_view` — counted as 1
//!    materialisation.
//! 5. Call `prepared_decl_bundle_with_store_view` again with the
//!    SAME view. The stored bundle MUST warm-hit (1 materialisation
//!    total, not 2). Pre-fix the bundle's `ImportRoute` fact carried
//!    the static miss-hash while the view carried the dynamic
//!    positive-hash; the warm-read validator rejected; the second
//!    call re-materialised → 2 materialisations total.

use std::sync::Arc;

use crate::resolver_core::{DerivedFactKind, StoreView};
use crate::types::FileKind;
use crate::{HostConfig, UpsertRequest, VerterHost};

fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {path} failed: {e:?}"));
}

#[test]
fn materialize_prepared_decl_bundle_uses_dynamic_import_route_hash() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let owner = "/proj_irh/owner.ts";
    // The owner imports a TYPE from `./late_dep`, which does NOT
    // exist yet — the import will be snapshotted as a known-miss in
    // `IndexedReady.import_routes` and `IndexedReady.import_route_hash`.
    upsert(
        &host,
        owner,
        "import type { LateType } from './late_dep';\n\
         export type Wrapper = { inner: LateType };\n",
    );

    // Materialise the owner's IndexedReady. This snapshots the
    // unresolved import as a known-miss in the content-pinned
    // import_routes / import_route_hash.
    let owner_indexed = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady must materialise");
    let static_hash = owner_indexed.import_route_hash;

    // Fixture invariant: the unresolved import IS snapshotted as a
    // known-miss in the IndexedReady's import_routes map.
    let snapshot = owner_indexed.import_routes.get("./late_dep").cloned();
    let snapshot_resolution = snapshot.expect(
        "fixture invariant: IndexedReady.import_routes must contain './late_dep' as a known-miss",
    );
    assert!(
        VerterHost::import_route_is_known_miss(&snapshot_resolution),
        "fixture invariant: './late_dep' must be a known-miss before the target appears"
    );

    // The late dep appears. The owner's IndexedReady is NOT evicted
    // (no eager reverse-dependent cascade), so:
    // - The owner's static `IndexedReady.import_route_hash` STILL
    //   reflects the miss.
    // - `generation_current_import_route_hash` re-resolves the miss
    //   against the now-current workspace and returns a DIFFERENT
    //   hash incorporating the positive resolution.
    let late_dep = "/proj_irh/late_dep.ts";
    upsert(
        &host,
        late_dep,
        "export type LateType = { resolved: true };\n",
    );

    // Sanity: the owner's IndexedReady survived (still carries the
    // stale known-miss snapshot).
    let owner_indexed_after = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady still present");
    let still_static_hash = owner_indexed_after.import_route_hash;
    assert_eq!(
        still_static_hash, static_hash,
        "sanity: owner's IndexedReady.import_route_hash is content-pinned \
         and MUST NOT change when an unrelated file appears (no eager \
         reverse-dependent cascade)"
    );

    // Compute the dynamic hash. The discriminator's core property:
    // dynamic != static after the late dep appears.
    let dynamic_hash = host
        .generation_current_import_route_hash(owner)
        .expect("owner has imports — dynamic hash must be Some");
    assert_ne!(
        Some(dynamic_hash),
        static_hash,
        "fixture invariant: after the late dep appears, the dynamic \
         hash MUST differ from the static IndexedReady.import_route_hash \
         — otherwise the discriminator does not characterise the unification"
    );

    // Build a HostStoreView. The view's
    // `snapshot_tracked_import_route_hashes` records the dynamic
    // hash. Verify that.
    let view = host.resolver_store_view_read().into_owned_view();
    let view_hash = view.derived_hash_for(owner, DerivedFactKind::ImportRoute);
    assert_eq!(
        view_hash,
        Some(dynamic_hash),
        "the HostStoreView MUST snapshot the dynamic import-route hash \
         (it uses generation_current_import_route_hash uniformly across \
         both the IndexedReady loop and the snapshot_tracked_* loop)"
    );

    // Reset the provenance counter so the assertion is scoped to the
    // two prepared_decl_bundle calls below.
    host.provenance().reset();

    // First call: cold-materialise the bundle. Counted as 1
    // materialisation.
    let _bundle_1 = host
        .prepared_decl_bundle_with_store_view(&view, owner)
        .expect("first prepared_decl_bundle call must materialise a bundle");
    let after_first = host.provenance().snapshot();
    assert_eq!(
        after_first.bundle_materializations, 1,
        "first prepared_decl_bundle call must materialise exactly 1 bundle"
    );

    // Second call: MUST warm-hit. Pre-fix the stored bundle's
    // ImportRoute fact carried the static (miss) hash; the view's
    // snapshot carries the dynamic (positive) hash; the warm-read
    // validator rejected the stored bundle; the second call
    // re-materialised → bundle_materializations == 2.
    //
    // Post-fix the stored bundle's ImportRoute fact carries the
    // dynamic hash (matching the view); the warm-read validator
    // accepts; bundle_materializations stays at 1.
    let _bundle_2 = host
        .prepared_decl_bundle_with_store_view(&view, owner)
        .expect("second prepared_decl_bundle call must return a bundle");
    let after_second = host.provenance().snapshot();
    assert_eq!(
        after_second.bundle_materializations, 1,
        "second prepared_decl_bundle call MUST warm-hit (no second \
         materialisation). Pre-fix the standard path admitted the \
         bundle with the static `facts.import_route_hash`, which \
         differs from the view's dynamic snapshot once `./late_dep` \
         appeared — the warm-read validator rejected and the second \
         call re-materialised (observed: \
         bundle_materializations == 2). The unification on the \
         dynamic hash closes this gap. Observed \
         bundle_materializations = {}",
        after_second.bundle_materializations
    );
    assert!(
        after_second.bundle_cache_hits >= 1,
        "second call must register at least one bundle cache hit; \
         observed cache hits = {}",
        after_second.bundle_cache_hits
    );
}
