// A bound graph is a handle on a keyed bundle: an independently supplied
// key cannot be paired with a bundle.
use std::sync::Arc;
use verter_session_query::flow::bundle::{BoundFlowGraph, FlowGraphBundle, FlowSliceFunctionKey};

fn pair(key: FlowSliceFunctionKey, bundle: Arc<FlowGraphBundle>) -> BoundFlowGraph {
    BoundFlowGraph { key, bundle }
}

fn main() {}
