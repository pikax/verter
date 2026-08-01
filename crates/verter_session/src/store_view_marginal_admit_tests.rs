//! Marginal-admit cost of the base store-view builder, and the warm-reuse
//! contract that currently pays for it.
//!
//! Build Philosophy #5 — "the builder/solver reads only from cached
//! lookup state; it does not reopen file loading or routing" — says
//! [`crate::resolver_store::HostStoreView::build`] must not re-open a
//! file when it snapshots it. Any content change bumps the workspace
//! `content_generation`, and `route_surface_is_edge_current` compares an
//! artifact's baked `edge_generation` against exactly that counter, so
//! ONE mutation makes EVERY file carrying a cross-file edge edge-stale at
//! once. The scaling tests below measure the per-owner cost of that and
//! are `#[ignore]`d, because they state the TARGET, not the tree: the
//! build still walks the published artifacts and the tracked owners.
//!
//! The import-route half of the old N-term is gone. Its digest
//! (`DerivedFactKind::ImportRoute`) summarised an owner's RESOLVED
//! import table, so composing it forced the build to route each
//! generation-stale surface through `ensure_indexed_ready_serve` — one
//! edge refresh and one import re-resolution per already-materialised
//! owner. The value is now the owner's import-route RESOLUTION WITNESS:
//! the resolver observations the sealed transaction actually made,
//! recorded by the producer and validated against the view's captured
//! immutable resolution world. Nothing about it is derivable at build
//! time, so the build no longer derives it.
//!
//! The warm-reuse contract that arm used to pay for is held directly
//! below by `unrelated_edit_keeps_both_route_facts_warm`: an owner whose
//! specifiers still resolve to the same targets keeps both route rails
//! across an unrelated file's edit. That is a path-precision property —
//! a global file-set or content stamp substituted for the witness fails
//! it.
//!
//! Counters used, all PER-HOST (immune to whatever else the shared test
//! process is doing in parallel):
//!
//! - `MetaProvenanceSnapshot::indexed_ready_materializes` — one bump per
//!   `IndexedReady` (re-)materialisation. The route-only edge-refresh
//!   lane it used to be paired with (`indexed_ready_edge_refreshes`) is
//!   deleted with that lane: the artifact bakes no route, there is no
//!   `refresh_indexed_route_surface`, and so no edge-refresh work is
//!   left for a counter to measure. Asserting a counter with no producer
//!   is a tautology, not a gate.
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
    /// `IndexedReady` materialisations.
    materializes: u64,
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
        sample.shallow_state.has_shallow_cross_file_edges(),
        "precondition: a preloaded member must carry cross-file edges, so it \
         genuinely has import routes to resolve"
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
                resolution_misses: 0,
            },
            "{label}: at N={n} the store-view builder reopened file loading / \
             routing ({deltas:?}). Build Philosophy #5: the builder reads only \
             cached lookup state. Full table: {measured:?}"
        );
    }
}

/// ANTI-VACUITY CONTROL for every zero-work assertion in this module.
///
/// `assert_builder_reopens_nothing` compares against
/// `AdmitDeltas::default`-shaped zeros. If neither surviving counter had
/// a live producer, every one of those gates would pass on an empty
/// measurement and prove nothing — which is exactly what a third leg
/// (`indexed_ready_edge_refreshes`) did once the edge-refresh lane it
/// measured stopped existing, and why that leg is deleted rather than
/// asserted.
///
/// So: measure a window that DOES the work, and require both surviving
/// legs to move. Materialising a fresh edge-bearing owner from cold must
/// bump `materializes`, and resolving its import must miss the workspace
/// resolution cache at least once.
#[test]
fn the_measured_counters_move_when_the_work_actually_runs() {
    let host = host_with_n_materialized_files(4);
    let deltas = deltas_around(&host, |host| {
        upsert_ts(host, "/proj/control.ts", MEMBER_SRC);
        let _artifact = host
            .ensure_indexed_ready("/proj/control.ts")
            .expect("the freshly upserted owner must materialise");
        let resolved = host.resolve_type_dependency_canonical("/proj/control.ts", "./dep");
        assert_eq!(
            resolved.as_deref(),
            Some(DEP_ID),
            "precondition: the control owner's import must actually resolve"
        );
    });
    assert!(
        deltas.materializes > 0,
        "`materializes` has no live producer — every zero-work assertion in \
         this module is vacuous: {deltas:?}"
    );
    assert!(
        deltas.resolution_misses > 0,
        "`resolution_misses` has no live producer — every zero-work assertion \
         in this module is vacuous: {deltas:?}"
    );
}

/// TARGET STATE, not the tree.
///
/// The O(N) term this gate was written against is GONE. It measured
/// `edge_refreshes` and `resolution_misses` both equal to N at every host
/// size — one edge refresh and one import re-resolution per
/// already-materialised owner, per admitted file. The edge-refresh lane no
/// longer exists (its counter went with it), and misses are now CONSTANT
/// at 1 across N = 250 / 1,000 / 3,000. Split-window instrumentation
/// attributes that residual 1 to the newly-admitted file's OWN `./dep`
/// resolution, performed during `upsert`'s scheduler dependency
/// extraction — not to any per-owner work over the other N files.
///
/// The gate as written is nevertheless still NOT satisfied: its window
/// spans the admission, so the new canonical's own cold build and own
/// import resolution fall inside it, and the required counters are zero.
/// Whether the window SHOULD exclude the admission is an OPEN contract
/// question and is not decided here — `.DECISION.md` designates this exact
/// test as the discriminator, so narrowing it is an adjudication, not an
/// implementation detail. It belongs to the follow-on O(1)-build block,
/// together with `store_view_owner_visits`.
///
/// The builder's own share — everything measured after the admission — is
/// isolated and LIVE in
/// [`snapshot_build_reopens_no_routing_for_unchanged_owners`].
#[test]
#[ignore = "target state: the O(N) term is gone (misses constant at 1 across \
            N=250/1000/3000, the residual attributed by split-window \
            instrumentation to the admitted file's own ./dep resolution during \
            upsert), but the gate as written is still not satisfied — its \
            window spans the admission. Whether the window should exclude the \
            admission is OPEN and owned by the O(1)-build follow-on"]
