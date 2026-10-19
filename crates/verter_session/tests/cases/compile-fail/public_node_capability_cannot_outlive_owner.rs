//! Compile-fail: a node capability cannot outlive the owner it borrows from,
//! nor be widened to a longer lifetime than that owner's.

use verter_type_engine::semantic_query_memo::node_data_lease::NodeDataLease;
use verter_type_engine::semantic_query_memo::node_roots::{NodeRef, NodeRoot};

fn outlive_lease(lease: NodeDataLease) -> NodeRef<'static> {
    lease.node_ref().unwrap()
}

fn outlive_root(root: NodeRoot) -> NodeRef<'static> {
    root.node_ref().unwrap()
}

fn extend<'a>(node: NodeRef<'a>) -> NodeRef<'static> {
    node
}

fn main() {}
