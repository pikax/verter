//! Overlay prepared-decl bundle identity — no base shared-cache pollution.
//!
//! Characterizes the invariant: a session-overlay-derived prepared-decl
//! bundle must be keyed on the **raw overlay owner**, never on the
//! `normalized_analysis_canonical` companion.
//!
//! ## The defect this discriminates
//!
//! A canonical id has two forms: the **raw** id the session edited and
//! the **normalized** id `normalized_analysis_canonical(raw)`, which is
//! non-identity for a `.js` runtime file whose `.d.ts` companion is the
//! analysis target.
//!
//! When a session overlays `/pkg/index.js` (raw) — companion
//! `/pkg/index.d.ts` — the overlay materialiser builds the prepared-decl
//! bundle from the overlay `IndexedReady` (overlay content + overlay
//! `whole_hash`). If the bundle is keyed on the normalized companion,
//! every `PreparedTypeDecl::root_identity.canonical_id` it produces is
//! the `.d.ts` companion. A downstream prepared-member / prepared-target
//! write-through then roots its shared-cache entry on
//! `authoritative_current_content_hash(root_identity.canonical_id)` —
//! and the `SessionView` overlay maps are **raw-keyed**, so the view
//! carries no overlay for the `.d.ts` companion. The hash resolves to
//! the BASE companion hash, so the overlay-derived member is admitted to
//! the shared cache under a base-valid signature: the base host, or an
//! unrelated session, would reuse the session's unsaved overlay data.
//!
//! ## Discrimination
//!
//! `overlay_prepared_decl_bundle_probe_for_tests` drives the production
//! path `prepared_decl_bundle_with_context` →
//! `materialize_prepared_decl_bundle_via_ctx` through a real
//! `SessionResolverContext` + `OverlaidView`, then reports the bundle's
//! `root_identity.canonical_id` and the
//! `authoritative_current_content_hash` of that canonical observed
//! through the session ctx vs the base host.
//!
//! - **Pre-fix** (bundle keyed on the normalized companion):
//!   `root_canonical_id` is the `.d.ts` companion;
//!   `session_ctx_root_hash == base_host_root_hash` (both resolve the
//!   base companion hash — no overlay covers the companion). A base view
//!   accepts the overlay-derived member's signature → pollution.
//! - **Post-fix** (bundle keyed on the raw owner): `root_canonical_id`
//!   is the raw `.js`; `session_ctx_root_hash` is the OVERLAY hash while
//!   `base_host_root_hash` is the base `.js` hash — they DIFFER, so a
//!   base view rejects the signature → no pollution.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_session::session_view::{OverlaidView, SessionView};
// `overlay_pollution_probe` hosts the test-support shims that drive the
// production `prepared_decl_bundle_with_context` path through a sealed
// `SessionResolverContext`. The module is gated
// `cfg(any(test, debug_assertions))` on the crate root.
use verter_session::tests::overlay_pollution_probe as probe_shim;
use verter_session::{CompileErrorPolicy, HostConfig, UpsertRequest, VerterHost};

/// Raw `.js` runtime file the session overlays.
const RAW_JS: &str = "/pkg/index.js";
/// `.d.ts` companion — `normalized_analysis_canonical(RAW_JS)`.
const DTS_COMPANION: &str = "/pkg/index.d.ts";

fn fresh_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    }))
}

/// Base-upsert one file into the host.
fn upsert_base(host: &VerterHost, canonical: &str, source: &str) {
    let result = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
    assert!(
        result.is_ok(),
        "base upsert of {canonical} failed: {:?}",
        result.err()
    );
}

/// Base-upsert `/pkg/index.d.ts` + `/pkg/index.js` so
/// `normalized_analysis_canonical("/pkg/index.js")` resolves to the
/// `.d.ts` companion (a non-identity normalisation), then return the
/// host.
fn host_with_js_dts_companion() -> Arc<VerterHost> {
    let host = fresh_host();
    // The `.d.ts` companion MUST exist as analysis source for
    // `normalized_analysis_canonical` to rewrite `.js` → `.d.ts`.
    upsert_base(
        &host,
        DTS_COMPANION,
        "export interface Foo { base: string }\n",
    );
    upsert_base(&host, RAW_JS, "export const Foo = {}\n");
    host
}

/// Control: the worktree fixture actually exhibits the non-identity
/// normalisation. If this fails, the rest of the file proves nothing —
/// the `.js`/`.d.ts` companion split is the precondition for the defect.
#[test]
fn control_js_normalizes_to_dts_companion() {
    let host = host_with_js_dts_companion();
    let normalized = probe_shim::normalized_analysis_canonical_probe(&host, RAW_JS);
    assert_eq!(
        normalized, DTS_COMPANION,
        "control: with a `.d.ts` companion present, `{RAW_JS}` MUST normalise \
         to `{DTS_COMPANION}` — the `.js`→`.d.ts` rewrite is the precondition \
         for the cache-pollution defect this file characterizes"
    );
}

