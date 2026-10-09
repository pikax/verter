//! Compile-fail fixture: a `NodeLease` is the only handle a caller outside
//! the semantic store may hold a node by, and only the store mints one —
//! over a node it verified live. Its fields are private, so a raw id can
//! never be dressed up as a lease: an unleased id cannot escape as one.

use verter_type_engine::semantic_query::SemanticNodeId;
use verter_type_engine::semantic_query_memo::NodeLease;

fn forge(id: SemanticNodeId) -> NodeLease {
    NodeLease { id }
}

fn main() {}
