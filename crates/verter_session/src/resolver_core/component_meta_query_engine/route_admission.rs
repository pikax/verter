//! The sealed route-admission carrier ([`AdmittedRouteProjectionNode`]) plus the
//! gated mint helpers that are the SOLE minters of it.
//!
//! THREE-LAYER STRUCTURAL SEAL (the primary defense): a carrier can be obtained
//! ONLY through a gated `admit_*` helper, the gate's fact input is itself
//! unforgeable, AND that input is NODE-BOUND so the helper cannot pair one node's
//! facts with a different node.
//!
//! 1. CONSTRUCTOR confinement — [`AdmittedRouteProjectionNode::new`] is PRIVATE
//!    to this module. No sibling — `surface`, `registry_decl`, `node_materialize`,
//!    the `meta_resolve` host-threaded wrappers, the route fixpoint — can
//!    construct the carrier or alias-forge one: a planted
//!    `use super::AdmittedRouteProjectionNode as Forge; Forge::new(node)` is
//!    `error[E0624]: associated function `new` is private`, because aliasing a
//!    path does not relax the item's visibility.
//! 2. FACT-INPUT seal — the gate inputs [`RaisedNodeShapeFacts`] / [`NodeShapeEq`]
//!    carry PRIVATE fields (constructable only inside the node-domain
//!    `shape_engine`). A sibling cannot fabricate a
//!    `RaisedNodeShapeFacts { node, facts }` (or a `NodeShapeEq { eq_to_expr:
//!    false, .. }`) struct literal to drive an `admit_*` helper past its gate: the
//!    literal is `error[E0451]: field … is private`. The facts read by the helpers
//!    can only be the legitimate output of the shared fold.
//! 3. NODE-BOUND input — every `admit_*` helper takes ONLY the node-bound
//!    witness (no separate `SemanticNodeId` parameter) and mints from
//!    `witness.node()` / `shape.node()` ALONE. The witness pairs a node with the
//!    facts computed FOR THAT NODE, so a "node A's facts, node B's carrier"
//!    mispair is UNREPRESENTABLE through the safe API — the terminal carrier proof
//!    in safe Rust (a regression re-adding a node parameter trips the
//!    `route_admission_admit_helpers_take_no_node_param` signature guard).
//!
//! The route/surface adapters mint EXCLUSIVELY through the gated
//! [`admit_expanded_surface`] / [`admit_expanded_surface_changed`] /
//! [`admit_materialized`] helpers below, each of which
//! encodes ONE call site's node-domain acceptance gate and is the only code that
//! can reach the private constructor. The helpers take the caller's
//! ALREADY-COMPUTED (and now sealed, node-bound) [`RaisedNodeShapeFacts`] /
//! [`NodeShapeEq`] witness (the route/surface adapters fold it for their own
//! decision anyway), so confining the mint here adds no extra hot-path fold.
//!
//! The type itself is `pub(crate)` ON PURPOSE so the cross-subtree
//! `meta_resolve::dispatch_helpers` host-threaded wrappers and the route fixpoint
//! can NAME and pass the carrier across the query-engine boundary — they cannot
//! FORGE one (`new` stays module-private; `node` is subtree-mint-scoped), so
//! widening the type's NAME does not widen the MINT or the materialise (which
//! stays cap-gated at the surface sink).

use crate::project_semantic_dispatch::raise::{NodeShapeEq, RaisedNodeShapeFacts};
use crate::semantic_query::SemanticNodeId;

