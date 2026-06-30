//! Project-scoped HTML-intrinsic surface rail (engine side).
//!
//! The host `host_manage::intrinsic_projection` callers resolve the
//! `JSX.IntrinsicElements` / `HTMLAttributes` root shape, per-tag member
//! surface, and per-member stabilisation through these engine demand methods.
//! Every node-domain decision (route projection, surface composition, the
//! member fixpoint) stays inside the query-engine sink; only finished
//! `ExpandedObjectShape` DTOs and already-materialised member `TypeExpr`s cross
//! back to the host. The raw `SemanticNodeId` projection helpers stay
//! subtree-confined exactly as the registry / route-fixpoint siblings keep them.

use verter_semantic::analysis::type_expand::ExpandedObjectShape;
use verter_type_expr::TypeExpr;

use super::route_admission::AdmittedRouteProjectionNode;
use super::surface::{
    materialize_route_projection_node, project_admitted_node_to_expanded_node,
    project_admitted_route_node_to_expanded_object_shape, route_projection_node_eq_to_expr,
    route_projection_nodes_eq, surface_view_to_expanded_shape,
};
use super::ComponentMetaQueryEngine;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{ProjectionMode, ProjectionReductionContext};

impl ComponentMetaQueryEngine<'_> {
    /// Project a root symbol's whole-surface to its [`ExpandedObjectShape`] in
    /// NODE DOMAIN.
    ///
    /// PRIMARY/FALLBACK order:
    /// - PRIMARY (root-symbol whole-surface): resolve the root NODE through the
    ///   shared dispatch surface projector, then read its one-level
    ///   `Published(Shallow)` `SurfaceView` and build the shape — the same
    ///   one-level surface composition the registry whole-surface candidate
    ///   produces. Budget-guarded so an exhausted projection budget yields no
    ///   primary surface and the Class-A fallback is tried.
    /// - FALLBACK (Class-A): re-export / namespace-qualified globals
    ///   (e.g. `JSX.IntrinsicElements`) the root-symbol path declines resolve
    ///   through the node-domain Class-A projector and its admitted-node →
    ///   object-shape rail.
    ///
    /// Member values are NOT materialised here — the host materialises them
    /// shallow-by-default through [`Self::stabilize_intrinsic_member_surface`].
    pub(crate) fn project_intrinsic_root_shape(
        &mut self,
        scope_canonical_id: &str,
        type_name: &str,
    ) -> Option<ExpandedObjectShape> {
        if let Some(shape) =
            self.project_intrinsic_root_shape_primary(scope_canonical_id, type_name)
        {
            return Some(shape);
        }
        // FALLBACK (Class-A): project the bare-named root in node domain, then
        // build the object shape from the ADMITTED route node.
        let ctx = self.ctx;
        let named = TypeExpr::named(type_name);
        let node = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
            ctx,
            Some(self),
            scope_canonical_id,
            &named,
        )?;
        project_admitted_route_node_to_expanded_object_shape(ctx, &node)
    }

    /// Project an intrinsic TAG's value `TypeExpr` (the `JSX.IntrinsicElements`
    /// member value, e.g. `HTMLAttributes & { … }`) to its [`ExpandedObjectShape`]
    /// in NODE DOMAIN, in the supplied (`NativeElements`) scope. PRIMARY: the
    /// shared node-domain Class-A projector's admitted route node → object shape.
    /// FALLBACK (route admission declines for a partially-resolvable value): the
    /// value's Shallow `SurfaceView` via the shared empty-path walker, recovering
    /// the resolvable one-level surface (the resolvable intersection arms survive;
    /// unresolved arms drop) — no whole-object materialise. The node-domain
    /// surface synthesiser merges an anonymous property-type intersection
    /// role-awarely (Authored arms value-INTERSECT — `number & string` — never
    /// last-arm-override), the TS-correct merge for `A & B`.
    pub(crate) fn project_intrinsic_tag_member_shape(
        &mut self,
        scope_canonical_id: &str,
        tag_type: &TypeExpr,
    ) -> Option<ExpandedObjectShape> {
        let ctx = self.ctx;
        // PRIMARY — the admitted route node carries the fully-resolved tag value
        // surface (the resolvable `HTMLAttributes & { … }` case value-intersects
        // conflicting members here).
        if let Some(node) = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
            ctx,
            Some(self),
            scope_canonical_id,
            tag_type,
        ) {
            if let Some(shape) = project_admitted_route_node_to_expanded_object_shape(ctx, &node) {
                return Some(shape);
            }
        }
        // FALLBACK — a value whose route admission declines (e.g. a partially
        // resolvable `MissingBase & { projectOnly?: string }`) still carries a
        // recoverable one-level surface from its RESOLVABLE arms. An intersection
        // arm that lowered to an unresolved `Unknown` sentinel contributes no
        // members (`unknown & T = T`) and otherwise poisons the whole-surface
        // projection to a non-object terminal, so the resolvable remainder is
        // taken first. Lower it to a base node and read its Shallow `SurfaceView`
        // through the shared empty-path walker — the same node-domain sink the
        // root-shape primary arm reads. `None` when nothing resolvable remains or
        // the recovered surface is empty.
        let resolvable = resolvable_intersection_remainder(tag_type)?;
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let base = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            &resolvable,
            ProjectionMode::Shallow,
        )?;
        let view = dispatch.resolve_typeinfo_surface_view(
            base,
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )?;
        let shape = surface_view_to_expanded_shape(ctx, &view);
        (!shape.properties.is_empty()
            || !shape.call_signatures.is_empty()
            || !shape.index_signatures.is_empty())
        .then_some(shape)
    }

    /// PRIMARY arm of [`Self::project_intrinsic_root_shape`]: the root-symbol
    /// whole-surface path. `None` (deferring to the Class-A fallback) when the
    /// projection budget is exhausted, the root symbol does not resolve, or the
    /// resolved node carries no one-level object surface.
    fn project_intrinsic_root_shape_primary(
        &mut self,
        scope_canonical_id: &str,
        type_name: &str,
    ) -> Option<ExpandedObjectShape> {
        // An exhausted projection budget yields no primary surface (the Class-A
        // fallback is tried instead).
        if self.projection_op_budget_exhausted() {
            return None;
        }
        let (_surface, node) =
            self.dispatch_projected_surface_with_node(scope_canonical_id, type_name)?;
        let ctx = self.ctx;
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let view = dispatch.resolve_typeinfo_surface_view(
            node,
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )?;
        Some(surface_view_to_expanded_shape(ctx, &view))
    }

    /// Stabilise a nested intrinsic member value to its converged surface and
    /// materialise it ONCE at the surface sink — a node-domain fixpoint with no
    /// per-iteration `TypeExpr` materialisation.
    ///
    /// Projects the member value to its converged route NODE through the
    /// route-fixpoint convergence helpers (no per-iteration materialisation), then
    /// materialises the converged node exactly once. `None` when the value does
    /// not project to a route node (the host keeps the input shallow).
    pub(crate) fn stabilize_intrinsic_member_surface(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let node =
            self.solve_or_project_intrinsic_member_node_until_stable(scope_canonical_id, expr)?;
        materialize_route_projection_node(self.ctx, &node)
    }

    /// Node-domain fixpoint driver for [`Self::stabilize_intrinsic_member_surface`]:
    /// repeatedly project the member value's route NODE until it stops advancing
    /// (or the iteration bound is reached), comparing successive results by
    /// interned raised-shape identity — never materialising a `TypeExpr`
    /// mid-flight. The converged node is returned for a single publication
    /// materialisation at the caller.
    fn solve_or_project_intrinsic_member_node_until_stable(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<AdmittedRouteProjectionNode> {
        let ctx = self.ctx;
        let mut prior: Option<AdmittedRouteProjectionNode> = None;
        for _ in 0..3 {
            // First iteration projects the input through the node-domain Class-A
            // sibling; later iterations re-project the already-admitted prior node
            // directly (no re-lowering, no materialisation).
            let produced = match prior {
                None => crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
                    ctx,
                    Some(self),
                    scope_canonical_id,
                    expr,
                ),
                Some(prior_node) => project_admitted_node_to_expanded_node(ctx, &prior_node),
            };
            let Some(produced) = produced else {
                return prior;
            };
            // Convergence: iteration 1 compares the produced node against the
            // input `expr`'s interned shape; later iterations compare against the
            // prior produced node's interned shape.
            let converged = match prior {
                None => route_projection_node_eq_to_expr(ctx, &produced, expr),
                Some(prior_node) => route_projection_nodes_eq(ctx, &produced, &prior_node),
            };
            if converged {
                return Some(produced);
            }
            prior = Some(produced);
        }
        prior
    }
}

