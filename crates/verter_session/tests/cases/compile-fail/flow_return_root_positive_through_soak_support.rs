//! The soak/tooling runner's capture-off profile (`soak-support` alone)
//! materializes a delivered flow return through the result's own lease: the
//! owning root it hands out feeds the host's bounded projection.

use verter_session::VerterHost;
use verter_type_engine::semantic_query::FlowReturnResult;

fn materialize(host: &VerterHost, result: &FlowReturnResult) -> Option<Vec<u8>> {
    let root = result.return_root().ok()?;
    host.project_node_to_type_expr_json_bytes(&root)
}

fn main() {}
