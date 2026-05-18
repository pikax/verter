//! Discriminating regression tests — `SessionView::content_hash_for`
//! is a **view-authoritative current-content oracle**, consistent with
//! `SessionView::source`.
//!
//! ## The substrate gap these tests pin
//!
//! `SessionView::content_hash_for` used to delegate, on every base
//! fallthrough, to a content-agnostic first-match scan over
//! `FileArtifactStore` (the now-deleted `content_hash_for_canonical`).
//! Its sibling `source()` reads the scheduler authority. With cache
//! invalidation made lazy (query-identity caches self-version-root a
//! same-canonical edit instead of an eager own-canonical drain), a
//! stale pre-edit `IndexedReady` can linger in `FileArtifactStore` past
//! a same-canonical edit — so `content_hash_for` returned the **stale**
//! pre-edit hash while `source()` returned the **fresh** bytes. The two
//! methods disagreed on freshness.
//!
//! `capture_component_meta_inputs_with_view` paired them: it fed the
//! fresh `source` plus the stale hash to the overlay materialiser,
//! whose fast path keyed a `FileArtifactStore` lookup by the stale hash
//! and returned the stale pre-edit owner `IndexedReady` → stale
//! component-meta after an owner-self edit.
//!
//! After the fix:
//!
//! - every base-fallthrough `content_hash_for` resolves the hash from
//!   `VerterHost::authoritative_current_content_hash` (the scheduler
//!   authority, no permissive fallback) — so it agrees with `source()`;
//! - the overlay materialiser derives BOTH the source and its content
//!   hash from the `SessionView` itself (a single authority), so a
//!   caller cannot pair a stale hash with a fresh source.
//!
//! ## Discriminating fixture
//!
//! A real `.ts` file is upserted + materialised; the scheduler then
//! holds the real current content hash. A synthetic STALE
//! `IndexedReady` (doctored `whole_hash`) is planted into
//! `FileArtifactStore`, replacing the real artifact — exactly the
//! lingering-stale state lazy invalidation produces. Each test then
//! asserts the view / materialiser surfaces the **fresh** scheduler
//! hash, never the planted stale one. Against the pre-fix tree the
//! base-scan `content_hash_for` returns the planted stale hash and
//! every assertion FAILS; against the post-fix tree they PASS.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::session_view::{HostView, HostViewRef, OverlaidView, SessionView};
use crate::{HostConfig, VerterHost};

/// Doctored hash that no real content ever produces — a planted stale
/// artifact carries this, so a content-agnostic `FileArtifactStore`
/// read is trivially distinguishable from the scheduler-authoritative
/// current hash.
const STALE_HASH: [u8; 16] = [0xEE; 16];

/// Upsert + materialise a single `.ts` file; return the host (in an
/// `Arc` for view construction) plus the real current-content hash.
fn host_with_materialized_ts(path: &str, source: &str) -> (Arc<VerterHost>, [u8; 16]) {
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
    (Arc::new(host), indexed.whole_hash)
}

/// Plant a synthetic STALE `IndexedReady` for `canonical` into
/// `FileArtifactStore`. `FileArtifactStore::insert` drains every prior
/// version of the same canonical, so afterwards the store holds ONLY
/// the stale entry while the scheduler still reports the real
/// `whole_hash` — the lingering-stale post-lazy-invalidation state.
fn plant_stale_indexed(host: &VerterHost, canonical: &str) {
    let real = host
        .project_type_store()
        .indexed()
        .get_any(canonical)
        .expect("real IndexedReady must exist before planting the stale one");
    let mut stale = (*real).clone();
    stale.whole_hash = STALE_HASH;
    host.project_type_store()
        .indexed()
        .insert(Arc::from(canonical), Arc::new(stale));
}

// ──────────────────────────────────────────────────────────────────
// Base-view fallthrough — `HostView` / `HostViewRef`.
// ──────────────────────────────────────────────────────────────────