/// The RESOLVABLE remainder of an intrinsic tag value for the partial-surface
/// fallback of [`ComponentMetaQueryEngine::project_intrinsic_tag_member_shape`].
///
/// An intersection arm that lowered to an unresolved `Unknown` sentinel
/// contributes no members (`unknown & T = T`) and poisons the whole-surface
/// projection to a non-object terminal, so it is dropped. A non-intersection
/// value (or an intersection carrying no `Unknown` arms) is returned unchanged.
/// `None` when every arm is an unresolved sentinel — nothing resolvable remains
/// to recover. Typed-IR structural only (variant match, no text inspection).
fn resolvable_intersection_remainder(tag_type: &TypeExpr) -> Option<TypeExpr> {
    let TypeExpr::Intersection(arms) = tag_type else {
        return Some(tag_type.clone());
    };
    let resolvable: Vec<TypeExpr> = arms
        .iter()
        .filter(|arm| !matches!(arm, TypeExpr::Unknown { .. }))
        .cloned()
        .collect();
    if resolvable.is_empty() {
        return None;
    }
    // `TypeExpr::intersection` unwraps a single survivor and rebuilds the
    // intersection for multiple — the non-empty case is guarded above so the
    // empty→`unknown` arm of the constructor is never reached.
    Some(TypeExpr::intersection(resolvable))
}
