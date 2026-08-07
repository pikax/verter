use verter_language::carrier_grammar::{
    AcceptedRegisteredCarrierSource, CanonicalCarrierGrammar,
};
use verter_language::registered_source_authority::RegisteredSourceSnapshot;

fn forge(source: RegisteredSourceSnapshot, grammar: CanonicalCarrierGrammar) {
    let _ = AcceptedRegisteredCarrierSource {
        source,
        grammar,
    };
}

fn main() {}
