//! Marginal-admit cost of the base store-view builder, and the warm-reuse
//! contract that currently pays for it.
//!
//! Build Philosophy #5 says [`crate::resolver_store::HostStoreView::build`]
//! captures immutable roots in O(1): it must not enumerate owners, reopen
//! files, or perform routing. The scaling tests below pin that absence of
//! per-owner work while separately accounting for legitimate admission cost.
//!
//! Every measured window is split so it contains ONLY builder work. A
//! newly-admitted file's own parse during `upsert` is legitimate work owed by
//! admission, not by the builder. Ordinary import resolution is workspace-owned,
//! however, and this fixture's `./dep` decision is already warm: admitting one
//! more owner must reuse it without another resolver miss. Admission is therefore
//! performed outside the build window and its observable routing cost is pinned
//! separately as zero at every host size.
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
//! Counters used, none of them process-global:
//!
//! - `MetaProvenanceSnapshot::indexed_ready_materializes` — PER-HOST, one
//!   bump per `IndexedReady` (re-)materialisation. The route-only
//!   edge-refresh lane it used to be paired with
//!   (`indexed_ready_edge_refreshes`) is deleted with that lane: the
//!   artifact bakes no route, there is no `refresh_indexed_route_surface`,
//!   and so no edge-refresh work is left for a counter to measure.
//!   Asserting a counter with no producer is a tautology, not a gate.
//! - `VfsProvenanceSnapshot::import_resolution_cache_miss_count` —
//!   PER-HOST, one bump per import resolution that misses the workspace
//!   lazy resolution cache and goes to the resolver (filesystem probing).
//! - [`store_view_owner_visits`] — PER-THREAD, one bump per read through a
//!   view's captured roots while a store-view BUILD scope is active. A
//!   build runs to completion on the calling thread, so a thread-local
//!   reading carries a per-measurement claim in exactly the way the
//!   process-global
//!   [`crate::resolver_store::store_view_coherent_build_sweeps`] cannot —
//!   which is why that one is still deliberately not asserted on here.
//!
//! Every number is derived as a pre/post delta around a single API call
//! and compared ACROSS host sizes. Nothing is hardcoded, nothing is
//! wall-clock.
//!
//! Each counter carries an anti-vacuity control proving it has a live
//! producer, because every gate below asserts a ZERO and a zero from a
//! counter nothing bumps is not evidence:
//! [`the_measured_counters_move_when_the_work_actually_runs`] for the two
//! per-host legs, and
//! [`the_owner_visit_counter_moves_only_inside_a_build_scope`] for the
//! owner-visit leg.

use std::sync::Arc;

use crate::resolver_core::{DerivedFactKind, FactVersionRef, StoreView};
use crate::store_view_roots::{reset_store_view_owner_visits, store_view_owner_visits};
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

/// Counter deltas measured around one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdmitDeltas {
    /// Owner reads through a view's captured roots performed while a
    /// store-view BUILD scope was active. The O(1)-build contract requires
    /// this to be zero: capture is a fixed number of scalar reads and
    /// `Arc` clones, so a build that touches an owner is a re-introduced
    /// N-term.
    owner_visits: u64,
    /// `IndexedReady` materialisations.
    materializes: u64,
    /// Import resolutions that missed the workspace lazy-resolution
    /// cache (each is a resolver walk / filesystem probe).
    resolution_misses: u64,
}

/// The all-zero reading every observe-only window must produce.
const NO_WORK: AdmitDeltas = AdmitDeltas {
    owner_visits: 0,
    materializes: 0,
    resolution_misses: 0,
};

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
/// non-empty, making accidental eager route refresh observable).
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
    reset_store_view_owner_visits();
    op(host);
    let owner_visits = store_view_owner_visits();
    let meta_after = host.provenance().snapshot();
    let vfs_after = host.ws().vfs_provenance_snapshot();
    AdmitDeltas {
        owner_visits,
        materializes: meta_after.indexed_ready_materializes
            - meta_before.indexed_ready_materializes,
        resolution_misses: vfs_after.import_resolution_cache_miss_count
            - vfs_before.import_resolution_cache_miss_count,
    }
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

/// The ADMISSION's own cost, with no store-view read in the window at all.
///
/// A brand-new owner still has to parse, but ordinary import resolution is
/// workspace-owned and `./dep` already has a reusable workspace decision from
/// fixture construction. This window pins that admission neither reopens an
/// owner nor rematerialises or re-resolves the already-warm dependency.
fn admission_only_deltas(n: usize) -> AdmitDeltas {
    let host = host_with_n_materialized_files(n);
    // Warm the view cache first, so nothing in the window is a store-view
    // build that happened to be owed from the fixture build-up.
    let _warm = host.resolver_store_view_read().into_owned_view();
    deltas_around(&host, |host| {
        upsert_ts(host, "/proj/new.ts", MEMBER_SRC);
    })
}

