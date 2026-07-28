//! Marginal-admit cost of the base store-view builder, and the warm-reuse
//! contract that currently pays for it.
//!
//! Build Philosophy #5 — "the builder/solver reads only from cached
//! lookup state; it does not reopen file loading or routing" — says
//! [`crate::resolver_store::HostStoreView::build`] must not re-open a
//! file when it snapshots it. It does. Any content change bumps the
//! workspace `content_generation`, `route_surface_is_edge_current`
//! compares an artifact's baked `edge_generation` against exactly that
//! counter, so ONE mutation makes EVERY file carrying a cross-file edge
//! edge-stale at once — and the builder's `ImportRoute` arm routes each
//! stale surface through `ensure_indexed_ready_serve`. Cost per
//! admission: one edge refresh and one import re-resolution per
//! already-materialised owner in the host. The scaling tests below
//! measure exactly that and are `#[ignore]`d, because they state the
//! TARGET, not the tree.
//!
//! **Why the obvious fix is not landable on its own.** Making the
//! `ImportRoute` arm observe-only (declining, or re-deriving the hash
//! side-effect-free) removes the whole N-term and keeps the `ImportRoute`
//! fact correct — and still regresses the project catastrophically,
//! because the `Route` fact FREE-RIDES on that arm's re-index. The
//! `Route` arm is already observe-only: it publishes nothing for an
//! edge-stale surface. Today it survives an unrelated edit only because
//! the `ImportRoute` arm re-indexes the owner mid-build,
//! `artifact_generation` moves, `build_coherent`'s coherence check fails,
//! and the RETRY re-runs the `Route` arm against the now-refreshed
//! artifact. Remove the re-index and every warm entry rooted on a `Route`
//! fact dies on every keystroke in any unrelated file. Measured, not
//! reasoned: `unrelated_edit_keeps_both_route_facts_warm` below holds
//! that contract, and it is what makes the observe-only change fail.
//!
//! So the durable fix is not in this builder at all: it is that
//! `content_generation` is the wrong stamp for EDGE currency. An import
//! resolves against the file SET; editing the contents of an existing
//! file cannot retarget anything. Until edge currency keys on something
//! that only moves when the file set moves, the builder has no cheap way
//! to know an owner's baked edges are still good, and the N-term cannot
//! be removed without giving up warm reuse.
//!
//! Counters used, all PER-HOST (immune to whatever else the shared test
//! process is doing in parallel):
//!
//! - `MetaProvenanceSnapshot::indexed_ready_materializes` /
//!   `indexed_ready_edge_refreshes` — one bump per `IndexedReady`
//!   (re-)materialisation and per route-surface edge refresh.
//! - `VfsProvenanceSnapshot::import_resolution_cache_miss_count` — one
//!   bump per import resolution that misses the workspace lazy
//!   resolution cache and goes to the resolver (filesystem probing).
//!
//! Every number is derived as a pre/post delta around a single API call
//! and compared ACROSS host sizes. Nothing is hardcoded, nothing is
//! wall-clock. The process-global
//! [`crate::resolver_store::store_view_coherent_build_sweeps`] counter is
//! deliberately NOT asserted on: it is shared by every test in the
//! process and cannot carry a per-host claim.

use std::sync::Arc;

use crate::resolver_core::{DerivedFactKind, FactVersionRef, StoreView};
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

/// Per-host counter deltas measured around one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdmitDeltas {
    /// `IndexedReady` materialisations (cold builds + edge refreshes).
    materializes: u64,
    /// The edge-refresh sub-lane of the above.
    edge_refreshes: u64,
    /// Import resolutions that missed the workspace lazy-resolution
    /// cache (each is a resolver walk / filesystem probe).
    resolution_misses: u64,
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|err| panic!("upsert {id} must succeed: {err:?}"));
}

const DEP_ID: &str = "/proj/dep.ts";
const DEP_SRC: &str = "export const d = 1\nexport interface D { x: number }\n";

fn member_id(i: usize) -> String {
    format!("/proj/f{i}.ts")
}

const MEMBER_SRC: &str =
    "import { d, type D } from './dep'\nexport const use = d\nexport type R = D\n";

