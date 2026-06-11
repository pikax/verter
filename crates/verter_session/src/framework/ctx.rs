//! Blessed token-gated carrier access for session-side adapters.
//!
//! The ONLY session home of the raw `verter_language` carrier downcast
//! (`__carrier_downcast_ref` / `__carrier_downcast_arc` — see the
//! `carrier_downcast_confined_to_owning_adapter` architecture guard).
//! Adapter accessors (the Vue adapter's `vue_parse()`, the adapter
//! registry's `FrameworkAdapterCtx::carrier_for`) route through the
//! bare helpers below; nothing else calls the raw downcast.

use verter_language::{CarrierAccessToken, CarrierParse, FrameworkParseArtifact};

/// Token-gated typed carrier access (reference form).
///
/// Returns the typed carrier ONLY when `token` is the artifact's own
/// adapter's registration proof (minted by `verter_language` during
/// `LanguageRegistry` carrier-row construction) AND the erased payload
/// is a `T`.
pub(crate) fn carrier_for<'a, T: CarrierParse>(
    artifact: &'a FrameworkParseArtifact,
    token: &CarrierAccessToken,
) -> Option<&'a T> {
    verter_language::__carrier_downcast_ref::<T>(artifact, token)
}

/// Token-gated typed carrier access (`Arc` form).
#[allow(dead_code)] // Part of the blessed accessor surface; adapter consumers land with the registry.
pub(crate) fn carrier_for_arc<T: CarrierParse>(
    artifact: &FrameworkParseArtifact,
    token: &CarrierAccessToken,
) -> Option<std::sync::Arc<T>> {
    verter_language::__carrier_downcast_arc::<T>(artifact, token)
}
