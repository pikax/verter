//! Block 1.J.1 item 1 — content-pinned artifact reads discriminator.
//!
//! Codex Block 1.J diagnosis: "some production reads use permissive
//! `get_any` instead of a current-content-pinned lookup. Fact
//! validation cannot work if the validator reads a stale artifact as
//! 'current.'"
//!
//! `current_derived_fact_hash(Route)` / `current_cached_import_route_hash`
//! are the Route / ImportRoute fact-validation oracles. Pre-fix they
//! read `FileArtifactStore::get_any` — which returns ANY cached
//! `IndexedReady` for a canonical regardless of content hash. Once
//! eager `evict_canonical` is retired (Block 2) a stale `IndexedReady`
//! can linger past a content change, and the oracle would surface its
//! stale `route_hash` / `import_route_hash` as the "current" derived
//! fact — confirming a stale dependent cache entry as valid.
//!
//! Post-fix the oracle reads `current_content_pinned_indexed`, which
//! resolves the canonical's authoritative current content hash from
//! the scheduler and reads the artifact store **pinned to that hash**
//! (`FileArtifactStore::get_for_current_content`). A stale candidate
//! yields `None`, so the oracle recomputes the truly-current route
//! surface hash instead.
//!
//! Discriminating fixture: a real `IndexedReady` is materialised, then
//! a synthetic STALE `IndexedReady` (doctored `whole_hash` +
//! `route_hash` + `import_route_hash`) is planted into
//! `FileArtifactStore` while the scheduler's authoritative content
//! hash stays at the real value — the lingering-stale post-Block-2
//! scenario. The test then asserts the pinned read and the two derived
//! fact oracles all reject the stale artifact. A pre-fix `get_any`
//! tree returns the planted stale hashes and the assertions FAIL.
use std::sync::Arc;

use crate::resolver_core::DerivedFactKind;
use crate::{HostConfig, VerterHost};

/// Doctored hash that no real content ever produces — the planted
/// stale artifact carries this so a permissive `get_any` read is
/// trivially distinguishable from a content-pinned read.
const STALE_HASH: [u8; 16] = [0xEE; 16];

/// Build a host with a single `.ts` file resolvable through the
/// workspace, materialise its `IndexedReady`, and return the host plus
/// the real (current-content) whole hash.
fn host_with_materialized_ts(path: &str, source: &str) -> (VerterHost, [u8; 16]) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_kind: crate::FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("seed upsert succeeds");
    let indexed = host
        .ensure_indexed_ready(path)
        .expect("IndexedReady must materialise for an upserted file");
    (host, indexed.whole_hash)
}

/// Plant a synthetic STALE `IndexedReady` for `canonical` into
/// `FileArtifactStore`. The planted entry clones the real artifact's
/// shape but overwrites every content-derived hash with a value no
/// real content produces. `FileArtifactStore::insert` drains prior
/// versions, so afterwards the store holds ONLY the stale entry while
/// the scheduler still reports the real `whole_hash` — exactly the
/// lingering-stale state Block 2's eviction removal would create.
fn plant_stale_indexed(host: &VerterHost, canonical: &str) {
    let real = host
        .project_type_store()
        .indexed()
        .get_any(canonical)
        .expect("real IndexedReady must exist before planting the stale one");
    let mut stale = (*real).clone();
    stale.whole_hash = STALE_HASH;
    // Drop the cached route-surface / import-route hashes to a sentinel
    // so a `get_any`-based oracle would observe THESE as "current".
    stale.route_hash = Some(STALE_HASH);
    stale.import_route_hash = Some(STALE_HASH);
    host.project_type_store()
        .indexed()
        .insert(Arc::from(canonical), Arc::new(stale));
}