/// A host holding `n` already-materialised `.ts` files, each carrying a
/// real cross-file import edge (so each artifact's `import_routes` is
/// non-empty and its edge currency is decided by the workspace
/// `content_generation` stamp).
fn host_with_n_materialized_files(n: usize) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_ts(&host, DEP_ID, DEP_SRC);
    for i in 0..n {
        upsert_ts(&host, &member_id(i), MEMBER_SRC);
    }
    // Materialise every file once. This is the state a warm host is in
    // after a project load: N published `IndexedReady` artifacts.
    let _ = host.ensure_indexed_ready_serve(DEP_ID);
    for i in 0..n {
        let _ = host.ensure_indexed_ready_serve(&member_id(i));
    }

    // Preconditions — without these the measurement could silently
    // degenerate into "there was nothing to re-route anyway".
    let sample = host
        .project_type_store()
        .indexed()
        .get_any(&member_id(0))
        .expect("every preloaded member must be materialised");
    assert!(
        !sample.import_routes.is_empty(),
        "precondition: a preloaded member must carry a resolved import route \
         (got an empty route table — the fixture no longer exercises the \
         ImportRoute domain)"
    );
    assert!(
        sample.has_cross_file_edges(),
        "precondition: a preloaded member must carry cross-file edges, so its \
         edge currency is decided by the workspace content generation"
    );
    assert!(
        host.indexed_surface_is_current(&member_id(0), &sample),
        "precondition: the preloaded members must start edge-current"
    );
    host
}

fn deltas_around(host: &VerterHost, op: impl FnOnce(&VerterHost)) -> AdmitDeltas {
    let meta_before = host.provenance().snapshot();
    let vfs_before = host.ws().vfs_provenance_snapshot();
    op(host);
    let meta_after = host.provenance().snapshot();
    let vfs_after = host.ws().vfs_provenance_snapshot();
    AdmitDeltas {
        materializes: meta_after.indexed_ready_materializes
            - meta_before.indexed_ready_materializes,
        edge_refreshes: meta_after.indexed_ready_edge_refreshes
            - meta_before.indexed_ready_edge_refreshes,
        resolution_misses: vfs_after.import_resolution_cache_miss_count
            - vfs_before.import_resolution_cache_miss_count,
    }
}

/// Admit ONE brand-new file into an N-file host and read the base store
/// view once. Everything in the window is the marginal cost of that one
/// admission.
fn marginal_admit_deltas(n: usize) -> AdmitDeltas {
    let host = host_with_n_materialized_files(n);
    deltas_around(&host, |host| {
        upsert_ts(host, "/proj/new.ts", MEMBER_SRC);
        let _view = host.resolver_store_view_read().into_owned_view();
    })
}

/// Re-upsert an EXISTING file with unchanged bytes and read the base
/// store view once.
fn recompile_existing_deltas(n: usize) -> AdmitDeltas {
    let host = host_with_n_materialized_files(n);
    deltas_around(&host, |host| {
        upsert_ts(host, &member_id(0), MEMBER_SRC);
        let _view = host.resolver_store_view_read().into_owned_view();
    })
}

/// Isolate the SNAPSHOT BUILD: admit the new file first (outside the
/// measured window), then measure only the store-view read that the
/// admission's token advance forces to rebuild.
fn snapshot_build_only_deltas(n: usize) -> AdmitDeltas {
    let host = host_with_n_materialized_files(n);
    upsert_ts(&host, "/proj/new.ts", MEMBER_SRC);
    deltas_around(&host, |host| {
        let _view = host.resolver_store_view_read().into_owned_view();
    })
}

/// The host sizes every scaling claim is derived over. A single pair
/// could be coincidence; three points an order of magnitude apart cannot
/// be flat by accident.
const HOST_SIZES: [usize; 3] = [250, 1000, 3000];

fn measure_across_host_sizes(
    label: &str,
    measure: fn(usize) -> AdmitDeltas,
) -> Vec<(usize, AdmitDeltas)> {
    let measured: Vec<(usize, AdmitDeltas)> = HOST_SIZES.iter().map(|&n| (n, measure(n))).collect();
    // Printed so the scaling table is reproducible evidence under
    // `--nocapture`, not just a pass/fail bit.
    eprintln!("{label}: {measured:?}");
    measured
}

fn assert_builder_reopens_nothing(label: &str, measure: fn(usize) -> AdmitDeltas) {
    let measured = measure_across_host_sizes(label, measure);
    for &(n, deltas) in &measured {
        assert_eq!(
            deltas,
            AdmitDeltas {
                materializes: 0,
                edge_refreshes: 0,
                resolution_misses: 0,
            },
            "{label}: at N={n} the store-view builder reopened file loading / \
             routing ({deltas:?}). Build Philosophy #5: the builder reads only \
             cached lookup state. Full table: {measured:?}"
        );
    }
}

