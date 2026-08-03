use serde::de::DeserializeOwned;
use verter_language::carrier_grammar::{
    AcceptedRegisteredCarrierSource, CanonicalCarrierGrammar, GrammarAuthorityNamespaceId,
};
use verter_language::registered_source_authority::{
    RegisteredSourceSnapshot, RegisteredSourceSnapshotId, SourceAuthorityNamespaceId,
};

fn assert_deserializable<T: DeserializeOwned>() {}

fn main() {
    assert_deserializable::<SourceAuthorityNamespaceId>();
    assert_deserializable::<GrammarAuthorityNamespaceId>();
    assert_deserializable::<RegisteredSourceSnapshotId>();
    assert_deserializable::<RegisteredSourceSnapshot>();
    assert_deserializable::<CanonicalCarrierGrammar>();
    assert_deserializable::<AcceptedRegisteredCarrierSource>();
}
