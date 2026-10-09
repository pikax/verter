//! Graph-native exactness classification for component-meta surfaces.

use verter_session_query::analysis::type_expand::ExpansionExactness;

use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::semantic_query::{SemanticNodeData, SemanticNodeId};

/// Classify a synthesized value node as concrete or symbolic.
pub(crate) fn classify_node(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    node: SemanticNodeId,
) -> ExpansionExactness {
    let unwrapped =
        match verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), node)
            .as_deref()
        {
            Some(SemanticNodeData::Alias(target)) => *target,
            _ => node,
        };
    match verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), unwrapped)
        .as_deref()
    {
        Some(SemanticNodeData::Primitive(_)) | Some(SemanticNodeData::Literal(_)) => {
            ExpansionExactness::ExactConcrete
        }
        Some(SemanticNodeData::Object(_)) if object_is_closed_node(dispatch, unwrapped) => {
            ExpansionExactness::ExactConcrete
        }
        // Both kinds intentionally: a resolved call OR construct signature
        // is a fully-materialised concrete shape.
        Some(SemanticNodeData::Signature { .. }) => ExpansionExactness::ExactConcrete,
        _ => ExpansionExactness::ExactSymbolic,
    }
}

/// Whether a registry-symbol body's root must remain symbolic.
pub(crate) fn node_root_should_stay_symbolic(
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    node: SemanticNodeId,
) -> bool {
    let unwrapped =
        match verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), node)
            .as_deref()
        {
            Some(SemanticNodeData::Alias(target)) => *target,
            _ => node,
        };
    matches!(
        verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), unwrapped)
            .as_deref(),
        Some(
            SemanticNodeData::Mapped { .. }
                | SemanticNodeData::Conditional { .. }
                | SemanticNodeData::IndexedAccess { .. }
                | SemanticNodeData::TypeOf(_)
                // The nominal terminal raises to the same `typeof …`
                // reference shape, so it takes the same operator arm.
                | SemanticNodeData::TypeOfNominal(_)
        )
    )
}

/// An object is closed only when every member value is already concrete.
fn object_is_closed_node(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    node: SemanticNodeId,
) -> bool {
    let Some(data) =
        verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), node)
    else {
        return false;
    };
    let SemanticNodeData::Object(view) = data.as_ref() else {
        return false;
    };
    view.positive_members().iter().all(|member| {
        verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), member.value)
            .is_some_and(|data| {
                !matches!(
                    data.as_ref(),
                    SemanticNodeData::InstantiationRef { .. }
                        | SemanticNodeData::IndexedAccess { .. }
                        | SemanticNodeData::Conditional { .. }
                        | SemanticNodeData::TypeParam { .. }
                        | SemanticNodeData::BareRef(_)
                        | SemanticNodeData::TypeOf(_)
                        | SemanticNodeData::TypeOfNominal(_)
                        | SemanticNodeData::ImportType(_)
                )
            })
    })
}
