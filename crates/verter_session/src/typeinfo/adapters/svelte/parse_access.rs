//! The blessed Svelte parse-carrier accessor.
//!
//! [`svelte_parse`] is the ONE way session code reaches a
//! [`ParsedSvelte`](verter_compiler::svelte::ParsedSvelte) out of the
//! framework-neutral [`FrameworkParseArtifact`]: a token-gated typed downcast
//! through the bare `framework::ctx::carrier_for` helper, authorized by the
//! Svelte adapter's [`CarrierAccessToken`] — received from the `.svelte`
//! `LanguageRegistry` carrier-row registration proof at host construction,
//! never constructed here.

use std::sync::{Arc, OnceLock};

use verter_compiler::svelte::carrier::SvelteParseCarrier;
use verter_compiler::svelte::ParsedSvelte;
use verter_language::{
    CarrierAccessToken, FrameworkAdapterId, FrameworkParseArtifact, LanguageRegistry,
};

/// The Svelte adapter's carrier registration proof.
///
/// Received once per process from `LanguageRegistry` carrier-row construction
/// (the `.svelte` row's minted token).
fn svelte_carrier_token() -> &'static CarrierAccessToken {
    static TOKEN: OnceLock<CarrierAccessToken> = OnceLock::new();
    TOKEN.get_or_init(|| {
        let (_registry, tokens) = LanguageRegistry::__built_in_with_carrier_tokens();
        tokens
            .into_iter()
            .find(|token| *token.adapter_id() == FrameworkAdapterId::svelte())
            .expect("the built-in registry registers the .svelte carrier row")
    })
}

/// A clone of the Svelte carrier registration proof for the adapter registry's
/// Svelte carrier leg. The registry and the blessed `svelte_parse()` accessor
/// converge on the SAME minted token (value-equal by adapter id).
pub(crate) fn svelte_carrier_token_clone() -> CarrierAccessToken {
    svelte_carrier_token().clone()
}

/// Token-gated typed access to a Svelte parse artifact's [`ParsedSvelte`].
///
/// Returns `None` for a non-Svelte artifact (wrong-adapter tokens never open
/// foreign carriers). The ONE accessor every session `ParsedSvelte` reader
/// routes through.
pub(crate) fn svelte_parse(artifact: &FrameworkParseArtifact) -> Option<&Arc<ParsedSvelte>> {
    crate::framework::ctx::carrier_for::<SvelteParseCarrier>(artifact, svelte_carrier_token())
        .map(SvelteParseCarrier::parsed_arc)
}