/// The pinned read rejects a stale artifact; the permissive `get_any`
/// returns it. This is the substrate-level discriminator for item 1.
#[test]
fn current_content_pinned_indexed_rejects_stale_artifact() {
    let canonical = "/pinned/probe.ts";
    let (host, real_hash) = host_with_materialized_ts(
        canonical,
        "export interface Probe { a: number; }\nexport const probe = 1;\n",
    );
    assert_ne!(
        real_hash, STALE_HASH,
        "fixture invariant: the real content hash must differ from the planted stale hash",
    );

    plant_stale_indexed(&host, canonical);

    // The permissive `get_any` surfaces the planted stale artifact —
    // this is the pre-fix read shape.
    let permissive = host
        .project_type_store()
        .indexed()
        .get_any(canonical)
        .expect("get_any must still return the (stale) entry");
    assert_eq!(
        permissive.whole_hash, STALE_HASH,
        "fixture invariant: get_any returns the planted stale artifact — \
         that is exactly the read shape the pre-fix oracle used",
    );

    // The content-pinned read resolves the scheduler's authoritative
    // current hash (the real hash) and finds NO artifact at that hash
    // (only the stale one is stored). Post-fix it returns `None`.
    let pinned = host.current_content_pinned_indexed(canonical);
    assert!(
        pinned.is_none(),
        "current_content_pinned_indexed MUST return None: the only cached \
         artifact is a stale candidate ({STALE_HASH:?}) while the scheduler's \
         authoritative content hash is the real value ({real_hash:?}). A \
         non-None result means the read is not content-pinned.",
    );
}

/// Codex P1.A discriminator — a content-pinned read MUST NOT resolve a
/// stale artifact via a `get_any`-derived hash.
///
/// When a file is evicted (`VerterHost::evict` sets the
/// `DerivedRawState.evicted` flag) its `IndexedReady` is NOT removed
/// from `FileArtifactStore` — the artifact lingers. The pre-fix
/// `current_content_pinned_indexed` derived its pin from
/// `get_whole_hash`, which — once the scheduler branch is gated off by
/// the eviction flag — falls back to `FileArtifactStore::get_any`. That
/// `get_any` returns the lingering artifact and surfaces *its own*
/// `whole_hash`; feeding that hash straight back into
/// `get_for_current_content` re-resolves the very same stale artifact.
/// The "pin" then confirms the stale artifact as current.
///
/// Post-fix the pin is resolved by `authoritative_current_content_hash`,
/// which is scheduler-only and returns `None` for an evicted canonical.
/// The content-pinned read therefore returns `None` (a genuine miss →
/// recompute) instead of the lingering stale artifact.
///
/// Discriminator: pre-fix this returns `Some(<lingering artifact>)`;
/// post-fix it returns `None`.
#[test]
fn current_content_pinned_indexed_returns_none_after_eviction() {
    let canonical = "/pinned/evicted_owner.ts";
    let (host, real_hash) = host_with_materialized_ts(
        canonical,
        "export type Exported = string;\nexport interface Surface { x: number; }\n",
    );

    // Before eviction the content-pinned read HITS the genuine current
    // artifact — this anchors the discriminator (the assertion below is
    // not vacuously satisfied by a systematically-missing read).
    assert!(
        host.current_content_pinned_indexed(canonical).is_some(),
        "fixture invariant: the content-pinned read must HIT the genuine \
         current artifact before eviction",
    );

    // Evict the file. `evict` flips `DerivedRawState.evicted` but leaves
    // the `IndexedReady` in `FileArtifactStore` — the exact
    // lingering-stale state.
    host.evict(canonical);

    // Fixture invariant: the artifact still lingers in the store under
    // its real content hash, so `get_any` (the pre-fix hash source)
    // still returns it.
    let lingering = host
        .project_type_store()
        .indexed()
        .get_any(canonical)
        .expect("evict must NOT remove the IndexedReady from FileArtifactStore");
    assert_eq!(
        lingering.whole_hash, real_hash,
        "fixture invariant: the lingering artifact keeps its real content \
         hash — a pre-fix `get_whole_hash` would surface THIS hash via \
         `get_any` and re-resolve the same artifact",
    );

    // Post-fix: the authoritative hash source is scheduler-only and
    // gated on the eviction flag, so it reports no current hash for an
    // evicted canonical.
    assert!(
        host.authoritative_current_content_hash(canonical).is_none(),
        "authoritative_current_content_hash MUST return None for an evicted \
         canonical — it must not fall back to a `get_any`-derived hash",
    );

    // The discriminating assertion: the content-pinned read returns
    // `None` (miss → recompute), NOT the lingering stale artifact. A
    // pre-fix tree returns `Some(lingering)` here.
    let pinned = host.current_content_pinned_indexed(canonical);
    assert!(
        pinned.is_none(),
        "current_content_pinned_indexed MUST return None after eviction: a \
         non-None result means the pin was derived from the lingering \
         artifact's own `get_any` hash, which re-resolves the stale artifact \
         and confirms it as current — the exact codex P1.A defect.",
    );
}