/// Isolate the SNAPSHOT BUILD: admit the new file first (outside the
/// measured window), then measure only the store-view read that the
/// admission's token advance forces to rebuild.
///
/// The token miss is FORCED and PROVEN, not assumed: the view is read
/// before the admission and its validation token recorded, and the token
/// of the view read inside the window must differ. Without that check a
/// zero reading could just mean the cached view was handed back and no
/// build ran at all — the same vacuity trap as a counter with no producer.
fn snapshot_build_only_deltas(n: usize) -> AdmitDeltas {
    let host = host_with_n_materialized_files(n);
    let token_before = host
        .resolver_store_view_read()
        .into_owned_view()
        .validation_token();

    // ── outside the measurement window ──
    upsert_ts(&host, "/proj/new.ts", MEMBER_SRC);

    let mut token_after = token_before;
    let deltas = deltas_around(&host, |host| {
        token_after = host
            .resolver_store_view_read()
            .into_owned_view()
            .validation_token();
    });
    assert_ne!(
        token_after, token_before,
        "precondition at N={n}: admitting a new file must advance the \
         validation token, so the cached view cannot be reused and the \
         measured read is a genuine BUILD. Equal tokens would make the \
         zero-work assertion vacuous."
    );
    deltas
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
            deltas, NO_WORK,
            "{label}: at N={n} the store-view builder reopened file loading / \
             routing, or visited an owner through its captured roots \
             ({deltas:?}). Build Philosophy #5: the builder reads only cached \
             lookup state, and capture is a fixed number of scalar reads and \
             `Arc` clones. Full table: {measured:?}"
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

/// ANTI-VACUITY CONTROL for the owner-visit leg of every zero above.
///
/// `store_view_owner_visits` is scope-gated: it counts a read through a
/// view's captured roots only while a store-view BUILD scope is active.
/// That gating is what makes it a build measurement rather than a
/// whole-process one, and it is also exactly how the counter could become
/// silently dead — instrument a boundary the build cannot reach, or gate on
/// a scope nothing enters, and the gate's zero means nothing.
///
/// So the control drives the SAME real owner read both ways and requires
/// the counter to discriminate: it moves inside a build scope, and it does
/// not move outside one. The read is required to resolve to a real owner,
/// because a read that found nothing would exercise neither the roots nor
/// the claim.
#[test]
fn the_owner_visit_counter_moves_only_inside_a_build_scope() {
    let host = host_with_n_materialized_files(4);
    let owner = member_id(0);
    let view = host.resolver_store_view_read().into_owned_view();

    reset_store_view_owner_visits();
    let outside = view.owner_read_through_roots_for_tests(&owner, false);
    let visits_outside = store_view_owner_visits();

    reset_store_view_owner_visits();
    let inside = view.owner_read_through_roots_for_tests(&owner, true);
    let visits_inside = store_view_owner_visits();

    assert!(
        inside.is_some(),
        "the probe must reach a REAL owner through the captured roots — a \
         read that resolved nothing would prove nothing about the \
         instrumentation"
    );
    assert_eq!(
        inside, outside,
        "the probe must be the identical read either way; only the scope \
         differs"
    );
    assert_eq!(
        visits_inside, 1,
        "`store_view_owner_visits` has no live producer on the root read \
         path — every zero-owner-visit assertion in this module is vacuous"
    );
    assert_eq!(
        visits_outside, 0,
        "`store_view_owner_visits` is not scope-gated: it counts demand-time \
         reads too, so its zero would be a claim about the whole window \
         rather than about the BUILD"
    );
}

/// **The gate.**
///
/// Admitting one new file into a host of N already-materialised files must
/// cost the store-view BUILD nothing that scales with N — nothing at all,
/// in fact: zero owner visits through the captured roots, zero
/// `IndexedReady` materialisations, zero import-resolution misses, at
/// N = 250 / 1,000 / 3,000.
///
/// The measurement window deliberately EXCLUDES the admission itself because
/// parse work is not store-view build work. The routing counters for that
/// excluded window are still pinned separately: the dependency decision is
/// already warm, so admission must add zero routing work at every host size in
/// [`admission_reuses_existing_workspace_resolution_at_any_host_size`].
///
/// What the window does contain is a genuine build, forced and proven: the
/// admission advances the validation token, so the cached view cannot be
/// reused, and `snapshot_build_only_deltas` asserts the token actually
/// moved before believing the reading.
///
/// The O(N) term this gate was written against measured `edge_refreshes`
/// and `resolution_misses` both equal to N at every host size — one edge
/// refresh and one import re-resolution per already-materialised owner, per
/// admitted file.
#[test]
fn marginal_admit_reopens_no_routing_regardless_of_host_size() {
    assert_builder_reopens_nothing(
        "the store-view build forced by admitting ONE new file",
        snapshot_build_only_deltas,
    );
}

/// The other half of the split: admission must reuse existing workspace facts.
///
/// `./dep` was resolved while constructing every fixture, so a new owner must
/// not cause another resolver miss. The separate anti-vacuity control above
/// resolves a genuinely cold dependency and proves the miss counter moves.
#[test]
fn admission_reuses_existing_workspace_resolution_at_any_host_size() {
    let measured = measure_across_host_sizes(
        "admitting ONE new file, no view read in the window",
        admission_only_deltas,
    );
    for &(n, deltas) in &measured {
        assert_eq!(
            deltas, NO_WORK,
            "at N={n}, admitting a new owner of the already-resolved `./dep` \
             must reuse workspace resolution and perform no store-view, \
             materialisation, or resolver-miss work. Full table: {measured:?}"
        );
    }
}

/// A byte-identical re-upsert is a true no-op: it never advances
/// `content_generation`, nothing goes edge-stale, and the builder does no
/// work at any host size. The zero-work baseline the content-CHANGING
/// gates above are measured against.
#[test]
fn recompile_existing_is_free_at_any_host_size() {
    let measured =
        measure_across_host_sizes("re-upserting an UNCHANGED file", recompile_existing_deltas);
    for &(n, deltas) in &measured {
        assert_eq!(
            deltas, NO_WORK,
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
