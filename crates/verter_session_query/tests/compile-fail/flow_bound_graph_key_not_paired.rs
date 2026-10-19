// A bound graph is a handle on a keyed bundle: an independently supplied
// key cannot be paired with a bundle.
use verter_session_query::flow::bundle::{BoundFlowGraph, FlowGraphBundleRef, FlowSliceFunctionKey};

fn pair(key: FlowSliceFunctionKey, bundle: FlowGraphBundleRef) -> BoundFlowGraph {
    BoundFlowGraph { key, bundle }
}

fn main() {}
