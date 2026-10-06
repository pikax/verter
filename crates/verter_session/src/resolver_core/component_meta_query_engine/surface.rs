//! Surface-projection helpers, prepared-substitution machinery, and
//! arc cache-key constructors used by `ComponentMetaQueryEngine`.
//!
//! Free functions (not engine methods) that operate on
//! `TypeExpr` / [`SurfaceView`] values produced by the engine and
//! dispatch layers; no engine-state dependencies beyond a borrowed
//! `VerterHost` reference.
//!
//! Cross-callers reach the public-API symbols here via the parent
//! module's `pub(crate) use surface::{...};` re-export at the bottom of
//! `component_meta_query_engine/mod.rs`. Internal helpers stay
//! parent-private (no visibility relaxation).

use rustc_hash::FxHashSet;
use verter_type_expr::TypeExpr;

use super::route_admission::{
    admit_expanded_surface, admit_expanded_surface_changed, AdmittedRouteProjectionNode,
};
use crate::output_sinks::OutputProjector;
use verter_type_engine::resolver_core::ResolverContext;
use verter_type_engine::semantic_query::{SemanticNodeData, SemanticNodeId, SurfaceView};

crate::output_sinks::define_output_capability! {
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
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    node: SemanticNodeId,
) -> Option<TypeExpr> {
    let cap = MetaQuerySurfaceOutputCap::new(dispatch);
    cap.materialize_output_type_expr(node)
        .map(|raised| raised.into_type_expr(cap.authority()))
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
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    node: &AdmittedRouteProjectionNode,
) -> Option<TypeExpr> {
    materialize_published_node(dispatch, node.node())
}

/// Demand-bound adapter for the empty-terminal Expanded publication path.
/// Lower `expr` at `Expanded`, dispatch `ProjectPath { base, [],
/// Published(Expanded) }`, gate on NODE-DOMAIN facts
/// (`materialized && expanded_surface`) plus the node-domain "changed" check
/// (`!raised_shape_eq_node_type_expr_with_dispatch(result, expr)`), and
/// materialise the accepted result node ONCE at this sink. `None` on
/// lower-miss, dispatch error/recursive, gate-reject, or raise-miss.
pub(crate) fn lower_and_project_to_expanded_node(
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    scope_canonical_id: &str,
    scope_owner: verter_type_expr::TopLevelOwnerId,
    expr: &TypeExpr,
) -> Option<AdmittedRouteProjectionNode> {
    use verter_type_engine::project_semantic_dispatch::raise::node_raised_shape_for_eq_with_dispatch;

    use verter_type_engine::semantic_query::{
        PathSegment, ProjectionMode, QueryResult, SemanticQueryKey,
    };

    let base = dispatch.lower_type_expr_in_owner_scope_with_mode(
        scope_canonical_id,
        scope_owner,
        expr,
        ProjectionMode::Expanded,
    )?;
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base,
        path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: verter_type_engine::semantic_query::ProjectionReductionContext::published(
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
    let shape = node_raised_shape_for_eq_with_dispatch(dispatch, result_node, expr)?;
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
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    scope_canonical_id: &str,
    scope_owner: verter_type_expr::TopLevelOwnerId,
    expr: &TypeExpr,
) -> Option<AdmittedRouteProjectionNode> {
    use verter_type_engine::project_semantic_dispatch::raise::node_raised_shape_facts_with_dispatch;

    use verter_type_engine::semantic_query::{
        PathSegment, ProjectionMode, QueryResult, SemanticQueryKey,
    };

    let (base_expr, path_segments) =
        crate::meta_resolve::dispatch_helpers::decompose_indexed_access_chain(expr);
    let (base, project_path) = if path_segments.is_empty() {
        let base = dispatch.lower_type_expr_in_owner_scope_with_mode(
            scope_canonical_id,
            scope_owner,
            expr,
            ProjectionMode::Expanded,
        )?;
        (
            base,
            std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        )
    } else {
        let base = dispatch.lower_type_expr_in_owner_scope_with_mode(
            scope_canonical_id,
            scope_owner,
            base_expr,
            ProjectionMode::Navigate,
        )?;
        (base, path_segments)
    };
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base,
        path: project_path,
        context: verter_type_engine::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    });
    let result_node = match read.value {
        QueryResult::Value(node) => node,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    // A `BareRef` carrier SURVIVING the `Published(Expanded)` demand is a
    // genuine unresolved route/declaration — this sink IS the resolving
    // demand point, so the class-A projection FAILS here exactly as the
    // pre-carrier `Opaque(Miss)` terminal did. (`DeclRef` /
    // `InstantiationRef` identity carriers are NOT in this class — they
    // name a resolved declaration.)
    if verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), result_node)
        .as_deref()
        .is_some_and(|data| data.bare_ref_head().is_some())
    {
        return None;
    }
    // Facts-only gate — reuses the dispatch above; no structural key interned.
    let witness = node_raised_shape_facts_with_dispatch(dispatch, result_node)?;
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
    ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities>,
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<TypeExpr> {
    let node = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
        ctx,
        dispatch,
        None,
        scope_canonical_id,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        expr,
    )?;
    materialize_route_projection_node(dispatch, &node)
}