/// **Primary discriminator — bundle identity tied to the raw owner.**
///
/// The overlay-bearing prepared-decl bundle's
/// `PreparedTypeDecl::root_identity.canonical_id` MUST be the RAW
/// overlay owner (`/pkg/index.js`), never the normalized `.d.ts`
/// companion.
///
/// Pre-fix: `root_canonical_id == "/pkg/index.d.ts"` → FAIL.
/// Post-fix: `root_canonical_id == "/pkg/index.js"` → PASS.
#[test]
fn overlay_prepared_decl_bundle_is_keyed_on_raw_owner() {
    let host = host_with_js_dts_companion();

    // The session overlays the RAW `.js` with a TS declaration body.
    // The overlay materialiser parses the overlay source against the
    // analysis canonical (the `.d.ts` companion), so `Foo` lands in
    // `prepared_type_decls`.
    let overlay_source = "export interface Foo { overlaid: number }\n";
    let probe =
        probe_shim::overlay_prepared_decl_bundle_probe(&host, RAW_JS, "Foo", overlay_source)
            .expect("overlay-bearing prepared-decl bundle materialises for the symbol");

    assert_eq!(
        probe.root_canonical_id, RAW_JS,
        "overlay prepared-decl bundle MUST be keyed on the raw overlay owner. \
         `root_identity.canonical_id` was `{}` — keying on the normalized \
         companion `{DTS_COMPANION}` roots prepared-member write-throughs on \
         the base companion hash (the session view's raw-keyed overlay maps \
         carry no overlay for the companion), polluting the shared cache.",
        probe.root_canonical_id,
    );
    // Negative assertion: the normalized companion MUST NOT be the
    // bundle identity.
    assert_ne!(
        probe.root_canonical_id, DTS_COMPANION,
        "the normalized `.d.ts` companion MUST NOT be the overlay bundle's \
         root identity — that is the cache-pollution defect"
    );
}

/// **Pollution discriminator — overlay member roots on an overlay-only
/// hash.**
///
/// A prepared-member / prepared-target write-through roots its shared
/// `PreparedMemberDb` / `PreparedTargetDb` / `DeclarationLookupDb` entry
/// on `authoritative_current_content_hash(root_identity.canonical_id)`
/// observed through the **session** resolver context. For the overlay
/// data to stay isolated, that hash MUST NOT equal the hash a **base**
/// view sees for the same canonical — otherwise the base view's
/// dep-signature validation accepts the overlay-derived entry.
///
/// Pre-fix: `root` is the `.d.ts` companion, which the session view
/// does NOT overlay (its maps are raw-keyed under `.js`), so
/// `session_ctx_root_hash == base_host_root_hash` — both the base
/// companion hash → a base view accepts the entry → FAIL.
/// Post-fix: `root` is the raw `.js`; the session view overlays it, so
/// `session_ctx_root_hash` is the overlay hash and `base_host_root_hash`
/// is the base `.js` hash — they DIFFER → a base view rejects the entry
/// → PASS.
#[test]
fn overlay_member_write_through_hash_is_not_base_visible() {
    let host = host_with_js_dts_companion();

    let overlay_source = "export interface Foo { overlaid: number }\n";
    let probe =
        probe_shim::overlay_prepared_decl_bundle_probe(&host, RAW_JS, "Foo", overlay_source)
            .expect("overlay-bearing prepared-decl bundle materialises for the symbol");

    let session_hash = probe
        .session_ctx_root_hash
        .expect("the session ctx resolves a current content hash for the bundle root");
    let base_hash = probe
        .base_host_root_hash
        .expect("the base host resolves a current content hash for the bundle root");

    assert_ne!(
        session_hash, base_hash,
        "a prepared-member write-through roots its shared-cache entry on the \
         session ctx's `authoritative_current_content_hash` of the bundle root \
         (`{}`). When that hash equals the base host's hash for the same \
         canonical, a base view's dep-signature validation accepts the \
         overlay-derived entry — the base host (or an unrelated session) reuses \
         the session's unsaved overlay data. session={session_hash:?} \
         base={base_hash:?}",
        probe.root_canonical_id,
    );

    // The session-ctx hash MUST be the overlay content hash: the bundle
    // is built from the overlay `IndexedReady`, so the write-through's
    // self-root content version is the overlay's.
    assert_eq!(
        session_hash, probe.bundle_owner_whole_hash,
        "the session ctx's root hash MUST equal the overlay bundle's \
         `owner_whole_hash` (the overlay content version the bundle was built \
         from) — the prepared-member write-through self-roots on exactly one \
         content version, the overlay's. session={session_hash:?} \
         bundle_owner={:?}",
        probe.bundle_owner_whole_hash,
    );
}

/// **Base-view isolation — the overlay hash is genuinely overlay-only.**
///
/// Direct substrate check that the raw owner's overlay content hash (the
/// hash the post-fix bundle roots on) is distinct from what a plain
/// `OverlaidView`-free base read sees for both the raw `.js` and the
/// `.d.ts` companion. This locks down that the post-fix rooting hash
/// cannot be produced by any base-view path.
#[test]
fn overlay_hash_distinct_from_base_js_and_dts_hashes() {
    let host = host_with_js_dts_companion();

    let overlay_source = "export interface Foo { overlaid: number }\n";
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(RAW_JS.to_string(), Arc::from(overlay_source));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    // The overlay content hash for the raw `.js` — the hash the post-fix
    // prepared-member write-through roots on.
    let overlay_hash = view
        .overlay_content_hash_for(RAW_JS)
        .expect("the view carries an explicit overlay for the raw .js");

    // The base host's hashes for the raw `.js` and the `.d.ts`
    // companion — what a base-view dep-signature validation compares
    // against.
    let base_js_hash = probe_shim::base_authoritative_current_content_hash_probe(&host, RAW_JS)
        .expect("base host has a current hash for the raw .js");
    let base_dts_hash =
        probe_shim::base_authoritative_current_content_hash_probe(&host, DTS_COMPANION)
            .expect("base host has a current hash for the .d.ts companion");

    assert_ne!(
        overlay_hash, base_js_hash,
        "the overlay content hash MUST differ from the base `.js` hash — the \
         session edited the file, so a base view rejects an entry rooted on \
         the overlay hash"
    );
    assert_ne!(
        overlay_hash, base_dts_hash,
        "the overlay content hash MUST differ from the base `.d.ts` companion \
         hash — otherwise a base read of the companion would accept the \
         overlay-rooted entry"
    );
}
