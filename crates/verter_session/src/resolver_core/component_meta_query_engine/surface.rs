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

use super::route_admission::{
    admit_expanded_surface, admit_expanded_surface_changed, AdmittedRouteProjectionNode,
};
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

/// A registry member-surface PUBLICATION node held in node domain for the
/// single registry publication sink ([`materialize_registry_publication_node`]).
///
/// DELIBERATELY DISTINCT from [`AdmittedRouteProjectionNode`]: that carrier asserts
/// its node passed a route/surface adapter's `materialized && expanded_surface`
/// acceptance gate, whereas a registry PUBLICATION node is the first-pass /
/// already-stabilised member-surface node the registry candidate path holds
/// DIRECTLY — it makes NO admission claim. The member surface can be an arbitrary
/// outcome (`Miss` / `Recursive` / `Tainted` / a degenerate reduce), so wrapping it
/// in `AdmittedRouteProjectionNode` would FORGE the post-gate invariant that carrier
/// documents. Minting this carrier instead keeps the admitted-carrier invariant
/// honest while still routing the one publication materialisation through the SAME
/// cap-gated `materialize_published_node` surface sink.
///
/// SEALED + subtree-mint-scoped exactly like [`AdmittedRouteProjectionNode`]: the
/// `node` field is module-private and `new` / `node` are
/// `pub(in …::component_meta_query_engine)`, so only the in-subtree registry
/// candidate path mints it and only the registry publication sink reads it — and the
/// materialisation it feeds stays cap-gated regardless of who holds the carrier.
#[derive(Debug, Clone, Copy)]
pub(in crate::resolver_core::component_meta_query_engine) struct RegistryPublicationNode {
    node: SemanticNodeId,
}

impl RegistryPublicationNode {
    /// Mint the carrier around a registry member-surface publication node. Makes NO
    /// admission claim (no `materialized && expanded_surface` gate) — the node is the
    /// first-pass / stabilised member surface, an arbitrary outcome the registry
    /// candidate path publishes once.
    #[must_use]
    pub(in crate::resolver_core::component_meta_query_engine) fn new(node: SemanticNodeId) -> Self {
        Self { node }
    }

