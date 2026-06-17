//! The blessed Vue parse-carrier accessor.
//!
//! [`vue_parse`] is the ONE way session code reaches a Vue
//! [`ParsedSfc`] out of the framework-neutral
//! [`FrameworkParseArtifact`]: a token-gated typed downcast through the
//! bare `framework::ctx::carrier_for` helper, authorized by the Vue
//! adapter's [`CarrierAccessToken`] — received from the `.vue`
//! `LanguageRegistry` carrier-row registration proof at host
//! construction ([`receive_vue_carrier_token`]), never constructed
//! here.

use std::sync::{Arc, OnceLock};

use verter_compiler::framework_common::vue_bridge::VueParseCarrier;
use verter_compiler::parser::types::ParsedSfc;
use verter_language::{CarrierAccessToken, FrameworkParseArtifact, LanguageRegistry};

/// The Vue adapter's carrier registration proof.
///
/// Received once per process from `LanguageRegistry` carrier-row
/// construction (the `.vue` row's minted token). `VerterHost`
/// construction calls [`receive_vue_carrier_token`] so the receipt
/// happens at host construction; `vue_parse` reuses the held proof.
fn vue_carrier_token() -> &'static CarrierAccessToken {
    static TOKEN: OnceLock<CarrierAccessToken> = OnceLock::new();
    TOKEN.get_or_init(|| {
        let (_registry, tokens) = LanguageRegistry::__built_in_with_carrier_tokens();
        tokens
            .into_iter()
            .find(|token| token.adapter_id().is_vue())
            .expect("the built-in registry registers the .vue carrier row")
    })
}

/// Receive the Vue carrier registration proof at host construction.
pub(crate) fn receive_vue_carrier_token() {
    let _ = vue_carrier_token();
}

/// A clone of the Vue carrier registration proof for the adapter registry's
/// Vue carrier leg.
///
/// The registry and the blessed `vue_parse()` accessor converge on the SAME
/// minted token (`CarrierAccessToken` is value-equal by adapter id), so the
/// registry leg receives a clone of the held proof rather than minting a second
/// one — there is exactly one mint channel (this module's `OnceLock`).
pub(crate) fn vue_carrier_token_clone() -> CarrierAccessToken {
    vue_carrier_token().clone()
}

/// Token-gated typed access to a Vue parse artifact's [`ParsedSfc`].
///
/// Returns `None` for a non-Vue artifact (wrong-adapter tokens never
/// open foreign carriers). The ONE accessor every session `ParsedSfc`
/// reader routes through.
pub(crate) fn vue_parse(artifact: &FrameworkParseArtifact) -> Option<&Arc<ParsedSfc>> {
    crate::framework::ctx::carrier_for::<VueParseCarrier>(artifact, vue_carrier_token())
        .map(VueParseCarrier::parsed_arc)
}
