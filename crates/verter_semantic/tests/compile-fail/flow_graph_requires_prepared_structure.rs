use verter_semantic::analysis::flow::flow_graph::build_function_flow_graph;
use verter_semantic::analysis::flow::FunctionBodySkeleton;

fn unprepared(skeleton: &FunctionBodySkeleton) {
    let _ = build_function_flow_graph(skeleton);
}

fn main() {}
