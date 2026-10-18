//! Compile-pass control: the owning and borrowed public carriers expose every
//! read a consumer needs — borrowed payloads and their nested arrays, child
//! capabilities, delivered values and diagnostics, signature candidates,
//! owning surface adapters and owning roots — without a detached copy. A
//! query error copies opaquely: it carries its operands without exposing
//! them. Origin reads, display and the canonical union builder keyed by an
//! ordinal stay callable: they answer only inside a construction lane the
//! caller holds, and a dispatcher merges a macro spread formula in its own
//! lane. A dispatcher's query output borrows nothing from the dispatcher: it
//! is kept, cloned and read after the dispatcher is gone, and a builder that
//! composes construction ordinals opens its own explicit owner.

use verter_session_query::flow::policy::NullabilityPolicy;
use verter_type_engine::project_semantic_dispatch::canonical_algebra;
use verter_type_engine::project_semantic_dispatch::one_level_surface::OneLevelSurface;
use verter_type_engine::project_semantic_dispatch::walk::ShallowDiagnostic;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::ResolverCapabilities;
use verter_type_engine::semantic_query::{
    display, CacheRead, DisplayNeeds, ObjectProjectionFormula, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    SemanticQueryValue, SurfaceEntry,
};
use verter_type_engine::semantic_query_memo::node_data_lease::{NodeDataLease, SurfaceLease};
use verter_type_engine::semantic_query_memo::node_roots::{NodeRoot, NodeRootScope};
use verter_type_engine::semantic_query_memo::SemanticGraphStore;
use verter_type_engine::signature_kernel::SignatureSetValue;

fn read_children(graph: &SemanticGraphStore, lease: &NodeDataLease) -> Result<usize, QueryError> {
    let mut members = 0;
    if let SemanticNodeData::Object(view) = &**lease {
        for member in view.positive_members() {
            let child = graph.read(lease.child(member.value)?)?;
            members += usize::from(matches!(&*child, SemanticNodeData::Object(_)));
        }
        members += view.entries().len() + view.call_signatures().len();
    }
    Ok(members)
}

fn read_delivery(
    graph: &SemanticGraphStore,
    read: &CacheRead<QueryResult<SemanticNodeId>>,
) -> Result<Option<NodeDataLease>, QueryError> {
    let diagnostics: &[ShallowDiagnostic] = read.walker_diagnostics();
    let _ = diagnostics.len();
    match read.value() {
        QueryResult::Value(node) => graph.read(read.node(*node)?).map(Some),
        QueryResult::Recursive(_) | QueryResult::Error(_) => Ok(None),
    }
}

fn hold_root(lease: &NodeDataLease) -> Result<NodeRoot, QueryError> {
    lease.node_ref()?.to_root()
}

fn keep_value(value: &SignatureSetValue) -> SignatureSetValue {
    let _ = value.candidate_nodes().len();
    value.clone()
}

fn keep_surface(surface: &SurfaceLease) -> SurfaceLease {
    let _ = surface.positive_members().len();
    surface.clone()
}

fn read_nested_arrays(lease: &NodeDataLease) -> Result<usize, QueryError> {
    Ok(match &**lease {
        SemanticNodeData::Tuple { elements, .. } => {
            let mut readable = 0;
            for element in elements.iter() {
                let _ = lease.child(element.value)?.read()?;
                readable += 1;
            }
            readable
        }
        SemanticNodeData::Signature {
            params,
            type_parameters,
            ..
        } => params.len() + type_parameters.len(),
        SemanticNodeData::Union(arms) => arms.len(),
        SemanticNodeData::ObjectSpreadProgram(program) => program.effects.len(),
        SemanticNodeData::Opaque(QueryError::RecursiveRef { args, .. }) => args.len(),
        _ => 0,
    })
}

fn keep_surface_member(surface: &OneLevelSurface) -> Result<Option<NodeRoot>, QueryError> {
    match surface.entries().first() {
        Some(SurfaceEntry::Member(member)) => surface.node(member.value)?.to_root().map(Some),
        _ => Ok(None),
    }
}

fn read_origin_in_lane(graph: &SemanticGraphStore, id: SemanticNodeId) -> usize {
    let mut edges = 0;
    graph.walk_origin_chain(id, |_, _| edges += 1);
    edges
        + graph.export_origin_edges_in_scope().len()
        + usize::from(graph.node_scope_in_scope(id).is_some())
}

