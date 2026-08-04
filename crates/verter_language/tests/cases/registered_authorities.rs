use std::sync::Arc;

use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::FileLanguage;

#[test]
fn public_authority_api_round_trips_only_sealed_values() {
    let source_authority = RegisteredSourceAuthority::new().expect("source authority");
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new("file:///workspace/App.vue"),
            FileIncarnation::new(1),
            SourceGeneration::new(2),
            FileLanguage::vue(),
            Arc::from("<template />"),
        )
        .expect("registered source");
    let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
    let config =
        CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).expect("Vue config");
    grammar_authority
        .register_carrier_grammar(
            FileLanguage::vue(),
            FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
            CarrierParserGrammarVersion::new(1).expect("grammar version"),
            config.clone(),
        )
        .expect("registered grammar");

    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .expect("accepted registered source");
    assert_eq!(accepted.source().bytes(), "<template />");
    assert_eq!(accepted.source().snapshot_id(), snapshot.snapshot_id());
    assert_eq!(accepted.grammar().canonical_config(), &config);
}
