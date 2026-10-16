//! Compile-fail: holding a graph grants no construction lane. A bare
//! ordinal is rooted or read only inside a construction scope, and a scope
//! opens only through the grant the engine minted with its graph; the grant
//! cannot be minted from the graph (and, see
//! `public_node_capability_cannot_be_forged`, not forged either).

use verter_type_engine::semantic_query::SemanticNodeId;
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

fn main() {}
