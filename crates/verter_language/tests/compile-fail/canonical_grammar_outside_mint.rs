use verter_language::carrier_grammar::{
    CanonicalCarrierGrammar, CarrierGrammarConfig, CarrierGrammarFingerprint,
    CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion, GrammarAuthorityNamespaceId,
};
use verter_language::{FrameworkAdapterId, LanguageId};

fn forge(
    authority: GrammarAuthorityNamespaceId,
    adapter_id: FrameworkAdapterId,
    adapter_semantic_version: FrameworkAdapterSemanticVersion,
    language_id: LanguageId,
    parser_grammar_version: CarrierParserGrammarVersion,
    canonical_config: CarrierGrammarConfig,
    fingerprint: CarrierGrammarFingerprint,
) {
    let _ = CanonicalCarrierGrammar {
        authority,
        adapter_id,
        adapter_semantic_version,
        language_id,
        parser_grammar_version,
        canonical_config,
        fingerprint,
    };
}

fn main() {}
