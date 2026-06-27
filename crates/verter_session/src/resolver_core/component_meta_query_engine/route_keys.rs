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

use super::helpers::{is_builtin_name, projected_surface_member_names, strip_parens_expr};
use super::{AdmittedRouteProjectionNode, ComponentMetaQueryEngine, PreparedProjectionContext};

/// The route fixpoint's cursor. The FIRST iteration projects the input
/// `&TypeExpr`; every later iteration re-projects the prior admitted node
/// directly (node-base re-projection, no re-lowering / no materialisation), so
/// the fixpoint stabilises on interned raised-shape identity with the sole
/// publication materialisation happening once after convergence.
#[derive(Clone, Copy)]
enum RouteFixpointCursor<'cursor> {
    Input(&'cursor TypeExpr),
    Node(AdmittedRouteProjectionNode),
}

impl<'a> ComponentMetaQueryEngine<'a> {
    /// Enumerate the literal string keys named by a `Pick`/`Omit`
    /// `keys` type argument (used by [`Self::project_direct_utility_surface_shape`]).
    /// Resolves `Ref` / `KeyOf` / indexed-access key sources through the
    /// dispatch-backed leaf stabiliser ([`Self::solve_or_project_prepared_member_leaf_expr`]).
    pub(super) fn enumerate_route_literal_keys(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<Vec<String>> {
        self.enumerate_route_literal_keys_inner(
            resolution_scope_canonical_id,
            active_scope_canonical_id,
            expr,
            0,
        )
    }

    fn enumerate_route_literal_keys_inner(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
        depth: usize,
    ) -> Option<Vec<String>> {
        use verter_type_expr::{LiteralValue, TypeExpr};

        if depth >= 4 {
            return None;
        }

        match expr {
            TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
            TypeExpr::Union(types) => {
                let mut keys = Vec::new();
                for ty in types.iter() {
                    keys.extend(self.enumerate_route_literal_keys_inner(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        ty,
                        depth + 1,
                    )?);
                }
                keys.sort();
                keys.dedup();
                Some(keys)
            }
            TypeExpr::Parenthesized(inner) => self.enumerate_route_literal_keys_inner(
                resolution_scope_canonical_id,
                active_scope_canonical_id,
                inner,
                depth + 1,
            ),
            TypeExpr::KeyOf(inner) => {
                if let TypeExpr::IndexedAccess { object, index } = inner.as_ref() {
                    if let TypeExpr::Literal(LiteralValue::String(member_name)) = index.as_ref() {
                        if let Some(keys) = self.enumerate_member_surface_keys_via_route(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            object,
                            member_name,
                            depth + 1,
                        ) {
                            return Some(keys);
                        }
                    }
                }

                if let Some(projected_expr) = self
                    .solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        expr,
                    )
                    .filter(|projected| projected != expr)
                {
                    return self.enumerate_route_literal_keys_inner(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &projected_expr,
                        depth + 1,
                    );
                }

                let projected_inner = self
                    .solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        inner,
                    )
                    .unwrap_or_else(|| inner.as_ref().clone());
                if let Some(keys) = projected_surface_member_names(&projected_inner) {
                    return Some(keys);
                }

                match &projected_inner {
                    TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                        // Accumulate enumerable arms only — `keyof (A
                        // & B)` returns the union of enumerable keys
                        // across A and B and fails only when EVERY
                        // arm is unresolvable. Mirrors the
                        // SemanticNode-level Intersection accumulation
                        // contract in `key_names_from_base_node`; an
                        // all-or-nothing `?` here would lose
                        // enumerable keys from `A` when `B` is
                        // unresolvable.
                        let mut keys = Vec::new();
                        let mut any_enumerable = false;
                        for part in parts.iter() {
                            if let Some(arm_keys) = self.enumerate_route_literal_keys_inner(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                &TypeExpr::KeyOf(std::sync::Arc::new(part.clone())),
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
                    _ => None,
                }
            }
            _ => {
                let projected = self.solve_or_project_prepared_member_leaf_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    expr,
                )?;
                if projected == *expr {
                    crate::resolver_core::component_meta_registry::component_meta_registry_string_literal_keys(
                        &projected,
                    )
                } else {
                    self.enumerate_route_literal_keys_inner(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &projected,
                        depth + 1,
                    )
                }
            }
        }
    }

