//! Graph-native materialization policy and reduction.
//!
//! This module owns the reduction context for an already-lowered semantic node,
//! the sole graph-native published-member reducer, and the identity-preserving
//! package-backed-root gate used by the projector.

use super::super::dep_signature::emit_dispatch_dep_signature_facts;

crate::project_semantic_dispatch::output_materialization::define_output_capability! {
    /// Capability held only by this graph-native output sink.
    pub(crate) struct MetaResolveFieldTypesOutputCap;
    mint: pub(in crate::meta_resolve::materialize::field_types)
}

fn materializer_context(
    mode: crate::semantic_query::ProjectionMode,
) -> crate::semantic_query::ProjectionReductionContext {
    if matches!(mode, crate::semantic_query::ProjectionMode::Navigate) {
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(mode)
    } else {
        crate::semantic_query::ProjectionReductionContext::published(mode)
    }
}

/// Whether the normalized raised root is an explicit published operator.
///
/// The shared shape-engine fold peels aliases and normalizes intersection
/// sentinels before classifying the root. Mapped carriers publish unless their
/// value is the typed semantic-miss carrier.
fn node_root_is_published_operator(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
) -> bool {
    use crate::project_semantic_dispatch::raise::node_root_is_published_operator_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;

    let dispatch = ProjectSemanticDispatch::new(ctx);
    node_root_is_published_operator_with_dispatch(&dispatch, node)
}

/// The exact reduction context for an already-lowered member node.
///
/// `Navigate` plus a published-operator root is explicit consumer demand and
/// therefore uses `Published(Navigate)`. Other `Navigate` roots remain
/// structural transit; all other modes publish directly.
pub(crate) fn node_materialize_reduction_context(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
    mode: crate::semantic_query::ProjectionMode,
) -> crate::semantic_query::ProjectionReductionContext {
    if matches!(mode, crate::semantic_query::ProjectionMode::Navigate)
        && node_root_is_published_operator(ctx, node)
    {
        crate::semantic_query::ProjectionReductionContext::published(mode)
    } else {
        materializer_context(mode)
    }
}

/// Reduce a settled member node through the single semantic dispatch and emit
/// the complete dependency signature to both active fact channels.
pub(crate) fn reduce_member_value_graph_native_with_context(
    ctx: &dyn crate::resolver_core::ResolverContext,
    _scope_canonical_id: &str,
    member_value: crate::semantic_query::SemanticNodeId,
    context: crate::semantic_query::ProjectionReductionContext,
) -> crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr {
    use crate::project_semantic_dispatch::output_materialization::OutputProjector;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;

    let dispatch = ProjectSemanticDispatch::new(ctx);
    let cap = MetaResolveFieldTypesOutputCap::new(&dispatch);
    let materialized = cap.materialize_reduced_output_type_expr(member_value, context);
    emit_dispatch_dep_signature_facts(ctx, materialized.dep_signature());
    materialized
}

/// Append a contributing declaration scope's authoritative content hash to a
/// cache fence. A missing hash makes the verdict non-cacheable; the keyed owner
/// is already rooted separately and is not duplicated here.
fn push_decl_scope_fence(
    ctx: &dyn crate::resolver_core::ResolverContext,
    canonical: &str,
    scope_canonical_id: &str,
    fence: &mut Vec<(std::sync::Arc<str>, crate::semantic_query::DepVersion)>,
    refused: &mut bool,
) {
    if *refused || canonical == scope_canonical_id || canonical.is_empty() {
        return;
    }
    match ctx.authoritative_current_content_hash(canonical) {
        Some(whole_hash) => fence.push((
            std::sync::Arc::<str>::from(canonical),
            crate::semantic_query::DepVersion::WholeHash(whole_hash),
        )),
        None => *refused = true,
    }
}

/// Decide whether an already-resolved declaration identity is a package-backed
/// object-like root and collect a content fence for every contributing file.
///
/// The declaration is always resolved from its own canonical file. A missing
/// contributing hash returns `None` for the fence so callers must refuse shared
/// cache admission instead of manufacturing a stand-in fact.
pub(crate) fn package_backed_object_like_root_identity_with_fence(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    root_identity: &crate::semantic_query::DeclIdentity,
) -> (bool, Option<crate::semantic_query::DepSignature>) {
    use std::sync::Arc;

    let empty_fence: crate::semantic_query::DepSignature = Arc::from(Vec::new());
    let declaration_scope = root_identity.canonical_id.as_ref();
    if !query_engine
        .ctx
        .workspace_is_package_backed(declaration_scope)
    {
        return (false, Some(empty_fence));
    }

    let mut fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let mut refused = false;
    push_decl_scope_fence(
        query_engine.ctx,
        declaration_scope,
        scope_canonical_id,
        &mut fence,
        &mut refused,
    );

    let declaration =
        query_engine.resolve_type_declaration(declaration_scope, root_identity.decl_name.as_ref());
    if matches!(
        declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface
            | crate::resolver_core::ResolvedDeclarationKind::Class,
    ) {
        return if refused {
            (true, None)
        } else {
            (true, Some(Arc::from(fence.into_boxed_slice())))
        };
    }

    let declaration_name = if declaration.resolved_name.is_empty() {
        root_identity.decl_name.to_string()
    } else {
        declaration.resolved_name
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope, declaration_name.as_str());
    push_decl_scope_fence(
        query_engine.ctx,
        target_scope.as_str(),
        scope_canonical_id,
        &mut fence,
        &mut refused,
    );

    let verdict = query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .and_then(|locator| {
            let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(
                query_engine.ctx,
            );
            dispatch.raise_authored_locator_to_hot(
                &locator,
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
            )
        })
        .is_some_and(|hot| {
            crate::resolver_core::component_meta_query_engine::component_meta_registry_node_has_explicit_object_surface(
                query_engine.ctx,
                hot.node(),
            )
        });
    if refused {
        return (verdict, None);
    }
    (verdict, Some(Arc::from(fence.into_boxed_slice())))
}
