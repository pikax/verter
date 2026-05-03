//! Routed expression projection, request-local caches, pick/member
//! route projection, inherited-member projection, and `type_surface_facts`
//! methods extracted from `component_meta_query_engine/mod.rs`.
//!
//! These methods project type expressions through `RouteDemand`
//! variants (Whole / MemberPath / Pick / Omit) and the cached
//! routed-expression surface paths. They cross-call into the prepared
//! surface, route key, and registry decl methods (in sibling files)
//! and into shallow_preserve helpers.
//!
//! Visibility:
//! - `pub(crate) fn project_routed_expr_surface_expr` — used by
//!   `meta_resolve` consumers.
//! - All other methods stay private and are visible inside the
//!   `component_meta_query_engine` folder via parent-private locality.
//!
//! LOC budget: ~1534 (within the 4000-LOC sibling budget).

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::query_engine::{ProjectedMember, ProjectedSurface};

use super::helpers::{
    is_builtin_name, is_package_source, prepared_decl_keeps_raw_symbolic_non_object_alias,
    prepared_member_body_stays_shallow, string_literal_keys_type_expr,
    type_expr_references_type_params,
};
use super::surface::{
    apply_type_param_substitutions, arc_prepared_member_cache_key,
    arc_routed_expr_surface_cache_key, build_default_type_param_substitutions,
    substitute_function_expr_if_needed, substituted_ref_expr_if_needed, PreparedSurfaceProjection,
};
use super::{
    assert_direct_pick_routed_expr_slow_lane_allowed, dispatch_member_for_root_symbol,
    engine_dep_signature_for_canonical, ComponentMetaQueryEngine, PreparedMemberCacheKey,
    PreparedMemberCacheKind, PreparedProjectionContext, PreparedSubstitutionKey,
    RoutedExprSurfaceCacheKey,
};
use crate::resolver_core::RouteDemand;

