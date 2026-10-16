//! Compile-fail: a signature set's candidate nodes are borrowed from the
//! value that owns them, and a type token's node is delivered only as an
//! owning root.

use std::sync::Arc;
use verter_type_engine::semantic_query::SemanticNodeId;
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

fn main() {}