/// `current_derived_fact_hash(Route)` is the Route fact-validation
/// oracle. With a stale artifact planted, a `get_any`-based oracle
/// would return the planted stale `route_hash`; the content-pinned
/// oracle recomputes the route surface hash from the live shallow
/// state instead.
#[test]
fn route_derived_fact_hash_ignores_stale_artifact_route_hash() {
    let canonical = "/pinned/route_owner.ts";
    let (host, real_hash) = host_with_materialized_ts(
        canonical,
        "export type Exported = string;\nexport interface Surface { x: number; }\n",
    );

    // Baseline: the Route fact hash from the freshly-materialised
    // (current-content) artifact.
    let route_fresh = host.current_derived_fact_hash(canonical, DerivedFactKind::Route);

    plant_stale_indexed(&host, canonical);

    let route_after_plant = host.current_derived_fact_hash(canonical, DerivedFactKind::Route);

    // Discriminating assertion: the oracle must NOT return the planted
    // stale `route_hash`. Pre-fix (`get_any`) it returns
    // `Some(STALE_HASH)`; post-fix (`current_content_pinned_indexed`)
    // the pinned read misses the stale candidate and the recompute
    // path produces the genuine current route surface hash.
    assert_ne!(
        route_after_plant,
        Some(STALE_HASH),
        "current_derived_fact_hash(Route) MUST NOT surface the planted stale \
         artifact's route_hash. A `get_any`-based oracle returns the stale \
         hash here, confirming a stale dependent cache entry as valid — the \
         exact codex item-1 defect.",
    );
    // The pinned recompute must agree with the genuine current route
    // surface (the fresh artifact's route hash, or a recompute of the
    // same live shallow state).
    assert_eq!(
        route_after_plant, route_fresh,
        "the content-pinned Route oracle must reproduce the genuine current \
         route surface hash regardless of the stale artifact in the store",
    );
    assert!(
        route_fresh.is_some(),
        "fixture invariant: the owner declares a resolvable route surface, so \
         the Route fact hash must be Some — otherwise the assertion above is \
         vacuous",
    );
    let _ = real_hash;
}

/// `current_cached_import_route_hash` is the ImportRoute fact oracle.
/// Same discrimination as the Route oracle: a stale artifact's
/// `import_route_hash` must not be served as the current ImportRoute
/// fact.
#[test]
fn import_route_derived_fact_hash_ignores_stale_artifact_hash() {
    let canonical = "/pinned/import_owner.ts";
    // A file with an import edge so `import_route_hash` is populated on
    // the real artifact.
    let (host, _real_hash) = host_with_materialized_ts(
        canonical,
        "import { dep } from './dep';\nexport const reexport = dep;\n",
    );

    plant_stale_indexed(&host, canonical);

    // With ONLY the stale artifact in the store, the pinned read misses
    // (its content hash is the doctored sentinel). Pre-fix the `get_any`
    // oracle returns the planted stale `import_route_hash`
    // (`Some(STALE_HASH)`); post-fix the content-pinned read misses and
    // the oracle answers from the genuine `DerivedRawState` import-route
    // table instead.
    let import_route_after_plant =
        host.current_derived_fact_hash(canonical, DerivedFactKind::ImportRoute);
    assert_ne!(
        import_route_after_plant,
        Some(STALE_HASH),
        "current_derived_fact_hash(ImportRoute) MUST NOT surface the planted \
         stale artifact's import_route_hash. Pre-fix the `get_any` oracle \
         returns the stale hash, confirming a stale dependent cache entry as \
         valid — the exact codex item-1 defect.",
    );

    // Now re-materialise the genuine current `IndexedReady` (the stale
    // entry is overwritten by the real-hash artifact). The content-pinned
    // read now HITS the current artifact and the oracle returns its
    // genuine `import_route_hash` — proving the pinned read serves the
    // current content rather than systematically falling through.
    host.project_type_store().indexed().remove(canonical);
    let fresh = host
        .ensure_indexed_ready(canonical)
        .expect("IndexedReady must re-materialise");
    let import_route_fresh =
        host.current_derived_fact_hash(canonical, DerivedFactKind::ImportRoute);
    assert_eq!(
        import_route_fresh, fresh.import_route_hash,
        "after the genuine current artifact is materialised, the content-pinned \
         ImportRoute oracle must return that artifact's import_route_hash",
    );
    assert_ne!(
        import_route_fresh,
        Some(STALE_HASH),
        "the genuine current ImportRoute hash must never equal the planted \
         stale sentinel",
    );
}

