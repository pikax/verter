//! Surface-projection helpers, prepared-substitution machinery, and
//! arc cache-key constructors used by `ComponentMetaQueryEngine`.
//!
//! Free functions (not engine methods) that operate on
//! `TypeExpr` / `ProjectedSurface` values produced by the engine and
//! dispatch layers; no engine-state dependencies beyond a borrowed
//! `VerterHost` reference.
//!
//! Cross-callers reach the public-API symbols here via the parent
//! module's `pub(crate) use surface::{...};` re-export at the bottom of
//! `component_meta_query_engine/mod.rs`. Internal helpers stay
//! parent-private (no visibility relaxation).

use rustc_hash::FxHashSet;
use verter_semantic::analysis::type_solver::query_engine::{ProjectedMember, ProjectedSurface};
use verter_type_expr::TypeExpr;

use super::{
    BUDGET_EXCEEDED_SENTINEL_PREFIX, SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE,
    SEMANTIC_SURFACE_MEMBER,
};
use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{QueryError, SemanticNodeData, SemanticNodeId, SurfaceView};

crate::project_semantic_dispatch::output_materialization::define_output_capability! {
    /// The component-meta query-engine SURFACE projector's output-sink
    /// capability. The surface projector here holds this to materialize a
    /// graph node into a sealed output carrier and unwrap it. Its constructor
    /// is visible ONLY within
    /// `crate::resolver_core::component_meta_query_engine::surface` — NOT the
    /// whole query-engine subtree — so no query-engine sibling can mint it
    /// (planted `MetaQuerySurfaceOutputCap::new` outside this leaf is
    /// `E0624`).
    pub(crate) struct MetaQuerySurfaceOutputCap;
    mint: pub(in crate::resolver_core::component_meta_query_engine::surface)
}

// ===========================================================================
// Demand-bound publication adapters (M4 — codex-settled).
//
// The Kind-B route helpers / route fixpoint make their convergence / gating /
// equality decisions NODE-DOMAIN (raised-shape facts + interned key, no
// `TypeExpr` materialisation). The single PUBLICATION `TypeExpr` they return is
// materialised ONCE here, at this registered surface sink, through the sealed
// [`MetaQuerySurfaceOutputCap`]. The adapters take a HIGH-LEVEL demand (scope +
// `&TypeExpr` + modes); they lower internally so no raw forgeable
// `SemanticNodeId` ever crosses in from a non-sink caller, and the bare
// `TypeExpr` leaves only as the accepted publication value.
// ===========================================================================

/// Materialise an accepted graph `node` into a published `TypeExpr` at this
/// surface sink. MODULE-PRIVATE: the bare `TypeExpr` is produced here and
/// handed back to the demand-bound adapters below as the accepted publication
/// value — a raw node is never accepted from outside the adapters.
fn materialize_published_node(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<TypeExpr> {
    let cap = MetaQuerySurfaceOutputCap::new(dispatch);
    cap.materialize_output_type_expr(node)
        .map(|raised| raised.into_type_expr(&cap))
}

/// A node-domain route-projection result: the admitted `SemanticNodeId` a
/// route/surface adapter produced AFTER its node-domain acceptance gate
/// (`materialized && expanded_surface`), held in node-domain so the route
/// fixpoint stabilises on interned [`shape_engine::RaisedShapeKey`] identity
/// and materialises EXACTLY ONCE at the terminal sink.
///
/// SEALED carrier: the `node` field is module-private (only this `surface`
/// leaf reads it, to materialise / compare it at a cap-gated sink) and `new` /
/// `node` are mint-scoped to the query-engine subtree
/// (`pub(in crate::resolver_core::component_meta_query_engine)`), so the route
/// adapters in `surface` / `registry_decl` mint and read it while the
/// host-threaded wrappers (`crate::meta_resolve`) and the fixpoint driver only
/// NAME and pass it — no forgeable `SemanticNodeId → TypeExpr` adapter crosses
/// the query-engine boundary, and materialisation stays cap-gated regardless of
/// who holds the carrier. Modeled on `AdmittedExpansionNode`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AdmittedRouteProjectionNode {
    node: SemanticNodeId,
}

impl AdmittedRouteProjectionNode {
    /// Mint the carrier around an admitted route-projection node. Subtree-scoped
    /// so only the route/surface adapters (`surface` + `registry_decl`) mint it,
    /// after their own node-domain acceptance gate.
    #[must_use]
    pub(in crate::resolver_core::component_meta_query_engine) fn new(node: SemanticNodeId) -> Self {
        Self { node }
    }

    /// The admitted node. Subtree-scoped so only the sink-owned materialise /
    /// compare helpers read it; the materialisation it feeds stays cap-gated.
    #[must_use]
    pub(in crate::resolver_core::component_meta_query_engine) fn node(&self) -> SemanticNodeId {
        self.node
    }
}

