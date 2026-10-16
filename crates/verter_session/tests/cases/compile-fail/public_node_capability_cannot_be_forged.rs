//! Compile-fail: a node capability is minted only by its owner. A bare
//! ordinal cannot be dressed up as one, not even by re-labelling a
//! capability another owner issued.

use verter_type_engine::semantic_query::SemanticNodeId;
use verter_type_engine::semantic_query_memo::node_roots::NodeRef;

fn forge(id: SemanticNodeId, issued: NodeRef<'static>) -> NodeRef<'static> {
    NodeRef { id, ..issued }
}

fn main() {}
