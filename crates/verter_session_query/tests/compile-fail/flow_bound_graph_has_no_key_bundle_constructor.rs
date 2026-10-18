// A bound graph takes its key from the keyed bundle it handles: there is
// no constructor accepting a separately supplied key.
use verter_session_query::flow::bundle::{BoundFlowGraph, FlowGraphBundleRef, FlowSliceFunctionKey};

fn pair(key: FlowSliceFunctionKey, bundle: FlowGraphBundleRef) -> BoundFlowGraph {
    BoundFlowGraph::new(key, bundle)
}

fn main() {}
