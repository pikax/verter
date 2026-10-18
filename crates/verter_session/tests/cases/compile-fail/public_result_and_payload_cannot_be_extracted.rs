//! Compile-fail: a delivered read's value and diagnostics are borrowed from
//! the read; a node payload is borrowed from its lease; and a bare ordinal
//! reads nothing. Each extraction would detach graph operands from their
//! owner.

use std::sync::Arc;
use verter_type_engine::project_semantic_dispatch::node_data_for;
use verter_type_engine::project_semantic_dispatch::walk::ShallowDiagnostic;
use verter_type_engine::semantic_query::{CacheRead, QueryResult, SemanticNodeData, SemanticNodeId};
use verter_type_engine::semantic_query_memo::node_data_lease::NodeDataLease;
use verter_type_engine::semantic_query_memo::SemanticGraphStore;

fn extract_value(read: CacheRead<QueryResult<SemanticNodeId>>) -> QueryResult<SemanticNodeId> {
    read.value
}

fn extract_diagnostics(read: &CacheRead<()>) -> Arc<[ShallowDiagnostic]> {
    Arc::clone(&read.walker_diagnostics)
}

fn extract_payload(lease: NodeDataLease) -> verter_type_engine::semantic_query_memo::NodeData {
    lease
}

fn copy_payload(lease: &NodeDataLease) -> SemanticNodeData {
    lease.as_ref().clone()
}

fn read_bare_ordinal(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _ = graph.node_data(id);
    let _ = node_data_for(graph, id);
}

fn main() {}
