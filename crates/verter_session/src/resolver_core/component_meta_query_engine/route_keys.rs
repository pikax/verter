//! Route literal-key enumeration and direct utility shape projection
//! methods extracted from `component_meta_query_engine/mod.rs`.
//!
//! These methods enumerate the literal string keys driving Pick / Omit
//! / MemberPath route demands, and project the shape of a direct
//! utility wrapper (`Pick<X, ...>` / `Omit<X, ...>`) without going
//! through the full prepared-surface pipeline.
//!
//! Visibility:
//! - `pub(crate) fn project_direct_utility_surface_shape` — used by
//!   `meta_resolve` consumers and the routed-expression projection.
//! - All other methods stay private and are visible inside the
//!   `component_meta_query_engine` folder via parent-private locality.

use verter_type_expr::TypeExpr;

use super::helpers::{is_builtin_name, strip_parens_expr};
use super::surface::RouteKeyspaceNode;
use super::{AdmittedRouteProjectionNode, ComponentMetaQueryEngine, PreparedProjectionContext};

/// The route fixpoint's cursor. The FIRST iteration projects the input
/// `&TypeExpr`; every later iteration re-projects the prior admitted node
/// directly (node-base re-projection, no re-lowering / no materialisation), so
/// the fixpoint stabilises on interned raised-shape identity — the route-key
/// enumerators read the converged node directly, never a materialised
/// `TypeExpr`.
#[derive(Clone, Copy)]
enum RouteFixpointCursor<'cursor> {
    Input(&'cursor TypeExpr),
    Node(AdmittedRouteProjectionNode),
}

/// A route-key leaf stabilised to NODE domain: the admitted route-projection
/// node plus the node-domain "did any scope advance the input?" decision
/// (`eq_to_input`, read from [`super::route_projection_node_eq_to_expr`] — the
/// interned raised-shape equality, NOT a materialised `TypeExpr ==`). The
/// route-key enumerators read `node` for keyspace / member-surface enumeration;
/// the per-shape scope dispatch reads `eq_to_input` to choose decl / arg / chain
/// scope fallbacks.
#[derive(Clone, Copy)]
struct StableRouteLeafNode {
    node: AdmittedRouteProjectionNode,
    eq_to_input: bool,
}

