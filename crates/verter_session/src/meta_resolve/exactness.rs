//! Graph-native exactness classification for component-meta surfaces.

use verter_semantic::analysis::type_expand::ExpansionExactness;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};

/// Classify a synthesized value node as concrete or symbolic.
pub(crate) fn classify_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> ExpansionExactness {
    let unwrapped =
        match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node).as_deref() {
            Some(SemanticNodeData::Alias(target)) => *target,
            _ => node,
        };
    match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, unwrapped).as_deref() {
        Some(SemanticNodeData::Primitive(_)) | Some(SemanticNodeData::Literal(_)) => {
            ExpansionExactness::ExactConcrete
        }
        Some(SemanticNodeData::Object(_)) if object_is_closed_node(dispatch, unwrapped) => {
            ExpansionExactness::ExactConcrete
        }
        Some(SemanticNodeData::Function { .. }) => ExpansionExactness::ExactConcrete,
        _ => ExpansionExactness::ExactSymbolic,
    }
}

/// Whether a registry-symbol body's root must remain symbolic.
pub(crate) fn node_root_should_stay_symbolic(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    let unwrapped = match crate::project_semantic_dispatch::node_data_for(ctx, node).as_deref() {
        Some(SemanticNodeData::Alias(target)) => *target,
        _ => node,
    };
    matches!(
        crate::project_semantic_dispatch::node_data_for(ctx, unwrapped).as_deref(),
        Some(
            SemanticNodeData::Mapped { .. }
                | SemanticNodeData::Conditional { .. }
                | SemanticNodeData::IndexedAccess { .. }
                | SemanticNodeData::TypeOf(_)
        )
    )
}

/// An object is closed only when every member value is already concrete.
fn object_is_closed_node(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> bool {
    let Some(data) = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node) else {
        return false;
    };
    let SemanticNodeData::Object(view) = data.as_ref() else {
        return false;
    };
    view.members.iter().all(|member| {
        crate::project_semantic_dispatch::node_data_for(dispatch.ctx, member.value).is_some_and(
            |data| {
                !matches!(
                    data.as_ref(),
                    SemanticNodeData::InstantiationRef { .. }
                        | SemanticNodeData::IndexedAccess { .. }
                        | SemanticNodeData::Conditional { .. }
                        | SemanticNodeData::TypeParam { .. }
                        | SemanticNodeData::BareRef(_)
                        | SemanticNodeData::TypeOf(_)
                        | SemanticNodeData::ImportType(_)
                )
            },
        )
    })
}
