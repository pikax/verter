//! Compile-fail: the bounded projection takes the owning root a delivered
//! result mints from its lease, never the result's raw return ordinal.

use verter_session::VerterHost;
use verter_type_engine::semantic_query::FlowReturnResult;

fn materialize(host: &VerterHost, result: &FlowReturnResult) -> Option<Vec<u8>> {
    host.project_node_to_type_expr_json_bytes(&result.return_type())
}

fn main() {}