impl<'a> ComponentMetaQueryEngine<'a> {
    /// Enumerate the literal string keys named by a `Pick` / `Omit` `keys`
    /// type argument (used by [`Self::project_direct_utility_surface_shape`]).
    ///
    /// Lowers the key-source to a NODE under the route-key
    /// [`PreparedProjectionContext`] (the one shared resolver, no whole-object
    /// `TypeExpr` materialise), then enumerates NODE-DOMAIN:
    ///
    /// 1. PRIMARY — the SINGLE shared dispatch keyspace enumerator
    ///    ([`super::surface::enumerate_keyspace_names_from_keyspace_node`] →
    ///    `key_names_from_keyspace_node`, the same enumerator the `Pick` /
    ///    `Omit` dispatch reducers consume). It owns the key-TYPE reduction:
    ///    literal unions, alias-to-union, `keyof X`, `keyof X['m']['n']`,
    ///    `never` — the IndexedAccess / Conditional / Intersection / `typeof`
    ///    distribution runs inside the shared dispatch (`KeyOf` producer +
    ///    structural-transit), not a hand-rolled member-route walker.
    /// 2. FALLBACK — for a `keyof <surface>` whose keyspace reduction did not
    ///    enumerate, stabilise the OPERAND to its member surface and read its
    ///    public member names via the narrow admitted-surface API
    ///    ([`super::surface::enumerate_public_surface_member_names_from_admitted_node`]).
    pub(super) fn enumerate_route_literal_keys(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<Vec<String>> {
        let context = PreparedProjectionContext {
            decl_scope: resolution_scope_canonical_id.to_string(),
            arg_scope: active_scope_canonical_id.to_string(),
        };
        // PRIMARY: lower the key-source and enumerate its keyspace.
        if let Some(node) = self.lower_route_key_source_node(&context, expr) {
            if let Some(keys) =
                super::surface::enumerate_keyspace_names_from_keyspace_node(self.ctx(), &node)
            {
                return Some(keys);
            }
        }
        // FALLBACK: `keyof <surface>` whose keyspace reduction did not resolve —
        // stabilise the operand to its surface and read its public member names.
        if let TypeExpr::KeyOf(inner) = strip_parens_expr(expr) {
            if let Some(leaf) = self.solve_or_project_leaf_node_with_context(&context, inner) {
                return super::surface::enumerate_public_surface_member_names_from_admitted_node(
                    self.ctx(),
                    &leaf.node,
                );
            }
        }
        None
    }

    /// Lower a route-key SOURCE expression to a keyspace NODE in `arg_scope` at
    /// `Expanded`, for the shared dispatch keyspace enumerator (which re-evaluates
    /// the node under structural-transit + the `KeyOf` producer). NO admission
    /// gate: a `keyof` / `IndexedAccess` keyspace carrier is INTENTIONALLY
    /// preserved un-admitted so the enumerator can reduce it — hence the distinct
    /// [`RouteKeyspaceNode`] carrier (never [`AdmittedRouteProjectionNode`], whose
    /// `materialized && expanded_surface` invariant a keyspace carrier does not
    /// satisfy).
    fn lower_route_key_source_node(
        &mut self,
        context: &PreparedProjectionContext,
        expr: &TypeExpr,
    ) -> Option<RouteKeyspaceNode> {
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
        let lowered = dispatch.lower_type_expr_in_scope_with_mode(
            &context.arg_scope,
            expr,
            crate::semantic_query::ProjectionMode::Expanded,
        )?;
        Some(RouteKeyspaceNode::new(lowered))
    }

    pub(crate) fn project_direct_utility_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        use verter_semantic::analysis::type_expand::ExpandedObjectShape;
        use verter_type_expr::TypeExpr;

        fn shape_has_surface(shape: &ExpandedObjectShape) -> bool {
            !shape.properties.is_empty() || !shape.call_signatures.is_empty()
        }

        fn projected_target_shape(
            query_engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            target: &TypeExpr,
        ) -> Option<ExpandedObjectShape> {
            // Budget guard preserved from the former host-threaded bridges (both
            // bailed on an exhausted projection budget before projecting).
            if query_engine.projection_op_budget_exhausted() {
                return None;
            }
            // (1) The node-domain surface-shape demand API (registry route /
            // direct-utility / general node path), reused DIRECTLY — no
            // host-threaded surface bridge, no `TypeExpr` materialise. This is the
            // node-domain successor of the former
            // `project_expr_surface_shape_via_host_threaded` bridge (which only
            // delegated to this same engine method).
            if let Some(shape) =
                query_engine.project_expr_to_surface_shape(scope_canonical_id, target)
            {
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
            // (2) Node-domain fallback: project the target's admitted surface NODE
            // arm-for-arm (registry fast-path → Expanded fallback →
            // Navigate-base/Shallow-terminal pure-dispatch — the EXACT routing of
            // the expr-domain surface bridge), then build
            // the shape from the admitted node's SurfaceView. NO mid-flight
            // materialise; `shape_has_surface` decides on a node-domain shape (so
            // it ignores index signatures exactly as before).
            if let Some(node) =
                crate::meta_resolve::project_expr_surface_expr_node_via_host_threaded(
                    query_engine,
                    scope_canonical_id,
                    target,
                    crate::semantic_query::ProjectionMode::Navigate,
                    crate::semantic_query::ProjectionMode::Shallow,
                    crate::semantic_query::ReductionDemand::Published,
                )
            {
                if let Some(shape) =
                    super::surface::project_admitted_route_node_to_expanded_object_shape(
                        query_engine.ctx(),
                        &node,
                    )
                {
                    if shape_has_surface(&shape) {
                        return Some(shape);
                    }
                }
            }
            // No structural-substitution fallback: the node-domain surface
            // projections above (the demand API + the arm-for-arm Navigate/Shallow
            // node bridge) are the sole authority for the utility-route target
            // shape, including generic-alias instantiation. A miss here is a clean
            // `None`.
            None
        }

        let TypeExpr::Ref {
            name,
            type_arguments,
        } = strip_parens_expr(expr)
        else {
            return None;
        };

        match (name.as_ref(), type_arguments.as_ref()) {
            ("Partial", [target]) => {
                projected_target_shape(self, scope_canonical_id, target).map(|mut shape| {
                    for property in &mut shape.properties {
                        property.optional = true;
                    }
                    shape
                })
            }
            ("Required", [target]) => {
                projected_target_shape(self, scope_canonical_id, target).map(|mut shape| {
                    for property in &mut shape.properties {
                        property.optional = false;
                    }
                    shape
                })
            }
            ("Readonly", [target]) => {
                projected_target_shape(self, scope_canonical_id, target).map(|mut shape| {
                    for property in &mut shape.properties {
                        property.readonly = true;
                    }
                    shape
                })
            }
            ("NonNullable", [target]) => projected_target_shape(self, scope_canonical_id, target),
            ("Pick", [target, keys]) => {
                let requested = self.enumerate_route_literal_keys(
                    scope_canonical_id,
                    scope_canonical_id,
                    keys,
                )?;
                let mut shape = projected_target_shape(self, scope_canonical_id, target)?;
                // `Pick<X, K>` is a PUBLIC-keyspace projection (TS:
                // `Pick<T, K extends keyof T>`, and `keyof` excludes
                // protected/private class members). Gate on
                // `visibility.is_public()` BEFORE the name predicate so a
                // `Pick` whose key names a non-public member (e.g.
                // `Pick<Partial<C>, 'privateMember'>`) yields an empty surface
                // rather than re-minting the non-public member — the same
                // public-keyspace gate the shared builtin Pick engine applies.
                // The full member set stays recorded on the source shape for
                // the keep-all `native_props` carrier; only this DERIVATION
                // gates.
                shape.properties.retain(|property| {
                    property.visibility.is_public()
                        && requested
                            .iter()
                            .any(|candidate| candidate == property.name.as_str())
                });
                shape_has_surface(&shape).then_some(shape)
            }
            ("Omit", [target, keys]) => {
                let omitted = self.enumerate_route_literal_keys(
                    scope_canonical_id,
                    scope_canonical_id,
                    keys,
                )?;
                let mut shape = projected_target_shape(self, scope_canonical_id, target)?;
                // `Omit<X, K>` = `Pick<X, Exclude<keyof X, K>>` — a
                // PUBLIC-keyspace projection. Gate on `visibility.is_public()`
                // BEFORE the name predicate so an `Omit` over a class never
                // LEAVES a non-public member published (the keyspace `Omit`
                // derives from is public-only). The full member set stays
                // recorded on the source shape for the keep-all `native_props`
                // carrier; only this DERIVATION gates.
                shape.properties.retain(|property| {
                    property.visibility.is_public()
                        && !omitted
                            .iter()
                            .any(|candidate| candidate == property.name.as_str())
                });
                shape_has_surface(&shape).then_some(shape)
            }
            _ => None,
        }
    }
    /// Per-`TypeExpr`-shape scope dispatch for the prepared-member-path leaf,
    /// returning the stabilised NODE plus the node-domain "did any scope advance
    /// the input?" decision ([`StableRouteLeafNode`]). The node-returning sibling
    /// of the former materialised leaf stabiliser: every scope-fallback decision
    /// reads `eq_to_input` (the interned raised-shape equality of the stabilised
    /// node against `expr`), NEVER a materialised `TypeExpr ==`.
    ///
    /// - `decl_scope == arg_scope` (the route-key entry's only live shape):
    ///   stabilise once in `arg_scope`.
    /// - bare `Ref { name, [] }`: try `decl_scope` (helper-body-internal
    ///   reference), fall back to `arg_scope`.
    /// - `TypeOf(value_ref)`: `arg_scope` first (caller-scoped value table),
    ///   then `decl_scope`.
    /// - `Ref { name, [args..] }`: try `decl_scope` (helper declaration
    ///   registry), then `arg_scope` (direct import).
    /// - compound shapes (`IndexedAccess`, `Conditional`, `Mapped`, `KeyOf`,
    ///   etc.): the two-scope retry (`arg_scope`, then `decl_scope` when the
    ///   expr references a prepared `decl_scope` symbol).
    fn solve_or_project_leaf_node_with_context(
        &mut self,
        context: &PreparedProjectionContext,
        expr: &TypeExpr,
    ) -> Option<StableRouteLeafNode> {
        let decl_scope = context.decl_scope.clone();
        let arg_scope = context.arg_scope.clone();

        if decl_scope == arg_scope {
            return self.stabilise_route_leaf_node(&arg_scope, expr);
        }

        match expr {
            TypeExpr::Ref {
                name: _,
                type_arguments,
            } if type_arguments.is_empty() => {
                // Bare `Ref { name, [] }`: helper-body-internal reference.
                // Try decl_scope first; fall back to arg_scope.
                if let Some(leaf) = self.stabilise_route_leaf_node(&decl_scope, expr) {
                    if !leaf.eq_to_input {
                        return Some(leaf);
                    }
                }
                self.stabilise_route_leaf_node(&arg_scope, expr)
            }
            TypeExpr::TypeOf(_) => {
                // `typeof value_ref`: caller-scoped first, then the decl scope.
                let arg_first = self.stabilise_route_leaf_node(&arg_scope, expr);
                if let Some(leaf) = arg_first {
                    if !leaf.eq_to_input {
                        return Some(leaf);
                    }
                }
                if let Some(leaf) = self.stabilise_route_leaf_node(&decl_scope, expr) {
                    if !leaf.eq_to_input {
                        return Some(leaf);
                    }
                }
                arg_first
            }
            TypeExpr::Ref { .. } => {
                // `Ref { name, [args..] }`: helper instantiation. Try decl_scope
                // (helper declaration registry), then arg_scope (direct import).
                let decl_first = self.stabilise_route_leaf_node(&decl_scope, expr);
                if let Some(leaf) = decl_first {
                    if !leaf.eq_to_input {
                        return Some(leaf);
                    }
                }
                let arg_result = self.stabilise_route_leaf_node(&arg_scope, expr);
                if let Some(leaf) = arg_result {
                    if !leaf.eq_to_input {
                        return Some(leaf);
                    }
                }
                arg_result.or(decl_first)
            }
            _ => {
                // Compound shapes (IndexedAccess, Conditional, Mapped, KeyOf,
                // Intersection, Union, Parenthesized, etc.): two-scope retry.
                let active_result = self.stabilise_route_leaf_node(&arg_scope, expr);
                if !self.expr_references_prepared_scope_symbol(&decl_scope, expr) {
                    return active_result;
                }
                self.stabilise_route_leaf_node(&decl_scope, expr)
                    .or(active_result)
            }
        }
    }

    /// Stabilise a leaf `TypeExpr` to its converged route NODE in
    /// `scope_canonical_id` and pair it with the node-domain `eq_to_input`
    /// decision ([`super::route_projection_node_eq_to_expr`]) — the
    /// [`StableRouteLeafNode`] the scope dispatch reads.
    fn stabilise_route_leaf_node(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<StableRouteLeafNode> {
        let node = self.solve_or_project_leaf_node_until_stable(scope_canonical_id, expr)?;
        let eq_to_input = super::route_projection_node_eq_to_expr(self.ctx(), &node, expr);
        Some(StableRouteLeafNode { node, eq_to_input })
    }

    /// Fixed-point driver: repeatedly stabilise a leaf `TypeExpr` in
    /// `scope_canonical_id` until it stops advancing (or the iteration budget is
    /// exhausted), returning the converged route NODE (never a materialised
    /// `TypeExpr`).
    fn solve_or_project_leaf_node_until_stable(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<AdmittedRouteProjectionNode> {
        // Node-domain fixpoint: stabilise on interned `RaisedShapeKey` identity.
        // Each iteration projects the cursor through the NODE adapters (no
        // materialisation); convergence compares interned raised-shape keys
        // (`route_projection_node_eq_to_expr` against the input `expr` on
        // iteration 1, `route_projection_nodes_eq` against the prior node on
        // later iterations). The converged node is returned directly; the sole
        // publication materialisation happens once, later, at the surface sink.
        let mut cursor = RouteFixpointCursor::Input(expr);
        let mut last: Option<AdmittedRouteProjectionNode> = None;
        for _ in 0..3 {
            let produced = match cursor {
                // FIRST iteration (and any iteration whose cursor is still the
                // input) — primary `.or_else` fallback: the empty-terminal
                // Expanded lower+project, then the Expanded-base / Expanded-
                // terminal Published surface bridge. Expanded is preserved on
                // both dimensions because reducing either below Expanded breaks
                // fixpoint convergence for imported alias helpers like
                // `Button['ui']`, where a Navigate carrier would freeze a generic
                // `InstantiationRef` at the empty-path terminal.
                RouteFixpointCursor::Input(input) => {
                    crate::meta_resolve::lower_and_project_to_expanded_node_via_host_threaded(
                        self,
                        scope_canonical_id,
                        input,
                    )
                    .or_else(|| {
                        crate::meta_resolve::project_expr_surface_expr_node_via_host_threaded(
                            self,
                            scope_canonical_id,
                            input,
                            crate::semantic_query::ProjectionMode::Expanded,
                            crate::semantic_query::ProjectionMode::Expanded,
                            crate::semantic_query::ReductionDemand::Published,
                        )
                    })
                }
                // Later iterations — re-project the already-admitted prior node
                // directly (node-base re-projection), no re-lowering / no
                // materialisation.
                RouteFixpointCursor::Node(prior) => {
                    crate::meta_resolve::project_admitted_node_to_expanded_node_via_host_threaded(
                        self, &prior,
                    )
                }
            };
            let Some(produced) = produced else {
                return last;
            };
            let converged = match cursor {
                RouteFixpointCursor::Input(input) => {
                    super::route_projection_node_eq_to_expr(self.ctx(), &produced, input)
                }
                RouteFixpointCursor::Node(prior) => {
                    super::route_projection_nodes_eq(self.ctx(), &produced, &prior)
                }
            };
            if converged {
                return Some(produced);
            }
            last = Some(produced);
            cursor = RouteFixpointCursor::Node(produced);
        }
        last
    }

    /// Predicate: does `expr` reference a prepared symbol that resolves
    /// within `scope_canonical_id`? Gates whether the resolution scope is
    /// the right one to stabilise a leaf against.
    fn expr_references_prepared_scope_symbol(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        use verter_type_expr::ObjectMember;

        match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                (!is_builtin_name(name.as_ref())
                    && self
                        .prepared_type_decl(scope_canonical_id, name.as_ref())
                        .is_some())
                    || type_arguments.iter().any(|arg| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, arg)
                    })
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, inner)
            }
            TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, &element.ty)
            }),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
                .iter()
                .any(|ty| self.expr_references_prepared_scope_symbol(scope_canonical_id, ty)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                ObjectMember::Property(property) => {
                    self.expr_references_prepared_scope_symbol(scope_canonical_id, &property.ty)
                }
                ObjectMember::Method(method) => {
                    method.function.parameters.iter().any(|param| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, &param.ty)
                    }) || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| {
                            self.expr_references_prepared_scope_symbol(
                                scope_canonical_id,
                                return_type,
                            )
                        })
                }
                ObjectMember::IndexSignature(signature) => {
                    self.expr_references_prepared_scope_symbol(
                        scope_canonical_id,
                        &signature.key_type,
                    ) || self.expr_references_prepared_scope_symbol(
                        scope_canonical_id,
                        &signature.value_type,
                    )
                }
                ObjectMember::CallSignature(function)
                | ObjectMember::ConstructSignature(function) => {
                    function.parameters.iter().any(|param| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, &param.ty)
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, return_type)
                    })
                }
            }),
            // A constructor type's signature is searched identically to a
            // function type's (same `FunctionExpr` payload).
            TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
                function.parameters.iter().any(|param| {
                    self.expr_references_prepared_scope_symbol(scope_canonical_id, &param.ty)
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    self.expr_references_prepared_scope_symbol(scope_canonical_id, return_type)
                })
            }
            TypeExpr::IndexedAccess { object, index } => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, object)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, index)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, check)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, extends)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, true_type)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, false_type)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, source)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, value)
                    || name_type.as_deref().is_some_and(|name_type| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, name_type)
                    })
            }
            TypeExpr::TemplateLiteral { expressions, .. } => expressions
                .iter()
                .any(|expr| self.expr_references_prepared_scope_symbol(scope_canonical_id, expr)),
            TypeExpr::TypeParameter(type_parameter) => {
                type_parameter
                    .constraint
                    .as_deref()
                    .is_some_and(|constraint| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, constraint)
                    })
                    || type_parameter.default.as_deref().is_some_and(|default| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, default)
                    })
            }
            TypeExpr::RecursiveRef {
                type_arguments,
                conditional_context,
                ..
            } => {
                type_arguments
                    .iter()
                    .any(|arg| self.expr_references_prepared_scope_symbol(scope_canonical_id, arg))
                    || conditional_context.iter().any(|frame| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, &frame.check)
                            || self.expr_references_prepared_scope_symbol(
                                scope_canonical_id,
                                &frame.extends,
                            )
                    })
            }
            // Mirrors the `Ref` arm's recursion into `type_arguments`. The
            // `specifier`/`qualifier` name a cross-file module path, not a
            // prepared declaration within `scope_canonical_id`, so only the
            // nested type-argument exprs are searched.
            TypeExpr::ImportType { type_arguments, .. } => type_arguments
                .iter()
                .any(|arg| self.expr_references_prepared_scope_symbol(scope_canonical_id, arg)),
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Infer { .. }
            // Synthetic carriers reference no prepared scope symbol â€”
            // their identity is intrinsic to the scope itself, not a
            // declaration name within it.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Unknown { .. } => false,
        }
    }
}

