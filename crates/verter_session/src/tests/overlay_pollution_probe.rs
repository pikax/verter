//! Test-support probe for the overlay prepared-decl bundle identity
//! invariant — drives the production
//! `prepared_decl_bundle_with_context` →
//! `materialize_prepared_decl_bundle_via_ctx` path through a real
//! `SessionResolverContext` + `OverlaidView`.
//!
//! Integration tests under `crates/verter_session/tests/*.rs` build as
//! a separate crate target and cannot reach `SessionResolverContext`
//! (sealed `pub(crate)`) or `VerterHost::authoritative_current_content_hash`
//! / `normalized_analysis_canonical` (`pub(crate)`). This module hosts
//! the shims; it sits under the crate-root `tests` module (gated
//! `cfg(any(test, debug_assertions))`), so the integration test reaches
//! it as `verter_session::tests::overlay_pollution_probe`.

use std::sync::Arc;

use crate::resolver_core::ResolverContext;
use crate::session_view::OverlaidView;
use crate::types::Hash16;
use crate::VerterHost;

/// Observable facts about an overlay-bearing prepared-decl bundle —
/// returned by [`overlay_prepared_decl_bundle_probe`].
///
/// Used by `tests/overlay_prepared_decl_no_base_cache_pollution.rs` to
/// discriminate the cache-pollution defect where a session-overlay
/// prepared-decl bundle is keyed under the normalised companion
/// canonical, so prepared-member write-throughs root overlay-derived
/// members on the BASE companion hash.
pub struct OverlayPreparedDeclProbe {
    /// The `root_identity.canonical_id` of the requested symbol's
    /// `PreparedTypeDecl` in the overlay-bearing bundle. This is the
    /// canonical a downstream prepared-member / prepared-target
    /// write-through uses as `scope_canonical_id` when rooting a
    /// shared-cache entry.
    pub root_canonical_id: String,
    /// The bundle's `owner_whole_hash` — the content version of the
    /// `ShallowFileState` the bundle was built from. For an
    /// overlay-bearing bundle this is the OVERLAY content hash.
    pub bundle_owner_whole_hash: Hash16,
    /// `authoritative_current_content_hash(root_canonical_id)` observed
    /// through the **session** resolver context — the exact hash a
    /// prepared-member write-through roots on.
    pub session_ctx_root_hash: Option<Hash16>,
    /// `authoritative_current_content_hash(root_canonical_id)` observed
    /// through the **base host** (no session view) — the hash a
    /// base-view dep-signature validation compares against.
    ///
    /// Pollution invariant: when this EQUALS `session_ctx_root_hash`
    /// the overlay-derived member is admitted to the shared cache under
    /// a signature a base view also accepts. The two MUST differ when
    /// the bundle was built from a real overlay over a
    /// `.js`-with-`.d.ts`-companion file.
    pub base_host_root_hash: Option<Hash16>,
}

/// Build an overlay-bearing prepared-decl bundle for `raw_canonical`
/// under a session that overlays `raw_canonical` with `overlay_source`,
/// and report observable facts about the bundle's root identity and the
/// hashes a prepared-member write-through would root on.
///
/// Returns `None` when no overlay-bearing bundle materialises or the
/// requested symbol has no `PreparedTypeDecl`.
pub fn overlay_prepared_decl_bundle_probe(
    host: &Arc<VerterHost>,
    raw_canonical: &str,
    symbol_name: &str,
    overlay_source: &str,
) -> Option<OverlayPreparedDeclProbe> {
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(raw_canonical.to_string(), Arc::from(overlay_source));
    let view = OverlaidView::new(Arc::clone(host), overlays);
    let store_view = host.resolver_store_view().with_session_overlay(host, &view);
    let session_ctx = crate::resolver_core::SessionResolverContext::new(host, &view, &store_view);

    let bundle = ResolverContext::prepared_decl_bundle(&session_ctx, raw_canonical)?;
    let prepared = bundle.prepared_type_decls.get(symbol_name)?;
    let root_canonical_id = prepared.root_identity.canonical_id.to_string();

    Some(OverlayPreparedDeclProbe {
        session_ctx_root_hash: ResolverContext::authoritative_current_content_hash(
            &session_ctx,
            root_canonical_id.as_str(),
        ),
        base_host_root_hash: host.authoritative_current_content_hash(root_canonical_id.as_str()),
        bundle_owner_whole_hash: bundle.owner_whole_hash,
        root_canonical_id,
    })
}

/// Resolve `normalized_analysis_canonical(canonical_id)` on `host`.
pub fn normalized_analysis_canonical_probe(host: &VerterHost, canonical_id: &str) -> String {
    host.normalized_analysis_canonical(canonical_id)
        .into_owned()
}

/// Resolve `VerterHost::authoritative_current_content_hash` — the
/// scheduler-authoritative base content hash, no overlay view.
pub fn base_authoritative_current_content_hash_probe(
    host: &VerterHost,
    canonical: &str,
) -> Option<Hash16> {
    host.authoritative_current_content_hash(canonical)
}
