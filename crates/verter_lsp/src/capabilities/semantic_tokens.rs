//! Semantic-tokens capability slot: the advertised legend and full-document
//! support.

use super::CapabilityContext;
use tower_lsp_server::ls_types::ServerCapabilities;
use tower_lsp_server::ls_types::{
    SemanticTokenModifier, SemanticTokenType, SemanticTokensFullOptions, SemanticTokensLegend,
    SemanticTokensOptions, SemanticTokensServerCapabilities,
};

/// The advertised legend IS the shared mapping owner's published
/// vocabulary (`verter_type_runtime::semantic_tokens`): every provider
/// lane remaps its own token space into these exact indices/bits, so the
/// wire legend must be BUILT from the same arrays — a hand-maintained
/// copy here would silently re-introduce the index-space mismatch the
/// shared owner exists to prevent.
pub(crate) fn contribute(caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {
    caps.semantic_tokens_provider = Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
        SemanticTokensOptions {
            full: Some(SemanticTokensFullOptions::Bool(true)),
            legend: SemanticTokensLegend {
                token_types: verter_type_runtime::semantic_tokens::VERTER_TOKEN_TYPES
                    .iter()
                    .map(|name| SemanticTokenType::new(name))
                    .collect(),
                token_modifiers: verter_type_runtime::semantic_tokens::VERTER_TOKEN_MODIFIERS
                    .iter()
                    .map(|name| SemanticTokenModifier::new(name))
                    .collect(),
            },
            ..Default::default()
        },
    ));
}
