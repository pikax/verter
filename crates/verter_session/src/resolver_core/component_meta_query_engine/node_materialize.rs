//! Node-domain registry-candidate materialisation for
//! `ComponentMetaQueryEngine<'a>`: the whole-surface / owner-local-generic-alias /
//! routed-member / pick-member candidate builders that each return a published
//! `TypeExpr` PAIRED with the node-domain object-surface fact decided off the
//! PRODUCING node, so the host-side registry loop carries a precomputed fact
//! instead of inspecting the materialised value. Co-located with the node-domain
//! predicate mirrors (`component_meta_registry_node_has_explicit_object_surface`,
//! `node_raises_to_object_surface`,
//! `component_meta_registry_node_has_non_object_top_level_surface`,
//! `node_is_indexed_access_shell`) and the `RegistryMemberSurface` carrier they
//! decide.
//!
//! The candidate builders extend the engine in a sibling `impl<'a>` block; the
//! predicates are free fns the builders reuse. Member surfaces materialise through
//! the first-pass `MaterializeStructureDb` node (`materialize_member_surface_to_node`)
//! and, where stabilisable, REDUCE that node through the `ShapeCacheDb` member-node
//! slot — never a raise-then-re-lower of a materialised value to recover facts.
use rustc_hash::FxHashSet;
use verter_type_expr::TypeExpr;

use super::surface::projected_surface_to_type_expr;
use super::ComponentMetaQueryEngine;
use crate::project_semantic_dispatch::raise::node_raised_shape_facts_with_dispatch;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    ProjectionMode, QueryResult, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput,
};

impl<'a> ComponentMetaQueryEngine<'a> {
    // ===================================================================
    // Node-first registry-candidate materialisation siblings.
    //
    // Each returns the published surface PAIRED with the node-domain
    // object-surface fact, decided off the PRODUCING node — so the host-side
    // registry loop carries a precomputed fact instead of inspecting the
    // materialised value. Member surfaces materialise through the first-pass
    // `MaterializeStructureDb` node (`materialize_member_surface_to_node`) and,
    // where the old path stabilised, REDUCE that node through the `ShapeCacheDb`
    // member-node slot (the node-first second pass) — never a raise-then-re-lower
    // of a materialised value to recover facts.
    // ===================================================================