    /// The publication node, for the registry publication sink.
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
///
/// Subtree-scoped (`pub(in …::component_meta_query_engine)`): every caller is a
/// route/surface adapter or the route fixpoint inside this subtree, so the
/// confinement is COMPILER-enforced — no out-of-subtree site can reach the
/// node→`TypeExpr` materialisation except through the engine's sink-local
/// publication methods.
pub(in crate::resolver_core::component_meta_query_engine) fn materialize_route_projection_node(
    ctx: &dyn ResolverContext,
    node: &AdmittedRouteProjectionNode,
) -> Option<TypeExpr> {
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    materialize_published_node(&dispatch, node.node())
}

/// Terminal sink: materialise a [`RegistryPublicationNode`] into a published
/// `TypeExpr` ONCE, at the same cap-gated `materialize_published_node` surface sink.
///
/// Distinct from [`materialize_route_projection_node`] in its INPUT carrier ONLY: the
/// registry candidate path holds a first-pass / stabilised member-surface node that
/// is NOT a route/surface adapter's admitted node, so it cannot honestly wear the
/// [`AdmittedRouteProjectionNode`] post-gate invariant. Both sinks materialise once
/// with no decision on the result; this one accepts the no-admission-claim carrier so
/// an arbitrary member-surface outcome (`Miss` / `Recursive` / `Tainted` / a
/// degenerate reduce) is published without forging the admitted-carrier contract.
pub(in crate::resolver_core::component_meta_query_engine) fn materialize_registry_publication_node(
    ctx: &dyn ResolverContext,
    node: &RegistryPublicationNode,
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
    let witness = node_raised_shape_facts_with_dispatch(&dispatch, result_node)?;
    admit_expanded_surface(&witness)
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
        .is_some_and(|shape| shape.eq_to_expr())
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

/// Demand-bound adapter for the empty-terminal Expanded publication path.
/// Lower `expr` at `Expanded`, dispatch `ProjectPath { base, [],
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
    // dispatch above); the gate (`materialized && expanded_surface && changed`,
    // where `changed = !shape.eq_to_expr` is the node-domain shape inequality
    // against `expr`) is encoded in `admit_expanded_surface_changed`.
    let shape = node_raised_shape_for_eq_with_dispatch(&dispatch, result_node, expr)?;
    admit_expanded_surface_changed(&shape)
}

/// Demand-bound NODE adapter for the Class-A path-precise projection (the
/// pure-dispatch tail of the node-domain Class-A projection). Decompose
/// the IndexedAccess chain INTERNALLY, lower the base (empty path → lower the
/// whole `expr` at `Expanded`; non-empty path → lower the chain root at
/// `Navigate`), dispatch `ProjectPath { base, path, Published(Expanded) }`,
/// gate on NODE-DOMAIN facts (`materialized && expanded_surface`), and return
/// the admitted node — NO materialisation. The lowering happens here so no raw
/// node crosses in; the `*_published` wrapper materialises the accepted node
/// ONCE at the surface sink.
pub(crate) fn project_class_a_terminal_node(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<AdmittedRouteProjectionNode> {
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
    let witness = node_raised_shape_facts_with_dispatch(&dispatch, result_node)?;
    admit_expanded_surface(&witness)
}

/// Publication wrapper over the FULL node-domain Class-A projection
/// ([`crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded`]):
/// resolve the registry route fast-path THEN the terminal node adapter in node
/// domain, then materialise the accepted route node ONCE at the surface sink.
///
/// This is the materialising counterpart of the node sibling: the node-domain
/// decision (route fast-path + terminal
/// `materialized && expanded_surface` gate) lives in the node fn; this wrapper
/// adds only the one terminal raise. It lives in the surface sink module so the
/// node→`TypeExpr` materialisation stays owner-confined. The engine is NOT
/// threaded (a transient engine is created internally), matching the engine-less
/// `project_expr_class_a_via_dispatch` callers.
pub(crate) fn project_class_a_published(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<TypeExpr> {
    let node = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
        ctx,
        None,
        scope_canonical_id,
        expr,
    )?;
    materialize_route_projection_node(ctx, &node)
}

/// Engine-THREADED variant of [`project_class_a_published`]: resolve the
/// Class-A decision through the node sibling using the CALLER's engine (so
/// engine-local re-export / prepared-decl resolution state persists across
/// callsites — the registry structural materialiser threads one engine across
/// every leaf), decide the object-surface fact off the PRODUCING node, then
/// materialise the accepted route node ONCE at the surface sink. Returns the
/// materialised `TypeExpr` paired with that node-domain object-surface fact, so
/// the registry materialiser never re-lowers the materialised value to recover
/// it. The node→`TypeExpr` materialisation stays owner-confined here.
pub(crate) fn project_class_a_published_threaded(
    engine: &mut super::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<(TypeExpr, bool)> {
    let ctx = engine.ctx;
    let node = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
        ctx,
        Some(engine),
        scope_canonical_id,
        expr,
    )?;
    let explicit_object_surface =
        super::node_materialize::component_meta_registry_node_has_explicit_object_surface(
            ctx,
            node.node(),
        );
    let type_expr = materialize_route_projection_node(ctx, &node)?;
    Some((type_expr, explicit_object_surface))
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
        // `None` here; the seam (`dispatch_projected_surface_with_node`) composes
        // the compound root via `projected_compound_root_surface_via_dispatch`
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
///
/// Returns the composed `ProjectedSurface` PAIRED with the terminal `Object`
/// NODE the walker read it from. That node IS the composed surface, so the
/// Whole-route publication gate folds its node-domain materializedness over
/// THAT node — never over the carrier-intact `node` decl anchor, whose own
/// raise keeps heritage / import carriers unresolved (materialized) and would
/// admit a partial composed surface the surface-materialization filter rejects.
pub(super) fn projected_compound_root_surface_via_dispatch(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<(ProjectedSurface, SemanticNodeId)> {
    use crate::semantic_query::{
        ProjectionMode, ProjectionReductionContext, SurfaceProvenanceContext,
    };

    let (surface, surface_node) = ctx.dispatch().resolve_typeinfo_surface_view_with_node(
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
    Some((projected, surface_node))
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
///
/// Production reads the node-domain root-sentinel fact
/// (`node_root_is_unmaterialized_sentinel_with_dispatch`); this `TypeExpr`
/// predicate survives ONLY as the `#[cfg(test)]` parity oracle the raised-shape
/// suite compares that node fact against.
#[cfg(test)]
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

/// Build an [`ExpandedObjectShape`](verter_semantic::analysis::type_expand::ExpandedObjectShape)
/// DTO directly from a one-level [`SurfaceView`], materialising ONLY the
/// terminal leaves (member value types, call/construct-signature parameters +
/// returns, index-signature key/value leaves) into the DTO — NEVER the whole
/// object `TypeExpr`, and NEVER `type_expr_to_object_shape`. The composition is
/// [`surface_view_to_projected_surface`] (the registered surface sink that mints
/// each leaf ONCE) + [`projected_surface_to_expanded_shape`] (a pure
/// `ProjectedSurface -> ExpandedObjectShape` map: no mint, no decision). The
/// produced shape carries the FULL surface (properties + call signatures + index
/// signatures); each CONSUMER applies its own has-surface gate (the direct-utility
/// `shape_has_surface` ignores index signatures, the registry general arm counts
/// them) — this builder makes no decision, it is a terminal DTO writer.
///
/// The projected shape preserves every member facet the `SurfaceView` carries —
/// per-member optionality (a member a union arm omits stays optional) and call
/// signatures (a single-call-signature surface keeps its call signature). See
/// [`project_admitted_route_node_to_expanded_object_shape`] for the full object-
/// shape projection invariant.
pub(super) fn surface_view_to_expanded_shape(
    ctx: &dyn ResolverContext,
    surface: &SurfaceView,
) -> verter_semantic::analysis::type_expand::ExpandedObjectShape {
    let projected = surface_view_to_projected_surface(ctx, surface);
    projected_surface_to_expanded_shape(&projected)
}

/// Project an admitted route / surface NODE to its
/// [`ExpandedObjectShape`](verter_semantic::analysis::type_expand::ExpandedObjectShape)
/// in NODE DOMAIN: resolve the node's one-level [`SurfaceView`] through the
/// shared empty-path Shallow surface walker
/// ([`ProjectSemanticDispatch::resolve_typeinfo_surface_view`](crate::project_semantic_dispatch::ProjectSemanticDispatch::resolve_typeinfo_surface_view)),
/// then build the DTO via [`surface_view_to_expanded_shape`] — materialising only
/// the surface's terminal leaves, never the whole object `TypeExpr`.
///
/// For a compound root (`A | B`, `A & B`, a `Pick<…>` / `Omit<…>` carrier) the
/// walker COMPOSES the one-level surface off the admitted node — which IS the
/// composed surface, NOT the carrier-intact decl anchor that would collapse a
/// generic heritage / `Omit` arm to `Opaque(Miss)` (the admitted route node is
/// projected `Navigate`-mode, carriers intact, so it re-resolves cleanly). The
/// `MacroObjectSurface` demand selects union-OF-members for a union root.
///
/// INVARIANT — object-shape projection semantics this function upholds:
/// - A member present in only SOME arms of a union root is published
///   OPTIONAL. A non-object arm (e.g. a bare primitive) contributes no
///   members, so any member that a legal arm omits is widened to optional —
///   the surface is the union OF members across every arm, and a member
///   absent from one arm cannot be guaranteed present.
/// - A single-call-signature object surface PRESERVES its call signature in
///   the projected shape: a callable member surface is published with its
///   call signature intact, never collapsed away into an empty shape.
///
/// Takes the SEALED [`AdmittedRouteProjectionNode`] carrier (never a raw forgeable
/// `SemanticNodeId`), so no node crosses the query-engine boundary. `None` when
/// the node resolves to no one-level object surface.
pub(crate) fn project_admitted_route_node_to_expanded_object_shape(
    ctx: &dyn ResolverContext,
    node: &AdmittedRouteProjectionNode,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    use crate::semantic_query::{
        ProjectionMode, ProjectionReductionContext, SurfaceProvenanceContext,
    };

    let surface = ctx.dispatch().resolve_typeinfo_surface_view(
        node.node(),
        ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        ),
    )?;
    Some(surface_view_to_expanded_shape(ctx, &surface))
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
