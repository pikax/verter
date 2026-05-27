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

use rustc_hash::FxHashSet;
use verter_type_expr::TypeExpr;

use super::helpers::{projected_surface_member_names, strip_parens_expr};
use super::{instantiate_local_generic_ref_via_engine, ComponentMetaQueryEngine};

impl<'a> ComponentMetaQueryEngine<'a> {
    pub(super) fn prepared_string_literal_keys(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<Vec<String>> {
        use verter_type_expr::{LiteralValue, TypeExpr};

        match expr {
            TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
            TypeExpr::Union(types) => {
                let mut keys = Vec::with_capacity(types.len());
                for ty in types.iter() {
                    keys.extend(self.prepared_string_literal_keys(
                        scope_canonical_id,
                        prepared,
                        ty,
                        active,
                    )?);
                }
                Some(keys)
            }
            TypeExpr::Parenthesized(inner) => {
                self.prepared_string_literal_keys(scope_canonical_id, prepared, inner, active)
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                let (target_canonical_id, target_symbol_name) =
                    self.resolve_prepared_surface_target(scope_canonical_id, prepared, name)?;
                let visit_key = (target_canonical_id.clone(), target_symbol_name.clone());
                if !active.insert(visit_key.clone()) {
                    return None;
                }
                let resolved = self
                    .prepared_type_decl(&target_canonical_id, &target_symbol_name)
                    .and_then(|target_prepared| {
                        self.prepared_string_literal_keys(
                            &target_canonical_id,
                            target_prepared.as_ref(),
                            &target_prepared.body,
                            active,
                        )
                    });
                active.remove(&visit_key);
                resolved
            }
            _ => None,
        }
    }

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    /// **Deletion target.** This walker is scheduled for removal — the
    /// architectural target is `PathWalker` (in
    /// `project_semantic_dispatch/walk.rs`) as the only path-precise
    /// walker. The walker remains in service of member-route resolution
    /// and projection-rescue helpers (`expr_needs_projection_rescue`,
    /// `compare_type_expr_improvement`,
    /// `select_imported_materialization_scope`, and the cycle-detection
    /// migration helper `lowered_root_reaches_transitive_cycle`). Once
    /// those callers migrate to the dispatch path, this walker and its
    /// 13 internal call sites should be deleted in the same change
    /// (per CLAUDE.md "Legacy Code Deletion" — no shims).
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    #[allow(dead_code)] // deletion target — see doc comment above
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
        if matches!(projected_expr, TypeExpr::Unknown { .. }) {
            // preserve the re-export chain walk that the
            // deleted `instantiate_local_generic_ref` engine method
            // performed via `resolve_final_prepared_type_target`.
            if let Some(expanded) =
                instantiate_local_generic_ref_via_engine(self, resolution_scope_canonical_id, expr)
                    .or_else(|| {
                        instantiate_local_generic_ref_via_engine(
                            self,
                            active_scope_canonical_id,
                            expr,
                        )
                    })
                    .filter(|expanded| expanded != expr)
            {
                return self.enumerate_member_surface_keys_via_route(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &expanded,
                    member_name,
                    depth + 1,
                );
            }
        }

        match &projected_expr {
            TypeExpr::Object(object) => {
                let member_ty = object.properties.iter().find_map(|member| match member {
                    ObjectMember::Property(property) if property.name == member_name => {
                        Some(property.ty.clone())
                    }
                    ObjectMember::Method(method) if method.name == member_name => Some(
                        TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                    ),
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
                        // against the substituted body.
                        // preserve the engine method's
                        // re-export chain walk via the engine helper.
                        let expanded = if !type_arguments.is_empty() {
                            instantiate_local_generic_ref_via_engine(
                                self,
                                resolution_scope_canonical_id,
                                object,
                            )
                            .or_else(|| {
                                instantiate_local_generic_ref_via_engine(
                                    self,
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
            // preserve the engine method's re-export chain
            // walk by routing through the engine helper rather than
            // the dispatch-only variant.
            let expanded_ref_opt =
                instantiate_local_generic_ref_via_engine(query_engine, scope_canonical_id, target);
            if let Some(expanded_ref) = expanded_ref_opt {
                if let Some(shape) =
                    crate::meta_resolve::project_expr_surface_shape_via_host_threaded(
                        query_engine,
                        scope_canonical_id,
                        &expanded_ref,
                    )
                {
                    if shape_has_surface(&shape) {
                        return Some(shape);
                    }
                }
                if let Some(projected) =
                    // Leak-close-2 — same disposition as the
                    // first fallback arm (Navigate+Shallow+Published);
                    // applied to the `expanded_ref` instantiation
                    // result so both arms stay symmetric with the
                    // sister-helper shape.
                    crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
                            query_engine,
                            scope_canonical_id,
                            &expanded_ref,
                            crate::semantic_query::ProjectionMode::Navigate,
                            crate::semantic_query::ProjectionMode::Shallow,
                            crate::semantic_query::ReductionDemand::Published,
                        )
                {
                    let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(
                        &projected,
                    );
                    if shape_has_surface(&shape) {
                        return Some(shape);
                    }
                }
                let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(
                    &expanded_ref,
                );
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
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
                shape.properties.retain(|property| {
                    requested
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
                shape.properties.retain(|property| {
                    !omitted
                        .iter()
                        .any(|candidate| candidate == property.name.as_str())
                });
                shape_has_surface(&shape).then_some(shape)
            }
            _ => None,
        }
    }
}