    /// Whole-surface registry candidate for `symbol` in `scope`: the node-domain
    /// root-surface authority. Projects the symbol's whole surface (the
    /// budget-gated `dispatch_projected_surface_with_node`), returns its `TypeExpr`
    /// plus the producing node's object-surface fact.
    pub(crate) fn materialize_registry_whole_surface_candidate(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<(TypeExpr, bool)> {
        if self.projection_op_budget_exhausted() {
            return None;
        }
        let (surface, node) =
            self.dispatch_projected_surface_with_node(scope_canonical_id, symbol_name)?;
        let type_expr = projected_surface_to_type_expr(&surface)?;
        let is_object = component_meta_registry_node_has_explicit_object_surface(self.ctx, node);
        Some((type_expr, is_object))
    }

    /// Owner-local generic-alias substituted registry candidate: the relocated body
    /// of the former host-side `owner_local_generic_alias_substituted_body_via_dispatch`.
    /// Lowers the generic ref (Navigate), gates on the owner-local
    /// `InstantiationRef` carrier + the prepared-decl reach constraints, runs the
    /// shared `Instantiate` query, and gates the result NODE on raising EXACTLY to
    /// an object surface (the node-domain replacement for
    /// `matches!(raised, TypeExpr::Object(_))`) before materialising it ONCE.
    pub(crate) fn owner_local_generic_alias_candidate(
        &mut self,
        scope_canonical_id: &str,
        raw: &TypeExpr,
    ) -> Option<(TypeExpr, bool)> {
        use crate::project_semantic_dispatch::node_data_for;
        use crate::semantic_query::{ProjectionReductionContext, SemanticNodeData};

        let TypeExpr::Ref { type_arguments, .. } = raw else {
            return None;
        };
        if type_arguments.is_empty() {
            return None;
        }
        let ctx = self.ctx;
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let navigate_context =
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
        let lowered = dispatch.lower_type_expr_in_scope_with_context(
            scope_canonical_id,
            raw,
            navigate_context,
        )?;
        let lowered_data = node_data_for(ctx, lowered)?;
        let SemanticNodeData::InstantiationRef { base, args } = lowered_data.as_ref() else {
            return None;
        };
        if base.canonical_id.as_ref() != scope_canonical_id {
            return None;
        }
        let prepared =
            self.prepared_type_decl(base.canonical_id.as_ref(), base.decl_name.as_ref())?;
        if prepared.type_parameters.len() < args.len() {
            return None;
        }
        if !matches!(prepared.body, TypeExpr::Object(_)) {
            return None;
        }
        let instantiate_prc =
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
        let node = match dispatch.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                dispatch.type_slot_for(
                    std::sync::Arc::clone(&base.canonical_id),
                    std::sync::Arc::clone(&base.decl_name),
                ),
                std::sync::Arc::clone(args),
                dispatch.instantiate_context_for(&base.canonical_id, instantiate_prc),
            ),
        )) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
            QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
        };
        if !node_raises_to_object_surface(ctx, node) {
            return None;
        }
        // `node_raises_to_object_surface` proves an OBJECT root only — a weaker
        // structural check than a route/surface adapter's
        // `materialized && expanded_surface` admission gate — so this instantiated
        // node is NOT a route-admitted node. Materialise it through the
        // no-admission-claim `RegistryPublicationNode` carrier +
        // `materialize_registry_publication_node` sink (the shared registry
        // publication helper), never by forging `AdmittedRouteProjectionNode` (whose
        // contract asserts the passed route-admission gate).
        let type_expr = materialize_member_node_to_type_expr(ctx, node)?;
        Some((type_expr, true))
    }

    /// Routed registry MEMBER surface (the per-member arm of a `Pick<…>` / member
    /// path route): project `route_expr` to its surface NODE through the shared
    /// class-A node dispatch, materialise its structure to the first-pass node, then
    /// reduce that node ONCE through the `ShapeCacheDb` member-node stabiliser. The
    /// no-poison selection is decided in the stabiliser on node-domain
    /// `!RaisedShapeFacts.materialized` facts. Returns the chosen value paired with
    /// its object-surface fact, carried untainted.
    pub(crate) fn materialize_registry_routed_member_surface(
        &mut self,
        scope_canonical_id: &str,
        route_expr: &TypeExpr,
    ) -> RegistryMemberSurface {
        let ctx = self.ctx;
        let projected = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
            ctx,
            Some(self),
            scope_canonical_id,
            route_expr,
        );
        let base_node = match projected {
            Some(admitted) => Some(admitted.node()),
            None => {
                let dispatch = ProjectSemanticDispatch::new(ctx);
                dispatch.lower_type_expr_in_scope_with_mode(
                    scope_canonical_id,
                    route_expr,
                    ProjectionMode::Navigate,
                )
            }
        };
        let first_node = base_node.and_then(|base| {
            self.materialize_member_surface_to_node(scope_canonical_id, base, true)
        });
        let Some(first_node) = first_node else {
            // Neither projection nor a raw lowering yields a base, or the structure
            // materialisation errored: best-effort single-pass route surface.
            return self.materialize_registry_member_value(scope_canonical_id, route_expr);
        };
        let stabilized = crate::meta_resolve::materialize::stabilize_registry_member_surface_node_with_shape_cache(
            ctx,
            scope_canonical_id,
            first_node,
            ProjectionMode::Navigate,
        );
        self.registry_member_surface_from_stabilized(stabilized)
    }

    /// Unwrap a [`crate::meta_resolve::materialize::RegistryMemberStabilizedValue`]
    /// into the published value + its object-surface fact: raise the chosen NODE
    /// (the first-pass node for `First`, the stabilised node for `Stable`) ONCE at
    /// the registered terminal sink, and read the object-surface fact off that SAME
    /// node — no decision rides the raised value.
    fn registry_member_surface_from_stabilized(
        &self,
        stabilized: crate::meta_resolve::materialize::RegistryMemberStabilizedValue,
    ) -> RegistryMemberSurface {
        use crate::meta_resolve::materialize::RegistryMemberStabilizedValue;
        let ctx = self.ctx;
        let node = match stabilized {
            RegistryMemberStabilizedValue::First { node }
            | RegistryMemberStabilizedValue::Stable { node } => node,
        };
        let value = materialize_member_node_to_type_expr(ctx, node).unwrap_or_else(|| {
            TypeExpr::Object(std::sync::Arc::new(verter_type_expr::ObjectExpr {
                properties: Vec::new(),
            }))
        });
        let explicit_object_surface =
            component_meta_registry_node_has_explicit_object_surface(ctx, node);
        RegistryMemberSurface {
            value,
            explicit_object_surface,
        }
    }

    /// Single-pass registry member value: lower `expr` (Navigate), materialise its
    /// structure to the first-pass node, raise it, and pair it with its
    /// object-surface fact (off the first-pass node). The Pick callable-descent-skip
    /// path projects a package-backed raw leaf directly through this (no route
    /// re-projection / stabilisation), and the routed sibling reuses it as the
    /// best-effort fallback.
    pub(crate) fn materialize_registry_member_value(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> RegistryMemberSurface {
        let ctx = self.ctx;
        let base = {
            let dispatch = ProjectSemanticDispatch::new(ctx);
            dispatch.lower_type_expr_in_scope_with_mode(
                scope_canonical_id,
                expr,
                ProjectionMode::Navigate,
            )
        };
        match base
            .and_then(|b| self.materialize_member_surface_to_node(scope_canonical_id, b, true))
        {
            Some(first_node) => {
                let value = materialize_member_node_to_type_expr(ctx, first_node)
                    .unwrap_or_else(|| expr.clone());
                let explicit_object_surface =
                    component_meta_registry_node_has_explicit_object_surface(ctx, first_node);
                RegistryMemberSurface {
                    value,
                    explicit_object_surface,
                }
            }
            None => RegistryMemberSurface {
                value: expr.clone(),
                explicit_object_surface: false,
            },
        }
    }

    /// Builtin-Pick registry candidate fallback through the shared `Pick<base, keys>`
    /// dispatch (the same single-dispatch path as [`Self::materialize_pick_member_surface`]):
    /// resolve the Pick result node, materialise its structure to the first-pass
    /// node, raise it, and pair it with the producing node's object-surface fact.
    pub(crate) fn materialize_pick_member_surface_candidate(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        members: &[String],
    ) -> Option<RegistryMemberSurface> {
        let pick_node = {
            let dispatch = ProjectSemanticDispatch::new(self.ctx);
            let symbol_ref = TypeExpr::Ref {
                name: std::sync::Arc::from(root_symbol),
                type_arguments: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            };
            let base = dispatch.lower_type_expr_in_scope_with_mode(
                scope_canonical_id,
                &symbol_ref,
                ProjectionMode::Navigate,
            )?;
            let members_arc: Vec<std::sync::Arc<str>> = members
                .iter()
                .map(|s| std::sync::Arc::from(s.as_str()))
                .collect();
            match dispatch.execute_pick(base, &members_arc, ProjectionMode::Expanded) {
                QueryResult::Value(id) => id,
                QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
            }
        };
        let first_node =
            self.materialize_member_surface_to_node(scope_canonical_id, pick_node, true)?;
        let value = materialize_member_node_to_type_expr(self.ctx, first_node)?;
        let explicit_object_surface =
            component_meta_registry_node_has_explicit_object_surface(self.ctx, first_node);
        Some(RegistryMemberSurface {
            value,
            explicit_object_surface,
        })
    }

    /// Project a member-path route expression to its leaf value PLUS the reject/accept
    /// facts the registry member-path arm decides on (`explicit_object_surface` /
    /// `non_object_top_level_surface` / `is_indexed_access_shell`). The leaf is the
    /// former `project_expr_class_a_via_dispatch(...).unwrap_or(route_expr)`; the three
    /// facts replace the host-side `matches!` / `has_*_surface` decisions on the former
    /// materialised leaf, computed PER BRANCH exactly as the old host path:
    /// - projection SUCCEEDED ⇒ off the leaf's projected NODE (node-domain facts on the
    ///   admitted node, equal to the `TypeExpr` facts on its materialised value); and
    /// - projection FAILED ⇒ on the RAW `route_expr` via the `TypeExpr` predicates (the
    ///   former `.unwrap_or(route_expr)` leaf), NEVER off `lower(route_expr, Navigate)`,
    ///   whose reduction would diverge from the raw-leaf facts.
    pub(crate) fn project_member_path_leaf_facts(
        &mut self,
        scope_canonical_id: &str,
        route_expr: &TypeExpr,
    ) -> (TypeExpr, bool, bool, bool) {
        let ctx = self.ctx;
        // The member-path leaf mirrors the NON-threaded `project_expr_class_a_via_dispatch`
        // (engine = None / transient), so the node projection threads no engine.
        let projected = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
            ctx,
            None,
            scope_canonical_id,
            route_expr,
        );
        match projected {
            Some(admitted) => {
                let node = admitted.node();
                let is_object = component_meta_registry_node_has_explicit_object_surface(ctx, node);
                let non_object_top =
                    component_meta_registry_node_has_non_object_top_level_surface(ctx, node);
                let is_indexed = node_is_indexed_access_shell(ctx, node);
                let leaf = super::surface::materialize_route_projection_node(ctx, &admitted)
                    .unwrap_or_else(|| route_expr.clone());
                (leaf, is_object, non_object_top, is_indexed)
            }
            None => {
                // Projection failed: the leaf IS the raw `route_expr` (the original
                // `project_expr_class_a_via_dispatch(...).unwrap_or(route_expr)`), so
                // the reject/accept facts are computed on the RAW `route_expr` via the
                // `TypeExpr` predicates — exactly as the former host-side path did on
                // `…unwrap_or(route_expr)`. They MUST NOT be read off
                // `lower(route_expr, Navigate)`: `Navigate` lowering can REDUCE a
                // nested `IndexedAccess` (`Symbol['a']['b']`) to an `Object`/`Ref`/
                // `Conditional`, flipping `is_indexed=false`/`is_object=true` so the
                // host's `path.len() > 1` member-path arm would PROCEED and publish a
                // member the raw-`route_expr` facts (a non-object `IndexedAccess` leaf)
                // reject. The raw-`route_expr` TypeExpr facts have no reduction, so
                // they match the OLD behaviour byte-for-byte.
                use crate::resolver_core::component_meta_registry::{
                    component_meta_registry_has_explicit_object_surface,
                    component_meta_registry_has_non_object_top_level_surface,
                };
                let is_object = component_meta_registry_has_explicit_object_surface(route_expr);
                let non_object_top =
                    component_meta_registry_has_non_object_top_level_surface(route_expr);
                let is_indexed = matches!(route_expr, TypeExpr::IndexedAccess { .. });
                (route_expr.clone(), is_object, non_object_top, is_indexed)
            }
        }
    }

    /// Refine an imported generic-alias Object surface member-by-member: the
    /// relocated body of the former host-side `maybe_refine_imported_generic_alias_object`
    /// closure. Each property re-projects `Ref{symbol}["<prop>"]` through the shared
    /// class-A node dispatch in the OWNER scope (keeping only a node-materialised
    /// projection with no semantic miss), raises it so the alias body's helper
    /// carriers re-resolve in the DEFINING scope, then materialises + stabilises it
    /// (node-domain no-poison). Returns the refined Object (a transformer — `source`
    /// is returned unchanged when it is not an Object).
    pub(crate) fn refine_imported_generic_alias_object_surface(
        &mut self,
        owner_scope: &str,
        materialize_scope: &str,
        symbol_name: &str,
        source: &TypeExpr,
    ) -> TypeExpr {
        use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty};
        let TypeExpr::Object(object) = source else {
            return source.clone();
        };
        let ctx = self.ctx;
        let mut properties: Vec<ObjectMember> = Vec::with_capacity(object.properties.len());
        for member in object.properties.iter() {
            let ObjectMember::Property(property) = member else {
                properties.push(member.clone());
                continue;
            };
            let route_expr =
                registry_indexed_access_expr(symbol_name, std::slice::from_ref(&property.name));
            // OWNER-scope projection NODE, kept only when it carries no semantic
            // miss (node fact, mirroring the former `.filter(!contains_semantic_miss)`).
            let projected = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
                ctx,
                Some(self),
                owner_scope,
                &route_expr,
            );
            let refine_input: TypeExpr = match projected {
                Some(admitted) => {
                    let node = admitted.node();
                    let has_miss = node_raised_shape_facts_with_dispatch(
                        &ProjectSemanticDispatch::new(ctx),
                        node,
                    )
                    .map(|facts| !facts.materialized())
                    .unwrap_or(false);
                    if has_miss {
                        property.ty.clone()
                    } else {
                        // Raise the owner-scope projection (= the former
                        // `project_class_a` `typed_projected`) through the registered
                        // sink so the defining-scope pass re-resolves the alias body's
                        // helper carriers.
                        materialize_member_node_to_type_expr(ctx, node)
                            .unwrap_or_else(|| property.ty.clone())
                    }
                }
                None => property.ty.clone(),
            };
            // Pass 1 + pass 2 in the DEFINING (materialize) scope — the remaining
            // carriers are the alias body's helper references, which resolve there.
            let base = {
                let dispatch = ProjectSemanticDispatch::new(ctx);
                dispatch.lower_type_expr_in_scope_with_mode(
                    materialize_scope,
                    &refine_input,
                    ProjectionMode::Navigate,
                )
            };
            let ty = match base
                .and_then(|b| self.materialize_member_surface_to_node(materialize_scope, b, true))
            {
                Some(first_node) => {
                    let stabilized =
                        crate::meta_resolve::materialize::stabilize_registry_member_surface_node_with_shape_cache(
                            ctx,
                            materialize_scope,
                            first_node,
                            ProjectionMode::Navigate,
                        );
                    self.registry_member_surface_from_stabilized(stabilized)
                        .value
                }
                None => refine_input,
            };
            properties.push(ObjectMember::Property(ObjectProperty::with_visibility(
                property.name.clone(),
                ty,
                property.optional,
                property.readonly,
                property.visibility,
                property.spans,
            )));
        }
        TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
    }
}

