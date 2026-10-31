//! A family continuation's stage constructs semantic nodes in the graph its
//! stage capability hands it.
use verter_type_engine::project_semantic_dispatch::ContinuationStage;
use verter_type_engine::resolver_core::ResolverCapabilities;

#[allow(dead_code)]
fn stage<C: ResolverCapabilities>(cx: &ContinuationStage<'_, '_, C>) -> bool {
    let graph = cx.graph_store();
    std::ptr::eq(graph, cx.graph_store())
}

fn main() {}