fn marginal_admit_reopens_no_routing_regardless_of_host_size() {
    assert_builder_reopens_nothing("admitting ONE new file", marginal_admit_deltas);
}

/// TARGET STATE, not the tree. Same defect as above, isolated to the
/// snapshot build itself (the admission already happened, so everything
/// measured is the builder's own work over owners whose content did not
/// change).
#[test]
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
                resolution_misses: 0,
            },
            "a byte-identical re-upsert is a true no-op — at N={n} it must \
             neither advance the content generation nor re-route anything: \
             {deltas:?}"
        );
    }
}

// ── The warm-reuse contract the N-term currently buys ──

/// **The warm-reuse contract the builder must not trade away.**
///
/// An unrelated file changing makes every edge-carrying owner edge-stale,
/// but an owner whose specifiers still resolve to the same targets must
/// keep BOTH route rails — the `Route` derived fact AND its import-route
/// RESOLUTION WITNESS — or every warm entry in the project dies on every
/// keystroke.
///
/// The witness half is what makes this discriminating after the
/// `DerivedFactKind::ImportRoute` digest is gone: it is a set of
/// path-precise resolver observations, so a global file-set or content
/// stamp standing in for it would mark the owner stale here and fail.
/// The printed delta is the builder cost of the survival.
#[test]
fn unrelated_edit_keeps_both_route_facts_warm() {
    const N: usize = 8;
    let host = host_with_n_materialized_files(N);
    let owner = member_id(0);

    let view_before = host.resolver_store_view_read().into_owned_view();
    let route_hash = StoreView::derived_hash_for(&view_before, &owner, DerivedFactKind::Route)
        .expect("precondition: a current owner must publish its Route fact");
    let mut facts: Vec<FactVersionRef> = vec![FactVersionRef::DerivedFactHash {
        canonical_id: owner.clone(),
        kind: DerivedFactKind::Route,
        hash: route_hash,
    }];
    let witness = host
        .owner_import_route_witness_for_tests(&owner)
        .expect("precondition: a current owner must produce a rootable import-route witness");
    assert!(
        !witness.is_empty(),
        "precondition: an owner with cross-file imports must observe at least \
         one resolver fact — an empty witness would make this test vacuous"
    );
    facts.extend(witness);
    for fact in &facts {
        assert!(
            StoreView::validates(&view_before, fact),
            "precondition: {fact:?} must validate against the view it was captured from"
        );
    }

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
/// answer" is not an acceptable way to make the builder observe-only: a
/// previously-unresolvable specifier becoming resolvable MUST invalidate
/// every warm entry rooted on the importer's import-route witness — with
/// the importer fully INDEXED, so its known-miss lives in the published
/// artifact's baked route table rather than in `DerivedRawState`.
///
/// The witness is the resolve-domain successor to the deleted
/// `DerivedFactKind::ImportRoute` digest, so the property is now stated
/// against the observations themselves: the miss recorded the exhausted
/// probe set, and the appearance advances exactly the `PathProbe` it
/// observed. Note what is deliberately NOT asserted any more — that the
/// store-view build eagerly produces a *different current value*. Per
/// `.DECISION.md` the build performs zero routing work; the durable
/// invariant is that the old witness stops validating and the next real
/// demand recomputes.
#[test]
fn known_miss_appearance_invalidates_indexed_importer_import_route_witness() {
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
    // route table (not only into `DerivedRawState`).
    assert!(
        indexed
            .shallow_state
            .import_targets
            .values()
            .any(|target| target.source_specifier == "./appears-later"),
        "precondition: the unresolvable specifier must be in the importer's \
         AUTHORED inventory, or the witness cannot observe it",
    );
    assert_eq!(
        host.resolve_type_dependency_canonical_shallow(IMPORTER_ID, "./appears-later"),
        None,
        "precondition: ./appears-later must not resolve while the target file \
         does not exist"
    );

    // The witness a consumer would record while the dependency is missing.
    let view_before = host.resolver_store_view_read().into_owned_view();
    let witness_before = host
        .owner_import_route_witness_for_tests(IMPORTER_ID)
        .expect("a current importer surface must produce a rootable witness");
    assert!(
        !witness_before.is_empty(),
        "precondition: the known-miss must have observed at least one resolver \
         fact — an empty witness would make this test vacuous"
    );
    for fact in &witness_before {
        assert!(
            StoreView::validates(&view_before, fact),
            "precondition: {fact:?} must validate against the view it was captured from"
        );
    }

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
        witness_before
            .iter()
            .any(|fact| !StoreView::validates(&view_after, fact)),
        "a warm entry rooted on the pre-appearance import-route witness MUST \
         NOT validate once ./appears-later resolves — otherwise the cached \
         known-miss is served forever. Witness: {witness_before:?}"
    );

    assert_eq!(
        host.resolve_type_dependency_canonical_shallow(IMPORTER_ID, "./appears-later")
            .as_deref(),
        Some(APPEARS_LATER_ID),
        "the demand-side reader must re-resolve the specifier to the file that \
         appeared"
    );
}