/// Module-private registry member-surface carrier: a materialised member value
/// PAIRED with its node-domain object-surface fact (decided off the producing
/// node). The ONLY value-bearing carrier crossing toward `host_manage` — no raw
/// `SemanticNodeId` / `AdmittedRouteProjectionNode` leaves the query engine.
pub(crate) struct RegistryMemberSurface {
    pub(crate) value: TypeExpr,
    pub(crate) explicit_object_surface: bool,
}

/// Build the registry indexed-access route expression `symbol['p0']['p1']…` from a
/// member-name path — the module-local node-engine copy of the host-side
/// `build_registry_indexed_access_expr` (pure `TypeExpr` construction).
fn registry_indexed_access_expr(symbol_name: &str, path: &[String]) -> TypeExpr {
    path.iter()
        .fold(TypeExpr::named(symbol_name), |object, member| {
            TypeExpr::IndexedAccess {
                object: std::sync::Arc::new(object),
                index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
            }
        })
}

/// Whole-tree EXISTENTIAL arm-set predicate (registry candidate selection): does
/// ANY node in the `Alias` / `Union` / `Intersection` frontier carry an
/// object-surface-bearing kind — an `Object`, a `MergedDecl`, or a
/// `VueMacroElements`? Read off the producing node so a candidate's
/// object-surface fact is decided in node domain instead of inspecting the
/// materialised `TypeExpr`.
///
/// NOT a normalized-root mirror and NOT a `predicate(raise(node))` parity
/// guarantee — it is DELIBERATELY a structural arm-set scan, distinct from the
/// root-shape classifiers ([`node_raises_to_object_surface`] et al.). It can
/// diverge from the folded root: a `VueMacroElements` arm counts here (a
/// structural object-surface candidate) even though it folds to a non-`Object`
/// placeholder root, and an arm the intersection fold would DROP still counts.
pub(crate) fn component_meta_registry_node_has_explicit_object_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    // Visited-set work-stack over the object-surface frontier (the `Alias` identity
    // hop plus `Union` / `Intersection` arms). The interned arena is append-only and
    // every edge references a strictly-smaller id, so it is ACYCLIC — a node-graph
    // cycle is not constructible. The `visited` set therefore provides DAG-dedup (a
    // shared-subtree DAG is walked linearly, not exponentially) and uncapped depth
    // (an acyclic chain of ANY depth is fully walked, matching the uncapped
    // `TypeExpr` predicate) — NOT cycle-termination. A data-less frame contributes
    // `false` and is dropped.
    let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
    let mut stack: Vec<SemanticNodeId> = vec![node];
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::Object(_)
            | SemanticNodeData::MergedDecl { .. }
            | SemanticNodeData::VueMacroElements(_) => return true,
            SemanticNodeData::Alias(inner) => stack.push(*inner),
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                stack.extend(arms.iter().copied());
            }
            _ => {}
        }
    }
    false
}

