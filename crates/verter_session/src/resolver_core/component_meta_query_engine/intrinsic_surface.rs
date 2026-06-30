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
use super::{SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE};
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
    /// the resolvable one-level surface. Resolvable intersection arms survive;
    /// only explicitly-vacuous degradation sentinels (the confident-empty miss /
    /// empty object-surface class) drop, and any OTHER unrepresentable `Unknown`
    /// arm REFUSES the recovery (the tag stays shallow) so a too-narrow remainder
    /// is never published as complete — see [`resolvable_intersection_remainder`].
    /// No whole-object materialise. The node-domain surface synthesiser merges an
    /// anonymous property-type intersection role-awarely (Authored arms
    /// value-INTERSECT — `number & string` — never last-arm-override), the
    /// TS-correct merge for `A & B`.
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
        // recoverable one-level surface from its RESOLVABLE arms. Only an
        // explicitly-vacuous degradation sentinel arm (the confident-empty miss /
        // empty object-surface class) contributes no members and is dropped; any
        // OTHER unrepresentable `Unknown` arm is incomplete and REFUSES the
        // recovery (`resolvable_intersection_remainder` returns `None`) so the
        // remainder is never published as a too-narrow complete surface. The
        // resolvable remainder is lowered to a base node and its Shallow
        // `SurfaceView` read through the shared empty-path walker — the same
        // node-domain sink the root-shape primary arm reads. `None` when nothing
        // resolvable remains or the recovered surface is empty.
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
/// `TypeExpr::Unknown { raw }` is the "lowering could not represent this type"
/// degradation SENTINEL — NOT the type-theoretic TS `unknown` (which is
/// `Primitive(PrimitiveName::Unknown)`, a real type that is always preserved).
/// The `raw` spelling is the only discriminator, so the drop/refuse decision is
/// taken HERE on the typed-IR arm rather than deferred to lowering:
///
/// - An explicitly-VACUOUS degradation sentinel contributes no members and is
///   dropped — the confident-empty / semantic-miss class ([`SEMANTIC_MISS`],
///   `<empty> & T = T`) and the [`SEMANTIC_OBJECT_SURFACE`] sentinel the owner
///   intersection reducer itself drops as a vacuous empty-surface arm
///   (`shape_engine::fold_node`'s Intersection `!is_object_surface_sentinel`
///   drop; emitted only for a fully-folded object surface with zero
///   representable members).
/// - Any OTHER unrepresentable `Unknown { raw }` arm — a budget-exceeded
///   carrier, a `QueryError::Other` import-type / opaque-error text, … — is an
///   INCOMPLETE arm that may carry unmaterialised members. Publishing the
///   resolvable remainder as the COMPLETE tag surface would be a too-narrow,
///   wrong-confident result, and the tag-member cache warm-caches it as complete
///   with no record of the dropped-arm incompleteness, so recovery is REFUSED
///   (`None`). Keeping such an arm and re-lowering is NOT a recovery: lowering
///   maps every `TypeExpr::Unknown` to `Opaque(Miss)` and the node intersection
///   reducer drops opaque arms, laundering it into a false miss that recovers
///   just as wrongly.
///
/// A non-intersection value (or an intersection carrying only resolvable /
/// vacuous-droppable arms) is returned unchanged. `None` when nothing resolvable
/// remains. Typed-IR structural only (variant + named-sentinel-constant match,
/// no text inspection).
fn resolvable_intersection_remainder(tag_type: &TypeExpr) -> Option<TypeExpr> {
    let TypeExpr::Intersection(arms) = tag_type else {
        return Some(tag_type.clone());
    };
    let mut resolvable: Vec<TypeExpr> = Vec::with_capacity(arms.len());
    for arm in arms.iter() {
        if let TypeExpr::Unknown { raw } = arm {
            // Drop ONLY the explicitly-vacuous degradation sentinels (the
            // confident-empty miss class + the empty object-surface sentinel the
            // owner reducer drops); REFUSE on any other unrepresentable Unknown
            // arm rather than publish a too-narrow remainder as complete.
            if raw.as_str() == SEMANTIC_MISS || raw.as_str() == SEMANTIC_OBJECT_SURFACE {
                continue;
            }
            return None;
        }
        // Non-`Unknown` arms — including `Primitive(Unknown)` (genuine TS
        // `unknown`, a real type) — are resolvable and survive into the remainder.
        resolvable.push(arm.clone());
    }
    if resolvable.is_empty() {
        return None;
    }
    // `TypeExpr::intersection` unwraps a single survivor and rebuilds the
    // intersection for multiple — the non-empty case is guarded above so the
    // empty→`unknown` arm of the constructor is never reached.
    Some(TypeExpr::intersection(resolvable))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

    use super::resolvable_intersection_remainder;
    use crate::resolver_core::component_meta_query_engine::{
        SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE,
    };

    /// The single resolvable inline arm `{ projectOnly?: string }` — the
    /// member that MUST survive the partial-surface fallback.
    fn project_only_object() -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "projectOnly".to_string(),
                TypeExpr::Primitive(PrimitiveName::String),
                true,  // optional
                false, // readonly
            ))],
        }))
    }

    fn intersection(arms: Vec<TypeExpr>) -> TypeExpr {
        TypeExpr::Intersection(Arc::from(arms))
    }

    /// POSITIVE control (GREEN pre- and post-fix): a `semanticMiss` arm is the
    /// confident-empty degradation sentinel (`<empty> & T = T`), so it is
    /// dropped and the resolvable object remainder is recovered.
    #[test]
    fn drops_semantic_miss_sentinel_arm_and_recovers_resolvable_remainder() {
        let tag = intersection(vec![
            TypeExpr::Unknown {
                raw: SEMANTIC_MISS.to_string(),
            },
            project_only_object(),
        ]);
        let remainder = resolvable_intersection_remainder(&tag)
            .expect("a semantic-miss arm is vacuous-droppable; the resolvable remainder survives");
        // The lone survivor unwraps to the object itself — proving the miss arm
        // is gone (NEGATIVE: no `Unknown` sentinel, no residual intersection).
        assert_eq!(remainder, project_only_object());
        assert!(!remainder.is_unknown());
        assert!(!matches!(remainder, TypeExpr::Intersection(_)));
    }

    /// DISCRIMINATING (RED pre-fix, GREEN post-fix): a budget-exceeded `Unknown`
    /// is a NON-MISS degradation sentinel — an INCOMPLETE arm that may have had
    /// members. Recovering the remainder as the COMPLETE tag surface would be a
    /// too-narrow, warm-cacheable wrong-confident result, so recovery is refused.
    /// Pre-fix this wrongly returned `Some({ projectOnly })`.
    #[test]
    fn refuses_recovery_on_budget_exceeded_unresolved_arm() {
        let tag = intersection(vec![
            TypeExpr::Unknown {
                raw: "budgetExceeded(depth=64)".to_string(),
            },
            project_only_object(),
        ]);
        assert_eq!(
            resolvable_intersection_remainder(&tag),
            None,
            "a non-miss (budget-exceeded) Unknown arm must refuse the false-complete recovery"
        );
    }

    /// DISCRIMINATING (RED pre-fix, GREEN post-fix): an import-type / `QueryError::Other`
    /// opaque error carrier is likewise a NON-MISS incomplete arm — recovery refused.
    #[test]
    fn refuses_recovery_on_import_type_error_unresolved_arm() {
        let tag = intersection(vec![
            TypeExpr::Unknown {
                raw: "import-type generic args on a multi-segment qualifier are not yet \
                      instantiated"
                    .to_string(),
            },
            project_only_object(),
        ]);
        assert_eq!(
            resolvable_intersection_remainder(&tag),
            None,
            "a non-miss (import-type error) Unknown arm must refuse the false-complete recovery"
        );
    }

    /// NEGATIVE control (GREEN pre- and post-fix): genuine TS `unknown` is
    /// `Primitive(PrimitiveName::Unknown)`, a REAL type — NOT the
    /// could-not-represent `TypeExpr::Unknown { raw }` sentinel. It is never
    /// dropped and never triggers a refusal; it stays in the remainder.
    #[test]
    fn preserves_genuine_ts_unknown_primitive_arm() {
        let tag = intersection(vec![
            TypeExpr::Primitive(PrimitiveName::Unknown),
            project_only_object(),
        ]);
        let remainder = resolvable_intersection_remainder(&tag).expect(
            "Primitive(Unknown) is a real type, not a degradation sentinel — never refused",
        );
        match &remainder {
            TypeExpr::Intersection(arms) => {
                assert!(
                    arms.iter()
                        .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::Unknown))),
                    "the genuine TS `unknown` arm must be preserved in the remainder; got {arms:?}"
                );
                assert!(
                    arms.iter().any(|arm| matches!(arm, TypeExpr::Object(_))),
                    "the resolvable object arm must be preserved; got {arms:?}"
                );
            }
            other => panic!("expected the two-arm intersection to be preserved, got {other:?}"),
        }
    }

    /// DISCRIMINATING for the STEP-1 droppable-set decision (GREEN under the
    /// `{ SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE }` set; would FAIL a minimal
    /// miss-only refuse-everything-else variant): the object-surface sentinel is
    /// the empty / unrepresentable object-surface arm the owner intersection
    /// reducer drops as vacuous (`fold_node` Intersection
    /// `!is_object_surface_sentinel`; produced only for a fully-folded object
    /// surface with zero representable members), so it is droppable here too —
    /// the resolvable object remainder is recovered.
    #[test]
    fn drops_object_surface_sentinel_arm_as_vacuous_like_miss() {
        let tag = intersection(vec![
            TypeExpr::Unknown {
                raw: SEMANTIC_OBJECT_SURFACE.to_string(),
            },
            project_only_object(),
        ]);
        let remainder = resolvable_intersection_remainder(&tag).expect(
            "the object-surface sentinel is a vacuous empty-surface arm — dropped, remainder \
             recovered",
        );
        assert_eq!(remainder, project_only_object());
    }
}