/// Terminal sink: materialise an [`AdmittedRouteProjectionNode`] into a
/// published `TypeExpr` ONCE, at the existing `materialize_published_node`
/// surface sink (the sealed [`MetaQuerySurfaceOutputCap`]). The route fixpoint
/// and the surface publication wrappers call this exactly once after their
/// node-domain decisions converge — there is no mid-flight materialisation. The
/// carrier's node was admitted by a route/surface adapter's node-domain gate,
/// so this is a pure one-shot publication with no decision on the result.
pub(crate) fn materialize_route_projection_node(
    ctx: &dyn ResolverContext,
    node: &AdmittedRouteProjectionNode,
) -> Option<TypeExpr> {
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    materialize_published_node(&dispatch, node.node())
}

/// Re-project an already-admitted route node ONE more fixpoint step, in
/// node-domain: dispatch `ProjectPath { base: node, [], Published(Expanded) }`
/// off the admitted node directly (no re-lowering, no materialisation) and
/// re-apply the `materialized && expanded_surface` acceptance. Used by the route
/// fixpoint for iterations after the first, where the cursor is already a node;
/// the fixpoint's node-vs-node convergence (`route_projection_nodes_eq`) decides
/// when to stop. Empty-path `Published(Expanded)` re-projection of an admitted
/// expanded surface is idempotent, so a stable cursor re-projects to an
/// equal-shaped node and converges.
pub(crate) fn project_admitted_node_to_expanded_node(
    ctx: &dyn ResolverContext,
    prior: &AdmittedRouteProjectionNode,
) -> Option<AdmittedRouteProjectionNode> {
    use crate::project_semantic_dispatch::raise::node_raised_shape_facts_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};

    let dispatch = ProjectSemanticDispatch::new(ctx);
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: prior.node(),
        path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    });
    let result_node = match read.value {
        QueryResult::Value(node) => node,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    let facts = node_raised_shape_facts_with_dispatch(&dispatch, result_node)?;
    (facts.materialized && facts.expanded_surface)
        .then(|| AdmittedRouteProjectionNode::new(result_node))
}

/// Node-domain "no-op/changed" convergence test for the route fixpoint's FIRST
/// iteration: does `node` raise to the SAME interned shape as the input `expr`?
/// Reads `eq_to_expr` from the single key-bearing fold — no materialisation.
pub(crate) fn route_projection_node_eq_to_expr(
    ctx: &dyn ResolverContext,
    node: &AdmittedRouteProjectionNode,
    expr: &TypeExpr,
) -> bool {
    use crate::project_semantic_dispatch::raise::node_raised_shape_for_eq_with_dispatch;
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    node_raised_shape_for_eq_with_dispatch(&dispatch, node.node(), expr)
        .is_some_and(|shape| shape.eq_to_expr)
}

/// Node-domain convergence test for later route-fixpoint iterations: do `a` and
/// `b` raise to the SAME interned [`shape_engine::RaisedShapeKey`]? Compares the
/// interned raised-shape keys (carriers drop identity on raise, so the key — not
/// the node id — is the comparison subject); no materialisation. A `None` raise
/// on either side is treated as not-converged (`false`).
pub(crate) fn route_projection_nodes_eq(
    ctx: &dyn ResolverContext,
    a: &AdmittedRouteProjectionNode,
    b: &AdmittedRouteProjectionNode,
) -> bool {
    crate::project_semantic_dispatch::raise::raised_shape_eq_nodes(ctx, a.node(), b.node())
        == Some(true)
}

/// Demand-bound adapter for the empty-terminal Expanded publication path
/// (former `lower_and_project_to_expanded_via_host_threaded` materialisation
/// tail). Lower `expr` at `Expanded`, dispatch `ProjectPath { base, [],
/// Published(Expanded) }`, gate on NODE-DOMAIN facts
/// (`materialized && expanded_surface`) plus the node-domain "changed" check
/// (`!raised_shape_eq_node_type_expr(result, expr)`), and materialise the
/// accepted result node ONCE at this sink. `None` on lower-miss, dispatch
/// error/recursive, gate-reject, or raise-miss.
pub(crate) fn lower_and_project_to_expanded_node(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<AdmittedRouteProjectionNode> {
    use crate::project_semantic_dispatch::raise::node_raised_shape_for_eq_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};

    let dispatch = ProjectSemanticDispatch::new(ctx);
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Expanded,
    )?;
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base,
        path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    });
    let result_node = match read.value {
        QueryResult::Value(node) => node,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    // Facts + the no-op/changed decision come from ONE node fold (reusing the
    // dispatch above): `!contains_miss && is_expanded_surface && reduced != expr`
    // — the `reduced != *expr` no-op check is the node-domain shape inequality.
    let shape = node_raised_shape_for_eq_with_dispatch(&dispatch, result_node, expr)?;
    let changed = !shape.eq_to_expr;
    (shape.facts.materialized && shape.facts.expanded_surface && changed)
        .then(|| AdmittedRouteProjectionNode::new(result_node))
}

