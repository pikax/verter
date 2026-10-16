//! Compile-fail: holding a graph grants no construction lane. A bare
//! ordinal is rooted or read only inside a construction scope, and a scope
//! opens only through the grant the engine minted with its graph; the grant
//! cannot be minted from the graph (and, see
//! `public_node_capability_cannot_be_forged`, not forged either). No graph
//! read keyed by a bare ordinal — alias chains, origin scopes, structure
//! verdicts, substitution memos — is callable outside the engine.

use verter_type_engine::semantic_query::{PrimitiveKind, SemanticNodeData, SemanticNodeId};
use verter_type_engine::semantic_query_memo::node_roots::NodeConstructionGrant;
use verter_type_engine::semantic_query_memo::SemanticGraphStore;

fn open_a_lane(graph: &SemanticGraphStore) {
    let _scope = graph.enter_node_root_scope();
}

fn root_a_bare_ordinal(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _roots = graph.root_nodes([id]);
}

fn mint_a_grant(graph: &SemanticGraphStore) -> NodeConstructionGrant {
    NodeConstructionGrant::mint(graph)
}

fn follow_an_alias_chain(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _end = graph.alias_chain_end(id, |_| {});
}

fn read_an_origin_scope(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _scope = graph.node_scope(id);
}

fn copy_an_origin_scope(graph: &SemanticGraphStore, id: SemanticNodeId) -> SemanticNodeId {
    graph.intern_preserving_scope(id, SemanticNodeData::Primitive(PrimitiveKind::String))
}

fn classify_a_structure(graph: &SemanticGraphStore, id: SemanticNodeId) -> bool {
    graph.node_reaches_unresolved(id) || graph.node_is_inert_structure(id)
}

fn read_a_substitution(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _hit = graph.substitute_memo_get(id, id, id);
    graph.substitute_memo_publish(id, id, id, id);
}

fn main() {}
