//! Compile-pass control: the owning and borrowed public carriers expose every
//! read a consumer needs — borrowed payloads, child capabilities, delivered
//! values and diagnostics, signature candidates and owning roots — without a
//! detached copy.

use verter_type_engine::project_semantic_dispatch::walk::ShallowDiagnostic;
use verter_type_engine::semantic_query::{
    CacheRead, QueryError, QueryResult, SemanticNodeData, SemanticNodeId,
};
use verter_type_engine::semantic_query_memo::node_data_lease::{NodeDataLease, SurfaceLease};
use verter_type_engine::semantic_query_memo::node_roots::NodeRoot;
use verter_type_engine::semantic_query_memo::SemanticGraphStore;
use verter_type_engine::signature_kernel::SignatureSetValue;

fn read_children(graph: &SemanticGraphStore, lease: &NodeDataLease) -> Result<usize, QueryError> {
    let mut members = 0;
    if let SemanticNodeData::Object(view) = &**lease {
        for member in view.positive_members() {
            let child = graph.read(lease.child(member.value)?)?;
            members += usize::from(matches!(&*child, SemanticNodeData::Object(_)));
        }
        members += view.entries().len() + view.call_signatures().len();
    }
    Ok(members)
}

fn read_delivery(
    graph: &SemanticGraphStore,
    read: &CacheRead<QueryResult<SemanticNodeId>>,
) -> Result<Option<NodeDataLease>, QueryError> {
    let diagnostics: &[ShallowDiagnostic] = read.walker_diagnostics();
    let _ = diagnostics.len();
    match read.value() {
        QueryResult::Value(node) => graph.read(read.node(*node)?).map(Some),
        QueryResult::Recursive(_) | QueryResult::Error(_) => Ok(None),
    }
}

fn hold_root(lease: &NodeDataLease) -> Result<NodeRoot, QueryError> {
    lease.node_ref()?.to_root()
}

fn keep_value(value: &SignatureSetValue) -> SignatureSetValue {
    let _ = value.candidate_nodes().len();
    value.clone()
}

fn keep_surface(surface: &SurfaceLease) -> SurfaceLease {
    let _ = surface.positive_members().len();
    surface.clone()
}

fn main() {}
