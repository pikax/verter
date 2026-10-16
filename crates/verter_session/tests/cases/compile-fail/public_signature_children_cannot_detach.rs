//! Compile-fail: a signature set's candidate nodes are borrowed from the
//! value that owns them, and a type token is kernel-internal shape identity:
//! a bare or forged ordinal cannot mint one, and no public read turns one
//! back into a node or an owning root.

use std::sync::Arc;
use verter_type_engine::semantic_query::SemanticNodeId;
use verter_type_engine::semantic_query_memo::SemanticGraphStore;
use verter_type_engine::signature_kernel::{
    SemanticReadView, SignatureCandidateNodes, SignatureSetValue, TypeToken,
};

fn detach_candidates(value: &SignatureSetValue) -> Arc<[SignatureCandidateNodes]> {
    Arc::clone(&value.nodes)
}

fn detach_candidate(candidate: &SignatureCandidateNodes) -> SignatureCandidateNodes {
    candidate.clone()
}

fn detach_constituents(candidate: &SignatureCandidateNodes) -> Arc<[SemanticNodeId]> {
    Arc::clone(&candidate.constituents)
}

fn bare_token_ordinal(view: &SemanticReadView, token: TypeToken) -> SemanticNodeId {
    view.type_token_node(token).unwrap()
}

fn mint_a_token_from_an_ordinal(graph: &SemanticGraphStore, id: SemanticNodeId) -> TypeToken {
    graph.signature_store().intern_type_token(id, None).unwrap()
}

fn forge_a_token() -> TypeToken {
    TypeToken::from_raw(1)
}

fn root_a_token(graph: &SemanticGraphStore, token: TypeToken) {
    let _root = graph.signature_store().type_token_root(token);
}

fn root_a_viewed_token(view: &SemanticReadView, token: TypeToken) {
    let _root = view.type_token_root(token);
}

fn main() {}