/// Codex P1.B discriminator — a content-pinned read through
/// `SessionResolverContext` MUST pin against the overlay content hash,
/// not the base host's hash.
///
/// When an overlay covers a file that also exists on the base host,
/// `materialize_overlay_indexed_ready` publishes the overlay
/// `IndexedReady` into `FileArtifactStore` under the *overlay* content
/// hash, as a multi-candidate sibling of the base artifact (which lives
/// under the *base* content hash).
///
/// The pre-fix `indexed_for_current_content` derived its pin from
/// `get_whole_hash`. `SessionResolverContext::get_whole_hash` delegates
/// straight to the base host, so it returns the BASE content hash — and
/// the pinned read resolves the BASE artifact (or misses the overlay
/// entirely) while the session is computing overlay component-meta /
/// proof data.
///
/// Post-fix the pin is resolved by `authoritative_current_content_hash`,
/// which `SessionResolverContext` overrides to consult the active
/// `SessionView`: an overlay-covered canonical resolves to the overlay
/// hash, so the content-pinned read returns the OVERLAY artifact.
///
/// Discriminator: pre-fix `indexed_for_current_content` returns the
/// artifact whose `whole_hash == base_hash`; post-fix it returns the
/// artifact whose `whole_hash == overlay_hash`.
#[test]
fn indexed_for_current_content_pins_overlay_artifact_through_session_context() {
    use crate::resolver_core::{ResolverContext, SessionResolverContext};
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;

    let canonical = "/overlay/probe.ts";
    // Base file: materialised on the host under the base content hash.
    let (host, base_hash) = host_with_materialized_ts(
        canonical,
        "export interface Probe { base: number; }\nexport const probe = 1;\n",
    );
    let host = Arc::new(host);

    // Overlay source: deliberately different bytes → different content
    // hash, so the base and overlay artifacts are distinguishable by
    // `whole_hash`.
    let overlay_source: Arc<str> =
        Arc::from("export interface Probe { overlay: string; }\nexport const probe = 2;\n");
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    // The overlay hash is the view's authoritative overlay-content hash.
    let overlay_hash = view
        .overlay_content_hash_for(canonical)
        .expect("OverlaidView must report an overlay content hash for the masked canonical");
    assert_ne!(
        overlay_hash, base_hash,
        "fixture invariant: the overlay source differs from the base, so its \
         content hash must differ — otherwise base/overlay are indistinguishable",
    );

    // Publish the overlay `IndexedReady` candidate under the
    // overlay-scoped key (multi-candidate sibling of the base
    // artifact).
    let overlay_indexed = host
        .materialize_overlay_indexed_ready_with_view(
            canonical,
            &overlay_source,
            overlay_hash,
            &view,
        )
        .expect("overlay IndexedReady must materialise");
    assert_eq!(
        overlay_indexed.whole_hash, overlay_hash,
        "fixture invariant: the overlay artifact is keyed by the overlay hash",
    );

    // Sanity: the base artifact is still in the store under the base
    // hash — both candidates coexist.
    let base_indexed = host
        .project_type_store()
        .indexed()
        .get(canonical, base_hash)
        .expect("base artifact must still be cached under the base hash");
    assert_eq!(base_indexed.whole_hash, base_hash);

    // Drive the content-pinned read through the session context.
    let ctx = SessionResolverContext::new(&host, &view);

    // The session view's authoritative current-content hash must be the
    // OVERLAY hash, not the base hash.
    assert_eq!(
        ctx.authoritative_current_content_hash(canonical),
        Some(overlay_hash),
        "SessionResolverContext::authoritative_current_content_hash MUST \
         resolve the overlay hash for an overlay-covered canonical, not the \
         base host's hash",
    );

    // The discriminating assertion: the content-pinned read returns the
    // OVERLAY artifact. Pre-fix it returns the base artifact (pinned by
    // the base host's hash).
    let pinned = ctx
        .indexed_for_current_content(canonical)
        .expect("the content-pinned read must HIT a candidate");
    assert_eq!(
        pinned.whole_hash, overlay_hash,
        "indexed_for_current_content through SessionResolverContext MUST \
         return the OVERLAY artifact (whole_hash == overlay_hash). A result \
         keyed by base_hash means the pin was derived from the base host's \
         hash rather than the session view — the exact codex P1.B defect.",
    );
    assert_ne!(
        pinned.whole_hash, base_hash,
        "the overlay-pinned read must NOT surface the base artifact",
    );
}