/// Whether `node` raises EXACTLY to a `TypeExpr::Object` — the node-domain mirror
/// of `matches!(raise(node), TypeExpr::Object(_))`. An `Object` / representable
/// empty object / `MergedDecl` all fold to a plain `Object` root; a
/// `VueMacroElements` carrier does NOT (it folds to the
/// `VueMacroElementsPlaceholder` sentinel, root `Other`). Unlike
/// [`component_meta_registry_node_has_explicit_object_surface`], a `Union` /
/// `Intersection` root (which raises to a `Union` / `Intersection`, not a plain
/// `Object`) is NOT an object root here.
///
/// Reads the POST-NORMALIZED raised root through the shared shape-engine fold
/// (`node_root_is_object_surface_with_dispatch`) — the SAME fold that drops the
/// Intersection sentinel / empty-object arms and peels the `Alias` hops — so the
/// answer equals `matches!(raise(node), TypeExpr::Object(_))` BY CONSTRUCTION,
/// including for shapes the former raw-node walk mis-classified (e.g.
/// `Intersection([{}, Object])`, raw ⇒ false but collapsed by the fold to its
/// `Object` arm ⇒ true).
pub(crate) fn node_raises_to_object_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    use crate::project_semantic_dispatch::raise::node_root_is_object_surface_with_dispatch;
    let dispatch = ProjectSemanticDispatch::new(ctx);
    node_root_is_object_surface_with_dispatch(&dispatch, node)
}

