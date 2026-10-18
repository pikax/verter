//! Construction nodes are borrowed from their owning carrier; public hot
//! adapters cannot return a raw ordinal in a production build, and a
//! carrier's value cannot be taken from it at the call that delivers it.
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::ResolverCapabilities;
use verter_type_engine::semantic_query::{HotTypeRef, ProjectionReductionContext, SemanticNodeId};
use verter_type_engine::semantic_query_memo::node_roots::{Constructed, NodeRef};

fn outlive(result: Constructed<SemanticNodeId>) -> NodeRef<'static> {
    result.node(*result.value()).unwrap()
}

fn strip_at_the_call<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    error: verter_type_engine::semantic_query::QueryError,
) -> SemanticNodeId {
    dispatch.owning().opaque(error).value
}

fn raw_hot<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    hot: HotTypeRef,
    context: ProjectionReductionContext,
) -> SemanticNodeId {
    dispatch.resolve_hot_handle_with_context(hot, context)
}

fn raw_decl<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    context: ProjectionReductionContext,
) -> Option<HotTypeRef> {
    dispatch.decl_body_hot_ref(
        "/public.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "Target",
        std::sync::Arc::from([]),
        context,
    )
}

fn main() {}

fn raw_callable<C: ResolverCapabilities>(
    callable: &verter_type_engine::project_semantic_dispatch::callable_view::CallableNodeView<
        '_,
        '_,
        C,
    >,
    context: ProjectionReductionContext,
) -> Option<SemanticNodeId> {
    callable.single_callable_arm(context)
}

fn missing_mode<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    expr: &verter_type_expr::TypeExpr,
) -> Constructed<Option<SemanticNodeId>> {
    dispatch
        .owning()
        .lower_type_expr_in_scope_with_mode("/public.ts", expr)
}

fn implicit_owned_mode<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    expr: &verter_type_expr::TypeExpr,
) -> Constructed<Option<SemanticNodeId>> {
    dispatch
        .owning()
        .lower_type_expr_in_scope("/public.ts", expr)
}

fn implicit_raw_mode<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    expr: &verter_type_expr::TypeExpr,
) -> Option<SemanticNodeId> {
    dispatch.lower_type_expr_in_scope("/public.ts", expr)
}