/// `HostStoreView::build` route-fact provenance — a STALE `IndexedReady`
/// retained for a canonical whose current content is route-owned-shallow
/// MUST NOT publish its stale `Route` derived hash, and MUST NOT suppress
/// the current route-owned-shallow `Route` fact.
///
/// `HostStoreView::build` snapshots `FileArtifactStore::snapshot_all()`,
/// which returns every retained `IndexedReady` regardless of content
/// hash. Once eager eviction is retired, a stale older-content
/// `IndexedReady` can linger for a canonical whose CURRENT content is
/// only carried by a `RouteOwnedShallowEntry`. A build that
/// unconditionally trusts the snapshotted `IndexedReady` would publish
/// the stale artifact's route surface as the canonical's `Route` derived
/// fact AND mark the canonical indexed-covered — suppressing the current
/// route-owned-shallow `Route` fact below. The view's `Route` hash would
/// then disagree with `current_route_surface_hash()` (the production
/// route-fact oracle, which is content-pinned and skips the stale
/// indexed artifact) until the stale artifact is swept — a false stale
/// miss for every route-dependent cache entry.
///
/// Discriminating fixture: `probe.ts` is materialised (real
/// `IndexedReady` at the real content hash), then a synthetic STALE
/// `IndexedReady` carrying a DIFFERENT file's shallow route surface is
/// planted for it (`FileArtifactStore::insert` drains the real entry, so
/// the store holds ONLY the stale candidate). A `RouteOwnedShallowEntry`
/// for `probe.ts` is then materialised at the real current hash — the
/// authoritative current route surface. The scheduler's content for
/// `probe.ts` is never touched, so the tracked current whole hash stays
/// at the real value while the lone retained `IndexedReady` is stale.
///
/// - **Pre-fix tree:** the build's indexed loop inserts the stale
///   artifact's route surface as `derived_hashes[(probe, Route)]` and
///   marks `probe` indexed-covered, so the route-owned-shallow loop
///   skips it. The view's `Route` hash is the STALE surface and
///   disagrees with `current_route_surface_hash()` — this test FAILS.
/// - **Post-fix tree:** the indexed loop gates on
///   `indexed.whole_hash == tracked`; the stale artifact is skipped, so
///   the route-owned-shallow loop publishes the current route surface.
///   The view's `Route` hash equals `current_route_surface_hash()`.
#[test]
fn host_store_view_route_fact_ignores_stale_indexed_when_current_is_route_owned() {
    let probe = "/pinned/route_provenance_probe.ts";
    // The current content of `probe.ts` — a resolvable route surface.
    let (host, real_hash) = host_with_materialized_ts(
        probe,
        "export interface Current { current: number; }\nexport const current = 1;\n",
    );
    assert_ne!(
        real_hash, STALE_HASH,
        "fixture invariant: the real content hash must differ from the stale sentinel",
    );

    // The authoritative current route surface for `probe.ts` — derived
    // from the live (current-content) shallow state. This is the hash a
    // correct `HostStoreView` build must publish.
    let current_route_surface = host
        .current_route_surface_hash(probe)
        .expect("probe declares a resolvable route surface → current_route_surface_hash is Some");

    // A DONOR file with a DIFFERENT export surface. Its shallow state is
    // harvested to give the planted stale `IndexedReady` a route surface
    // that genuinely differs from `probe.ts`'s current one.
    let donor = "/pinned/route_provenance_donor.ts";
    upsert_donor_with_distinct_surface(&host, donor);
    let donor_indexed = host
        .project_type_store()
        .indexed()
        .get_any(donor)
        .expect("donor IndexedReady must materialise");
    let stale_route_surface =
        crate::resolver_store::hash_route_surface(donor_indexed.shallow_state.as_ref());
    assert_ne!(
        stale_route_surface, current_route_surface,
        "fixture invariant: the donor's route surface must differ from probe's current \
         surface — otherwise the stale-vs-current Route hashes are indistinguishable",
    );

    // Plant a STALE `IndexedReady` for `probe.ts`: the real artifact's
    // shape, but doctored to a content hash no real content produces AND
    // carrying the DONOR's shallow route surface. `FileArtifactStore::insert`
    // drains probe's real artifact, so the store retains ONLY this stale
    // candidate while the scheduler still reports the real content hash.
    let real_probe_indexed = host
        .project_type_store()
        .indexed()
        .get_any(probe)
        .expect("real probe IndexedReady must exist before planting the stale one");
    let mut stale = (*real_probe_indexed).clone();
    stale.whole_hash = STALE_HASH;
    stale.route_hash = Some(STALE_HASH);
    stale.import_route_hash = Some(STALE_HASH);
    stale.shallow_state = Arc::clone(&donor_indexed.shallow_state);
    host.project_type_store()
        .indexed()
        .insert(Arc::from(probe), Arc::new(stale));

    // Materialise a `RouteOwnedShallowEntry` for `probe.ts` at its real
    // current content hash — the authoritative current route surface.
    // The route-owned producer publishes here because the only retained
    // `IndexedReady` (the planted stale one) does not match the current
    // content hash.
    let route_owned = host
        .ensure_route_owned_shallow_entry(probe)
        .expect("route-owned-shallow entry must materialise for probe at the current hash");
    assert_eq!(
        route_owned.whole_hash, real_hash,
        "fixture invariant: the route-owned-shallow entry is keyed by probe's real \
         current content hash",
    );

    // Build the production `HostStoreView`.
    let view = host.resolver_store_view();
    let view_route_hash = view.derived_hash(probe, crate::resolver_core::DerivedFactKind::Route);

    // Discriminating assertion: the view's `Route` derived hash must be
    // the CURRENT route surface (from the route-owned-shallow entry),
    // NOT the planted stale artifact's surface.
    assert_eq!(
        view_route_hash,
        Some(current_route_surface),
        "HostStoreView::build MUST publish the CURRENT route surface for a canonical \
         whose only retained `IndexedReady` is stale — the route-owned-shallow entry is \
         the current authority. A pre-fix build inserts the stale artifact's route \
         surface (and suppresses the route-owned-shallow fallback via \
         `indexed_route_canonicals`), so the view's Route hash is the stale surface.",
    );
    assert_ne!(
        view_route_hash,
        Some(stale_route_surface),
        "HostStoreView::build MUST NOT publish the STALE `IndexedReady`'s route surface \
         as the canonical's `Route` derived fact",
    );
    // The view's Route fact must agree with the production route-fact
    // oracle — both must read the current route surface, not the stale
    // artifact. A disagreement is a false stale miss for every
    // route-dependent cache entry.
    assert_eq!(
        view_route_hash,
        host.current_route_surface_hash(probe),
        "HostStoreView::build's `Route` derived hash MUST agree with \
         `current_route_surface_hash()` — the producer and the validator must observe \
         one route surface for the canonical",
    );
}