/// Thin publication wrapper over [`lower_and_project_to_expanded_node`]:
/// resolve the node-domain decision, then materialise the accepted node ONCE at
/// the surface sink. The node-domain gate (`materialized && expanded_surface &&
/// changed`) lives in the node fn; this wrapper adds only the terminal raise.
pub(crate) fn lower_and_project_to_expanded_published(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<TypeExpr> {
    let node = lower_and_project_to_expanded_node(ctx, scope_canonical_id, expr)?;
    materialize_route_projection_node(ctx, &node)
}

/// Demand-bound adapter for the mode-explicit dispatch-direct surface
/// projection (former `project_expr_surface_expr_via_host_threaded`
/// materialisation tail). Lower `expr` at `base_mode`, dispatch
/// `ProjectPath { base, [], { terminal_mode, demand } }`, refuse a
/// `semanticMiss`-bearing result (node-domain `!materialized`), then apply the
/// mode-aware acceptance and materialise the accepted node ONCE at this sink.
pub(crate) fn project_expr_surface_expr_node(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
    base_mode: crate::semantic_query::ProjectionMode,
    terminal_mode: crate::semantic_query::ProjectionMode,
    demand: crate::semantic_query::ReductionDemand,
) -> Option<AdmittedRouteProjectionNode> {
    use crate::project_semantic_dispatch::raise::node_raised_shape_facts_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticQueryKey,
    };

    let dispatch = ProjectSemanticDispatch::new(ctx);
    let base = dispatch.lower_type_expr_in_scope_with_context(
        scope_canonical_id,
        expr,
        ProjectionReductionContext {
            mode: base_mode,
            demand,
            provenance: crate::semantic_query::SurfaceProvenanceContext::Structural,
            merge_role: crate::semantic_query::MemberMergeRole::Authored,
        },
    )?;
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base,
        path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext {
            mode: terminal_mode,
            demand,
            provenance: crate::semantic_query::SurfaceProvenanceContext::Structural,
            merge_role: crate::semantic_query::MemberMergeRole::Authored,
        },
    });
    let result_node = match read.value {
        QueryResult::Value(node) => node,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    // Facts-only gate — reuses the dispatch above; no structural key interned.
    let facts = node_raised_shape_facts_with_dispatch(&dispatch, result_node)?;
    // Refuse `semanticMiss`-bearing results (node-domain `!materialized`).
    if !facts.materialized {
        return None;
    }
    let accept = match terminal_mode {
        // Expanded terminal — only fully materialised surfaces qualify.
        ProjectionMode::Expanded => facts.expanded_surface,
        // Shallow / Identity / Navigate / Skeleton — admit the carrier shape.
        ProjectionMode::Shallow
        | ProjectionMode::Identity
        | ProjectionMode::Navigate
        | ProjectionMode::Skeleton => true,
    };
    accept.then(|| AdmittedRouteProjectionNode::new(result_node))
}

/// Thin publication wrapper over [`project_expr_surface_expr_node`]: resolve the
/// node-domain decision (mode-aware acceptance + `!materialized` refusal), then
/// materialise the accepted node ONCE at the surface sink.
pub(crate) fn project_expr_surface_expr_published(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
    base_mode: crate::semantic_query::ProjectionMode,
    terminal_mode: crate::semantic_query::ProjectionMode,
    demand: crate::semantic_query::ReductionDemand,
) -> Option<TypeExpr> {
    let node = project_expr_surface_expr_node(
        ctx,
        scope_canonical_id,
        expr,
        base_mode,
        terminal_mode,
        demand,
    )?;
    materialize_route_projection_node(ctx, &node)
}

/// Demand-bound adapter for the Class-A path-precise projection (former
/// `project_expr_class_a_via_dispatch_threaded` pure-dispatch tail). Decompose
/// the IndexedAccess chain INTERNALLY, lower the base (empty path → lower the
/// whole `expr` at `Expanded`; non-empty path → lower the chain root at
/// `Navigate`), dispatch `ProjectPath { base, path, Published(Expanded) }`,
/// gate on `materialized && expanded_surface`, and materialise the accepted
/// node ONCE at this sink. The lowering happens here so no raw node crosses in.
pub(crate) fn project_class_a_terminal_published(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<TypeExpr> {
    use crate::project_semantic_dispatch::raise::node_raised_shape_facts_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};

    let (base_expr, path_segments) =
        crate::meta_resolve::dispatch_helpers::decompose_indexed_access_chain(expr);
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let (base, project_path) = if path_segments.is_empty() {
        let base = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            expr,
            ProjectionMode::Expanded,
        )?;
        (
            base,
            std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        )
    } else {
        let base = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            base_expr,
            ProjectionMode::Navigate,
        )?;
        (base, path_segments)
    };
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base,
        path: project_path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    });
    let result_node = match read.value {
        QueryResult::Value(node) => node,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    // Facts-only gate — reuses the dispatch above; no structural key interned.
    let facts = node_raised_shape_facts_with_dispatch(&dispatch, result_node)?;
    (facts.materialized && facts.expanded_surface)
        .then(|| materialize_published_node(&dispatch, result_node))
        .flatten()
}