/// `HostView::content_hash_for` returns the **fresh** scheduler hash
/// after a stale `IndexedReady` is planted — never the stale artifact's
/// own hash.
///
/// Pre-fix: `content_hash_for` scans `FileArtifactStore` and returns
/// `STALE_HASH` (the only stored entry). Post-fix: it resolves the
/// scheduler-authoritative current hash and returns the real hash.
#[test]
fn host_view_content_hash_is_scheduler_authoritative_not_stale_artifact() {
    let canonical = "/base/probe.ts";
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
    // this is the pre-fix `content_hash_for` read shape.
    let permissive = host
        .project_type_store()
        .indexed()
        .get_any(canonical)
        .expect("get_any must still return the (stale) entry");
    assert_eq!(
        permissive.whole_hash, STALE_HASH,
        "fixture invariant: a content-agnostic FileArtifactStore scan returns the \
         planted stale artifact — that is exactly the pre-fix read shape",
    );

    // The discriminating assertion: the view's content hash is the
    // FRESH scheduler hash, agreeing with `source()`.
    let view = HostView::new(Arc::clone(&host));
    let view_hash = view
        .content_hash_for(canonical)
        .expect("HostView reports a current content hash for a live canonical");
    assert_eq!(
        view_hash, real_hash,
        "HostView::content_hash_for MUST return the scheduler-authoritative \
         current hash, NOT the stale lingering artifact's hash",
    );
    assert_ne!(
        view_hash, STALE_HASH,
        "HostView::content_hash_for MUST NOT surface the planted stale hash",
    );

    // Freshness contract: `content_hash_for` and `source()` agree.
    let source = view
        .source(canonical)
        .expect("HostView reports source for a live canonical");
    let source_hash = crate::hash::hash_16(source.as_bytes());
    assert_eq!(
        view_hash, source_hash,
        "HostView::content_hash_for MUST equal the hash of the bytes source() \
         returns — the two methods agree on freshness by contract",
    );
}

/// `HostViewRef::content_hash_for` — byte-identical twin of the
/// `HostView` impl — carries the same scheduler-authoritative contract.
#[test]
fn host_view_ref_content_hash_is_scheduler_authoritative_not_stale_artifact() {
    let canonical = "/base/ref-probe.ts";
    let (host, real_hash) = host_with_materialized_ts(
        canonical,
        "export interface Probe { b: string; }\nexport const probe = 2;\n",
    );
    plant_stale_indexed(&host, canonical);

    let view = HostViewRef::new(host.as_ref());
    let view_hash = view
        .content_hash_for(canonical)
        .expect("HostViewRef reports a current content hash for a live canonical");
    assert_eq!(
        view_hash, real_hash,
        "HostViewRef::content_hash_for MUST return the scheduler-authoritative \
         current hash, NOT the stale lingering artifact's hash",
    );
    assert_ne!(view_hash, STALE_HASH);
}

/// An evicted / deleted canonical with only a stale `IndexedReady`
/// lingering reports `content_hash_for = None` — a content-pinned
/// lookup keyed by this value then becomes a true miss instead of
/// resolving the stale artifact via its own hash.
#[test]
fn host_view_content_hash_is_none_for_evicted_canonical_with_lingering_artifact() {
    let canonical = "/base/evicted.ts";
    let (host, _real_hash) = host_with_materialized_ts(canonical, "export const gone = 1;\n");

    // Plant a stale artifact, THEN evict the canonical: the scheduler's
    // `DerivedRawState` entry is now evicted while the stale artifact
    // lingers in `FileArtifactStore`.
    plant_stale_indexed(&host, canonical);
    host.evict(canonical);

    // The stale artifact is still in the store.
    assert!(
        host.project_type_store()
            .indexed()
            .get_any(canonical)
            .is_some(),
        "fixture invariant: the stale artifact lingers in FileArtifactStore after evict",
    );

    // The view reports `None` — there is no current content for an
    // evicted canonical, even though a stale artifact lingers.
    let view = HostView::new(Arc::clone(&host));
    assert_eq!(
        view.content_hash_for(canonical),
        None,
        "HostView::content_hash_for MUST return None for an evicted canonical — \
         the lingering stale artifact's hash must NOT be surfaced as 'current'",
    );
}

// ──────────────────────────────────────────────────────────────────
// Overlay-view base fallthrough — `OverlaidView`.
// ──────────────────────────────────────────────────────────────────

