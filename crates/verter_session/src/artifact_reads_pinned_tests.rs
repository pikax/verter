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

    // Publish the overlay `IndexedReady` candidate under the overlay
    // hash (multi-candidate sibling of the base artifact).
    let overlay_indexed = host
        .materialize_overlay_indexed_ready(canonical, &overlay_source, overlay_hash)
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