/// TARGET STATE, not the tree.
///
/// Currently fails with `edge_refreshes` and `resolution_misses` both
/// equal to N at every host size — one edge refresh and one import
/// re-resolution per already-materialised owner, per admitted file. That
/// is the O(N²) cold-build term.
///
/// Un-ignoring this requires edge currency to stop keying on
/// `content_generation` (see the module docs): the `ImportRoute` arm
/// cannot simply be made observe-only, because
/// `unrelated_edit_keeps_both_route_facts_warm` below shows the `Route`
/// fact free-rides on its re-index.
#[test]
#[ignore = "target state: the builder still re-indexes every edge-stale owner; \
            blocked on edge currency keying on the file set rather than \
            content_generation — see the module docs"]
fn marginal_admit_reopens_no_routing_regardless_of_host_size() {
    assert_builder_reopens_nothing("admitting ONE new file", marginal_admit_deltas);
}

/// TARGET STATE, not the tree. Same defect as above, isolated to the
/// snapshot build itself (the admission already happened, so everything
/// measured is the builder's own work over owners whose content did not
/// change).
#[test]
#[ignore = "target state: same defect as \
            marginal_admit_reopens_no_routing_regardless_of_host_size, \
            isolated to the snapshot build"]
fn snapshot_build_reopens_no_routing_for_unchanged_owners() {
    assert_builder_reopens_nothing(
        "the store-view snapshot build after an admission",
        snapshot_build_only_deltas,
    );
}

/// Holds today: a byte-identical re-upsert is a true no-op, so it never
/// advances `content_generation`, nothing goes edge-stale, and the
/// builder does no work at any host size. This is the shape the two
/// ignored tests above are asking for on the content-CHANGING paths.
#[test]
fn recompile_existing_is_free_at_any_host_size() {
    let measured =
        measure_across_host_sizes("re-upserting an UNCHANGED file", recompile_existing_deltas);
    for &(n, deltas) in &measured {
        assert_eq!(
            deltas,
            AdmitDeltas {
                materializes: 0,
                edge_refreshes: 0,
                resolution_misses: 0,
            },
            "a byte-identical re-upsert is a true no-op — at N={n} it must \
             neither advance the content generation nor re-route anything: \
             {deltas:?}"
        );
    }
}

// ── The warm-reuse contract the N-term currently buys ──

/// **The constraint that blocks the obvious fix.**
///
/// An unrelated file changing makes every edge-carrying owner edge-stale,
/// but an owner whose specifiers still resolve to the same targets must
/// keep BOTH its derived facts — `Route` as well as `ImportRoute` — or
/// every warm entry in the project dies on every keystroke.
///
/// `ImportRoute` survives because its producer re-resolves. `Route`
/// survives only as a SIDE EFFECT: the `ImportRoute` arm re-indexes the
/// owner mid-build, which moves `artifact_generation`, fails
/// `build_coherent`'s coherence check, and the retry re-runs the
/// (observe-only, publishes-nothing-for-stale) `Route` arm against the
/// refreshed artifact.
///
/// So this test fails the moment the `ImportRoute` arm stops
/// materialising — which is exactly what the ignored scaling tests above
/// ask for. Any change that removes the N-term must keep this green by
/// giving `Route` its own currency answer, not by dropping the fact. The
/// printed delta is the price currently paid for that survival.
#[test]
fn unrelated_edit_keeps_both_route_facts_warm() {
    const N: usize = 8;
    let host = host_with_n_materialized_files(N);
    let owner = member_id(0);

    let view_before = host.resolver_store_view_read().into_owned_view();
    let facts: Vec<FactVersionRef> = [DerivedFactKind::Route, DerivedFactKind::ImportRoute]
        .into_iter()
        .map(|kind| {
            let hash =
                StoreView::derived_hash_for(&view_before, &owner, kind).unwrap_or_else(|| {
                    panic!("precondition: a current owner must publish its {kind:?} fact")
                });
            FactVersionRef::DerivedFactHash {
                canonical_id: owner.clone(),
                kind,
                hash,
            }
        })
        .collect();

    // Edit an UNRELATED file. The owner does not import it, its own
    // content is unchanged, and none of its specifiers retarget.
    let deltas = deltas_around(&host, |host| {
        upsert_ts(host, "/proj/unrelated.ts", "export const q = 1\n");
        let _view = host.resolver_store_view_read().into_owned_view();
    });
    eprintln!("unrelated-edit warm survival cost at N={N}: {deltas:?}");

    let stale = host
        .project_type_store()
        .indexed()
        .get_any(&owner)
        .expect("the owner artifact is still published");
    let edge_current_after = host.indexed_surface_is_current(&owner, &stale);
    let view_after = host.resolver_store_view_read().into_owned_view();
    for fact in &facts {
        assert!(
            StoreView::validates(&view_after, fact),
            "a warm entry rooted on the owner's {fact:?} MUST survive an \
             unrelated file's edit — the owner imports nothing that changed. \
             Owner surface edge-current after the edit: {edge_current_after}. \
             Builder cost of this survival: {deltas:?}"
        );
    }
}