    /// Enumerate the literal keys of a `keyof X['member']` route source
    /// for `Pick`/`Omit` key resolution. Resolves the member's surface
    /// through the dispatch-backed leaf stabiliser
    /// ([`Self::solve_or_project_prepared_member_leaf_expr`]) and walks
    /// conditional / intersection / union arms, accumulating enumerable
    /// keys. Used by [`Self::enumerate_route_literal_keys_inner`].
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn enumerate_member_surface_keys_via_route(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
        member_name: &str,
        depth: usize,
    ) -> Option<Vec<String>> {
        use verter_type_expr::ObjectMember;

        // Depth budget of 8 accommodates multi-step navigation
        // chains like
        // `(typeof theme & GetComponentAppConfig<AppConfig, "ui", "button">)['variants']['color']`
        // which require: (1) IndexedAccess(Intersection,..)
        // distribute → (2) IndexedAccess(Ref,..) expand alias →
        // (3) IndexedAccess(Conditional,..) distribute →
        // (4) IndexedAccess(IndexedAccess,..) recurse on inner →
        // and so on.
        if depth >= 8 {
            return None;
        }

        let projected_expr = self
            .solve_or_project_prepared_member_leaf_expr(
                resolution_scope_canonical_id,
                active_scope_canonical_id,
                expr,
            )
            .unwrap_or_else(|| expr.clone());
        // A leaf that the dispatch stabiliser left as `Unknown` is a clean
        // miss: the `match &projected_expr` below falls through to its
        // `_ => None` arm. (Generic-Ref instantiation of the route object
        // is owned by the dispatch leaf stabiliser + the `IndexedAccess`
        // arm's dispatch instantiation, not a structural-substitution
        // fallback.)

        match &projected_expr {
            TypeExpr::Object(object) => {
                // Public-keyspace member lookup: `keyof X['member']` reaches a
                // member's surface only when that member is on `X`'s PUBLIC
                // surface — TS rejects external indexed access of a
                // protected/private class member (`X['privateMember']` is an
                // error), exactly as `keyof X` excludes non-public members. A
                // non-public match is therefore a miss (the member is not on
                // the public surface this route-key enumeration derives from);
                // the full member set stays recorded on the source surface for
                // the keep-all `native_props` carrier.
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
                    .solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &member_ty,
                    )
                    .unwrap_or(member_ty);
                projected_surface_member_names(&projected_member)
            }
            TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                // Accumulate enumerable arms only — see the matching
                // accumulation in `enumerate_route_literal_keys_inner`.
                // `keyof (typeof theme & GetComponentAppConfig<...>)['variants']['color']`
                // must merge `theme.variants.color`'s keys with the
                // conditional's resolvable arm keys, even when a
                // deferred conditional arm cannot enumerate.
                let mut keys = Vec::new();
                let mut any_enumerable = false;
                for part in parts.iter() {
                    if let Some(arm_keys) = self.enumerate_member_surface_keys_via_route(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
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
                if let Some(true_keys) = self.enumerate_member_surface_keys_via_route(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    true_type,
                    member_name,
                    depth + 1,
                ) {
                    keys.extend(true_keys);
                }
                if let Some(false_keys) = self.enumerate_member_surface_keys_via_route(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    false_type,
                    member_name,
                    depth + 1,
                ) {
                    keys.extend(false_keys);
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
                // resolve the value root via the
                // dispatch-aligned bare-name resolver + ctx
                // prepared_value_decl directly. Mirrors `build_typeof`.
                let scope_payload = self.scope_payload_for_scope(active_scope_canonical_id);
                let root_name = value_ref.path.first()?;
                let root_identity =
                    crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                        self.ctx,
                        active_scope_canonical_id,
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
                    return self.enumerate_member_surface_keys_via_route(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &object_expr,
                        member_name,
                        depth + 1,
                    );
                }

                if let Some(type_annotation) = prepared_value.type_annotation.as_ref() {
                    return self.enumerate_member_surface_keys_via_route(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        type_annotation,
                        member_name,
                        depth + 1,
                    );
                }

                None
            }
            TypeExpr::Parenthesized(inner) => self.enumerate_member_surface_keys_via_route(
                resolution_scope_canonical_id,
                active_scope_canonical_id,
                inner,
                member_name,
                depth + 1,
            ),
            // Distribute member-name lookup over an `IndexedAccess`
            // whose object is compound or reducible. For
            // `(typeof theme & GetComponentAppConfig<...>)['variants']['color']`
            // we want `theme.variants.color`'s keys merged with the
            // conditional arm's `variants.color`'s keys; without this
            // distribution the catch-all would return `None` because
            // the dispatch cannot reduce the outer IndexedAccess to a
            // single concrete shape.
            //
            // Handles:
            // - object = Intersection / Union: distribute over arms.
            // - object = Conditional: distribute over true / false branches.
            // - object = Ref with type_arguments: expand the alias body
            //   and retry.
            // - object = nested IndexedAccess: recurse on inner before
            //   re-applying the outer index.
            TypeExpr::IndexedAccess { object, index } => {
                match object.as_ref() {
                    TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                        let parts = std::sync::Arc::clone(parts);
                        let mut keys = Vec::new();
                        let mut any_enumerable = false;
                        for arm in parts.iter() {
                            let arm_indexed = TypeExpr::IndexedAccess {
                                object: std::sync::Arc::new(arm.clone()),
                                index: index.clone(),
                            };
                            if let Some(arm_keys) = self.enumerate_member_surface_keys_via_route(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
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
                            if let Some(branch_keys) = self.enumerate_member_surface_keys_via_route(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                &branch_indexed,
                                member_name,
                                depth + 1,
                            ) {
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
                    TypeExpr::Ref {
                        name,
                        type_arguments,
                    } => {
                        // Try expanding the alias's body (substituting
                        // type arguments), then retry the indexed access
                        // against the substituted body. Generic-Ref
                        // instantiation goes through the dispatch
                        // instantiation bridge (`build_instantiate` binds
                        // args into the env and lowers the body); the
                        // dispatcher resolves re-export / barrel routes
                        // via `resolve_bare_name_in_scope` +
                        // `resolve_imported_type_root`.
                        let expanded = if !type_arguments.is_empty() {
                            crate::meta_resolve::instantiate_local_generic_ref_via_dispatch(
                                self.ctx,
                                resolution_scope_canonical_id,
                                object,
                            )
                            .or_else(|| {
                                crate::meta_resolve::instantiate_local_generic_ref_via_dispatch(
                                    self.ctx,
                                    active_scope_canonical_id,
                                    object,
                                )
                            })
                        } else {
                            // Non-generic Ref: look up the alias's body
                            // directly via prepared decl resolution.
                            let try_body = |me: &mut Self, scope: &str| -> Option<TypeExpr> {
                                let declaration = me.resolve_type_declaration(scope, name.as_ref());
                                let target_canonical = if declaration.canonical_source.is_empty() {
                                    scope.to_string()
                                } else {
                                    declaration.canonical_source.clone()
                                };
                                let resolved_name = if declaration.resolved_name.is_empty() {
                                    name.as_ref().to_string()
                                } else {
                                    declaration.resolved_name.clone()
                                };
                                me.prepared_type_decl(&target_canonical, &resolved_name)
                                    .map(|p| p.body.clone())
                            };
                            try_body(self, resolution_scope_canonical_id)
                                .or_else(|| try_body(self, active_scope_canonical_id))
                        }?;
                        let expanded_indexed = TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(expanded),
                            index: index.clone(),
                        };
                        self.enumerate_member_surface_keys_via_route(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &expanded_indexed,
                            member_name,
                            depth + 1,
                        )
                    }
                    TypeExpr::IndexedAccess { .. } => {
                        // Try resolving the inner IndexedAccess to a
                        // concrete object, then re-apply the outer
                        // index.
                        let resolved_inner = self
                            .solve_or_project_prepared_member_leaf_expr(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                object,
                            )
                            .filter(|resolved| resolved != object.as_ref())?;
                        let next = TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(resolved_inner),
                            index: index.clone(),
                        };
                        self.enumerate_member_surface_keys_via_route(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &next,
                            member_name,
                            depth + 1,
                        )
                    }
                    _ => None,
                }
            }
            _ => None,
        }
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
            // route through the dispatch-based bridges in
            // `meta_resolve` instead of the deprecated engine methods.
            // The bridges compose dispatch + the engine's surviving
            // `pub(crate)` cycle-protected helpers, preserving the
            // engine method's "lower whole expr, dispatch with empty
            // path" semantics (no IndexedAccess decomposition).
            if let Some(shape) = crate::meta_resolve::project_expr_surface_shape_via_host_threaded(
                query_engine,
                scope_canonical_id,
                target,
            ) {
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
            if let Some(projected) =
                // Leak-close-2 — Navigate base + Shallow
                // terminal, Published. The sister
                // `project_expr_surface_shape_via_host_threaded` at
                // the line above already uses Navigate+Shallow; this
                // fallback was the asymmetric Expanded path that the
                // 3-way consult identified as a leak source for
                // utility-route fallbacks.
                crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
                        query_engine,
                        scope_canonical_id,
                        target,
                        crate::semantic_query::ProjectionMode::Navigate,
                        crate::semantic_query::ProjectionMode::Shallow,
                        crate::semantic_query::ReductionDemand::Published,
                    )
            {
                let shape =
                    verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected);
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
            // No structural-substitution fallback: the dispatch surface
            // bridges above (Navigate+Shallow+Published) are the sole
            // authority for the utility-route target shape, including
            // generic-alias instantiation. A miss here is a clean `None`.
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
    /// Route-key leaf stabiliser: project a single prepared-member-path
    /// leaf `TypeExpr` against its resolution + active scopes, returning
    /// the stabilised expression (or `None` when no scope can advance it).
    pub(super) fn solve_or_project_prepared_member_leaf_expr(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let context = PreparedProjectionContext {
            decl_scope: resolution_scope_canonical_id.to_string(),
            arg_scope: active_scope_canonical_id.to_string(),
            chain_scopes: self.projection_chain_scopes.clone(),
        };
        self.solve_or_project_leaf_expr_with_context(&context, expr)
    }

    /// Per-TypeExpr-shape scope dispatch for the prepared-member-path
    /// projection.
    ///
    /// Earlier logic tried `active_scope` first and then fell back to
    /// `resolution_scope` only when the expression referenced a prepared
    /// symbol in that scope. That gate missed transitive helper refs
    /// (e.g., `ComponentUI<typeof theme>` where `ComponentUI` lives in a
    /// type-file reached via the prepared decl's import chain, not the
    /// decl's immediate symbol map).
    ///
    /// The current implementation uses a
    /// `PreparedProjectionContext { decl_scope, arg_scope }`:
    /// - bare `Ref { name, type_arguments: [] }`: try `decl_scope` first
    ///   (helper-body-internal reference), fall back to `arg_scope`.
    /// - `TypeOf(value_ref)`: always resolve in `arg_scope` (caller-
    ///   scoped value symbol table).
    /// - `Ref { name, type_arguments }`: resolve the NAME in `decl_scope`
    ///   so helper aliases lower against their own declaration site;
    ///   lower `type_arguments` in `arg_scope` so caller-scoped
    ///   `typeof theme` / explicit type arguments stay resolvable.
    ///   After both halves resolve, re-run
    ///   `solve_or_project_leaf_expr_until_stable` in `decl_scope` to
    ///   bridge the instantiation.
    /// - compound shapes (`IndexedAccess`, `Conditional`, `Mapped`,
    ///   `KeyOf`, etc.): fall back to the two-scope retry path (active
    ///   first, resolution fallback). The compound shapes don't need
    ///   per-sub-expression scope splitting because their sub-
    ///   expressions are already `TypeExpr` leaves that round-trip
    ///   through this function.
    fn solve_or_project_leaf_expr_with_context(
        &mut self,
        context: &PreparedProjectionContext,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let decl_scope = context.decl_scope.clone();
        let arg_scope = context.arg_scope.clone();
        let chain_scopes = context.chain_scopes.clone();

        if decl_scope == arg_scope {
            return self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
        }

        match expr {
            TypeExpr::Ref {
                name: _,
                type_arguments,
            } if type_arguments.is_empty() => {
                // Bare `Ref { name, [] }`: helper-body-internal reference.
                // Try decl_scope first; fall back to arg_scope.
                if let Some(result) =
                    self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                {
                    if &result != expr {
                        return Some(result);
                    }
                }
                self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr)
            }
            TypeExpr::TypeOf(_) => {
                // `typeof value_ref`: caller-scoped first (the most
                // common case is `Foo['x']` where `Foo` is a value
                // imported into the calling SFC). Some
                // helper-aliased patterns reference values that are
                // visible in OUTER helper scopes â€” e.g.,
                // `type Button = ComponentConfig<typeof theme>`
                // declared in `button-types.ts`, where `theme` is
                // visible there, but by the time the projection
                // recurses into `ComponentConfig`'s body in
                // `types.ts`, neither `decl_scope=types.ts` nor
                // `arg_scope=ImportedSlotButton.vue` can resolve
                // `theme`. The `chain_scopes` carry the outer
                // declaration scopes through the recursion so
                // the value reference can find its visible scope.
                let arg_first = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if let Some(ref result) = arg_first {
                    if result != expr {
                        return arg_first;
                    }
                }
                if let Some(decl_result) =
                    self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                {
                    if &decl_result != expr {
                        return Some(decl_result);
                    }
                }
                for chain_scope in &chain_scopes {
                    if chain_scope == &decl_scope || chain_scope == &arg_scope {
                        continue;
                    }
                    if let Some(chain_result) =
                        self.solve_or_project_leaf_expr_until_stable(chain_scope, expr)
                    {
                        if &chain_result != expr {
                            return Some(chain_result);
                        }
                    }
                }
                arg_first
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                // `Ref { name, [args..] }`: helper instantiation. Resolve
                // the name in decl_scope (so the helper's declaration
                // registry is consulted), and lower type_arguments in
                // arg_scope (so caller-side `typeof`, locally-declared
                // types, etc. stay resolvable). The simplest way to
                // plumb both is to try decl_scope first â€” the helper's
                // body will instantiate against its own declaration-
                // site symbol table. If decl_scope resolves the helper
                // (non-trivially), return the decl_scope projection.
                // Otherwise fall back to arg_scope where the ref name
                // may be reachable via direct import.
                let decl_first = self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr);
                if let Some(ref result) = decl_first {
                    if result != expr {
                        return decl_first;
                    }
                }
                let arg_result = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if let Some(ref result) = arg_result {
                    if result != expr {
                        return arg_result;
                    }
                }
                // Split-scope projection. When the ref's name belongs
                // to one scope (e.g. `ComponentUI` declared in
                // `types.ts`) and its type_arguments reference values
                // from another scope (e.g. `typeof theme` visible
                // only in `button-types.ts`), pre-resolve each
                // `TypeOf(value)` argument in any chain scope where
                // the value is visible, then re-try the projection
                // with the resolved arguments substituted.
                if !chain_scopes.is_empty() {
                    let mut resolved_args: Vec<TypeExpr> = Vec::with_capacity(type_arguments.len());
                    let mut any_argument_resolved = false;
                    for arg in type_arguments.iter() {
                        let mut resolved = arg.clone();
                        if matches!(arg, TypeExpr::TypeOf(_)) {
                            for chain_scope in &chain_scopes {
                                if chain_scope == &decl_scope || chain_scope == &arg_scope {
                                    continue;
                                }
                                if let Some(chain_arg) =
                                    self.solve_or_project_leaf_expr_until_stable(chain_scope, arg)
                                {
                                    if &chain_arg != arg {
                                        resolved = chain_arg;
                                        any_argument_resolved = true;
                                        break;
                                    }
                                }
                            }
                        }
                        resolved_args.push(resolved);
                    }
                    if any_argument_resolved {
                        let resolved_expr = TypeExpr::Ref {
                            name: name.clone(),
                            type_arguments: std::sync::Arc::from(resolved_args),
                        };
                        if let Some(result) = self
                            .solve_or_project_leaf_expr_until_stable(&decl_scope, &resolved_expr)
                        {
                            return Some(result);
                        }
                        if let Some(result) =
                            self.solve_or_project_leaf_expr_until_stable(&arg_scope, &resolved_expr)
                        {
                            return Some(result);
                        }
                    }
                }
                arg_result.or(decl_first)
            }
            _ => {
                // Compound shapes (IndexedAccess, Conditional, Mapped,
                // KeyOf, Intersection, Union, Parenthesized, etc.) use
                // the two-scope retry. Inner sub-expressions come back
                // through this function so per-shape dispatch still
                // applies transitively.
                let active_result = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if !self.expr_references_prepared_scope_symbol(&decl_scope, expr) {
                    return active_result;
                }
                self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                    .or(active_result)
            }
        }
    }