/// A node-domain route-projection result: the admitted [`SemanticNodeId`] a
/// route/surface adapter produced AFTER its node-domain acceptance gate, held in
/// node domain so the route fixpoint stabilises on interned
/// `shape_engine::RaisedShapeKey` identity and materialises EXACTLY ONCE at the
/// terminal surface sink.
///
/// ACCEPTANCE INVARIANT (honest, gate-specific): a carrier is minted only by one
/// of this module's gated helpers, so its MINIMAL guarantee is that the node's
/// raised shape is `materialized` — the gate the registry route arms admit on
/// ([`admit_materialized`]). The surface / empty-terminal / class-A arms admit on
/// the STRONGER `materialized && expanded_surface` ([`admit_expanded_surface`] /
/// [`admit_expanded_surface_changed`]). The carrier does NOT itself assert the stronger
/// expanded-surface property for every node it holds — the registry / Shallow-
/// terminal arms can mint a materialized-but-not-expanded node — so consumers
/// must not infer expanded-surface from possession of the carrier.
///
/// SEALED: the `node` field is module-private and `new` is PRIVATE to
/// `route_admission` (only the gated helpers reach it); `node` is subtree-mint-
/// scoped (`pub(in …::component_meta_query_engine)`) so only the in-subtree sink /
/// compare helpers read it, and the materialisation it feeds stays cap-gated at
/// the surface sink regardless of who holds the carrier. The seal is COMPLETE
/// end-to-end and NODE-BOUND: the gate input is the sealed
/// [`RaisedNodeShapeFacts`] / [`NodeShapeEq`] witness (private fields, fold-only),
/// the helpers mint from `witness.node()` ALONE (no free node parameter), so the
/// node a carrier holds is ALWAYS the node whose own gate the witness facts
/// passed — neither the carrier nor a passing-gate node-mispair can be forged
/// outside the legitimate fold + gated helper.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AdmittedRouteProjectionNode {
    node: SemanticNodeId,
}

impl AdmittedRouteProjectionNode {
    /// PRIVATE to `route_admission` — the structural seal. The ONLY callers are the
    /// gated `admit_*` helpers in this module; a sibling cannot name it (alias
    /// laundering included), so no un-gated / forged carrier can exist.
    #[must_use]
    fn new(node: SemanticNodeId) -> Self {
        Self { node }
    }

    /// The admitted node. Subtree-scoped so only the sink-owned materialise /
    /// compare helpers read it; the materialisation it feeds stays cap-gated.
    #[must_use]
    pub(in crate::resolver_core::component_meta_query_engine) fn node(&self) -> SemanticNodeId {
        self.node
    }
}

/// Gate: `materialized && expanded_surface`. Mints when the node's raised shape is
/// a fully-materialised expanded surface. The `surface` empty-terminal re-projection
/// and class-A terminal arms admit on this gate.
#[must_use]
pub(in crate::resolver_core::component_meta_query_engine) fn admit_expanded_surface(
    witness: &RaisedNodeShapeFacts,
) -> Option<AdmittedRouteProjectionNode> {
    (witness.materialized() && witness.expanded_surface())
        .then(|| AdmittedRouteProjectionNode::new(witness.node()))
}

/// Gate: `materialized && expanded_surface && changed`, where
/// `changed = !shape.eq_to_expr` (the node's raised shape differs from the
/// caller's input `&TypeExpr`). The `lower_and_project_to_expanded` arm admits on
/// this gate so a no-op re-projection (a stable cursor that did not change) is not
/// re-admitted.
#[must_use]
pub(in crate::resolver_core::component_meta_query_engine) fn admit_expanded_surface_changed(
    shape: &NodeShapeEq,
) -> Option<AdmittedRouteProjectionNode> {
    (shape.facts().materialized() && shape.facts().expanded_surface() && shape.changed())
        .then(|| AdmittedRouteProjectionNode::new(shape.node()))
}

/// Gate: `materialized` only — the registry route arms (`MemberPath` / `Pick` /
/// `Omit`). The typed equivalent of the former
/// `.filter(dispatch_route_expr_is_materialized)` over the materialised route
/// TypeExpr: a registry route admits a materialised node WITHOUT requiring it be an
/// expanded surface (the registry publication sink materialises it once with no
/// decision on the result).
#[must_use]
pub(in crate::resolver_core::component_meta_query_engine) fn admit_materialized(
    witness: &RaisedNodeShapeFacts,
) -> Option<AdmittedRouteProjectionNode> {
    witness
        .materialized()
        .then(|| AdmittedRouteProjectionNode::new(witness.node()))
}
