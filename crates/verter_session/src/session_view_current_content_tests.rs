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

// ──────────────────────────────────────────────────────────────────
// Overlay materialiser — view lookups use the RAW canonical, not the
// `normalized_analysis_canonical` rewrite.
// ──────────────────────────────────────────────────────────────────

/// The overlay materialiser reads `view.source` / `view.content_hash_for`
/// under the RAW requested canonical, even when
/// `normalized_analysis_canonical` rewrites that canonical to a
/// companion (a runtime `.js` whose `.d.ts` companion is the analysis
/// target).
///
/// ## The defect this test pins
///
/// `materialize_overlay_indexed_ready_with_view` normalises
/// `canonical_id` for its artifact-store key and build target. A prior
/// shape normalised FIRST, then called every `view.*` lookup on the
/// normalised id. The `SessionView` overlay maps are keyed by the RAW
/// requested canonical, so for a `.js` runtime file with a `.d.ts`
/// companion the materialiser read `view.source("/pkg/index.d.ts")` —
/// the BASE `.d.ts` source — instead of the overlaid `/pkg/index.js`
/// bytes. The session's overlay was silently dropped.
///
/// ## Fixture
///
/// Base workspace carries `/pkg/index.js` (a runtime stub) AND its
/// `/pkg/index.d.ts` companion, so `normalized_analysis_canonical`
/// rewrites `/pkg/index.js` → `/pkg/index.d.ts` (asserted as a fixture
/// invariant). The base `.d.ts` declares NOTHING relative; the base
/// `.js` imports nothing. The `OverlaidView` overlays `/pkg/index.js`
/// with a body that imports `./helper`, and overlays the overlay-only
/// `/pkg/helper.ts` (no disk / base presence).
///
/// ## Discrimination property
///
/// Post-fix the materialiser builds from the OVERLAID `.js` bytes:
/// `IndexedReady.raw_source` is the overlay source and the `./helper`
/// route resolves to the overlay-only `/pkg/helper.ts`. Pre-fix it
/// builds from the base `.d.ts` companion: `raw_source` is the `.d.ts`
/// text and there is NO `./helper` route. Both assertions FAIL against
/// the pre-fix tree and PASS post-fix.
#[test]
fn overlay_materialiser_view_lookups_use_raw_canonical_for_normalised_js() {
    // Base `.js` runtime stub — imports nothing relative.
    const BASE_JS: &str = "export const runtime = 1;\n";
    // Base `.d.ts` companion — declares nothing relative. This is what
    // `normalized_analysis_canonical` rewrites `/pkg/index.js` to, and
    // what a pre-fix materialiser would wrongly read through the view.
    const BASE_DTS: &str = "export declare const runtime: number;\n";
    // Overlaid `/pkg/index.js` body — imports the overlay-only
    // `./helper`. This is the content the materialiser MUST build from.
    const OVERLAY_JS: &str = "import { helper } from './helper';\nexport const runtime = helper;\n";
    // Overlay-only relative helper — no base / disk presence.
    const OVERLAY_HELPER: &str = "export const helper = 1;\n";

    let host = VerterHost::new_standalone(HostConfig::default());
    for (path, source) in [("/pkg/index.js", BASE_JS), ("/pkg/index.d.ts", BASE_DTS)] {
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(path.to_string()),
                input_id: path.to_string(),
                source: Arc::from(source),
                file_kind: crate::FileKind::from_path(path),
                aliases: Vec::new(),
            })
            .expect("base seed upsert succeeds");
    }
    let host = Arc::new(host);

    // Fixture invariant: the `.d.ts` companion exists, so the runtime
    // `.js` canonical normalises to a NON-IDENTITY analysis target.
    // Without this the bug cannot reproduce (identity normalisation
    // makes the raw and normalised lookups coincide).
    let normalized = host.normalized_analysis_canonical("/pkg/index.js");
    assert_eq!(
        normalized.as_ref(),
        "/pkg/index.d.ts",
        "fixture invariant: `/pkg/index.js` with a `.d.ts` companion must \
         normalise to `/pkg/index.d.ts` — this non-identity rewrite is the \
         precondition the bug needs",
    );

    // Overlay `/pkg/index.js` with the `./helper`-importing body, and
    // overlay the overlay-only `/pkg/helper.ts`. The overlay map is
    // keyed by the RAW `.js` canonical.
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert("/pkg/index.js".to_string(), Arc::from(OVERLAY_JS));
    overlays.insert("/pkg/helper.ts".to_string(), Arc::from(OVERLAY_HELPER));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let indexed = host
        .materialize_overlay_indexed_ready_with_view("/pkg/index.js", &view)
        .expect("the overlay materialiser produces an IndexedReady for the overlaid .js");

    // Discriminator 1 — the materialised artifact's source is the
    // OVERLAID `.js` bytes, not the base `.d.ts` companion the
    // normalised id would have selected.
    assert_eq!(
        indexed.raw_source.as_ref(),
        OVERLAY_JS,
        "the materialiser MUST build from the overlaid `/pkg/index.js` source — \
         a `raw_source` equal to the base `.d.ts` companion means the view \
         lookup ran on the NORMALISED id (`/pkg/index.d.ts`) and dropped the \
         session overlay",
    );
    assert_ne!(
        indexed.raw_source.as_ref(),
        BASE_DTS,
        "the materialised `raw_source` must NOT be the base `.d.ts` companion text",
    );

    // Discriminator 2 — the overlay-only `./helper` import resolved to
    // the overlay-only `/pkg/helper.ts`. The base `.d.ts` companion has
    // no `./helper` import at all, so a pre-fix build from it carries
    // no such route.
    let helper_route = indexed
        .import_routes
        .get("./helper")
        .and_then(|resolution| resolution.resolved_canonical_id.clone());
    assert_eq!(
        helper_route.as_deref(),
        Some("/pkg/helper.ts"),
        "the overlaid `.js` imports `./helper`; the materialiser MUST resolve it \
         to the overlay-only `/pkg/helper.ts`. A missing route means the \
         materialiser parsed the base `.d.ts` companion (which has no `./helper` \
         import) instead of the overlaid `.js`",
    );
}

