//! Compile-fail: a dispatcher's query output is the owning carrier of its
//! value. Its delivery cannot be taken out of it, and a capability borrowed
//! from it cannot outlive it.

use verter_type_engine::semantic_query::{SemanticNodeId, SemanticQueryOutput};
use verter_type_engine::semantic_query_memo::delivery_lease::SemanticDeliveryLease;
use verter_type_engine::semantic_query_memo::node_roots::NodeRef;

fn detach(output: SemanticQueryOutput<SemanticNodeId>) -> SemanticDeliveryLease {
    output.delivery
}

fn outlive(output: SemanticQueryOutput<SemanticNodeId>) -> NodeRef<'static> {
    output.node(output.value).unwrap()
}

fn main() {}