/// Resolve a root node to its one-level `Object` [`SurfaceView`], following
/// `Alias` identity hops (cycle-guarded).
///
/// Compound roots (`A | B`, `A & B` / heritage overlay, `Foo<Bar>`) carry no
/// single `Object` surface on the post-`Published(Expanded)` instantiated
/// node, and that node can collapse a generic heritage / `Omit` carrier arm
/// to `Opaque(Miss)`. So this projector returns `None` for them; the seam
/// (`dispatch_projected_surface_with_node`) composes the compound root via
/// [`compound_root_surface_view_via_dispatch`] driven from the decl anchor
/// (carrier intact).
pub(super) fn surface_view_from_semantic_node(
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    node: SemanticNodeId,
) -> Option<SurfaceView> {
    let mut active = FxHashSet::default();
    surface_view_from_semantic_node_inner(dispatch, node, &mut active)
}

fn surface_view_from_semantic_node_inner(
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<SurfaceView> {
    let data =
        verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), node)?;
    match data.as_ref() {
        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return None;
            }
            let result = surface_view_from_semantic_node_inner(dispatch, *target, active);
            active.remove(&node);
            result
        }
        SemanticNodeData::Object(surface) => Some(surface.clone()),
        _ => None,
    }
}

/// Compose the shallow surface of a compound root node (`Union` /
/// `Intersection` / `InstantiationRef`) through the shared empty-path
/// Shallow surface walker: drives `ProjectPath { base: node, path: [],
/// macro_object_surface(Shallow, Structural) }` via
/// `resolve_typeinfo_surface_view_with_node` and returns the terminal
/// [`SurfaceView`] directly — no materialisation.
///
/// `node` is the decl-anchor base the seam supplies — NOT the
/// post-`Published(Expanded)` instantiated root, which can collapse a
/// generic heritage / `Omit` carrier arm to `Opaque(Miss)` (the shared
/// walker cannot re-resolve an already-collapsed node, whereas the decl
/// anchor still carries the carrier intact). Returns `None` when the walker
/// resolves no `Object` terminal OR the composed surface is empty (an empty
/// surface is never a COMPLETE compound-root projection).
///
/// Returns the composed [`SurfaceView`] PAIRED with the terminal `Object`
/// NODE the walker read it from. That node IS the composed surface, so the
/// Whole-route publication gate folds its node-domain materializedness over
/// THAT node — never over the carrier-intact `node` decl anchor, whose own
/// raise keeps heritage / import carriers unresolved (materialized) and would
/// admit a partial composed surface the surface-materialization filter rejects.
pub(super) fn compound_root_surface_view_via_dispatch(
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    node: SemanticNodeId,
) -> Option<(SurfaceView, SemanticNodeId)> {
    use verter_type_engine::semantic_query::{
        ProjectionMode, ProjectionReductionContext, SurfaceProvenanceContext,
    };

    let (surface, surface_node) = dispatch.resolve_typeinfo_surface_view_with_node(
        node,
        ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        ),
    )?;
    if surface_view_is_empty(&surface) {
        return None;
    }
    Some((surface, surface_node))
}

/// A surface with no members, no call/construct signatures, and no index
/// signature carries nothing to publish (never a COMPLETE compound-root
/// projection). Node-domain — no materialisation feeds this decision.
pub(super) fn surface_view_is_empty(surface: &SurfaceView) -> bool {
    surface.closed().is_empty()
}
