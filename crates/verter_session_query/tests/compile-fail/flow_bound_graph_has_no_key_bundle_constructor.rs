// A bound graph takes its key from the keyed bundle it handles: there is
// no constructor accepting a separately supplied key.
use std::sync::Arc;
use verter_session_query::flow::bundle::{BoundFlowGraph, FlowGraphBundle, FlowSliceFunctionKey};

fn pair(key: FlowSliceFunctionKey, bundle: Arc<FlowGraphBundle>) -> BoundFlowGraph {
    BoundFlowGraph::new(key, bundle)
}

fn main() {}