/// Demand-bound adapter for local generic-`Ref` instantiation (former
/// `instantiate_local_generic_ref_via_dispatch`). Bails on a non-generic `Ref`
/// (non-`Ref` / empty type-arguments). Lowers `expr` to a node at `Expanded`,
/// then decides the no-op NODE-DOMAIN via `raised_shape_eq_node_type_expr`
/// (distinct nodes raise to equal shapes, so bare node-id identity is wrong);
/// on a real change materialises the instantiated node ONCE at this sink. `None`
/// for non-ref / empty-args / lower-miss / shape-miss / raised-shape no-op.
pub(crate) fn instantiate_local_generic_ref_published(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<TypeExpr> {
    use crate::project_semantic_dispatch::raise::node_raised_shape_for_eq_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::ProjectionMode;

    let TypeExpr::Ref { type_arguments, .. } = expr else {
        return None;
    };
    if type_arguments.is_empty() {
        return None;
    }
    let dispatch = ProjectSemanticDispatch::new(ctx);
    // Lower at Expanded so the body materialises in one step (the caller reads
    // the published body directly, no path-walking follow-up).
    let lowered = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Expanded,
    )?;
    // A no-op (the lowered body raises to the SAME shape as `expr`) surfaces as
    // `None` so the caller's own fallback path runs; a miss-shaped raise likewise
    // (`None` from the projection). Decided WITHOUT materialising — node-domain
    // shape equality, folded ONCE through the reused dispatch. The `Some(false)`
    // gate (a genuine change) is `eq_to_expr == false`.
    if node_raised_shape_for_eq_with_dispatch(&dispatch, lowered, expr).is_none_or(|s| s.eq_to_expr)
    {
        return None;
    }
    materialize_published_node(&dispatch, lowered)
}

pub(crate) fn projected_surface_from_semantic_node(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<ProjectedSurface> {
    let mut active = FxHashSet::default();
    projected_surface_from_semantic_node_inner(ctx, node, &mut active)
}

fn projected_surface_from_semantic_node_inner(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<ProjectedSurface> {
    let data = ctx.dispatch_node_data(node)?;
    match data.as_ref() {
        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return None;
            }
            let result = projected_surface_from_semantic_node_inner(ctx, *target, active);
            active.remove(&node);
            result
        }
        SemanticNodeData::Object(surface) => Some(surface_view_to_projected_surface(ctx, surface)),
        // Compound roots (`A | B`, `A & B` / heritage overlay, `Foo<Bar>`)
        // carry no single `Object` surface on the post-`Published(Expanded)`
        // instantiated node, and that node can collapse a generic heritage /
        // `Omit` carrier arm to `Opaque(Miss)`. So this projector returns
        // `None` here; the seam (`dispatch_projected_surface`) composes the
        // compound root via `projected_compound_root_surface_via_dispatch`
        // driven from the decl anchor (carrier intact).
        _ => None,
    }
}

/// Compose the shallow surface of a compound root node (`Union` /
/// `Intersection` / `InstantiationRef`) through the shared empty-path
/// Shallow surface walker: drives `ProjectPath { base: node, path: [],
/// macro_object_surface(Shallow, Structural) }` via
/// `resolve_typeinfo_surface_view`, then reconstructs the terminal
/// `SurfaceView` into a `ProjectedSurface`.
///
/// `node` is the decl-anchor base the seam supplies — NOT the
/// post-`Published(Expanded)` instantiated root, which can collapse a
/// generic heritage / `Omit` carrier arm to `Opaque(Miss)` (the shared
/// walker cannot re-resolve an already-collapsed node, whereas the decl
/// anchor still carries the carrier intact). Returns `None` when the walker
/// resolves no `Object` terminal OR the composed surface is empty (an empty
/// surface is never a COMPLETE compound-root projection).
pub(super) fn projected_compound_root_surface_via_dispatch(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<ProjectedSurface> {
    use crate::semantic_query::{
        ProjectionMode, ProjectionReductionContext, SurfaceProvenanceContext,
    };

    let surface = ctx.dispatch().resolve_typeinfo_surface_view(
        node,
        ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        ),
    )?;
    let projected = surface_view_to_projected_surface(ctx, &surface);
    if projected_surface_is_empty(&projected) {
        return None;
    }
    Some(projected)
}