// ──────────────────────────────────────────────────────────────────
// Overlay artifact — downstream reachability when `normalize(raw)` is
// non-identity (the two-identity overlay-artifact-keying contract).
// ──────────────────────────────────────────────────────────────────

/// A `SessionResolverContext` content-pinned reader
/// (`indexed_for_current_content`) reaches the overlay artifact the
/// materialiser published, even when the requested canonical normalises
/// to a different analysis canonical (a `.js` runtime file with a
/// `.d.ts` companion).
///
/// ## The defect this test pins
///
/// `materialize_overlay_indexed_ready_with_view` publishes the overlay
/// `IndexedReady` under a `FileArtifactKey` whose `canonical` is the
/// NORMALISED analysis canonical (`normalized_analysis_canonical(raw)` —
/// e.g. `/pkg/index.d.ts` for `/pkg/index.js`) but whose `content_hash`
/// and `parse_env_hash` discriminator are derived from the RAW overlay
/// owner. The `SessionResolverContext` content-pinned readers
/// (`observe_materialize_scope`, `indexed_for_current_content`) built
/// their `overlay_scoped` lookup key under the RAW canonical with no
/// `normalized_analysis_canonical` call, so for any overlay whose
/// canonical normalises non-identically they MISSED the published
/// artifact: the lookup key's `canonical` was `/pkg/index.js` while the
/// publish key's `canonical` was `/pkg/index.d.ts`.
///
/// ## Fixture
///
/// Base workspace carries `/pkg/index.js` AND its `/pkg/index.d.ts`
/// companion, so `normalized_analysis_canonical("/pkg/index.js")`
/// rewrites to `/pkg/index.d.ts` (asserted as a fixture invariant). The
/// `OverlaidView` overlays `/pkg/index.js` with bytes that differ from
/// both the base `.js` and the base `.d.ts`, so the overlay content
/// hash is distinct and the overlay artifact is unambiguously
/// distinguishable from either base candidate by `whole_hash` +
/// `raw_source`.
///
/// ## Discrimination property
///
/// Post-fix `indexed_for_current_content("/pkg/index.js")` returns the
/// overlay artifact: `whole_hash == overlay_hash` and `raw_source` is
/// the overlay bytes. Pre-fix (`6f5425720`) the reader builds the
/// `overlay_scoped` key under the raw `/pkg/index.js` canonical, misses
/// the `/pkg/index.d.ts`-keyed publish, and — having no base artifact
/// under the overlay hash either — returns `None`. The
/// `expect(...)` on the lookup FAILS against the pre-fix tree and the
/// `raw_source` / `whole_hash` assertions PASS only post-fix.
#[test]
fn overlay_artifact_downstream_reachable_for_normalised_js() {
    use crate::resolver_core::{ResolverContext, SessionResolverContext};

    // Base `.js` runtime stub.
    const BASE_JS: &str = "export const runtime = 1;\n";
    // Base `.d.ts` companion — the non-identity normalisation target.
    const BASE_DTS: &str = "export declare const runtime: number;\n";
    // Overlaid `/pkg/index.js` body — distinct bytes from BOTH bases so
    // the overlay content hash is unambiguous.
    const OVERLAY_JS: &str =
        "export const runtime = 42;\nexport interface OverlayOnly { tag: string; }\n";

    let host = VerterHost::new_standalone(HostConfig::default());
    for (path, source) in [("/pkg/index.js", BASE_JS), ("/pkg/index.d.ts", BASE_DTS)] {
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(path.to_string()),
                input_id: path.to_string(),
                source: Arc::from(source),
                file_kind: crate::FileKind::from_path(path),
                aliases: Vec::new(),
            })
            .expect("base seed upsert succeeds");
    }
    let host = Arc::new(host);

    // Fixture invariant: the `.d.ts` companion exists, so the runtime
    // `.js` canonical normalises to a NON-IDENTITY analysis target.
    // Without this the bug cannot reproduce (identity normalisation
    // makes the raw and normalised lookup keys coincide).
    let normalized = host.normalized_analysis_canonical("/pkg/index.js");
    assert_eq!(
        normalized.as_ref(),
        "/pkg/index.d.ts",
        "fixture invariant: `/pkg/index.js` with a `.d.ts` companion must \
         normalise to `/pkg/index.d.ts` — this non-identity rewrite is the \
         precondition the keying-asymmetry bug needs",
    );

    // Overlay `/pkg/index.js`. The overlay map is keyed by the RAW
    // `.js` canonical.
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert("/pkg/index.js".to_string(), Arc::from(OVERLAY_JS));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let overlay_hash = view
        .overlay_content_hash_for("/pkg/index.js")
        .expect("OverlaidView must report an overlay content hash for the masked `.js`");

    // Materialise + publish the overlay `IndexedReady` candidate. The
    // materialiser publishes under the NORMALISED `/pkg/index.d.ts`
    // analysis canonical with the RAW-derived overlay hash +
    // discriminator.
    let materialised = host
        .materialize_overlay_indexed_ready_with_view("/pkg/index.js", &view)
        .expect("the overlay materialiser produces an IndexedReady for the overlaid `.js`");
    assert_eq!(
        materialised.raw_source.as_ref(),
        OVERLAY_JS,
        "fixture invariant: the materialised artifact is built from the overlay bytes",
    );
    assert_eq!(
        materialised.whole_hash, overlay_hash,
        "fixture invariant: the overlay artifact is keyed by the overlay content hash",
    );

    // Drive the content-pinned read through the session context — the
    // downstream reader that is a genuine miss site.
    let session_store_view = host
        .resolver_store_view()
        .with_session_overlay(&host, &view);
    let ctx = SessionResolverContext::new(
        &host,
        &view,
        &session_store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );

    // The discriminating assertion: `indexed_for_current_content` keyed
    // by the RAW `/pkg/index.js` canonical MUST reach the overlay
    // artifact the materialiser published under the NORMALISED
    // `/pkg/index.d.ts` canonical. Pre-fix the reader builds an
    // `overlay_scoped` key under `/pkg/index.js` and misses the
    // `/pkg/index.d.ts`-keyed publish entirely → `None`.
    let pinned = ctx.indexed_for_current_content("/pkg/index.js").expect(
        "indexed_for_current_content MUST reach the overlay artifact \
             for a `.js` canonical whose `.d.ts` companion makes \
             `normalized_analysis_canonical` non-identity — a `None` here \
             means the content-pinned reader built its `overlay_scoped` \
             lookup key under the raw `.js` id and missed the \
             normalised-keyed publish",
    );
    assert_eq!(
        pinned.whole_hash, overlay_hash,
        "the content-pinned read MUST return the OVERLAY artifact \
         (whole_hash == overlay_hash)",
    );
    assert_eq!(
        pinned.raw_source.as_ref(),
        OVERLAY_JS,
        "the content-pinned read MUST surface the overlaid bytes, not the \
         base `.d.ts` companion text",
    );
    assert_ne!(
        pinned.raw_source.as_ref(),
        BASE_DTS,
        "the content-pinned read must NOT surface the base `.d.ts` companion",
    );
}