/// The other direction, and the reason "reuse the owner's last recorded
/// hash" is not an acceptable way to make the builder observe-only: a
/// previously-unresolvable specifier becoming resolvable MUST invalidate
/// every warm entry rooted on the importer's `ImportRoute` fact — with
/// the importer fully INDEXED, so its known-miss lives in the published
/// artifact's baked route table rather than in `DerivedRawState`.
#[test]
fn known_miss_appearance_invalidates_indexed_importer_import_route_fact() {
    const IMPORTER_ID: &str = "/proj/importer.ts";
    const APPEARS_LATER_ID: &str = "/proj/appears-later.ts";

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_ts(
        &host,
        IMPORTER_ID,
        "import { X } from './appears-later'\nexport const y = X\n",
    );
    let indexed = host
        .ensure_indexed_ready_serve(IMPORTER_ID)
        .expect("the importer must materialise")
        .indexed;

    // Precondition: the miss is baked into the PUBLISHED artifact's
    // route table (not only into `DerivedRawState`), which is the arm
    // the store-view builder reads.
    let recorded = indexed.import_routes.get("./appears-later").expect(
        "precondition: the unresolvable specifier must be recorded in the \
         artifact's baked route table",
    );
    assert!(
        VerterHost::import_route_is_known_miss(recorded),
        "precondition: ./appears-later must be recorded as a known-miss while \
         the target file does not exist"
    );

    // The fact a consumer would record while the dependency is missing.
    let view_before = host.resolver_store_view_read().into_owned_view();
    let hash_before =
        StoreView::derived_hash_for(&view_before, IMPORTER_ID, DerivedFactKind::ImportRoute)
            .expect("a current importer surface must publish an ImportRoute fact");
    let cached_fact = FactVersionRef::DerivedFactHash {
        canonical_id: IMPORTER_ID.to_string(),
        kind: DerivedFactKind::ImportRoute,
        hash: hash_before,
    };
    assert!(
        StoreView::validates(&view_before, &cached_fact),
        "precondition: the fact must validate against the view it was captured from"
    );

    // The dependency appears. The importer's own content — hence its
    // published `IndexedReady` — does not change.
    upsert_ts(&host, APPEARS_LATER_ID, "export const X = 1\n");
    assert_eq!(
        host.project_type_store()
            .indexed()
            .get_any(IMPORTER_ID)
            .map(|i| i.whole_hash),
        Some(indexed.whole_hash),
        "precondition: the importer's published artifact must be unchanged — \
         this test is about a dependency-set change, not a content change"
    );

    let view_after = host.resolver_store_view_read().into_owned_view();
    assert!(
        !StoreView::validates(&view_after, &cached_fact),
        "a warm entry rooted on the pre-appearance ImportRoute fact MUST NOT \
         validate once ./appears-later resolves — otherwise the cached \
         known-miss is served forever"
    );

    let hash_after =
        StoreView::derived_hash_for(&view_after, IMPORTER_ID, DerivedFactKind::ImportRoute)
            .expect("the importer must still publish an ImportRoute fact");
    assert_ne!(
        hash_before, hash_after,
        "the post-appearance ImportRoute hash must differ from the \
         pre-appearance one"
    );
    assert_eq!(
        host.current_content_pinned_indexed(IMPORTER_ID)
            .and_then(|i| i
                .import_routes
                .get("./appears-later")
                .and_then(|r| r.resolved_canonical_id.clone())),
        Some(APPEARS_LATER_ID.to_string()),
        "the demand-side reader must re-resolve the specifier to the file that \
         appeared"
    );
}