pub(crate) fn surface_view_to_projected_surface(
    ctx: &dyn ResolverContext,
    surface: &SurfaceView,
) -> ProjectedSurface {
    let dispatch = ctx.dispatch();
    // Mint the surface-projector output capability (constructor visible only
    // within `crate::resolver_core::component_meta_query_engine::surface`):
    // this projector is a true publication sink, so it materializes graph
    // nodes into sealed output carriers and unwraps them via the capability.
    let cap = MetaQuerySurfaceOutputCap::new(&dispatch);
    let members = surface
        .members
        .iter()
        .map(|member| ProjectedMember {
            name: member.name.as_ref().to_string(),
            ty: cap
                .materialize_output_type_expr(member.value)
                .map(|raised| raised.into_type_expr(&cap))
                .unwrap_or(TypeExpr::Unknown {
                    raw: semantic_query_error_raw(&QueryError::UnrepresentableSurfaceMember),
                }),
            optional: member.optional,
            readonly: member.readonly,
            is_method: member.is_method,
            // Carry the graph `SurfaceMember`'s declared accessibility verbatim
            // so the SurfaceView -> ProjectedMember -> TypeExpr round-trip is
            // visibility-lossless: a non-public class member stays non-public
            // through the reconstruction (`projected_surface_to_type_expr`).
            visibility: member.visibility,
            declared_in_macro_type_arg: member.declared_in_macro_type_arg,
            // Graph `SurfaceMember` carries the real OXC declaration-site spans
            // (stamped during shallow lowering) AND the member's declaration
            // file; carry both verbatim so the reconstruction re-emits the spans
            // paired with the correct file (a cross-file surface's members keep
            // their own declaring file, not the projection scope).
            spans: member.spans,
            declaration_origin: member.declaration_origin.clone(),
        })
        .collect();
    let call_signatures = surface
        .call_signatures
        .iter()
        .filter_map(|signature| {
            cap.materialize_output_type_expr(*signature)
                .map(|raised| raised.into_type_expr(&cap))
        })
        .collect();
    let construct_signatures = surface
        .construct_signatures
        .iter()
        .filter_map(|signature| {
            cap.materialize_output_type_expr(*signature)
                .map(|raised| raised.into_type_expr(&cap))
        })
        .collect();
    // Graph `SurfaceView::index_signatures` carries the declared key/value
    // nodes + real OXC spans + the declaration file. Raise the key/value nodes
    // to `TypeExpr` and carry the spans/origin verbatim so the reconstruction
    // re-emits a real `[k: K]: V` rather than the synthetic open placeholder.
    let index_signatures = surface
        .index_signatures
        .iter()
        .map(|signature| {
            use verter_semantic::analysis::type_solver::query_engine::ProjectedIndexSignature;
            ProjectedIndexSignature {
                key_name: "key".to_string(),
                key_type: cap
                    .materialize_output_type_expr(signature.key_type)
                    .map(|raised| raised.into_type_expr(&cap))
                    .unwrap_or(TypeExpr::Unknown {
                        raw: semantic_query_error_raw(&QueryError::UnrepresentableSurfaceMember),
                    }),
                value_type: cap
                    .materialize_output_type_expr(signature.value_type)
                    .map(|raised| raised.into_type_expr(&cap))
                    .unwrap_or(TypeExpr::Unknown {
                        raw: semantic_query_error_raw(&QueryError::UnrepresentableSurfaceMember),
                    }),
                readonly: signature.readonly,
                spans: signature.spans,
                declaration_origin: signature.declaration_origin.clone(),
            }
        })
        .collect();
    ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        index_signatures,
        has_index_signature: surface.has_index_signature,
    }
}