fn render_and_build_in_lane(graph: &SemanticGraphStore, id: SemanticNodeId) -> bool {
    let shown = display::display_in_scope(
        graph,
        &SemanticQueryValue::TypeNode(id),
        DisplayNeeds::empty(),
    );
    let union =
        canonical_algebra::intern_ordered_union_in_scope(graph, &[id], NullabilityPolicy::Strict);
    shown.is_some() && union.is_some()
}

fn spread_through_a_dispatcher<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    formula: &ObjectProjectionFormula,
) -> usize {
    dispatch
        .owning()
        .spread_formula_positive_members_for_macro(formula)
        .value()
        .as_ref()
        .map_or(0, |members| members.len())
}

fn keep_error(error: &QueryError) -> QueryError {
    error.clone()
}

fn read_import_head(data: &SemanticNodeData) -> Option<(&str, usize, bool)> {
    let (specifier, qualifier, typeof_query) = data.import_type_head()?;
    let specifier: &verter_type_expr::OwnedStr = specifier;
    let qualifier: &verter_type_expr::OwnedSlice<verter_type_expr::OwnedStr> = qualifier;
    Some((specifier, qualifier.len(), typeof_query))
}

fn keep_output_past_its_dispatcher<C: ResolverCapabilities>(
    dispatch: ProjectSemanticDispatch<'_, C>,
    key: SemanticQueryKey,
) -> Result<Option<SemanticQueryOutput<SemanticNodeId>>, QueryError> {
    let QueryResult::Value(output) = dispatch.execute_type_node(key) else {
        return Ok(None);
    };
    drop(dispatch);
    let kept = output.clone();
    drop(output);
    let _ = kept.node(kept.value)?.read()?;
    Ok(Some(kept))
}

fn build_under_an_explicit_owner<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    error: QueryError,
) -> Result<(NodeRootScope, NodeRoot), QueryError> {
    let builder = dispatch.enter_construction_scope();
    let constructed = dispatch.owning().opaque(error);
    let root = constructed.node(*constructed.value())?.to_root()?;
    Ok((builder, root))
}

fn hand_a_label_to_an_explicit_builder<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    graph: &SemanticGraphStore,
    error: QueryError,
) -> bool {
    let builder = dispatch.enter_construction_scope();
    let label = dispatch.owning().opaque(error).into_value_in(&builder);
    let readable = graph.read_in_scope(label).is_some();
    drop(builder);
    readable
}

fn main() {}

fn keep_construction_past_dispatcher<C: ResolverCapabilities>(
    dispatch: ProjectSemanticDispatch<'_, C>,
) -> Result<
    Option<verter_type_engine::semantic_query_memo::node_roots::Constructed<SemanticNodeId>>,
    QueryError,
> {
    let result = dispatch.owning().decl_body_hot_ref(
        "/public.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "Target",
        std::sync::Arc::from([]),
        verter_type_engine::semantic_query::ProjectionReductionContext::published(
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        ),
    );
    let Some(hot) = *result.value() else {
        return Ok(None);
    };
    let result = result.map(|_| hot.node());
    drop(dispatch);
    let clone = result.clone();
    drop(result);
    let _ = clone.node(*clone.value())?.read()?;
    Ok(Some(clone))
}

fn keep_callable_param<C: ResolverCapabilities>(
    dispatch: ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
    context: verter_type_engine::semantic_query::ProjectionReductionContext,
) -> Option<verter_type_engine::semantic_query_memo::node_roots::Constructed<Option<SemanticNodeId>>>
{
    let callable =
        verter_type_engine::project_semantic_dispatch::callable_view::CallableNodeView::new(
            &dispatch, node,
        );
    let signature = callable.owning().signature(context)?;
    let first = signature.first_param();
    drop(signature);
    drop(dispatch);
    Some(first)
}

fn lower_with_an_explicit_mode<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    expr: &verter_type_expr::TypeExpr,
    mode: verter_type_engine::semantic_query::ProjectionMode,
) -> verter_type_engine::semantic_query_memo::node_roots::Constructed<Option<SemanticNodeId>> {
    dispatch
        .owning()
        .lower_type_expr_in_scope_with_mode("/public.ts", expr, mode)
}
