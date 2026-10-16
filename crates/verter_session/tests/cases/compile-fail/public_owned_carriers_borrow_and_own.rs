//! Compile-pass control: the owning and borrowed public carriers expose every
//! read a consumer needs — borrowed payloads and their nested arrays, child
//! capabilities, delivered values and diagnostics, signature candidates,
//! owning surface adapters and owning roots — without a detached copy. A
//! query error copies opaquely: it carries its operands without exposing
//! them. Origin reads, display and the canonical union builder keyed by an
//! ordinal stay callable: they answer only inside a construction lane the
//! caller holds, and a dispatcher merges a macro spread formula in its own
//! lane.

use verter_session_query::flow::policy::NullabilityPolicy;
use verter_type_engine::project_semantic_dispatch::canonical_algebra;
use verter_type_engine::project_semantic_dispatch::one_level_surface::OneLevelSurface;
use verter_type_engine::project_semantic_dispatch::walk::ShallowDiagnostic;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::ResolverCapabilities;
use verter_type_engine::semantic_query::{
    display, CacheRead, DisplayNeeds, ObjectProjectionFormula, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryValue, SurfaceEntry,
};
use verter_type_engine::semantic_query_memo::node_data_lease::{NodeDataLease, SurfaceLease};
use verter_type_engine::semantic_query_memo::node_roots::NodeRoot;
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
    dispatch.spread_formula_positive_members_for_macro(formula).len()
}

fn keep_error(error: &QueryError) -> QueryError {
    error.clone()
}

fn main() {}