/// `OverlaidView::content_hash_for` for an **unmasked** canonical (the
/// overlay covers a different file) falls through to the
/// scheduler-authoritative base hash — never the stale artifact scan.
///
/// Pre-fix the base-fallthrough arm scanned `FileArtifactStore` and
/// returned `STALE_HASH`. Post-fix it routes through
/// `authoritative_current_content_hash`.
#[test]
fn overlaid_view_base_fallthrough_content_hash_is_scheduler_authoritative() {
    let owner = "/overlay/owner.ts";
    let (host, owner_real_hash) = host_with_materialized_ts(
        owner,
        "export interface Owner { a: number; }\nexport const owner = 1;\n",
    );
    plant_stale_indexed(&host, owner);

    // The overlay covers an UNRELATED canonical — `owner` is unmasked,
    // so `content_hash_for(owner)` exercises the base-fallthrough arm.
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(
        "/overlay/unrelated.ts".to_string(),
        Arc::from("export const unrelated = 1;\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let fallthrough_hash = view
        .content_hash_for(owner)
        .expect("OverlaidView reports a current content hash for the unmasked owner");
    assert_eq!(
        fallthrough_hash, owner_real_hash,
        "OverlaidView::content_hash_for base fallthrough MUST return the \
         scheduler-authoritative current hash, NOT the stale lingering artifact's hash",
    );
    assert_ne!(
        fallthrough_hash, STALE_HASH,
        "OverlaidView base fallthrough MUST NOT surface the planted stale hash",
    );

    // The overlay-covered canonical still resolves to the overlay hash.
    let overlay_hash = view
        .content_hash_for("/overlay/unrelated.ts")
        .expect("the overlay-covered canonical has a content hash");
    assert_eq!(
        overlay_hash,
        crate::hash::hash_16(b"export const unrelated = 1;\n"),
        "the overlay-covered canonical resolves to the overlay source's hash",
    );
}

// ──────────────────────────────────────────────────────────────────
// Overlay materialiser — derives source + hash from the view itself.
// ──────────────────────────────────────────────────────────────────

/// The overlay materialiser does NOT serve a stale lingering
/// `IndexedReady` from its fast path.
///
/// `materialize_overlay_indexed_ready_with_view` derives both the
/// overlay source and its content hash from the `SessionView` itself.
/// For a base-passthrough view the content hash is the
/// scheduler-authoritative current hash, so the fast-path
/// `FileArtifactStore` lookup is keyed by the FRESH hash and misses the
/// planted stale entry (keyed under the stale hash) — the materialiser
/// then cold-builds a fresh candidate stamped with the fresh hash.
///
/// Pre-fix the materialiser trusted a caller-supplied hash; the caller
/// (`capture_component_meta_inputs_with_view`) derived that hash from
/// the scanning `content_hash_for`, which returned the stale hash — so
/// the fast path `get(canonical, STALE_HASH)` HIT the planted stale
/// artifact and the materialiser returned it. The discriminating
/// assertion: the returned `IndexedReady.whole_hash` is the real hash.
#[test]
fn overlay_materialiser_does_not_serve_stale_lingering_indexed_ready() {
    let canonical = "/materialiser/probe.ts";
    let (host, real_hash) = host_with_materialized_ts(
        canonical,
        "export interface Probe { a: number; }\nexport const probe = 1;\n",
    );
    plant_stale_indexed(&host, canonical);

    // A base-passthrough view: `source()` returns the scheduler bytes,
    // `content_hash_for()` returns the scheduler-authoritative hash.
    let view = HostView::new(Arc::clone(&host));

    let materialised = host
        .materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("the materialiser must produce an IndexedReady for a live canonical");

    assert_eq!(
        materialised.whole_hash, real_hash,
        "the overlay materialiser MUST return an IndexedReady stamped with the \
         scheduler-authoritative current hash — a stale lingering artifact \
         (whole_hash == STALE_HASH) must NOT be served from the fast path",
    );
    assert_ne!(
        materialised.whole_hash, STALE_HASH,
        "the materialiser MUST NOT return the planted stale IndexedReady",
    );

    // The materialiser published its fresh candidate under the fresh
    // hash — a content-pinned read at the real hash now hits it.
    let pinned = host
        .project_type_store()
        .indexed()
        .get(canonical, real_hash)
        .expect("the materialiser publishes the fresh candidate under the real hash");
    assert_eq!(pinned.whole_hash, real_hash);
}

/// The materialiser refuses (`None`) when the view carries no source
/// for the canonical — there is no source/hash pair to fabricate.
#[test]
fn overlay_materialiser_returns_none_when_view_has_no_source() {
    let host: Arc<VerterHost> = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let view = HostView::new(Arc::clone(&host));
    assert!(
        host.materialize_overlay_indexed_ready_with_view("/never/upserted.ts", &view)
            .is_none(),
        "the materialiser MUST return None for a canonical the view carries no \
         source for — it derives source + hash from the view, so a missing \
         source is a refusal, not a fabricated empty artifact",
    );
}
