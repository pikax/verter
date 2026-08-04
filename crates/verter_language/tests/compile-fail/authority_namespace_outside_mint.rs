use verter_language::carrier_grammar::GrammarAuthorityNamespaceId;
use verter_language::registered_source_authority::SourceAuthorityNamespaceId;

fn main() {
    let _ = SourceAuthorityNamespaceId([0; 16]);
    let _ = GrammarAuthorityNamespaceId([0; 16]);
}