pub(super) fn dispatch_route_expr_is_materialized(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Unknown { raw } => {
            // Every sentinel emitted by the `shape_engine::fold_node`
            // materialisation algebra (exact matches) or by
            // `semantic_query_error_raw` (prefix matches for parameterised
            // errors) must round-trip to
            // "not materialised" so the dispatch-first path falls back
            // to `owner_engine` for fuller expansion. The sentinel set
            // is owned by the shared `raise_sentinel` classifier so the
            // node-domain raised-shape projection and this `TypeExpr`
            // recogniser can never disagree on the spelling.
            !crate::project_semantic_dispatch::raise_sentinel::raw_is_unmaterialized_sentinel(raw)
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => dispatch_route_expr_is_materialized(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| dispatch_route_expr_is_materialized(&element.ty)),
        TypeExpr::Object(object) => object.properties.iter().all(|member| match member {
            verter_type_expr::ObjectMember::Property(property) => {
                dispatch_route_expr_is_materialized(&property.ty)
            }
            verter_type_expr::ObjectMember::Method(method) => {
                method
                    .function
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && method
                        .function
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_type_expr::ObjectMember::CallSignature(signature)
            | verter_type_expr::ObjectMember::ConstructSignature(signature) => {
                signature
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && signature
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_type_expr::ObjectMember::IndexSignature(signature) => {
                dispatch_route_expr_is_materialized(&signature.key_type)
                    && dispatch_route_expr_is_materialized(&signature.value_type)
            }
        }),
        // A constructor type's signature is checked identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            function
                .return_type
                .as_deref()
                .is_none_or(dispatch_route_expr_is_materialized)
                && function
                    .parameters
                    .iter()
                    .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
        }
        TypeExpr::IndexedAccess { object, index } => {
            dispatch_route_expr_is_materialized(object)
                && dispatch_route_expr_is_materialized(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            dispatch_route_expr_is_materialized(check)
                && dispatch_route_expr_is_materialized(extends)
                && dispatch_route_expr_is_materialized(true_type)
                && dispatch_route_expr_is_materialized(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            dispatch_route_expr_is_materialized(source)
                && dispatch_route_expr_is_materialized(value)
                && name_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Infer { .. }
        // Synthetic carriers are fully materialised at the projector
        // surface — they ARE the published leaf, not a deferred token.
        | TypeExpr::SyntheticSlotBinding(_)
        // An import-type is a published shallow carrier (like a bare `Ref`),
        // not an unmaterialised dispatch sentinel — count it as materialised.
        | TypeExpr::ImportType { .. }
        | TypeExpr::RecursiveRef { .. } => true,
    }
}

/// Detects sentinel tokens emitted by the `shape_engine::fold_node`
/// materialisation algebra when dispatch cannot materialise a node.
/// Dispatch-first paths fall back to `owner_engine` when the sentinel is
/// present — transitional until §5.8 retires the owner_engine bridge.
pub(crate) fn type_expr_contains_semantic_miss(expr: &TypeExpr) -> bool {
    !dispatch_route_expr_is_materialized(expr)
}

/// Root-level (carrier-position) unmaterialised-sentinel recogniser.
///
/// Returns `true` when the expression IS a raise sentinel at its root
/// (unwrapping only `Parenthesized`) — the shape produced when a
/// published carrier is re-lowered by NAME in a scope where the name
/// does not resolve, so the demanded reduction itself failed. Distinct
/// from [`type_expr_contains_semantic_miss`], which also fires on
/// genuine NESTED partial values: an unresolvable member-value
/// reference (`element?: HTMLElement` without the DOM lib) inside an
/// otherwise-materialised surface is a contract-conformant partial
/// result (Macro Type Traversal — the field that transitively depends
/// on the unresolved name publishes partially; sibling members resolve
/// normally), not a failed reduction.
pub(crate) fn type_expr_root_is_unmaterialized_sentinel(expr: &TypeExpr) -> bool {
    let mut current = expr;
    while let TypeExpr::Parenthesized(inner) = current {
        current = inner;
    }
    match current {
        TypeExpr::Unknown { .. } => !dispatch_route_expr_is_materialized(current),
        _ => false,
    }
}

/// Returns `true` when `expr` is the budget-exceeded sentinel
/// (`TypeExpr::Unknown { raw }` whose `raw` starts with
/// [`BUDGET_EXCEEDED_SENTINEL_PREFIX`]). This is the single shared
/// recognizer for the spelling `semantic_query_error_raw` emits for
/// `QueryError::BudgetExceeded` — production routing and every test that
/// scans a published surface for a leaked budget sentinel call this so
/// the spelling can never drift between producer and detector.
pub(crate) fn type_expr_is_budget_exceeded_sentinel(expr: &TypeExpr) -> bool {
    matches!(expr, TypeExpr::Unknown { raw } if raw.starts_with(BUDGET_EXCEEDED_SENTINEL_PREFIX))
}

/// Returns `true` when `expr` still carries open deferred shell shapes
/// (`KeyOf`, `IndexedAccess`, `Mapped`, `TypeOf`, `Conditional`) that
/// indicate dispatch could not structurally expand the surface further.
//
// The node-domain `expanded_surface` fact (computed bottom-up in `shape_engine`)
// now drives every production gate; this `TypeExpr` predicate survives ONLY as
// the `#[cfg(test)]` parity ORACLE the raised-shape suite compares the bottom-up
// fact against, so the non-test build sees it as dead.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "TypeExpr parity oracle for the bottom-up expanded_surface fact; \
                  production gates read the node-domain fact via shape_engine"
    )
)]
pub(crate) fn type_expr_is_expanded_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::KeyOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. } => false,
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(type_expr_is_expanded_surface)
        }
        _ => true,
    }
}

pub(crate) fn semantic_query_error_raw(err: &QueryError) -> String {
    match err {
        QueryError::Miss => SEMANTIC_MISS.to_string(),
        QueryError::Other(text) => text.as_ref().to_string(),
        QueryError::UnsupportedIntrinsic { name } => format!("unsupportedIntrinsic({name})"),
        QueryError::BudgetExceeded(failure) => format!("budgetExceeded({:?})", failure.domain),
        QueryError::UnstableState { attempts } => format!("unstableState({attempts})"),
        QueryError::AliasCycle { chain } => format!("aliasCycle({})", chain.len()),
        QueryError::RecursiveRef { name } => format!("recursiveRef({name})"),
        QueryError::DeclPlaceholder { name, .. } => format!("declPlaceholder({name})"),
        QueryError::ValueDomainMismatch { expected, actual } => {
            format!("valueDomainMismatch(expected={expected:?},actual={actual:?})")
        }
        // Typed semantic-sentinel carriers → BYTE-IDENTICAL legacy raw
        // strings. A future stage that swaps a raw `Unknown { raw: "X" }`
        // construction for `Opaque(QueryError::…)` must raise to the same
        // text, so these mappings are pinned by
        // `typed_query_error_sentinels_round_trip_to_legacy_raw`.
        QueryError::RaiseAliasCycle => "semanticAliasCycle".to_string(),
        QueryError::TypeParamCycle => "semanticTypeParamCycle".to_string(),
        QueryError::RaiseMiss => "<raise miss>".to_string(),
        QueryError::UnrepresentableSurface => SEMANTIC_OBJECT_SURFACE.to_string(),
        QueryError::UnrepresentableSurfaceMember => SEMANTIC_SURFACE_MEMBER.to_string(),
        QueryError::VueMacroElementsPlaceholder => "VueMacroElements".to_string(),
    }
}

