//! A rootless signature's binder space is named by its complete stable key.

use super::RootlessSpace;
use crate::semantic_query::stable_key::StableKey;
use crate::signature_kernel::{SignatureStore, StoreError};

/// Two rootless signatures whose stable keys share a forced fingerprint but
/// differ in bytes name two binder spaces: they never claim one identity,
/// so the store either keeps them under two keys or reports the typed
/// collision, and never merges them. The same bytes under any fingerprint
/// name one space.
#[test]
fn a_forced_fingerprint_collision_never_merges_two_binder_spaces() {
    const FINGERPRINT: u64 = 0x5eed;
    let first = RootlessSpace::of(&StableKey::with_forced_fingerprint(
        b"rootless-a".to_vec(),
        FINGERPRINT,
    ));
    let second = RootlessSpace::of(&StableKey::with_forced_fingerprint(
        b"rootless-b".to_vec(),
        FINGERPRINT,
    ));
    assert_ne!(
        first.identity, second.identity,
        "keys that share only a fingerprint name two binder spaces"
    );
    let store = SignatureStore::new();
    assert_eq!(store.claim_space_key(first.key, first.identity), Ok(()));
    match store.claim_space_key(second.key, second.identity) {
        Ok(()) => assert_ne!(first.key, second.key, "two spaces under one key"),
        Err(error) => assert_eq!(error, StoreError::BinderKeyCollision),
    }
    assert_eq!(
        RootlessSpace::of(&StableKey::with_forced_fingerprint(
            b"rootless-a".to_vec(),
            FINGERPRINT ^ 1,
        )),
        first,
        "the space is a function of the key bytes, not of the fingerprint"
    );
}

/// Rootless signatures that differ in one literal claim distinct binder
/// spaces: the key keeps 31 bits of a hash that mixes a literal's
/// high-order bytes poorly into its low bits unless it is finalized, so
/// without the finalizer `(x: 249) => void` and `(x: 511) => void` collide,
/// and a thousand such signatures fail at the first collision instead of
/// answering.
#[test]
fn sibling_literal_signatures_claim_distinct_binder_spaces() {
    use crate::semantic_query::{
        FunctionParam, LiteralValue, PrimitiveKind, SemanticNodeData, SignatureKind,
        SignatureReturnCarrier,
    };
    use std::sync::Arc;
    let graph = crate::semantic_query_memo::SemanticGraphStore::new();
    let void = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));
    let store = SignatureStore::new();
    let mut keys = std::collections::HashMap::new();
    for n in 0..1_000u32 {
        let literal = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(
            f64::from(n),
        )));
        let signature = graph.intern_node(SemanticNodeData::Signature {
            kind: SignatureKind::Call,
            params: Arc::from([FunctionParam::synthetic(
                Some(Arc::from("x")),
                literal,
                false,
                false,
            )]),
            return_type: void,
            type_parameters: Arc::from(Vec::new()),
            occurrence: None,
            return_carrier: SignatureReturnCarrier::Declared(void),
            signature_span: None,
            return_type_span: None,
            predicate: None,
            is_abstract: false,
        });
        let space = RootlessSpace::of(&crate::semantic_query::stable_key::stable_key_for_node(
            &graph, signature,
        ));
        if let Some(previous) = keys.insert(space.key, n) {
            panic!(
                "(x: {previous}) => void and (x: {n}) => void share binder-space key {}",
                space.key
            );
        }
        assert_eq!(store.claim_space_key(space.key, space.identity), Ok(()));
    }
}
