//! Compile-fail: a node capability is minted only by its owner, never from a
//! bare ordinal, and it cannot outlive the owner it borrows from. A builder's
//! construction copy cannot be requested outside the store.

use verter_type_engine::semantic_query::{SemanticNodeId, SurfaceMember};
use verter_type_engine::semantic_query_memo::construction_copy::ConstructionCopy;
use verter_type_engine::semantic_query_memo::node_data_lease::NodeDataLease;
use verter_type_engine::semantic_query_memo::node_roots::{NodeRef, NodeRoot};

fn forge(id: SemanticNodeId) -> NodeRef<'static> {
    NodeRef { id, arena: todo!() }
}

fn outlive_lease(lease: NodeDataLease) -> NodeRef<'static> {
    lease.node_ref().unwrap()
}

fn outlive_root(root: NodeRoot) -> NodeRef<'static> {
    root.node_ref().unwrap()
}

fn extend<'a>(node: NodeRef<'a>) -> NodeRef<'static> {
    node
}

fn copy_outside_store(member: &SurfaceMember) -> SurfaceMember {
    member.copy_with(verter_type_engine::semantic_query_memo::construction_copy::sealed::CopyToken(()))
}

fn main() {}