/// Companion to `host_store_view_route_fact_ignores_stale_indexed_when_current_is_route_owned`:
/// the CURRENT `IndexedReady` exists but its shallow surface is NOT
/// route-resolvable (the route export was removed by an edit), while a
/// route-owned-shallow entry from a prior route-bearing version still
/// LINGERS at the same content hash.
///
/// `HostStoreView::build`'s indexed loop must mark such a canonical in
/// `indexed_route_canonicals` so the route-owned-shallow loop suppresses
/// the lingering entry — a current `IndexedReady` is the route-surface
/// authority for the canonical whether or not its surface is
/// route-resolvable. The producer-side authority
/// `current_route_surface_hash()` returns `None` (no route-owned-shallow
/// fallback) the moment a current indexed artifact exists,
/// route-resolvable or not; the store-view validator side must match, so
/// the view ends up with NO `Route` derived fact for the canonical.
///
/// Discriminating fixture: `probe.ts` is materialised, then a synthetic
/// CURRENT-content `IndexedReady` carrying a non-route-resolvable shallow
/// surface (empty symbol/export inventory) is planted for it at the real
/// content hash (`FileArtifactStore::insert` drains the real entry). A
/// route-owned-shallow entry carrying a DONOR's route-bearing shallow
/// surface is then published for `probe.ts` at the same real content
/// hash — the lingering prior-version route-owned entry.
///
/// - **Pre-fix tree:** the indexed loop gates the
///   `indexed_route_canonicals` mark on `has_resolvable_surface()`, so a
///   current-but-non-route-resolvable `IndexedReady` does NOT mark the
///   canonical. The route-owned-shallow loop therefore does not skip it
///   and inserts the lingering route-owned `Route` hash into the view —
///   the view carries a `Route` derived fact that `current_route_surface_hash()`
///   (which returns `None`) cannot reproduce. This test FAILS.
/// - **Post-fix tree:** the indexed loop marks the canonical in
///   `indexed_route_canonicals` on `indexed.whole_hash == tracked` alone;
///   the route-owned-shallow loop skips it, so the view has NO `Route`
///   derived hash for it — agreeing with `current_route_surface_hash()`.
#[test]
fn host_store_view_suppresses_lingering_route_owned_hash_when_current_indexed_lacks_route_surface()
{
    let probe = "/pinned/route_suppression_probe.ts";
    // The current content of `probe.ts` — a resolvable route surface.
    let (host, real_hash) = host_with_materialized_ts(
        probe,
        "export interface Current { current: number; }\nexport const current = 1;\n",
    );
    assert_ne!(
        real_hash, STALE_HASH,
        "fixture invariant: the real content hash must differ from the stale sentinel",
    );

    // A DONOR file with a resolvable export surface. Its shallow state is
    // harvested to give the lingering route-owned-shallow entry a genuine
    // route-bearing surface (so the route-owned snapshot computes a
    // `Some(route_hash)` the pre-fix view would publish).
    let donor = "/pinned/route_suppression_donor.ts";
    upsert_donor_with_distinct_surface(&host, donor);
    let donor_indexed = host
        .project_type_store()
        .indexed()
        .get_any(donor)
        .expect("donor IndexedReady must materialise");
    assert!(
        donor_indexed.shallow_state.has_resolvable_surface(),
        "fixture invariant: the donor surface must be route-resolvable so the \
         lingering route-owned-shallow entry contributes a `Route` hash",
    );
    let lingering_route_surface =
        crate::resolver_store::hash_route_surface(donor_indexed.shallow_state.as_ref());

    // Plant a CURRENT-content `IndexedReady` for `probe.ts` whose shallow
    // surface is NOT route-resolvable: clone the real artifact (so
    // `whole_hash` stays at the real current content hash — this is the
    // *current* indexed artifact, not a stale one) and swap in an empty
    // shallow state. `has_resolvable_surface()` is then false, exactly the
    // shape an edit that removes the file's last export would produce.
    let real_probe_indexed = host
        .project_type_store()
        .indexed()
        .get_any(probe)
        .expect("real probe IndexedReady must exist before planting the current non-route one");
    let non_route_shallow = {
        use rustc_hash::{FxHashMap, FxHashSet};
        crate::resolver_core::shallow_file_state::ShallowFileState {
            whole_hash: real_hash,
            exports: FxHashMap::default(),
            wildcard_reexports: Vec::new(),
            symbols: FxHashMap::default(),
            value_symbols: FxHashMap::default(),
            import_locals: FxHashSet::default(),
            import_targets: FxHashMap::default(),
            analysis: Arc::clone(&real_probe_indexed.shallow_state.analysis),
        }
    };
    assert!(
        !non_route_shallow.has_resolvable_surface(),
        "fixture invariant: the planted current `IndexedReady` surface must NOT be \
         route-resolvable — that is the scenario under test",
    );
    let mut current_non_route = (*real_probe_indexed).clone();
    // `whole_hash` is kept at `real_hash` — the scheduler still tracks
    // `real_hash`, so this artifact is the CURRENT-content indexed
    // artifact (`indexed.whole_hash == tracked` in the view build), unlike
    // the sibling test's deliberately-stale `STALE_HASH` artifact.
    current_non_route.whole_hash = real_hash;
    current_non_route.route_hash = None;
    current_non_route.shallow_state = Arc::new(non_route_shallow);
    host.project_type_store()
        .indexed()
        .insert(Arc::from(probe), Arc::new(current_non_route));

    // The producer-side route-fact oracle: with a current `IndexedReady`
    // whose surface is not route-resolvable, `current_route_surface_hash()`
    // returns `None` and suppresses the route-owned-shallow fallback. The
    // store-view build must agree.
    assert_eq!(
        host.current_route_surface_hash(probe),
        None,
        "fixture invariant: a current `IndexedReady` with a non-route-resolvable surface \
         makes `current_route_surface_hash()` return `None` — the route-owned-shallow \
         fallback is suppressed at the producer side",
    );

    // Plant a LINGERING route-owned-shallow entry for `probe.ts` at the
    // real current content hash, carrying the donor's route-bearing
    // shallow surface — the entry a prior route-bearing version of
    // `probe.ts` left behind in `RouteOwnedShallowDb`.
    let mut lingering =
        crate::project_type_store::RouteOwnedShallowEntry::test_stub(Arc::from(probe));
    lingering.whole_hash = real_hash;
    lingering.shallow_state = Arc::clone(&donor_indexed.shallow_state);
    host.project_type_store()
        .route_owned_shallow()
        .publish(Arc::from(probe), Arc::new(lingering));

    // Build the production `HostStoreView`.
    let view = host.resolver_store_view();
    let view_route_hash = view.derived_hash(probe, crate::resolver_core::DerivedFactKind::Route);

    // Discriminating assertion: the view must carry NO `Route` derived
    // hash for `probe.ts`. A current `IndexedReady` exists, so the
    // route-owned-shallow fallback must be suppressed; the indexed
    // surface is not route-resolvable, so the indexed loop contributes no
    // `Route` fact either.
    assert_eq!(
        view_route_hash, None,
        "HostStoreView::build MUST NOT publish a `Route` derived fact for a canonical \
         whose current `IndexedReady` has a non-route-resolvable surface — the lingering \
         route-owned-shallow entry must be suppressed via `indexed_route_canonicals`. A \
         pre-fix build gates that mark on `has_resolvable_surface()`, so the lingering \
         route-owned `Route` hash leaks into the view.",
    );
    assert_ne!(
        view_route_hash,
        Some(lingering_route_surface),
        "HostStoreView::build MUST NOT publish the LINGERING route-owned-shallow entry's \
         route surface as the canonical's `Route` derived fact once a current \
         `IndexedReady` exists",
    );
    // The view's `Route` fact (absent) must agree with the production
    // route-fact oracle `current_route_surface_hash()` (also `None`) —
    // producer and validator must observe one route-surface verdict.
    assert_eq!(
        view_route_hash,
        host.current_route_surface_hash(probe),
        "HostStoreView::build's `Route` derived hash MUST agree with \
         `current_route_surface_hash()` — both must observe NO route surface for a \
         canonical whose current `IndexedReady` removed it",
    );
}

/// Upsert a route-only `.ts` donor whose export surface is deliberately
/// distinct from the other fixtures in this file, so its hashed route
/// surface differs from any current-content probe surface.
fn upsert_donor_with_distinct_surface(host: &VerterHost, path: &str) {
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(
                "export interface DonorAlpha { donorAlpha: string; }\n\
                 export interface DonorBeta { donorBeta: boolean; }\n\
                 export type DonorGamma = DonorAlpha | DonorBeta;\n\
                 export const donorValue = 99;\n",
            ),
            file_kind: crate::FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("donor upsert succeeds");
    let _ = host
        .ensure_indexed_ready(path)
        .expect("donor IndexedReady must materialise");
}
