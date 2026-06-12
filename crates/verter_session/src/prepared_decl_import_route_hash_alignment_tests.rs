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
//! - The routed-shallow path
//!   (`materialize_prepared_decl_bundle_from_routed_shallow`) uses
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
//! The fix unified both paths on the dynamic hash. The edge-currency
//! gate has since widened to EVERY cross-file-edge surface, which
//! closes the divergence class at its root: a gated
//! `ensure_indexed_ready` read after a dependency-set change takes the
//! edge-refresh and re-resolves the owner's routes, so the
//! content-pinned `IndexedReady.import_route_hash` a bundle admission
//! reads IS the generation-current hash — static/dynamic agreement is
//! now by construction at gated reads. This test pins that agreement
//! end-to-end plus the warm-hit property the unification bought.
//!
//! ## Pinned properties
//!
//! 1. Upsert an owner whose import `./late_dep` is unresolvable at
//!    index time — `IndexedReady.import_routes` snapshots it as a
//!    known-miss with `resolved_canonical_id = None`.
//! 2. Upsert the late dep AFTER the owner is indexed — the owner's
//!    content does not change, but its surface carries cross-file
//!    edges, so the gated re-read takes the EDGE-REFRESH: the
//!    refreshed artifact's `import_route_hash` moves off the stale
//!    miss-hash and equals `generation_current_import_route_hash`.
//! 3. Build a `HostStoreView` — its snapshot records that same hash.
//! 4. Cold-materialise the bundle via
//!    `prepared_decl_bundle_with_store_view` — counted as 1
//!    materialisation.
//! 5. Call `prepared_decl_bundle_with_store_view` again with the
//!    SAME view. The stored bundle MUST warm-hit (1 materialisation
//!    total, not 2): the admitted `ImportRoute` fact matches the
//!    view's snapshot. Pre-unification the standard path admitted the
//!    stale static miss-hash while the view carried the dynamic
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

    // The late dep appears. The owner's content does not change, but
    // its surface carries cross-file edges, so the dependency-set
    // change stales it at the edge-currency gate: the gated re-read
    // takes the EDGE-REFRESH and re-resolves `./late_dep` positively.
    let late_dep = "/proj_irh/late_dep.ts";
    upsert(
        &host,
        late_dep,
        "export type LateType = { resolved: true };\n",
    );

    let owner_indexed_after = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady still present");
    let refreshed_hash = owner_indexed_after.import_route_hash;
    assert_ne!(
        refreshed_hash, static_hash,
        "the gated re-read must serve an EDGE-REFRESHED surface whose \
         import_route_hash moved off the stale miss-hash — serving the \
         static snapshot means the edge-currency gate failed to stale \
         the owner's surface on the dependency-set change"
    );

    // Static/dynamic agreement at gated reads — the property the
    // unification (plus the edge-currency widening) guarantees by
    // construction: the refreshed content-pinned hash IS the
    // generation-current hash.
    let dynamic_hash = host
        .generation_current_import_route_hash(owner)
        .expect("owner has imports — dynamic hash must be Some");
    assert_eq!(
        refreshed_hash,
        Some(dynamic_hash),
        "the refreshed IndexedReady.import_route_hash must equal \
         generation_current_import_route_hash — bundle admissions and \
         view snapshots must observe ONE hash for one surface"
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

/// Unit-level pin on the bundle-admission `ImportRoute` hash SOURCE.
///
/// The fixture above proves static/dynamic AGREEMENT at gated reads, so a
/// regression re-introducing static-hash admission in
/// `materialize_prepared_decl_bundle` would still pass its warm-hit
/// assertion (the two sources are value-identical there by construction).
/// This pin builds the one state where the sources DIVERGE at admission
/// time — an owner with NO syntax imports (static `IndexedReady`
/// route table is empty ⇒ static `import_route_hash` is `None`) whose
/// route surface lives ONLY in the post-index `DerivedRawState` memo (the
/// prefetch class records routes after the artifact materialised; they do
/// not back-fill the `IndexedReady`) — and then inspects the ADMITTED
/// bundle's recorded fact signature directly:
///
/// - dynamic admission (correct): the signature carries
///   `DerivedFactHash { owner, ImportRoute, generation_current hash }`.
/// - static admission (regression): `facts.import_route_hash` is `None`,
///   so NO `ImportRoute` fact is recorded at all — and the missing fact
///   does NOT fail warm validation (fewer facts only), which is exactly
///   why the warm-hit assertion alone cannot catch the regression.
#[test]
fn bundle_admission_records_dynamic_import_route_hash_when_static_is_absent() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let owner = "/proj_irh2/owner.ts";
    // NO imports — the IndexedReady bakes an EMPTY route table.
    upsert(&host, owner, "export type Local = { ok: boolean };\n");
    let dep = "/proj_irh2/dep.ts";
    upsert(&host, dep, "export type Dep = { d: boolean };\n");

    let indexed = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady must materialise");
    assert_eq!(
        indexed.import_route_hash, None,
        "fixture invariant: an import-less owner has NO static import_route_hash"
    );

    // Post-index host-memoized positive — lands in DerivedRawState only.
    host.cache_positive_import_route_result_for_tests(
        owner,
        "./dep",
        dep,
        host.ws().content_generation(),
    );

    let dynamic_hash = host
        .generation_current_import_route_hash(owner)
        .expect("the DerivedRawState fallback answers with the post-index route");

    let view = host.resolver_store_view_read().into_owned_view();
    let _bundle = host
        .prepared_decl_bundle_with_store_view(&view, owner)
        .expect("bundle materialises for the import-less owner");

    let signatures = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .candidate_signatures_for_key(&owner.to_string());
    assert!(
        !signatures.is_empty(),
        "the bundle admission must have recorded a fact signature"
    );
    let expected = crate::resolver_core::FactVersionRef::DerivedFactHash {
        canonical_id: owner.to_string(),
        kind: DerivedFactKind::ImportRoute,
        hash: dynamic_hash,
    };
    assert!(
        signatures
            .iter()
            .any(|signature| signature.iter().any(|fact| *fact == expected)),
        "ADMISSION HASH SOURCE REGRESSION: the admitted bundle's fact \
         signature must carry the DYNAMIC \
         `generation_current_import_route_hash` ImportRoute fact. A static \
         `facts.import_route_hash` admission records NO ImportRoute fact \
         here (the static hash is None for an import-less owner), leaving \
         the bundle blind to route changes that only the DerivedRawState \
         memo tracks. Recorded signatures: {signatures:?}"
    );
}