/// A `SessionResolverContext::observe_materialize_scope` observation
/// RECOVERS the scope's `SyntacticExportSet` parse fact for an overlay
/// whose canonical normalises non-identically (a `.js` runtime file
/// with a `.d.ts` companion).
///
/// ## The defect this test pins
///
/// `observe_materialize_scope` reads the overlay `IndexedReady` through
/// [`crate::host_manage::overlay_materialize::OverlayArtifactIdentity`]
/// (the normalised analysis canonical keys the `FileArtifactStore`
/// artifact), but the subsequent
/// [`crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content`]
/// call recovered the parse facts through a `FileArtifactStore` lookup
/// keyed by the RAW canonical. The only overlay artifact is keyed by the
/// NORMALISED canonical (`/pkg/index.d.ts` for `/pkg/index.js`), so the
/// raw-keyed `get_artifacts_for_content` missed → the helper returned
/// `None` → `MaterializeScopeObservation::syntactic_export_set` was
/// `None`. The `MaterializeMemoDb` write-through `?`-returns on a `None`
/// `syntactic_export_set`, so shared-cache admission was silently
/// skipped on every such overlay.
///
/// ## Fixture
///
/// Base workspace carries `/pkg/index.js` AND its `/pkg/index.d.ts`
/// companion, so `normalized_analysis_canonical("/pkg/index.js")`
/// rewrites to `/pkg/index.d.ts` (asserted as a fixture invariant). The
/// `OverlaidView` overlays `/pkg/index.js` with bytes carrying real
/// exports (`export const runtime`, `export interface OverlayOnly`), so
/// the overlay artifact's `SyntacticExportSet` parse fact has a non-zero
/// semantic hash — a value distinguishable from the absent-fact zero
/// sentinel.
///
/// ## Discrimination property
///
/// Post-fix `observe_materialize_scope("/pkg/index.js")` carries a
/// `Some` `syntactic_export_set` whose `expected_hash` is the overlay
/// artifact's real `SyntacticExportSet` semantic hash (non-zero). The
/// `ParseFactRef.canonical_id` stays the raw `/pkg/index.js` scope —
/// `engine_fact_signature_for_materialize_memo` requires it to equal the
/// observation's `canonical_id`, and `OverlaidView::with_session_overlay`
/// keys the parse-domain validator (`file_facts`) by the raw overlay
/// owner. Pre-fix (`f9e6016c4`) the raw-keyed `get_artifacts_for_content`
/// misses the normalised-keyed publish → `syntactic_export_set` is
/// `None`. The `is_some()` assertion FAILS against the pre-fix tree and
/// the non-zero-hash assertion PASSES only post-fix.
#[test]
fn observe_materialize_scope_recovers_parse_facts_for_normalised_js_overlay() {
    use crate::resolver_core::{ResolverContext, SessionResolverContext};

    // Base `.js` runtime stub.
    const BASE_JS: &str = "export const runtime = 1;\n";
    // Base `.d.ts` companion — the non-identity normalisation target.
    const BASE_DTS: &str = "export declare const runtime: number;\n";
    // Overlaid `/pkg/index.js` body — real exports so the overlay
    // artifact's `SyntacticExportSet` fact is non-empty (non-zero hash).
    const OVERLAY_JS: &str =
        "export const runtime = 42;\nexport interface OverlayOnly { tag: string; }\n";

    let host = VerterHost::new_standalone(HostConfig::default());
    for (path, source) in [("/pkg/index.js", BASE_JS), ("/pkg/index.d.ts", BASE_DTS)] {
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(path.to_string()),
                input_id: path.to_string(),
                source: Arc::from(source),
                file_kind: crate::FileKind::from_path(path),
                aliases: Vec::new(),
            })
            .expect("base seed upsert succeeds");
    }
    let host = Arc::new(host);

    // Fixture invariant: the `.d.ts` companion exists, so the runtime
    // `.js` canonical normalises to a NON-IDENTITY analysis target.
    // Without this the bug cannot reproduce (identity normalisation
    // makes the raw and normalised lookup keys coincide).
    let normalized = host.normalized_analysis_canonical("/pkg/index.js");
    assert_eq!(
        normalized.as_ref(),
        "/pkg/index.d.ts",
        "fixture invariant: `/pkg/index.js` with a `.d.ts` companion must \
         normalise to `/pkg/index.d.ts` — this non-identity rewrite is the \
         precondition the parse-fact-recovery bug needs",
    );

    // Overlay `/pkg/index.js`. The overlay map is keyed by the RAW
    // `.js` canonical.
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert("/pkg/index.js".to_string(), Arc::from(OVERLAY_JS));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let overlay_hash = view
        .overlay_content_hash_for("/pkg/index.js")
        .expect("OverlaidView must report an overlay content hash for the masked `.js`");

    // Materialise + publish the overlay `IndexedReady` candidate under
    // the NORMALISED `/pkg/index.d.ts` analysis canonical (RAW-derived
    // overlay hash + discriminator).
    let materialised = host
        .materialize_overlay_indexed_ready_with_view("/pkg/index.js", &view)
        .expect("the overlay materialiser produces an IndexedReady for the overlaid `.js`");
    assert_eq!(
        materialised.whole_hash, overlay_hash,
        "fixture invariant: the overlay artifact is keyed by the overlay content hash",
    );

    // Recover the overlay artifact's REAL `SyntacticExportSet` fact hash
    // directly — the value `observe_materialize_scope` must reproduce.
    // The lookup is keyed by the NORMALISED canonical (the artifact-store
    // identity) so it reaches the published overlay artifact.
    let overlay_facts = host
        .project_type_store()
        .indexed()
        .get_artifacts_for_content(normalized.as_ref(), overlay_hash)
        .expect("the overlay artifact's FileArtifacts are reachable under the normalised key");
    let expected_export_set_hash = overlay_facts
        .facts
        .syntactic_export_set()
        .expect("the overlay artifact carries a SyntacticExportSet parse fact")
        .semantic_hash;
    assert_ne!(
        expected_export_set_hash, [0u8; 16],
        "fixture invariant: the overlaid `.js` has real exports, so its \
         SyntacticExportSet fact has a non-zero semantic hash — a zero hash \
         would make the recovered-vs-sentinel distinction undetectable",
    );

    // Drive the materialize-scope observation through the session
    // context — the parse-fact-recovery site.
    let session_store_view = host
        .resolver_store_view()
        .with_session_overlay(&host, &view);
    let ctx = SessionResolverContext::new(
        &host,
        &view,
        &session_store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
    let observation = ctx.observe_materialize_scope("/pkg/index.js").expect(
        "observe_materialize_scope MUST return an observation for the overlaid `.js` — \
         the overlay IndexedReady is reachable via OverlayArtifactIdentity",
    );

    // The discriminating assertion: the observation's
    // `syntactic_export_set` MUST be `Some`. Pre-fix the parse-fact
    // recovery keys `get_artifacts_for_content` by the raw `/pkg/index.js`
    // canonical, misses the `/pkg/index.d.ts`-keyed overlay artifact, and
    // returns `None` → the memo write-through skips shared-cache
    // admission on every such overlay.
    let recovered = observation.syntactic_export_set.as_ref().expect(
        "observe_materialize_scope MUST RECOVER the scope's SyntacticExportSet \
         parse fact for a `.js` overlay whose `.d.ts` companion makes \
         `normalized_analysis_canonical` non-identity — a `None` here means the \
         parse-fact recovery built its `FileArtifactStore` lookup key under the \
         raw `.js` id and missed the normalised-keyed overlay artifact, so \
         MaterializeMemoDb shared-cache admission is silently skipped",
    );
    assert_eq!(
        recovered.expected_hash, expected_export_set_hash,
        "the recovered parse fact MUST carry the overlay artifact's REAL \
         SyntacticExportSet semantic hash — a zero sentinel means the recovery \
         lookup missed the overlay artifact entirely",
    );
    // The `ParseFactRef.canonical_id` stays the RAW scope:
    // `engine_fact_signature_for_materialize_memo` requires it to equal
    // the observation's `canonical_id`, and `OverlaidView::with_session_overlay`
    // keys the parse-domain validator (`file_facts`) by the raw overlay
    // owner — so the emitted fact's id must NOT be normalised.
    assert_eq!(
        recovered.canonical_id.as_str(),
        "/pkg/index.js",
        "the recovered ParseFactRef MUST stay rooted on the raw overlay scope — \
         the materialize-memo signature builder requires `ParseFactRef.canonical_id == \
         observation.canonical_id`, and the OverlaidView parse-domain validator keys \
         `file_facts` by the raw overlay owner",
    );
    assert_eq!(
        observation.canonical_id.as_ref(),
        "/pkg/index.js",
        "the observation's `canonical_id` is the raw overlay scope — the \
         materialize-memo self-root and the ParseFactRef must agree on it",
    );
}