    /// Fixed-point driver: repeatedly stabilise a leaf `TypeExpr` in
    /// `scope_canonical_id` until it stops advancing (or the iteration
    /// budget is exhausted), returning the final stabilised expression.
    fn solve_or_project_leaf_expr_until_stable(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        // Node-domain fixpoint: stabilise on interned `RaisedShapeKey` identity.
        // Each iteration projects the cursor through the NODE adapters (no
        // materialisation); convergence compares interned raised-shape keys
        // (`route_projection_node_eq_to_expr` against the input `expr` on
        // iteration 1, `route_projection_nodes_eq` against the prior node on
        // later iterations). The sole publication `TypeExpr` is materialised
        // ONCE after convergence / exhaustion at the surface sink — there is no
        // per-iteration materialisation.
        let mut cursor = RouteFixpointCursor::Input(expr);
        let mut last: Option<AdmittedRouteProjectionNode> = None;
        for _ in 0..3 {
            let produced = match cursor {
                // FIRST iteration (and any iteration whose cursor is still the
                // input) — primary `.or_else` fallback, SAME order as the legacy
                // TypeExpr tail: the empty-terminal Expanded lower+project, then
                // the Expanded-base / Expanded-terminal Published surface bridge.
                // Expanded is preserved on both dimensions because reducing
                // either below Expanded breaks fixpoint convergence for imported
                // alias helpers like `Button['ui']`, where a Navigate carrier
                // would freeze a generic `InstantiationRef` at the empty-path
                // terminal.
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
                // No projection this iteration — materialise `last` once.
                return last
                    .and_then(|node| super::materialize_route_projection_node(self.ctx(), &node));
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
                return super::materialize_route_projection_node(self.ctx(), &produced);
            }
            last = Some(produced);
            cursor = RouteFixpointCursor::Node(produced);
        }
        last.and_then(|node| super::materialize_route_projection_node(self.ctx(), &node))
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
