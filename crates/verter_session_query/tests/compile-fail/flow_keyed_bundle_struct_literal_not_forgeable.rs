// A keyed bundle's key, graph and binding map are derived from one admitted
// prepared structure. Re-keying a bundle, or keying a prepared structure
// without admission, does not compile.
use verter_session_query::flow::bundle::{
    FlowGraphBundle, FlowSliceFunctionKey, KeyedFunctionStructure,
};
use verter_session_query::flow::skeleton::PreparedFunctionBodySkeleton;

fn rekey(key: FlowSliceFunctionKey, bundle: &FlowGraphBundle) -> FlowGraphBundle {
    FlowGraphBundle {
        key,
        skeleton: bundle.skeleton().clone(),
        bindings: bundle.bindings().clone(),
        graph: bundle.graph().clone(),
    }
}

fn unchecked(
    key: FlowSliceFunctionKey,
    prepared: PreparedFunctionBodySkeleton,
) -> KeyedFunctionStructure {
    KeyedFunctionStructure { key, prepared }
}

fn main() {}