// ===========================================================================
// TEST-ONLY faithful reconstruction of the retired pre-node-domain route-key
// walker — the differential ORACLE the node-domain `enumerate_route_literal_keys`
// is proved against.
//
// Recovered branch-for-branch from `git show 0810933b9` (the pre-conversion
// tree): the retired `enumerate_route_literal_keys_inner` recursive `TypeExpr`
// key-algebra (depth limit 4, the whole-`KeyOf` step-2, union all-or-nothing,
// intersection/union arm accumulation) and the retired
// `enumerate_member_surface_keys_via_route` `keyof X['m']['n']` hand-distributor
// (depth limit 8, object member lookup, conditional / typeof / intersection /
// union / nested-indexed-access distribution).
//
// The retired materialised leaf stabiliser `solve_or_project_prepared_member_leaf_expr`
// is reconstructed as the SURVIVING node fixpoint
// (`solve_or_project_leaf_node_until_stable`) + the surface-sink materialise —
// the EXACT computation the retired stabiliser performed (the conversion only
// moved the single materialise to the sink). It deliberately NEVER routes through
// the node-domain key/member enumerators the differential is proving
// (`key_names_from_keyspace_node` /
// `enumerate_public_surface_member_names_from_admitted_node`), so a regression in
// either of those production paths changes the node-domain result WITHOUT moving
// this oracle — the differential fails and discriminates.
// ===========================================================================

