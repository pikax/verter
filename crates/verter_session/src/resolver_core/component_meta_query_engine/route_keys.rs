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
    ///    ([`super::surface::enumerate_keyspace_names_from_admitted_node`] →
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
            chain_scopes: self.projection_chain_scopes.clone(),
        };
        // PRIMARY: lower the key-source and enumerate its keyspace.
        if let Some(node) = self.lower_route_key_source_node(&context, expr) {
            if let Some(keys) =
                super::surface::enumerate_keyspace_names_from_admitted_node(self.ctx(), &node)
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

    /// Lower a route-key SOURCE expression to a keyspace NODE under the route-key
    /// [`PreparedProjectionContext`], for the shared dispatch keyspace enumerator
    /// (which re-evaluates the node under structural-transit + the `KeyOf`
    /// producer). No surface gate: a `keyof` / `IndexedAccess` keyspace carrier is
    /// preserved so the enumerator can reduce it.
    ///
    /// - split-scope generic `Ref` (`decl_scope != arg_scope`, non-empty chain):
    ///   resolve the NAME in `decl_scope`, lower the args in `arg_scope` (chain
    ///   fallback for `typeof`), dispatch `Instantiate` with NODE args.
    /// - otherwise: lower in `arg_scope` at `Expanded`.
    fn lower_route_key_source_node(
        &mut self,
        context: &PreparedProjectionContext,
        expr: &TypeExpr,
    ) -> Option<AdmittedRouteProjectionNode> {
        if context.decl_scope != context.arg_scope && !context.chain_scopes.is_empty() {
            if let TypeExpr::Ref {
                name,
                type_arguments,
            } = strip_parens_expr(expr)
            {
                if !type_arguments.is_empty() {
                    if let Some(node) = self.instantiate_split_scope_ref(
                        &context.decl_scope,
                        &context.arg_scope,
                        &context.chain_scopes,
                        name.as_ref(),
                        type_arguments,
                    ) {
                        return Some(node);
                    }
                }
            }
        }
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
        let lowered = dispatch.lower_type_expr_in_scope_with_mode(
            &context.arg_scope,
            expr,
            crate::semantic_query::ProjectionMode::Expanded,
        )?;
        Some(AdmittedRouteProjectionNode::new(lowered))
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
    ///   then `decl_scope`, then the outer `chain_scopes` (the value reference
    ///   may be visible only in an outer helper scope).
    /// - `Ref { name, [args..] }`: resolve the NAME in `decl_scope`, lower
    ///   `type_arguments` in `arg_scope` (chain fallback for `typeof`), then
    ///   dispatch `Instantiate` with NODE args
    ///   ([`Self::instantiate_split_scope_ref`]) — no reconstructed `TypeExpr`.
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
        let chain_scopes = context.chain_scopes.clone();

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
                // `typeof value_ref`: caller-scoped first, then the decl scope,
                // then the outer declaration chain scopes.
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
                for chain_scope in &chain_scopes {
                    if chain_scope == &decl_scope || chain_scope == &arg_scope {
                        continue;
                    }
                    if let Some(leaf) = self.stabilise_route_leaf_node(chain_scope, expr) {
                        if !leaf.eq_to_input {
                            return Some(leaf);
                        }
                    }
                }
                arg_first
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
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
                // Split-scope NODE instantiation: the ref NAME belongs to one
                // scope and a `typeof` argument to another. Resolve the name in
                // decl_scope, lower each arg in arg_scope (chain fallback for
                // `typeof`), then dispatch `Instantiate` with NODE args — no
                // reconstructed `TypeExpr::Ref`.
                if !chain_scopes.is_empty() {
                    if let Some(node) = self.instantiate_split_scope_ref(
                        &decl_scope,
                        &arg_scope,
                        &chain_scopes,
                        name.as_ref(),
                        type_arguments,
                    ) {
                        let eq_to_input =
                            super::route_projection_node_eq_to_expr(self.ctx(), &node, expr);
                        return Some(StableRouteLeafNode { node, eq_to_input });
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

    /// Split-scope generic-`Ref` instantiation: resolve the ref NAME in
    /// `decl_scope`, lower each `type_argument` in `arg_scope` (chain-scope
    /// fallback for `typeof` args), then dispatch `Instantiate` with NODE args
    /// ([`super::surface::instantiate_split_scope_route_node`]). Returns the
    /// admitted node, or `None` when the name does not resolve, an argument
    /// cannot lower, or the instantiation gate rejects. Shared by the keyspace
    /// lowering ([`Self::lower_route_key_source_node`]) and the leaf stabiliser's
    /// `Ref { name, [args..] }` scope-dispatch arm.
    fn instantiate_split_scope_ref(
        &mut self,
        decl_scope: &str,
        arg_scope: &str,
        chain_scopes: &[String],
        name: &str,
        type_arguments: &[TypeExpr],
    ) -> Option<AdmittedRouteProjectionNode> {
        let declaration = self.resolve_type_declaration(decl_scope, name);
        let target_canonical = if declaration.canonical_source.is_empty() {
            decl_scope.to_string()
        } else {
            declaration.canonical_source.clone()
        };
        let resolved_name = if declaration.resolved_name.is_empty() {
            name.to_string()
        } else {
            declaration.resolved_name.clone()
        };
        let mut arg_nodes = Vec::with_capacity(type_arguments.len());
        for arg in type_arguments {
            arg_nodes.push(self.lower_route_arg_with_chain_fallback(
                arg_scope,
                decl_scope,
                chain_scopes,
                arg,
            )?);
        }
        super::surface::instantiate_split_scope_route_node(
            self.ctx(),
            &target_canonical,
            &resolved_name,
            &arg_nodes,
        )
    }

    /// Lower one split-scope type argument to a NODE: in `arg_scope` first; for
    /// a `typeof` argument whose `arg_scope` lowering leaves the shape unchanged
    /// (the value is not visible there), retry in each outer `chain_scope` until
    /// one resolves it. The node-domain replacement for the former
    /// `solve_or_project_leaf_expr_until_stable(chain_scope, arg)` pre-resolution
    /// that fed a reconstructed `TypeExpr::Ref`.
    fn lower_route_arg_with_chain_fallback(
        &mut self,
        arg_scope: &str,
        decl_scope: &str,
        chain_scopes: &[String],
        arg: &TypeExpr,
    ) -> Option<crate::semantic_query::SemanticNodeId> {
        use crate::project_semantic_dispatch::raise::node_raised_shape_for_eq_with_dispatch;
        use crate::project_semantic_dispatch::ProjectSemanticDispatch;
        use crate::semantic_query::ProjectionMode;

        let dispatch = ProjectSemanticDispatch::new(self.ctx);
        let lowered = dispatch.lower_type_expr_in_scope_with_mode(
            arg_scope,
            arg,
            ProjectionMode::Expanded,
        )?;
        // Only a `typeof` argument can need an outer scope: an arg_scope lowering
        // that raises to the SAME shape as the input arg did not resolve the
        // value there, so retry in each chain scope.
        if matches!(arg, TypeExpr::TypeOf(_))
            && node_raised_shape_for_eq_with_dispatch(&dispatch, lowered, arg)
                .is_none_or(|shape| shape.eq_to_expr)
        {
            for chain_scope in chain_scopes {
                if chain_scope == arg_scope || chain_scope == decl_scope {
                    continue;
                }
                if let Some(chain_node) = dispatch.lower_type_expr_in_scope_with_mode(
                    chain_scope,
                    arg,
                    ProjectionMode::Expanded,
                ) {
                    if node_raised_shape_for_eq_with_dispatch(&dispatch, chain_node, arg)
                        .is_some_and(|shape| !shape.eq_to_expr)
                    {
                        return Some(chain_node);
                    }
                }
            }
        }
        Some(lowered)
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