pub(super) fn projected_surface_is_empty(surface: &ProjectedSurface) -> bool {
    surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
}

pub(crate) fn projected_surface_to_type_expr(surface: &ProjectedSurface) -> Option<TypeExpr> {
    use std::sync::Arc;
    use verter_type_expr::{
        FunctionExpr, IndexSignature, MethodSignature, ObjectExpr, ObjectMember, ObjectProperty,
        PrimitiveName,
    };

    if surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
    {
        return None;
    }

    if surface.members.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
        && surface.call_signatures.len() == 1
    {
        return surface.call_signatures.first().cloned();
    }

    // `ProjectedMember` carries the real OXC declaration-site spans
    // (`member.spans`), threaded from the graph `SurfaceMember` / `PreparedMember`
    // / IR source the surface was projected from. Re-emit them verbatim onto the
    // reconstructed IR member so the projection path is span-lossless end-to-end.
    let mut properties = surface
        .members
        .iter()
        .map(|member| {
            // Reconstruct via `with_visibility` (NOT `with_spans`, which defaults
            // Public) so a non-public class member projected onto the surface
            // survives the reconstruction with its true accessibility — both a
            // leak-prevention and a `native_props` fidelity requirement.
            if member.is_method {
                if let TypeExpr::Function(function) = &member.ty {
                    return ObjectMember::Method(MethodSignature::with_visibility(
                        member.name.clone(),
                        (**function).clone(),
                        member.optional,
                        member.visibility,
                        member.spans,
                    ));
                }
            }

            ObjectMember::Property(ObjectProperty::with_visibility(
                member.name.clone(),
                member.ty.clone(),
                member.optional,
                member.readonly,
                member.visibility,
                member.spans,
            ))
        })
        .collect::<Vec<_>>();

    for signature in &surface.call_signatures {
        if let TypeExpr::Function(function) = signature {
            // Preserve the call-signature function shape's OXC spans verbatim.
            properties.push(ObjectMember::CallSignature(FunctionExpr::with_spans(
                function.parameters.clone(),
                function.return_type.clone(),
                function.type_parameters.clone(),
                function.spans,
            )));
        }
    }

    for signature in &surface.construct_signatures {
        if let TypeExpr::Function(function) = signature {
            properties.push(ObjectMember::ConstructSignature(FunctionExpr::with_spans(
                function.parameters.clone(),
                function.return_type.clone(),
                function.type_parameters.clone(),
                function.spans,
            )));
        }
    }

    // A REAL `[k: K]: V` index signature (sourced from an OXC declaration site,
    // carried structurally on `ProjectedSurface::index_signatures`) re-emits its
    // declared key/value shape AND its real spans — losslessly. Reverting this
    // to the synthetic-`None` placeholder (the pre-fix state) drops both the
    // shape and the spans.
    for signature in &surface.index_signatures {
        properties.push(ObjectMember::IndexSignature(IndexSignature::with_spans(
            signature.key_name.clone(),
            signature.key_type.clone(),
            signature.value_type.clone(),
            signature.readonly,
            signature.spans,
        )));
    }

    // Emit the synthetic open-surface placeholder ONLY when the surface is
    // GENUINELY OPEN — `has_index_signature` is set but no concrete signature
    // payload was carried (e.g. a mapped/inferred open surface). This placeholder
    // has no single OXC declaration site, so its spans stay `None` by design
    // (not a deferral): there is no source range to anchor to.
    if surface.has_index_signature && surface.index_signatures.is_empty() {
        properties.push(ObjectMember::IndexSignature(IndexSignature::synthetic(
            "key".to_string(),
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Unknown {
                raw: "projectedOpenSurface".to_string(),
            },
            false,
        )));
    }

    Some(TypeExpr::Object(Arc::new(ObjectExpr { properties })))
}