/// TEST-ONLY faithful copy of the retired
/// `helpers::projected_surface_member_names` `TypeExpr` walker — the legacy
/// public-keyspace member-name reader both the route-key walker and the
/// member-name differential read.
#[cfg(test)]
pub(super) fn legacy_projected_surface_member_names(expr: &TypeExpr) -> Option<Vec<String>> {
    use verter_type_expr::ObjectMember;

    match expr {
        TypeExpr::Object(object) => {
            let mut members = Vec::new();
            for member in object.properties.iter() {
                match member {
                    ObjectMember::Property(property) if property.visibility.is_public() => {
                        members.push(property.name.clone())
                    }
                    ObjectMember::Method(method) if method.visibility.is_public() => {
                        members.push(method.name.clone())
                    }
                    _ => {}
                }
            }
            members.sort();
            members.dedup();
            Some(members)
        }
        TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
            let mut members = Vec::new();
            for part in parts.iter() {
                members.extend(legacy_projected_surface_member_names(part)?);
            }
            members.sort();
            members.dedup();
            Some(members)
        }
        TypeExpr::Parenthesized(inner) => legacy_projected_surface_member_names(inner),
        _ => None,
    }
}

#[cfg(test)]
impl ComponentMetaQueryEngine<'_> {
    /// TEST-ONLY reconstruction of the retired materialised leaf stabiliser
    /// `solve_or_project_prepared_member_leaf_expr` for the differential's
    /// same-scope input class: the SURVIVING node fixpoint
    /// ([`Self::solve_or_project_leaf_node_until_stable`]) + the one surface-sink
    /// materialise. The node-domain conversion only relocated the materialise to
    /// the sink, so this returns the EXACT `TypeExpr` the retired stabiliser did.
    fn legacy_materialised_leaf(&mut self, scope: &str, expr: &TypeExpr) -> Option<TypeExpr> {
        let node = self.solve_or_project_leaf_node_until_stable(scope, expr)?;
        super::surface::materialize_route_projection_node(self.ctx(), &node)
    }

    /// TEST-ONLY faithful reconstruction of the retired
    /// `enumerate_route_literal_keys_inner` recursive route-key walker (depth
    /// limit 4). Branch order recovered verbatim from `0810933b9`.
    pub(super) fn legacy_enumerate_route_literal_keys(
        &mut self,
        scope: &str,
        expr: &TypeExpr,
        depth: usize,
    ) -> Option<Vec<String>> {
        use verter_type_expr::LiteralValue;

        if depth >= 4 {
            return None;
        }

        match expr {
            TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
            TypeExpr::Union(types) => {
                let mut keys = Vec::new();
                for ty in types.iter() {
                    keys.extend(self.legacy_enumerate_route_literal_keys(scope, ty, depth + 1)?);
                }
                keys.sort();
                keys.dedup();
                Some(keys)
            }
            TypeExpr::Parenthesized(inner) => {
                self.legacy_enumerate_route_literal_keys(scope, inner, depth + 1)
            }
            TypeExpr::KeyOf(inner) => {
                // (1) `keyof X['member']` nested indexed-access → member-surface route.
                if let TypeExpr::IndexedAccess { object, index } = inner.as_ref() {
                    if let TypeExpr::Literal(LiteralValue::String(member_name)) = index.as_ref() {
                        if let Some(keys) = self.legacy_enumerate_member_surface_keys_via_route(
                            scope,
                            object,
                            member_name,
                            depth + 1,
                        ) {
                            return Some(keys);
                        }
                    }
                }
                // (2) whole-`KeyOf` step-2: stabilise the WHOLE `keyof` expr first;
                //     if it advanced, recurse on the projection. THIS is the step
                //     the pre-fix oracle omitted — it makes `keyof (A | B)` reduce
                //     to the common-keys answer instead of the union of arm keys.
                if let Some(projected_expr) = self
                    .legacy_materialised_leaf(scope, expr)
                    .filter(|projected| projected != expr)
                {
                    return self.legacy_enumerate_route_literal_keys(
                        scope,
                        &projected_expr,
                        depth + 1,
                    );
                }
                // (3) project the OPERAND and read its public member names.
                let projected_inner = self
                    .legacy_materialised_leaf(scope, inner)
                    .unwrap_or_else(|| inner.as_ref().clone());
                if let Some(keys) = legacy_projected_surface_member_names(&projected_inner) {
                    return Some(keys);
                }
                // (4) intersection/union operand → accumulate enumerable
                //     `keyof part` arms (an all-or-nothing `?` would lose
                //     enumerable keys from one arm when another is unresolvable).
                match &projected_inner {
                    TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                        let mut keys = Vec::new();
                        let mut any_enumerable = false;
                        for part in parts.iter() {
                            let arm = TypeExpr::KeyOf(std::sync::Arc::new(part.clone()));
                            if let Some(arm_keys) =
                                self.legacy_enumerate_route_literal_keys(scope, &arm, depth + 1)
                            {
                                any_enumerable = true;
                                keys.extend(arm_keys);
                            }
                        }
                        if !any_enumerable {
                            return None;
                        }
                        keys.sort();
                        keys.dedup();
                        Some(keys)
                    }
                    _ => None,
                }
            }
            _ => {
                let projected = self.legacy_materialised_leaf(scope, expr)?;
                if projected == *expr {
                    crate::resolver_core::component_meta_registry::component_meta_registry_string_literal_keys(
                        &projected,
                    )
                } else {
                    self.legacy_enumerate_route_literal_keys(scope, &projected, depth + 1)
                }
            }
        }
    }

    /// TEST-ONLY faithful reconstruction of the retired
    /// `enumerate_member_surface_keys_via_route` `keyof X['m']['n']`
    /// hand-distributor (depth limit 8). Recovered from `0810933b9`. The one
    /// deviation is the `IndexedAccess`-of-`Ref` arm: the retired
    /// `instantiate_local_generic_ref_via_dispatch` helper it called was deleted
    /// in the conversion, so the generic-ref body is re-expanded through the
    /// surviving shared-dispatch leaf stabiliser (net-equivalent: expand the body,
    /// re-apply the index) — an arm the differential fixture does not reach.
    fn legacy_enumerate_member_surface_keys_via_route(
        &mut self,
        scope: &str,
        expr: &TypeExpr,
        member_name: &str,
        depth: usize,
    ) -> Option<Vec<String>> {
        use verter_type_expr::ObjectMember;

        if depth >= 8 {
            return None;
        }

        let projected_expr = self
            .legacy_materialised_leaf(scope, expr)
            .unwrap_or_else(|| expr.clone());

        match &projected_expr {
            TypeExpr::Object(object) => {
                // Public-keyspace member lookup: `keyof X['member']` reaches a
                // member's surface only when that member is on `X`'s PUBLIC
                // surface (TS rejects external indexed access of a non-public
                // class member), exactly as `keyof X` excludes non-public members.
                let member_ty = object.properties.iter().find_map(|member| match member {
                    ObjectMember::Property(property)
                        if property.name == member_name && property.visibility.is_public() =>
                    {
                        Some(property.ty.clone())
                    }
                    ObjectMember::Method(method)
                        if method.name == member_name && method.visibility.is_public() =>
                    {
                        Some(TypeExpr::Function(std::sync::Arc::new(
                            method.function.clone(),
                        )))
                    }
                    _ => None,
                })?;
                let projected_member = self
                    .legacy_materialised_leaf(scope, &member_ty)
                    .unwrap_or(member_ty);
                legacy_projected_surface_member_names(&projected_member)
            }
            TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                let mut keys = Vec::new();
                let mut any_enumerable = false;
                for part in parts.iter() {
                    if let Some(arm_keys) = self.legacy_enumerate_member_surface_keys_via_route(
                        scope,
                        part,
                        member_name,
                        depth + 1,
                    ) {
                        any_enumerable = true;
                        keys.extend(arm_keys);
                    }
                }
                if !any_enumerable {
                    return None;
                }
                keys.sort();
                keys.dedup();
                Some(keys)
            }
            TypeExpr::Conditional {
                true_type,
                false_type,
                ..
            } => {
                let mut keys = Vec::new();
                for branch in [true_type.as_ref(), false_type.as_ref()] {
                    if let Some(branch_keys) = self.legacy_enumerate_member_surface_keys_via_route(
                        scope,
                        branch,
                        member_name,
                        depth + 1,
                    ) {
                        keys.extend(branch_keys);
                    }
                }
                if keys.is_empty() {
                    None
                } else {
                    keys.sort();
                    keys.dedup();
                    Some(keys)
                }
            }
            TypeExpr::TypeOf(value_ref) => {
                // Resolve the value root via the dispatch-aligned bare-name
                // resolver + ctx `prepared_value_decl` directly (mirrors
                // `build_typeof`), then enumerate over its object shape /
                // type annotation.
                let scope_payload = self.scope_payload_for_scope(scope);
                let root_name = value_ref.path.first()?;
                let root_identity =
                    crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                        self.ctx,
                        scope,
                        scope_payload.as_deref(),
                        root_name,
                    )?;
                let prepared_value = self
                    .ctx
                    .prepared_value_decl(&root_identity.canonical_id, &root_identity.symbol_name)
                    .or_else(|| {
                        if root_identity.canonical_id.is_empty() {
                            return None;
                        }
                        let target = self.ctx.resolve_value_export_target(
                            &root_identity.canonical_id,
                            &root_identity.symbol_name,
                        )?;
                        if target.canonical_id == root_identity.canonical_id
                            && target.name == root_identity.symbol_name
                        {
                            return None;
                        }
                        self.ctx
                            .prepared_value_decl(&target.canonical_id, &target.name)
                    })?;

                if let Some(object_shape) = prepared_value.object_shape.as_ref() {
                    let object_expr = TypeExpr::Object(std::sync::Arc::new(object_shape.clone()));
                    return self.legacy_enumerate_member_surface_keys_via_route(
                        scope,
                        &object_expr,
                        member_name,
                        depth + 1,
                    );
                }
                if let Some(type_annotation) = prepared_value.type_annotation.as_ref() {
                    return self.legacy_enumerate_member_surface_keys_via_route(
                        scope,
                        type_annotation,
                        member_name,
                        depth + 1,
                    );
                }
                None
            }
            TypeExpr::Parenthesized(inner) => self.legacy_enumerate_member_surface_keys_via_route(
                scope,
                inner,
                member_name,
                depth + 1,
            ),
            TypeExpr::IndexedAccess { object, index } => match object.as_ref() {
                TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                    let parts = std::sync::Arc::clone(parts);
                    let mut keys = Vec::new();
                    let mut any_enumerable = false;
                    for arm in parts.iter() {
                        let arm_indexed = TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(arm.clone()),
                            index: index.clone(),
                        };
                        if let Some(arm_keys) = self.legacy_enumerate_member_surface_keys_via_route(
                            scope,
                            &arm_indexed,
                            member_name,
                            depth + 1,
                        ) {
                            any_enumerable = true;
                            keys.extend(arm_keys);
                        }
                    }
                    if any_enumerable {
                        keys.sort();
                        keys.dedup();
                        Some(keys)
                    } else {
                        None
                    }
                }
                TypeExpr::Conditional {
                    true_type,
                    false_type,
                    ..
                } => {
                    let mut keys = Vec::new();
                    let mut any_enumerable = false;
                    for branch in [true_type.as_ref(), false_type.as_ref()] {
                        let branch_indexed = TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(branch.clone()),
                            index: index.clone(),
                        };
                        if let Some(branch_keys) = self
                            .legacy_enumerate_member_surface_keys_via_route(
                                scope,
                                &branch_indexed,
                                member_name,
                                depth + 1,
                            )
                        {
                            any_enumerable = true;
                            keys.extend(branch_keys);
                        }
                    }
                    if any_enumerable {
                        keys.sort();
                        keys.dedup();
                        Some(keys)
                    } else {
                        None
                    }
                }
                TypeExpr::Ref { .. } => {
                    // The retired arm expanded the alias body (via the
                    // since-removed `instantiate_local_generic_ref_via_dispatch`)
                    // and re-applied the index. The shared dispatch now lowers a
                    // generic `Ref` directly, so stabilise the indexed-access
                    // OBJECT to its materialised body and re-apply the index — the
                    // net-equivalent reconstruction. Unreached by the differential
                    // fixture.
                    let expanded = self
                        .legacy_materialised_leaf(scope, object)
                        .filter(|expanded| expanded != object.as_ref())?;
                    let expanded_indexed = TypeExpr::IndexedAccess {
                        object: std::sync::Arc::new(expanded),
                        index: index.clone(),
                    };
                    self.legacy_enumerate_member_surface_keys_via_route(
                        scope,
                        &expanded_indexed,
                        member_name,
                        depth + 1,
                    )
                }
                TypeExpr::IndexedAccess { .. } => {
                    let resolved_inner = self
                        .legacy_materialised_leaf(scope, object)
                        .filter(|resolved| resolved != object.as_ref())?;
                    let next = TypeExpr::IndexedAccess {
                        object: std::sync::Arc::new(resolved_inner),
                        index: index.clone(),
                    };
                    self.legacy_enumerate_member_surface_keys_via_route(
                        scope,
                        &next,
                        member_name,
                        depth + 1,
                    )
                }
                _ => None,
            },
            _ => None,
        }
    }
}
