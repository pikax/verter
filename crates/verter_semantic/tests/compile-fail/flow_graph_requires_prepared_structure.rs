use verter_session_query::flow::flow_graph::build_function_flow_graph;
use verter_session_query::flow::skeleton::FunctionBodySkeleton;

fn unprepared(skeleton: &FunctionBodySkeleton) {
    let _ = build_function_flow_graph(skeleton);
}

fn main() {}