/// Whole-tree EXISTENTIAL arm-set predicate (registry candidate selection): does
/// ANY node in the `Alias` / `Union` / `Intersection` frontier carry a
/// non-object top-level kind — a `Ref`-carrier (`DeclRef` / `InstantiationRef` /
/// `BareRef`) / `IndexedAccess` / `Conditional` / `Mapped` — OR does any
/// `Union` / `Intersection` arm not reduce to a plain object (`KeyOf` / `TypeOf`
/// and primitives / objects do NOT qualify)?
///
/// NOT a normalized-root mirror and NOT a `predicate(raise(node))` parity
/// guarantee — it is DELIBERATELY a structural arm-set scan, distinct from the
/// root-shape classifiers. It can diverge from the folded root for a dropped-arm
/// intersection (an arm the fold would collapse away still counts here).
pub(crate) fn component_meta_registry_node_has_non_object_top_level_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    // Visited-set work-stack over the non-object top-level frontier (the `Alias` hop
    // plus `Union` / `Intersection` arms). A `Union` / `Intersection` qualifies when
    // any arm does NOT raise to a plain object OR any arm recursively qualifies; the
    // two disjuncts are pure boolean reads, so checking the arm-raise disjunct in
    // place and deferring the recursive disjunct onto the stack yields the same
    // verdict as the short-circuit `||`. The interned arena is append-only and every
    // edge references a strictly-smaller id, so it is ACYCLIC — a node-graph cycle is
    // not constructible. The `visited` set therefore provides DAG-dedup (a
    // shared-subtree DAG is walked linearly) and uncapped depth (an acyclic chain of
    // ANY depth is walked, matching the uncapped `TypeExpr` predicate) — NOT
    // cycle-termination.
    let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
    let mut stack: Vec<SemanticNodeId> = vec![node];
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::Alias(inner) => stack.push(*inner),
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                if arms
                    .iter()
                    .any(|arm| !node_raises_to_object_surface(ctx, *arm))
                {
                    return true;
                }
                stack.extend(arms.iter().copied());
            }
            SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::InstantiationRef { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::Mapped { .. } => return true,
            _ => {}
        }
    }
    false
}

