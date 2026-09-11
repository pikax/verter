use std::sync::Arc;
use verter_semantic::analysis::flow::flow_ir::ReturnSlicePlan;

fn rewrite(plan: &mut ReturnSlicePlan) {
    plan.value_states = 0;
    plan.value_nodes = Arc::from([]);
}

fn main() {}
