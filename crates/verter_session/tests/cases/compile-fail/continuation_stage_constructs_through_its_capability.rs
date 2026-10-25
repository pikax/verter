//! A family continuation's stage constructs semantic nodes through the stage
//! capability it runs with.
use verter_audit::observe::Uncaptured;
use verter_type_engine::project_semantic_dispatch::ContinuationStage;
use verter_type_engine::request_observers::CaptureTargets;
use verter_type_engine::resolver_core::ResolverCapabilities;
use verter_type_engine::semantic_query::{PrimitiveKind, SemanticNodeData, SemanticNodeId};

#[allow(dead_code)]
fn stage<C: ResolverCapabilities>(cx: &ContinuationStage<'_, '_, C>) -> SemanticNodeId {
    cx.graph_store().intern_node_in::<Uncaptured>(
        &CaptureTargets::installed::<Uncaptured>(),
        SemanticNodeData::Primitive(PrimitiveKind::String),
    )
}

fn main() {}