impl<'a> ComponentMetaQueryEngine<'a> {
    pub(crate) fn project_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &RouteDemand,
    ) -> Option<TypeExpr> {
        fn single_member_route_cache_entry(
            query_engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            root_symbol: &str,
            member_name: &str,
            projected_expr: &TypeExpr,
        ) -> Option<ProjectedMember> {
            // dispatch a single-member ProjectPath query for
            // the (root_symbol, member_name) pair, then fall back to the
            // engine's prepared/inherited route helpers (kept on the
            // engine because they consume the per-engine prepared-decl
            // request-root state).
            dispatch_member_for_root_symbol(
                query_engine,
                scope_canonical_id,
                root_symbol,
                member_name,
            )
            .or_else(|| {
                query_engine.project_prepared_member_route_projection(
                    scope_canonical_id,
                    root_symbol,
                    member_name,
                )
            })
            .or_else(|| {
                query_engine.project_inherited_member_route_projection(
                    scope_canonical_id,
                    root_symbol,
                    member_name,
                )
            })
            .or_else(|| {
                let prepared = query_engine.prepared_type_decl(scope_canonical_id, root_symbol)?;
                let member = prepared.member(member_name)?;
                Some(ProjectedMember {
                    name: member_name.to_string(),
                    ty: projected_expr.clone(),
                    optional: member.optional,
                    readonly: member.readonly,
                    is_method: member.is_method,
                })
            })
        }

        if let Some(cached_expr) =
            self.cached_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
        {
            return Some(cached_expr);
        }

        if let Some(projected_expr) =
            self.project_routed_expr_surface_expr_direct(scope_canonical_id, root_symbol, route)
        {
            self.cache_routed_expr_surface_expr(
                scope_canonical_id,
                root_symbol,
                route,
                &projected_expr,
            );
            if let RouteDemand::MemberPath(path) = route {
                if let [member_name] = path.as_slice() {
                    if let Some(projected_member) = single_member_route_cache_entry(
                        self,
                        scope_canonical_id,
                        root_symbol,
                        member_name,
                        &projected_expr,
                    ) {
                        self.cache_projected_member(
                            scope_canonical_id,
                            root_symbol,
                            &projected_member,
                        );
                    }
                }
            }
            if let RouteDemand::Pick(members) = route {
                self.cache_pick_members_from_projected_expr(
                    scope_canonical_id,
                    root_symbol,
                    members,
                    &projected_expr,
                );
            }
            return Some(projected_expr);
        }

        if let RouteDemand::MemberPath(path) = route {
            if let Some(projected_expr) = self.project_prepared_member_path_route_surface_expr(
                scope_canonical_id,
                root_symbol,
                path,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                if let [member_name] = path.as_slice() {
                    if let Some(projected_member) = single_member_route_cache_entry(
                        self,
                        scope_canonical_id,
                        root_symbol,
                        member_name,
                        &projected_expr,
                    ) {
                        self.cache_projected_member(
                            scope_canonical_id,
                            root_symbol,
                            &projected_member,
                        );
                    }
                }
                return Some(projected_expr);
            }
            if let [member_name] = path.as_slice() {
                // dispatch the single-member projection.
                let projected_member = dispatch_member_for_root_symbol(
                    self,
                    scope_canonical_id,
                    root_symbol,
                    member_name,
                )?;
                let projected_expr = projected_member.ty.clone();
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                self.cache_projected_member(scope_canonical_id, root_symbol, &projected_member);
                return Some(projected_expr);
            }
        }

        if let RouteDemand::Pick(members) = route {
            if let Some(projected_expr) = self.project_prepared_pick_route_surface_expr(
                scope_canonical_id,
                root_symbol,
                members,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                return Some(projected_expr);
            }
            if let Some(projected_expr) = self.project_pick_route_surface_expr_via_members(
                scope_canonical_id,
                root_symbol,
                members,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                return Some(projected_expr);
            }
            if let Some(projected_expr) = self.project_pick_route_surface_expr_via_routed_expr(
                scope_canonical_id,
                root_symbol,
                route,
                members,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                self.cache_pick_members_from_projected_expr(
                    scope_canonical_id,
                    root_symbol,
                    members,
                    &projected_expr,
                );
                return Some(projected_expr);
            }
        }

        None
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn cached_routed_expr_surface_expr(
        &self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &RouteDemand,
    ) -> Option<TypeExpr> {
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("routed_expr_surface_cache");
        let local_key = RoutedExprSurfaceCacheKey {
            scope_canonical_id: scope_canonical_id.to_owned(),
            root_symbol: root_symbol.to_owned(),
            route: route.clone(),
        };
        if let Some(cached) = self
            .routed_expr_surface_cache
            .borrow()
            .get(&local_key)
            .cloned()
        {
            return Some(cached);
        }
        // Step 3 closure: peek ctx-owned RoutedExprSurfaceDb.
        let arc_key =
            arc_routed_expr_surface_cache_key(scope_canonical_id, root_symbol, route.clone());
        let host_db = self.ctx.project_type_store().routed_expr_surface_db();
        let arc_value = host_db.peek(&arc_key, self.ctx)?;
        let value = arc_value.as_ref().clone();
        self.routed_expr_surface_cache
            .borrow_mut()
            .insert(local_key, value.clone());
        Some(value)
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn cache_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &RouteDemand,
        projected_expr: &TypeExpr,
    ) {
        let local_key = RoutedExprSurfaceCacheKey {
            scope_canonical_id: scope_canonical_id.to_owned(),
            root_symbol: root_symbol.to_owned(),
            route: route.clone(),
        };
        // Step 3 closure: write-through to ctx-owned RoutedExprSurfaceDb.
        let arc_key =
            arc_routed_expr_surface_cache_key(scope_canonical_id, root_symbol, route.clone());
        let ctx = self.ctx;
        let host_db = ctx.project_type_store().routed_expr_surface_db();
        let captured_value = projected_expr.clone();
        let captured_canonical = scope_canonical_id.to_string();
        let _ = host_db.get_or_compute(&arc_key, ctx, move || {
            let dep_sig = engine_dep_signature_for_canonical(ctx, captured_canonical.as_str());
            Some((captured_value, dep_sig))
        });
        self.routed_expr_surface_cache
            .borrow_mut()
            .insert(local_key, projected_expr.clone());
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn cache_pick_members_from_projected_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        members: &[String],
        projected_expr: &TypeExpr,
    ) {
        use std::collections::BTreeSet;
        use verter_semantic::analysis::type_expr::ObjectMember;

        let requested: BTreeSet<_> = members.iter().map(String::as_str).collect();
        let TypeExpr::Object(object) = projected_expr else {
            return;
        };
        for member in &object.properties {
            let projected_member = match member {
                ObjectMember::Property(property) if requested.contains(property.name.as_str()) => {
                    Some(ProjectedMember {
                        name: property.name.clone(),
                        ty: property.ty.clone(),
                        optional: property.optional,
                        readonly: property.readonly,
                        is_method: false,
                    })
                }
                ObjectMember::Method(method) if requested.contains(method.name.as_str()) => {
                    Some(ProjectedMember {
                        name: method.name.clone(),
                        ty: TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                        optional: method.optional,
                        readonly: false,
                        is_method: true,
                    })
                }
                _ => None,
            };
            if let Some(projected_member) = projected_member {
                self.cache_projected_member(scope_canonical_id, root_symbol, &projected_member);
            }
        }
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn cache_projected_member(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        projected_member: &ProjectedMember,
    ) {
        let _ = (scope_canonical_id, root_symbol, projected_member);
    }

    pub(super) fn cached_prepared_requested_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> Option<ProjectedMember> {
        let _ = (scope_canonical_id, symbol_name, member_name, substitutions);
        None
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    pub(super) fn cached_prepared_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        let _ = (scope_canonical_id, symbol_name, substitutions);
        None
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    pub(super) fn cache_prepared_surface_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        projection: &PreparedSurfaceProjection,
    ) {
        let _ = (scope_canonical_id, symbol_name, substitutions, projection);
    }

    pub(super) fn cache_prepared_requested_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        projected_member: &ProjectedMember,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) {
        let _ = (
            scope_canonical_id,
            symbol_name,
            projected_member,
            substitutions,
        );
    }

    #[allow(dead_code)]
    fn prepared_requested_member_shared_cache_enabled(
        &self,
        scope_canonical_id: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> bool {
        !substitutions.is_empty()
            && self
                .current_prepared_request_root
                .as_deref()
                .is_some_and(|request_root| request_root != scope_canonical_id)
    }

    #[allow(dead_code)]
    fn prepared_surface_shared_cache_enabled(
        &self,
        scope_canonical_id: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> bool {
        self.current_prepared_request_root
            .as_deref()
            .is_some_and(|request_root| request_root != scope_canonical_id)
            && (!substitutions.is_empty() || is_package_source(Some(scope_canonical_id)))
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_prepared_member_route_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<ProjectedMember> {
        let prepared = self.prepared_type_decl(scope_canonical_id, symbol_name)?;
        let member = prepared.member(member_name)?;
        self.project_prepared_member_from_decl(scope_canonical_id, &prepared, member_name, member)
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_prepared_member_from_decl(
        &mut self,
        scope_canonical_id: &str,
        prepared: &std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>,
        member_name: &str,
        member: &verter_semantic::analysis::type_solver::prepared::PreparedMember,
    ) -> Option<ProjectedMember> {
        if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
            return None;
        }
        let projected_ty = match &member.ty {
            TypeExpr::Object(_) => Some(member.ty.clone()),
            _ if prepared_member_body_stays_shallow(&member.ty) => Some(member.ty.clone()),
            _ if prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, &member.ty) => {
                Some(member.ty.clone())
            }
            _ if crate::meta_resolve::component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                &member.ty,
                scope_canonical_id,
                self,
            ) =>
            {
                Some(member.ty.clone())
            }
            _ => {
                // dispatch path replaces the deprecated method.
                crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
                    self,
                    scope_canonical_id,
                    &member.ty,
                )
            }
        }?;
        Some(ProjectedMember {
            name: member_name.to_string(),
            ty: projected_ty,
            optional: member.optional,
            readonly: member.readonly,
            is_method: member.is_method,
        })
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_prepared_member_path_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        path: &[String],
    ) -> Option<TypeExpr> {
        let mut visited = FxHashSet::default();
        self.project_prepared_member_path_route_projection_from_symbol(
            scope_canonical_id,
            scope_canonical_id,
            symbol_name,
            path,
            &FxHashMap::default(),
            &mut visited,
        )
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn expr_references_prepared_scope_symbol(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::ObjectMember;

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
            TypeExpr::Function(function) => {
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
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => false,
        }
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
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

    /// Path C C11b — per-TypeExpr-shape scope dispatch for the prepared-
    /// member-path projection (plan §2 Stage 6 Pass C11b).
    ///
    /// The pre-C11b logic tried `active_scope` first and then fell back to
    /// `resolution_scope` only when the expression referenced a prepared
    /// symbol in that scope. That gate missed transitive helper refs
    /// (e.g., `ComponentUI<typeof theme>` where `ComponentUI` lives in a
    /// type-file reached via the prepared decl's import chain, not the
    /// decl's immediate symbol map).
    ///
    /// C11b uses a `PreparedProjectionContext { decl_scope, arg_scope }`:
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
    #[allow(dead_code)] // deletion in 5g per call-graph closure
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
                // imported into the calling SFC). Path C C11-residual-B:
                // some helper-aliased patterns reference values that
                // are visible in OUTER helper scopes (e.g.,
                // `type Button = ComponentConfig<typeof theme>` declared
                // in `button-types.ts` — `theme` is visible there, but
                // by the time the projection recurses into
                // `ComponentConfig`'s body in `types.ts`, neither
                // `decl_scope=types.ts` nor `arg_scope=ImportedSlotButton.vue`
                // can resolve `theme`. The `chain_scopes` carry the
                // outer declaration scopes through the recursion so
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
                // plumb both is to try decl_scope first — the helper's
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
                // Path C C11-residual-B: split-scope projection. When
                // the ref's name belongs to one scope (e.g. `ComponentUI`
                // declared in `types.ts`) and its type_arguments
                // reference values from another scope (e.g.
                // `typeof theme` visible only in `button-types.ts`),
                // pre-resolve each `TypeOf(value)` argument in any
                // chain scope where the value is visible, then re-try
                // the projection with the resolved arguments substituted.
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
                // KeyOf, Intersection, Union, Parenthesized, etc.)
                // preserve the pre-C11b two-scope retry. Inner
                // sub-expressions come back through this function so
                // per-shape dispatch still applies transitively.
                let active_result = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if !self.expr_references_prepared_scope_symbol(&decl_scope, expr) {
                    return active_result;
                }
                self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                    .or(active_result)
            }
        }
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn solve_or_project_leaf_expr_until_stable(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let mut current = expr.clone();
        let mut last = None;
        for _ in 0..3 {
            // dispatch the lower+project tail and the
            // expr-surface bridge from `meta_resolve` instead of the
            // deprecated engine methods. The bridges share the engine's
            // cycle-protection helpers so behavior matches the legacy
            // method path.
            let next = crate::meta_resolve::lower_and_project_to_expanded_via_host_threaded(
                self,
                scope_canonical_id,
                &current,
            )
            .or_else(|| {
                crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
                    self,
                    scope_canonical_id,
                    &current,
                )
            });
            let Some(next) = next else {
                return last;
            };
            if next == current {
                return Some(next);
            }
            last = Some(next.clone());
            current = next;
        }
        last
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    pub(super) fn project_prepared_member_path_route_projection_from_symbol(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        symbol_name: &str,
        path: &[String],
        substitutions: &FxHashMap<String, TypeExpr>,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<TypeExpr> {
        let visit_key = (
            resolution_scope_canonical_id.to_string(),
            symbol_name.to_string(),
        );
        if !visited.insert(visit_key.clone()) {
            return None;
        }
        if substitutions.is_empty() {
            if let Some(prepared) =
                self.prepared_type_decl(resolution_scope_canonical_id, symbol_name)
            {
                if let Some(default_substitutions) =
                    build_default_type_param_substitutions(prepared.as_ref(), &[])
                {
                    if !default_substitutions.is_empty() {
                        let result = self
                            .project_prepared_member_path_route_projection_from_symbol(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                symbol_name,
                                path,
                                &default_substitutions,
                                visited,
                            );
                        visited.remove(&visit_key);
                        return result;
                    }
                }
            }
        }

        let result = self
            .prepared_type_decl(resolution_scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member_name) = path.first() {
                    if let Some(member) = prepared.member(member_name) {
                        let member_ty = apply_type_param_substitutions(&member.ty, substitutions);
                        if path.len() == 1 {
                            return self
                                .solve_or_project_prepared_member_leaf_expr(
                                    resolution_scope_canonical_id,
                                    active_scope_canonical_id,
                                    &member_ty,
                                )
                                .or(Some(member_ty));
                        }
                        return self.project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared.as_ref(),
                            &member_ty,
                            &path[1..],
                            &FxHashMap::default(),
                            visited,
                        );
                    }
                }

                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared.as_ref(),
                    &prepared.body,
                    path,
                    substitutions,
                    visited,
                )
            });

        visited.remove(&visit_key);
        result
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_prepared_member_path_route_projection_from_expr(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        path: &[String],
        substitutions: &FxHashMap<String, TypeExpr>,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_expr::ObjectMember;

        let Some((member_name, tail)) = path.split_first() else {
            let projected_expr = apply_type_param_substitutions(expr, substitutions);
            return self
                .solve_or_project_prepared_member_leaf_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &projected_expr,
                )
                .or(Some(projected_expr));
        };

        match expr {
            TypeExpr::Parenthesized(inner) => self
                .project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    inner,
                    path,
                    substitutions,
                    visited,
                ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    part,
                    path,
                    substitutions,
                    visited,
                )
            }),
            TypeExpr::Object(object) => {
                let member_ty = object.properties.iter().find_map(|member| match member {
                    ObjectMember::Property(property) if property.name == *member_name => {
                        Some(apply_type_param_substitutions(&property.ty, substitutions))
                    }
                    ObjectMember::Method(method) if method.name == *member_name => {
                        Some(TypeExpr::Function(std::sync::Arc::new(
                            substitute_function_expr_if_needed(&method.function, substitutions),
                        )))
                    }
                    _ => None,
                })?;
                if tail.is_empty() {
                    return self
                        .solve_or_project_prepared_member_leaf_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &member_ty,
                        )
                        .or(Some(member_ty));
                }
                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    &member_ty,
                    tail,
                    &FxHashMap::default(),
                    visited,
                )
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(substituted) =
                    substituted_ref_expr_if_needed(expr, name.as_ref(), substitutions)
                {
                    return self.project_prepared_member_path_route_projection_from_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        prepared,
                        &substituted,
                        path,
                        &FxHashMap::default(),
                        visited,
                    );
                }

                match (name.as_ref(), type_arguments.as_ref()) {
                    ("Partial", [inner])
                    | ("Required", [inner])
                    | ("Readonly", [inner])
                    | ("NonNullable", [inner]) => self
                        .project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared,
                            inner,
                            path,
                            substitutions,
                            visited,
                        ),
                    ("Pick", [target, keys]) => {
                        let requested = self.prepared_string_literal_keys(
                            resolution_scope_canonical_id,
                            prepared,
                            keys,
                            visited,
                        )?;
                        if !requested.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared,
                            target,
                            path,
                            substitutions,
                            visited,
                        )
                    }
                    ("Omit", [target, keys]) => {
                        let omitted = self.prepared_string_literal_keys(
                            resolution_scope_canonical_id,
                            prepared,
                            keys,
                            visited,
                        )?;
                        if omitted.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared,
                            target,
                            path,
                            substitutions,
                            visited,
                        )
                    }
                    _ if matches!(name.as_ref(), "Array" | "ReadonlyArray" | "Promise") => None,
                    _ if is_builtin_name(name.as_ref()) => None,
                    _ => {
                        let (target_canonical_id, target_symbol_name) = self
                            .resolve_prepared_surface_target(
                                resolution_scope_canonical_id,
                                prepared,
                                name.as_ref(),
                            )?;
                        let target_prepared =
                            self.prepared_type_decl(&target_canonical_id, &target_symbol_name)?;
                        let target_substitutions = build_default_type_param_substitutions(
                            target_prepared.as_ref(),
                            type_arguments.as_ref(),
                        )?;
                        // Path C C11-residual-B: as we descend into the
                        // target alias's declaration scope, push the
                        // current `resolution_scope_canonical_id` onto
                        // the projection chain. The leaf-expr handler
                        // uses this chain to find the scope where a
                        // `TypeOf(value)` reference was visible at the
                        // outer call site (e.g., `theme` imported in
                        // `button-types.ts` while we're now recursing
                        // into `ComponentConfig`'s body in `types.ts`).
                        let pushed = if !self
                            .projection_chain_scopes
                            .iter()
                            .any(|s| s == resolution_scope_canonical_id)
                            && resolution_scope_canonical_id != target_canonical_id
                        {
                            self.projection_chain_scopes
                                .push(resolution_scope_canonical_id.to_string());
                            true
                        } else {
                            false
                        };
                        let result = self
                            .project_prepared_member_path_route_projection_from_symbol(
                                &target_canonical_id,
                                active_scope_canonical_id,
                                &target_symbol_name,
                                path,
                                &target_substitutions,
                                visited,
                            );
                        if pushed {
                            self.projection_chain_scopes.pop();
                        }
                        result
                    }
                }
            }
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                name_type,
                ..
            } if name_type.is_none() => {
                let substituted_source = apply_type_param_substitutions(source, substitutions);
                let Some(keys) = self.enumerate_route_literal_keys(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &substituted_source,
                ) else {
                    let nested_expr = path.iter().fold(
                        apply_type_param_substitutions(expr, substitutions),
                        |object, member| TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(object),
                            index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
                        },
                    );
                    return self.solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &nested_expr,
                    );
                };
                if !keys.iter().any(|candidate| candidate == member_name) {
                    return None;
                }

                let mut member_substitutions = substitutions.clone();
                member_substitutions.insert(
                    parameter.clone(),
                    TypeExpr::string_literal(member_name.clone()),
                );
                let member_ty = apply_type_param_substitutions(value, &member_substitutions);
                if tail.is_empty() {
                    if let Some(keys) = self.enumerate_route_literal_keys(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &member_ty,
                    ) {
                        return string_literal_keys_type_expr(keys);
                    }
                    return self
                        .solve_or_project_prepared_member_leaf_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &member_ty,
                        )
                        .or(Some(member_ty));
                }
                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    &member_ty,
                    tail,
                    &FxHashMap::default(),
                    visited,
                )
            }
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::TypeOf(_)
            | TypeExpr::Union(_)
            | TypeExpr::Tuple { .. }
            | TypeExpr::Array { .. }
            | TypeExpr::KeyOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Rest(_)
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Infer { .. } => {
                let nested_expr = path.iter().fold(
                    apply_type_param_substitutions(expr, substitutions),
                    |object, member| TypeExpr::IndexedAccess {
                        object: std::sync::Arc::new(object),
                        index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
                    },
                );
                self.solve_or_project_prepared_member_leaf_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &nested_expr,
                )
            }
            TypeExpr::Function(_)
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. } => None,
        }
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_inherited_member_route_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<ProjectedMember> {
        let mut visited = FxHashSet::default();
        self.project_inherited_member_route_projection_from_symbol(
            scope_canonical_id,
            symbol_name,
            member_name,
            &mut visited,
        )
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_inherited_member_route_projection_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        let cache_key = PreparedMemberCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            member_name: member_name.to_string(),
            kind: PreparedMemberCacheKind::InheritedRoute,
            substitutions: PreparedSubstitutionKey::Empty,
        };
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("prepared_member_cache");
        if let Some(cached) = self.prepared_member_cache.borrow().get(&cache_key).cloned() {
            return cached;
        }
        // Step 3 closure: peek ctx-owned PreparedMemberDb (InheritedRoute).
        {
            let arc_key = arc_prepared_member_cache_key(
                scope_canonical_id,
                symbol_name,
                member_name,
                crate::resolver_core::cache_keys::PreparedMemberCacheKind::InheritedRoute,
                &FxHashMap::default(),
            );
            let host_db = self.ctx.project_type_store().prepared_member_db();
            if let Some(opt_arc) = host_db.peek(&arc_key, self.ctx) {
                let value = opt_arc.map(|arc_member| arc_member.as_ref().clone());
                self.prepared_member_cache
                    .borrow_mut()
                    .insert(cache_key, value.clone());
                return value;
            }
        }

        let visit_key = (scope_canonical_id.to_string(), symbol_name.to_string());
        if !visited.insert(visit_key.clone()) {
            return None;
        }

        let result = self
            .prepared_type_decl(scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member) = prepared.member(member_name) {
                    return self.project_prepared_member_from_decl(
                        scope_canonical_id,
                        &prepared,
                        member_name,
                        member,
                    );
                }

                self.project_inherited_member_route_projection_from_expr(
                    scope_canonical_id,
                    &prepared,
                    &prepared.body,
                    member_name,
                    visited,
                )
            });

        visited.remove(&visit_key);
        self.publish_prepared_member_to_host_db(
            scope_canonical_id,
            symbol_name,
            member_name,
            crate::resolver_core::cache_keys::PreparedMemberCacheKind::InheritedRoute,
            &FxHashMap::default(),
            &result,
        );
        self.prepared_member_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_inherited_member_route_projection_from_expr(
        &mut self,
        _scope_canonical_id: &str,
        prepared: &std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>,
        expr: &TypeExpr,
        member_name: &str,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        match expr {
            TypeExpr::Parenthesized(inner) => self
                .project_inherited_member_route_projection_from_expr(
                    _scope_canonical_id,
                    prepared,
                    inner,
                    member_name,
                    visited,
                ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_inherited_member_route_projection_from_expr(
                    _scope_canonical_id,
                    prepared,
                    part,
                    member_name,
                    visited,
                )
            }),
            TypeExpr::Ref { name, .. } => {
                let resolved = prepared.name_resolution.get(name.as_ref())?;
                self.project_inherited_member_route_projection_from_symbol(
                    resolved.canonical_id.as_str(),
                    resolved.symbol_name.as_str(),
                    member_name,
                    visited,
                )
            }
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn project_prepared_member_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<TypeExpr> {
        self.project_prepared_member_route_projection(scope_canonical_id, symbol_name, member_name)
            .map(|projected_member| projected_member.ty)
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_pick_route_surface_expr_via_members(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        members: &[String],
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_expr::{
            MethodSignature, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
        };

        let prepared = self.prepared_type_decl(scope_canonical_id, symbol_name);
        let mut properties = Vec::with_capacity(members.len());
        for member_name in members {
            let projected_member = if prepared
                .as_ref()
                .and_then(|prepared| prepared.member(member_name))
                .is_some()
            {
                self.project_prepared_member_route_projection(
                    scope_canonical_id,
                    symbol_name,
                    member_name,
                )?
            } else if let Some(projected_member) = self.project_inherited_member_route_projection(
                scope_canonical_id,
                symbol_name,
                member_name,
            ) {
                projected_member
            } else {
                // dispatch path replaces the deprecated method.
                dispatch_member_for_root_symbol(self, scope_canonical_id, symbol_name, member_name)?
            };
            self.cache_projected_member(scope_canonical_id, symbol_name, &projected_member);
            if projected_member.is_method {
                if let TypeExpr::Function(function) = &projected_member.ty {
                    properties.push(ObjectMember::Method(MethodSignature {
                        name: projected_member.name,
                        function: (**function).clone(),
                        optional: projected_member.optional,
                    }));
                    continue;
                }
            }
            properties.push(ObjectMember::Property(ObjectProperty {
                name: projected_member.name,
                ty: projected_member.ty,
                optional: projected_member.optional,
                readonly: projected_member.readonly,
            }));
        }
        Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties,
        })))
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_pick_route_surface_expr_via_routed_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        route: &RouteDemand,
        _members: &[String],
    ) -> Option<TypeExpr> {
        assert_direct_pick_routed_expr_slow_lane_allowed();
        self.project_routed_expr_surface_expr_direct(scope_canonical_id, symbol_name, route)
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_routed_expr_surface_expr_direct(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &RouteDemand,
    ) -> Option<TypeExpr> {
        self.dispatch_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    pub(super) fn project_prepared_pick_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        members: &[String],
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_expr::{
            MethodSignature, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
        };

        let prepared = self.prepared_type_decl(scope_canonical_id, symbol_name)?;
        let mut properties = Vec::with_capacity(members.len());
        for member_name in members {
            let member = prepared.member(member_name)?;
            if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
                return None;
            }
            if member.is_method {
                if let TypeExpr::Function(function) = &member.ty {
                    properties.push(ObjectMember::Method(MethodSignature {
                        name: member_name.clone(),
                        function: (**function).clone(),
                        optional: member.optional,
                    }));
                    continue;
                }
            }
            properties.push(ObjectMember::Property(ObjectProperty {
                name: member_name.clone(),
                ty: member.ty.clone(),
                optional: member.optional,
                readonly: member.readonly,
            }));
        }
        Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties,
        })))
    }

    #[allow(dead_code)]
    fn type_surface_facts(
        &self,
        scope_canonical_id: &str,
    ) -> Option<Vec<crate::resolver_core::FactVersionRef>> {
        let store_view = self.ctx.resolver_store_view();
        let mut facts = Vec::new();
        // Post-cut: live-ctx whole-hash with store-view as the first
        // consultation, falling back to the live ctx probe for
        // untracked-but-present canonicals.
        let hash = store_view
            .whole_hash(scope_canonical_id)
            .or_else(|| self.ctx.get_whole_hash(scope_canonical_id));
        if let Some(hash) = hash {
            facts.push(crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: scope_canonical_id.to_string(),
                hash,
            });
        }
        if let Some(hash) = store_view.derived_hash(
            scope_canonical_id,
            crate::resolver_core::DerivedFactKind::Route,
        ) {
            facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: scope_canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::Route,
                hash,
            });
        }
        (!facts.is_empty()).then_some(facts)
    }

    #[cfg(test)]
    pub(super) fn debug_prepared_surface_cache_len(&self) -> usize {
        self.prepared_surface_cache.borrow().len()
    }

    #[cfg(test)]
    pub(super) fn debug_prepared_member_cache_len(&self) -> usize {
        self.prepared_member_cache.borrow().len()
    }

    #[cfg(test)]
    pub(super) fn debug_prepared_target_cache_len(&self) -> usize {
        self.prepared_target_cache.borrow().len()
    }
}
