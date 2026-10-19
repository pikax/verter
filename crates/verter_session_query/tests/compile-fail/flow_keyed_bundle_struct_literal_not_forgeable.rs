// A keyed bundle's key, graph and binding map are derived from one admitted
// prepared structure. Re-keying a bundle's parts, or keying a prepared
// structure without admission, does not compile.
use verter_session_query::flow::binding::FlowBindingMap;
use verter_session_query::flow::bundle::{
    FlowGraphBundle, FlowSliceFunctionKey, KeyedFunctionStructure,
};
use verter_session_query::flow::flow_graph::FunctionFlowGraph;
use verter_session_query::flow::skeleton::{FunctionBodySkeleton, PreparedFunctionBodySkeleton};

fn rekey(
    key: FlowSliceFunctionKey,
    skeleton: FunctionBodySkeleton,
    bindings: FlowBindingMap,
    graph: FunctionFlowGraph,
) -> FlowGraphBundle {
    FlowGraphBundle {
        key,
        skeleton,
        bindings,
        graph,
    }
}

fn unchecked(
    key: FlowSliceFunctionKey,
    prepared: PreparedFunctionBodySkeleton,
) -> KeyedFunctionStructure {
    KeyedFunctionStructure { key, prepared }
}

fn main() {}