/// Whether `node` raises to a deferred `IndexedAccess` shell — the node-domain
/// mirror of `matches!(raise(node), TypeExpr::IndexedAccess { .. })`.
///
/// Reads the POST-NORMALIZED raised root through the shared shape-engine fold
/// (`node_root_is_indexed_access_with_dispatch`) — the SAME fold that drops the
/// Intersection sentinel / empty-object arms and peels the `Alias` hops — so the
/// answer equals the `TypeExpr` predicate on the raised leaf BY CONSTRUCTION,
/// including for shapes a raw top-level data check mis-classified (e.g.
/// `Intersection([{}, IndexedAccess])`, raw top-level ⇒ `Intersection` ⇒ false but
/// collapsed by the fold to its `IndexedAccess` arm ⇒ true).
pub(crate) fn node_is_indexed_access_shell(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    use crate::project_semantic_dispatch::raise::node_root_is_indexed_access_with_dispatch;
    let dispatch = ProjectSemanticDispatch::new(ctx);
    node_root_is_indexed_access_with_dispatch(&dispatch, node)
}

/// Raise a member-surface NODE to its published `TypeExpr` through the registered
/// `materialize_registry_publication_node` terminal sink — the materialisation happens
/// INSIDE the sink (`materialize_published_node`), so the registry siblings hold no
/// `into_type_expr` / `materialize_output_type_expr` mint of their own. The object
/// fact is read off the node separately, so no semantic decision rides the raised
/// value.
///
/// `node` is the first-pass / stabilised member-surface node the candidate path holds
/// directly — an arbitrary outcome (`Miss` / `Recursive` / `Tainted` / a degenerate
/// reduce), NOT a route/surface adapter's admitted node. So it is wrapped in the
/// no-admission-claim [`RegistryPublicationNode`] carrier, NOT
/// [`AdmittedRouteProjectionNode`] (whose contract asserts the node passed a
/// `materialized && expanded_surface` acceptance gate); forging that carrier here
/// would break the admitted-carrier invariant for an un-admitted node.
fn materialize_member_node_to_type_expr(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> Option<TypeExpr> {
    super::surface::materialize_registry_publication_node(
        ctx,
        &super::surface::RegistryPublicationNode::new(node),
    )
}

#[cfg(test)]
#[path = "node_materialize_predicate_tests.rs"]
mod node_predicate_parity_tests;