pub(crate) fn projected_surface_to_expanded_shape(
    surface: &ProjectedSurface,
) -> verter_semantic::analysis::type_expand::ExpandedObjectShape {
    use verter_semantic::analysis::type_expand::{
        ExpandedCallSignature, ExpandedIndexSignature, ExpandedObjectShape, ExpandedParameter,
        ExpandedProperty,
    };
    use verter_type_expr::PrimitiveName;

    let properties = surface
        .members
        .iter()
        .map(|member| ExpandedProperty {
            name: member.name.clone(),
            ty: member.ty.clone(),
            optional: member.optional,
            readonly: member.readonly,
            // Carry the projected member's declared accessibility verbatim so a
            // downstream key-filtering derivation (`Pick`/`Omit` over the
            // shape) can re-apply the public-keyspace gate.
            visibility: member.visibility,
            declared_in_macro_type_arg: member.declared_in_macro_type_arg,
        })
        .collect::<Vec<_>>();

    let mut call_signatures = surface
        .call_signatures
        .iter()
        .chain(surface.construct_signatures.iter())
        .filter_map(|signature| match signature {
            TypeExpr::Function(function) => Some(ExpandedCallSignature {
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| ExpandedParameter {
                        name: parameter.name.clone().unwrap_or_default(),
                        ty: parameter.ty.clone(),
                        optional: parameter.optional,
                        rest: parameter.rest,
                    })
                    .collect(),
                return_type: function
                    .return_type
                    .as_ref()
                    .map(|return_type| return_type.as_ref().clone())
                    .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
                type_parameters: function.type_parameters.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut index_signatures = Vec::new();
    // Concrete declared index signatures preserve their real key/value shape
    // (the expand layer does not track spans).
    for signature in &surface.index_signatures {
        index_signatures.push(ExpandedIndexSignature {
            key_type: signature.key_type.clone(),
            value_type: signature.value_type.clone(),
            readonly: signature.readonly,
        });
    }
    // Genuinely-open surface (flag set, no concrete payload) → open placeholder.
    if surface.has_index_signature && surface.index_signatures.is_empty() {
        index_signatures.push(ExpandedIndexSignature {
            key_type: TypeExpr::Primitive(PrimitiveName::String),
            value_type: TypeExpr::Unknown {
                raw: "projectedOpenSurface".to_string(),
            },
            readonly: false,
        });
    }

    // Preserve previous round-trip behavior: call and construct signatures
    // both become call signatures after object-shape extraction.
    if !surface.call_signatures.is_empty() && !surface.construct_signatures.is_empty() {
        call_signatures.shrink_to_fit();
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}

pub(super) fn type_expr_references_names(
    expr: &TypeExpr,
    contains_name: &impl Fn(&str) -> bool,
) -> bool {
    fn visit(expr: &TypeExpr, contains_name: &impl Fn(&str) -> bool) -> bool {
        match expr {
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::TypeOf(_)
            // Synthetic carriers reference no substitutable names —
            // their identity is closed and intrinsic to the carrier
            // tuple.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Infer { .. } => false,
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                contains_name(name.as_ref())
                    || type_arguments.iter().any(|arg| visit(arg, contains_name))
            }
            // Mirrors the `Ref` arm's recursion into `type_arguments`. The
            // `specifier`/`qualifier` are a module path, not substitutable
            // names, so only the nested type-argument exprs are visited.
            TypeExpr::ImportType { type_arguments, .. } => {
                type_arguments.iter().any(|arg| visit(arg, contains_name))
            }
            TypeExpr::TypeParameter(param) => {
                contains_name(param.name.as_str())
                    || param
                        .constraint
                        .as_deref()
                        .is_some_and(|constraint| visit(constraint, contains_name))
                    || param
                        .default
                        .as_deref()
                        .is_some_and(|default| visit(default, contains_name))
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => visit(inner, contains_name),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| visit(&element.ty, contains_name)),
            TypeExpr::Union(types)
            | TypeExpr::Intersection(types)
            | TypeExpr::TemplateLiteral {
                expressions: types, ..
            } => types.iter().any(|ty| visit(ty, contains_name)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                verter_type_expr::ObjectMember::Property(property) => {
                    visit(&property.ty, contains_name)
                }
                verter_type_expr::ObjectMember::IndexSignature(signature) => {
                    visit(&signature.key_type, contains_name)
                        || visit(&signature.value_type, contains_name)
                }
                verter_type_expr::ObjectMember::CallSignature(function)
                | verter_type_expr::ObjectMember::ConstructSignature(function) => {
                    function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, contains_name))
                        || function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, contains_name))
                }
                verter_type_expr::ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, contains_name))
                        || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, contains_name))
                }
            }),
            // A constructor type's signature is searched identically to a
            // function type's (same `FunctionExpr` payload).
            TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| visit(&parameter.ty, contains_name))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| visit(return_type, contains_name))
                    || function.type_parameters.iter().any(|parameter| {
                        parameter
                            .constraint
                            .as_deref()
                            .is_some_and(|constraint| visit(constraint, contains_name))
                            || parameter
                                .default
                                .as_deref()
                                .is_some_and(|default| visit(default, contains_name))
                    })
            }
            TypeExpr::IndexedAccess { object, index } => {
                visit(object, contains_name) || visit(index, contains_name)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                visit(check, contains_name)
                    || visit(extends, contains_name)
                    || visit(true_type, contains_name)
                    || visit(false_type, contains_name)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                visit(source, contains_name)
                    || visit(value, contains_name)
                    || name_type
                        .as_deref()
                        .is_some_and(|name_type| visit(name_type, contains_name))
            }
        }
    }

    visit(expr, contains_name)
}
