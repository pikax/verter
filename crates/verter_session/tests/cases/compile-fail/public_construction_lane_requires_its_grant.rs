//! Compile-fail: holding a graph grants no construction lane. A bare
//! ordinal is rooted or read only inside a construction scope, and a scope
//! opens only through the grant the engine minted with its graph; the grant
//! cannot be minted from the graph (and, see
//! `public_node_capability_cannot_be_forged`, not forged either). No graph
//! read keyed by a bare ordinal — alias chains, origin scopes, structure
//! verdicts, substitution memos, member indexes, origin-edge exports — is
//! callable outside the engine, and no bare ordinal releases kernel roots.

use verter_type_engine::semantic_query::{
    PrimitiveKind, SemanticNodeData, SemanticNodeId, SurfaceView,
};
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

fn index_a_surface(graph: &SemanticGraphStore, id: SemanticNodeId, view: &SurfaceView) {
    let _index = graph.member_ordinal_index(id, view);
}

fn export_every_origin_edge(graph: &SemanticGraphStore) {
    let _edges = graph.export_all_origin_edges();
}

fn release_kernel_roots(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _stranded = graph.signature_store().note_released_nodes([id]);
    let _epoch = graph.release_signature_records_naming([id]);
}

fn main() {}