/// A `SessionResolverContext::shallow_file_state` read keyed by the RAW
/// `.js` canonical observes the OVERLAID shallow surface — not the base
/// `.d.ts` companion — when the requested canonical normalises to a
/// different analysis canonical (a runtime `.js` with a `.d.ts`
/// companion).
///
/// ## The defect this test pins
///
/// `shallow_file_state` delegates to `shallow_file_state_with_context`,
/// which resolved the canonical through `resolve_eval_dependency_canonical`
/// (the `normalized_analysis_canonical` rewrite) BEFORE handing it to the
/// overlay-aware reads `ctx.indexed_for_current_content(..)` and
/// `route_owned_shallow_state_with_context(..)`. For an overlay on
/// `/pkg/index.js` whose `/pkg/index.d.ts` companion exists, that
/// pre-normalisation turned the raw `/pkg/index.js` id into
/// `/pkg/index.d.ts` — and the `SessionView` overlay maps are keyed by
/// the RAW overlay owner. `overlay_content_hash_for("/pkg/index.d.ts")`
/// reports `None`, so the overlay gate never fires and the reader falls
/// back to the BASE `.d.ts` companion's shallow state. The overlay the
/// materialiser published is silently dropped.
///
/// Normalisation is one-way: the raw overlay owner cannot be recovered
/// from the normalised companion, so the raw id MUST be carried forward
/// to the overlay-detection point. The fix removes the pre-normalisation
/// from `shallow_file_state_with_context`; the overlay-aware accessors
/// own the raw→normalised split internally.
///
/// ## Fixture
///
/// Base workspace carries `/pkg/index.js` AND its `/pkg/index.d.ts`
/// companion, so `normalized_analysis_canonical("/pkg/index.js")`
/// rewrites to `/pkg/index.d.ts` (asserted as a fixture invariant). The
/// `OverlaidView` overlays `/pkg/index.js` with bytes that declare an
/// `export interface OverlayOnly` — a type symbol NEITHER the base `.js`
/// nor the base `.d.ts` carries — so the overlaid shallow surface is
/// unambiguously distinguishable from either base candidate.
///
/// ## Discrimination property
///
/// Post-fix `ctx.shallow_file_state("/pkg/index.js")` returns the
/// overlay's shallow state: `whole_hash == overlay_hash` and the
/// `symbols` map carries `OverlayOnly`. Pre-fix (`d840d3ecd`)
/// `shallow_file_state_with_context` normalises `/pkg/index.js` →
/// `/pkg/index.d.ts` first, the overlay-aware reads receive the
/// normalised id, `overlay_content_hash_for` misses, and the base
/// `.d.ts` companion's shallow state is returned — `whole_hash` is the
/// base `.d.ts` hash and `OverlayOnly` is absent. The `whole_hash` and
/// `symbols` assertions FAIL against the pre-fix tree and PASS only
/// post-fix.
#[test]
fn shallow_file_state_observes_overlay_for_normalised_js() {
    use crate::resolver_core::{ResolverContext, SessionResolverContext};

    // Base `.js` runtime stub — no `OverlayOnly` symbol.
    const BASE_JS: &str = "export const runtime = 1;\n";
    // Base `.d.ts` companion — the non-identity normalisation target;
    // declares no `OverlayOnly` interface.
    const BASE_DTS: &str = "export declare const runtime: number;\n";
    // Overlaid `/pkg/index.js` body — declares an `OverlayOnly`
    // interface neither base candidate carries.
    const OVERLAY_JS: &str =
        "export const runtime = 42;\nexport interface OverlayOnly { tag: string; }\n";

    let host = VerterHost::new_standalone(HostConfig::default());
    for (path, source) in [("/pkg/index.js", BASE_JS), ("/pkg/index.d.ts", BASE_DTS)] {
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(path.to_string()),
                input_id: path.to_string(),
                source: Arc::from(source),
                file_kind: crate::FileKind::from_path(path),
                aliases: Vec::new(),
            })
            .expect("base seed upsert succeeds");
    }
    let host = Arc::new(host);

    // Fixture invariant: the `.d.ts` companion exists, so the runtime
    // `.js` canonical normalises to a NON-IDENTITY analysis target.
    // Without this the bug cannot reproduce (identity normalisation
    // makes the raw and normalised lookup keys coincide).
    let normalized = host.normalized_analysis_canonical("/pkg/index.js");
    assert_eq!(
        normalized.as_ref(),
        "/pkg/index.d.ts",
        "fixture invariant: `/pkg/index.js` with a `.d.ts` companion must \
         normalise to `/pkg/index.d.ts` — this non-identity rewrite is the \
         precondition the shallow-state keying bug needs",
    );

    // Overlay `/pkg/index.js`. The overlay map is keyed by the RAW
    // `.js` canonical.
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert("/pkg/index.js".to_string(), Arc::from(OVERLAY_JS));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let overlay_hash = view
        .overlay_content_hash_for("/pkg/index.js")
        .expect("OverlaidView must report an overlay content hash for the masked `.js`");

    // The base `.d.ts` companion's shallow state — the stale fallback
    // the pre-fix path returns. Recovered here so the post-fix
    // assertions can prove the overlay surface is distinct from it.
    let base_dts_shallow = host
        .shallow_file_state("/pkg/index.d.ts")
        .expect("the base `.d.ts` companion has a shallow state");
    assert!(
        !base_dts_shallow.symbols.contains_key("OverlayOnly"),
        "fixture invariant: the base `.d.ts` companion declares no \
         `OverlayOnly` interface — a pre-fix `shallow_file_state` of the \
         `.js` overlay falls back to THIS surface",
    );

    // Materialise + publish the overlay `IndexedReady` candidate so the
    // overlay-aware reader has an artifact to reach (prewarm parity with
    // the session-bearing query entry points).
    let materialised = host
        .materialize_overlay_indexed_ready_with_view("/pkg/index.js", &view)
        .expect("the overlay materialiser produces an IndexedReady for the overlaid `.js`");
    assert_eq!(
        materialised.whole_hash, overlay_hash,
        "fixture invariant: the overlay artifact is keyed by the overlay content hash",
    );
    assert!(
        materialised
            .shallow_state
            .symbols
            .contains_key("OverlayOnly"),
        "fixture invariant: the overlay artifact's shallow surface carries \
         the `OverlayOnly` interface",
    );

    // Drive the shallow-state read through the session context — the
    // miss site (`shallow_file_state_with_context`
    // normalises before the overlay-aware reads).
    let session_store_view = host
        .resolver_store_view()
        .with_session_overlay(&host, &view);
    let ctx = SessionResolverContext::new(
        &host,
        &view,
        &session_store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
    let observed = ctx.shallow_file_state("/pkg/index.js").expect(
        "shallow_file_state MUST resolve a shallow state for the overlaid \
         `.js` canonical",
    );

    // Discriminating assertion 1 — the observed `whole_hash` is the
    // OVERLAY content hash, not the base `.d.ts` companion's hash.
    assert_eq!(
        observed.whole_hash, overlay_hash,
        "shallow_file_state keyed by the RAW `/pkg/index.js` MUST observe \
         the OVERLAY content (whole_hash == overlay_hash) — a pre-fix \
         `shallow_file_state_with_context` normalises the raw id to \
         `/pkg/index.d.ts` before the overlay-aware reads, the overlay gate \
         misses, and the base `.d.ts` companion's shallow state is returned",
    );
    assert_ne!(
        observed.whole_hash, base_dts_shallow.whole_hash,
        "shallow_file_state MUST NOT return the base `.d.ts` companion's \
         shallow state for the `.js` overlay",
    );

    // Discriminating assertion 2 — the observed shallow surface carries
    // the overlay-only `OverlayOnly` type symbol.
    assert!(
        observed.symbols.contains_key("OverlayOnly"),
        "shallow_file_state MUST observe the overlaid shallow surface — the \
         `OverlayOnly` interface declared only by the overlay bytes must be \
         present. A pre-fix read falls back to the base `.d.ts` companion \
         (no `OverlayOnly`). Got {:?}",
        observed.symbols.keys().collect::<Vec<_>>(),
    );
}
