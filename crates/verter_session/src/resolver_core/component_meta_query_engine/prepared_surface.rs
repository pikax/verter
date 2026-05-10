//! Prepared root/member surface projection and ctx-cache publication
//! methods extracted from `component_meta_query_engine/mod.rs`.
//!
//! These methods operate on prepared type declarations
//! (`PreparedTypeDecl`) and project their root surface or requested
//! members. They publish results into the ctx-owned
//! `PreparedSurfaceDb` and `PreparedMemberDb` caches via `post_publish`
//! cooperative-admission, then mirror the publication in the engine's
//! per-request read-through view.
//!
//! Visibility:
//! - `pub(crate) fn cached_prepared_root_surface` — used by
//!   `meta_resolve` consumers.
//! - `pub(crate) fn project_prepared_requested_member_from_symbol` —
//!   used by route projection methods in `routed_expr.rs`.
//! - All other methods stay private (no visibility qualifier) and are
//!   visible inside the `component_meta_query_engine` folder via
//!   parent-private locality.

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_solver::query_engine::{ProjectedMember, ProjectedSurface};
use verter_type_expr::TypeExpr;

use super::helpers::{is_builtin_name, type_expr_references_type_params};
use super::surface::{
    apply_surface_member_filter, apply_surface_member_modifier, apply_type_param_substitutions,
    arc_prepared_member_cache_key, arc_prepared_surface_cache_key, arc_prepared_target_cache_key,
    build_default_type_param_substitutions, prepared_substitution_key,
    projected_surface_from_function_expr_with_substitutions,
    projected_surface_from_object_expr_with_substitutions,
    projected_surface_from_parts_intersection, projected_surface_from_parts_union,
    projected_surface_is_empty, projected_surface_unwrap_or_clone,
    substitute_function_expr_if_needed, substituted_ref_expr_if_needed, PreparedSurfaceProjection,
};
use super::{
    engine_dep_signature_for_canonical, ComponentMetaQueryEngine, PreparedMemberCacheKey,
    PreparedMemberCacheKind, PreparedSurfaceCacheKey, PreparedTargetCacheKey,
};

impl<'a> ComponentMetaQueryEngine<'a> {
    // -------------------------------------------------------------------
    // WS3: Projection-based surface extraction
    // -------------------------------------------------------------------

    /// Project the full surface of a type expression in a declaration scope.
    ///
    /// **Trampoline.** Body is dispatch-centric: tries
    /// `dispatch_projected_surface` (which
    /// instantiates the symbol through the `Instantiate { .., body_mode:
    /// Expanded }` memo and raises the resulting node back to a
    /// `ProjectedSurface`), falling back to the prepared-decl
    /// projection (`cached_prepared_root_surface`) when dispatch
    /// returns `None` for re-exported / barrel-routed declarations.
    /// The prepared-decl helper itself is a dispatch consumer (it
    /// reads via the ctx store and reduces the prepared decl body
    /// without embedding a separate resolver). Callers migrate off
    /// this method in 5d-5f; the method retires in 5g along with the
    /// prepared-projection helpers per §F call-graph closure.
    pub(crate) fn cached_prepared_root_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ProjectedSurface> {
        // When the requested symbol is not declared at the passed scope but
        // is instead re-exported through a barrel (e.g. `reka-ui/dist/index.d.ts`
        // re-exports `ListboxRootProps` from `dist/index3.d.ts`), chase the
        // re-export chain to the declaring file so the prepared bundle lookup
        // hits the actual declaration. This is a pure routing step — the
        // request-local prepared cache still keys on the original scope so
        // repeated queries stay cheap, but the projection itself runs against
        // the declaring scope where the prepared decl lives.
        let (resolved_scope, resolved_symbol) =
            self.resolve_final_prepared_type_target(scope_canonical_id, symbol_name);
        self.project_prepared_root_surface(resolved_scope.as_str(), resolved_symbol.as_str())
            .map(projected_surface_unwrap_or_clone)
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    pub(super) fn project_prepared_root_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        let previous_root = self
            .current_prepared_request_root
            .replace(scope_canonical_id.to_string());
        let result = self.project_prepared_root_surface_inner(scope_canonical_id, symbol_name);
        self.current_prepared_request_root = previous_root;
        result
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_prepared_root_surface_inner(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        #[cfg(test)]
        {
            self.prepared_root_surface_projection_count += 1;
        }
        let mut active = FxHashSet::default();
        match self.project_prepared_surface_from_symbol(
            scope_canonical_id,
            symbol_name,
            &FxHashMap::default(),
            &mut active,
        ) {
            PreparedSurfaceProjection::Surface(surface)
                if !projected_surface_is_empty(&surface) =>
            {
                Some(surface)
            }
            _ => None,
        }
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_prepared_surface_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        let cache_key = PreparedSurfaceCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            substitutions: prepared_substitution_key(substitutions),
        };
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("prepared_surface_cache");
        if let Some(cached) = self
            .prepared_surface_cache
            .borrow()
            .get(&cache_key)
            .cloned()
        {
            return cached;
        }
        // Step 3 closure: peek ctx-owned PreparedSurfaceDb. The compute
        // path below is non-trivial (recursion, active-set), so we
        // peek-then-compute rather than wrapping the whole compute in
        // get_or_compute. Cold-compute writes back through
        // `cache_prepared_surface_projection` and the local view.
        {
            let arc_key =
                arc_prepared_surface_cache_key(scope_canonical_id, symbol_name, substitutions);
            let host_db = self.ctx.project_type_store().prepared_surface_db();
            if let Some(payload) = host_db.peek(&arc_key, self.ctx) {
                let projection = match payload {
                    crate::component_meta_caches::PreparedSurfacePayload::Surface(arc_surface) => {
                        PreparedSurfaceProjection::Surface(arc_surface)
                    }
                    crate::component_meta_caches::PreparedSurfacePayload::Empty => {
                        PreparedSurfaceProjection::Empty
                    }
                    crate::component_meta_caches::PreparedSurfacePayload::Unsupported => {
                        PreparedSurfaceProjection::Unsupported
                    }
                };
                self.prepared_surface_cache
                    .borrow_mut()
                    .insert(cache_key, projection.clone());
                return projection;
            }
        }
        if substitutions.is_empty() {
            if let Some(prepared) = self.prepared_type_decl(scope_canonical_id, symbol_name) {
                if let Some(default_substitutions) =
                    build_default_type_param_substitutions(prepared.as_ref(), &[])
                {
                    if !default_substitutions.is_empty() {
                        let result = self.project_prepared_surface_from_symbol(
                            scope_canonical_id,
                            symbol_name,
                            &default_substitutions,
                            active,
                        );
                        self.publish_prepared_surface_to_host_db(
                            scope_canonical_id,
                            symbol_name,
                            substitutions,
                            &result,
                        );
                        self.prepared_surface_cache
                            .borrow_mut()
                            .insert(cache_key, result.clone());
                        return result;
                    }
                }
            }
        }

        if let Some(cached) =
            self.cached_prepared_surface(scope_canonical_id, symbol_name, substitutions)
        {
            let cached = PreparedSurfaceProjection::Surface(cached);
            self.publish_prepared_surface_to_host_db(
                scope_canonical_id,
                symbol_name,
                substitutions,
                &cached,
            );
            self.prepared_surface_cache
                .borrow_mut()
                .insert(cache_key.clone(), cached.clone());
            return cached;
        }

        let key = (scope_canonical_id.to_string(), symbol_name.to_string());
        if !active.insert(key.clone()) {
            return PreparedSurfaceProjection::Unsupported;
        }

        let result = self
            .prepared_type_decl(scope_canonical_id, symbol_name)
            .map(|prepared| {
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared.as_ref(),
                    &prepared.body,
                    substitutions,
                    active,
                )
            })
            .unwrap_or(PreparedSurfaceProjection::Unsupported);

        active.remove(&key);
        self.cache_prepared_surface_projection(
            scope_canonical_id,
            symbol_name,
            substitutions,
            &result,
        );
        self.publish_prepared_surface_to_host_db(
            scope_canonical_id,
            symbol_name,
            substitutions,
            &result,
        );
        self.prepared_surface_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    /// Step 3 closure helper: write-through to ctx-owned
    /// PreparedSurfaceDb. Called after compute publishes a result so
    /// the next request (or a concurrent reader) gets the warm hit.
    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn publish_prepared_surface_to_host_db(
        &self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        result: &PreparedSurfaceProjection,
    ) {
        let arc_key =
            arc_prepared_surface_cache_key(scope_canonical_id, symbol_name, substitutions);
        let payload = match result {
            PreparedSurfaceProjection::Surface(s) => {
                crate::component_meta_caches::PreparedSurfacePayload::Surface(s.clone())
            }
            PreparedSurfaceProjection::Empty => {
                crate::component_meta_caches::PreparedSurfacePayload::Empty
            }
            PreparedSurfaceProjection::Unsupported => {
                crate::component_meta_caches::PreparedSurfacePayload::Unsupported
            }
        };
        let ctx = self.ctx;
        let host_db = ctx.project_type_store().prepared_surface_db();
        let captured_canonical = scope_canonical_id.to_string();
        let _ = host_db.get_or_compute(&arc_key, ctx, move || {
            let dep_sig = engine_dep_signature_for_canonical(ctx, captured_canonical.as_str());
            Some((payload, dep_sig))
        });
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_prepared_surface_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        match expr {
            TypeExpr::Parenthesized(inner) => self.project_prepared_surface_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                substitutions,
                active,
            ),
            TypeExpr::Object(object) => PreparedSurfaceProjection::Surface(std::sync::Arc::new(
                projected_surface_from_object_expr_with_substitutions(
                    object,
                    &prepared.type_parameters,
                    substitutions,
                ),
            )),
            TypeExpr::Function(function) => PreparedSurfaceProjection::Surface(
                std::sync::Arc::new(projected_surface_from_function_expr_with_substitutions(
                    function,
                    &prepared.type_parameters,
                    substitutions,
                )),
            ),
            TypeExpr::Intersection(parts) => {
                let mut surfaces = Vec::with_capacity(parts.len());
                for part in parts.iter() {
                    match self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        part,
                        substitutions,
                        active,
                    ) {
                        PreparedSurfaceProjection::Surface(surface) => surfaces.push(surface),
                        PreparedSurfaceProjection::Empty => {}
                        PreparedSurfaceProjection::Unsupported => {
                            return PreparedSurfaceProjection::Unsupported;
                        }
                    }
                }
                projected_surface_from_parts_intersection(surfaces)
            }
            TypeExpr::Union(parts) => {
                let mut surfaces = Vec::with_capacity(parts.len());
                for part in parts.iter() {
                    match self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        part,
                        substitutions,
                        active,
                    ) {
                        PreparedSurfaceProjection::Surface(surface) => surfaces.push(surface),
                        PreparedSurfaceProjection::Empty => {}
                        PreparedSurfaceProjection::Unsupported => {
                            return PreparedSurfaceProjection::Unsupported;
                        }
                    }
                }
                projected_surface_from_parts_union(surfaces)
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(substituted) =
                    substituted_ref_expr_if_needed(expr, name.as_ref(), substitutions)
                {
                    return self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        &substituted,
                        &FxHashMap::default(),
                        active,
                    );
                }
                self.project_prepared_surface_from_ref(
                    scope_canonical_id,
                    prepared,
                    name.as_ref(),
                    type_arguments.as_ref(),
                    active,
                )
            }
            TypeExpr::Array { .. }
            | TypeExpr::Tuple { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::TypeParameter(_)
            | TypeExpr::KeyOf(_)
            | TypeExpr::Rest(_)
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Infer { .. } => PreparedSurfaceProjection::Empty,
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::TypeOf(_) => PreparedSurfaceProjection::Unsupported,
        }
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_prepared_surface_from_ref(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        name: &str,
        type_arguments: &[TypeExpr],
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        match (name, type_arguments) {
            ("Partial", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    active,
                ),
                |member| member.optional = true,
            ),
            ("Required", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    active,
                ),
                |member| member.optional = false,
            ),
            ("Readonly", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    active,
                ),
                |member| member.readonly = true,
            ),
            ("NonNullable", [inner]) => self.project_prepared_surface_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                &FxHashMap::default(),
                active,
            ),
            ("Pick", [target, keys]) => {
                let Some(requested) =
                    self.prepared_string_literal_keys(scope_canonical_id, prepared, keys, active)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                self.project_prepared_requested_member_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    target,
                    &requested,
                    &FxHashMap::default(),
                    active,
                )
            }
            ("Omit", [target, keys]) => {
                let Some(omitted) =
                    self.prepared_string_literal_keys(scope_canonical_id, prepared, keys, active)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                apply_surface_member_filter(
                    self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        target,
                        &FxHashMap::default(),
                        active,
                    ),
                    move |member_name| !omitted.iter().any(|candidate| candidate == member_name),
                )
            }
            _ if matches!(name, "Array" | "ReadonlyArray" | "Promise") => {
                PreparedSurfaceProjection::Empty
            }
            _ if is_builtin_name(name) => PreparedSurfaceProjection::Unsupported,
            _ => {
                let Some((target_canonical_id, target_symbol_name)) =
                    self.resolve_prepared_surface_target(scope_canonical_id, prepared, name)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                let Some(target_prepared) =
                    self.prepared_type_decl(&target_canonical_id, &target_symbol_name)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                let Some(target_substitutions) = build_default_type_param_substitutions(
                    target_prepared.as_ref(),
                    type_arguments,
                ) else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                self.project_prepared_surface_from_symbol(
                    &target_canonical_id,
                    &target_symbol_name,
                    &target_substitutions,
                    active,
                )
            }
        }
    }

    #[allow(dead_code)] // deletion in 5g per call-graph closure
    fn project_prepared_requested_member_surface_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        requested: &[String],
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        let mut members = Vec::with_capacity(requested.len());
        for member_name in requested {
            let Some(projected_member) = self.project_prepared_requested_member_from_expr(
                scope_canonical_id,
                prepared,
                expr,
                member_name,
                substitutions,
                active,
            ) else {
                return PreparedSurfaceProjection::Unsupported;
            };
            members.push(projected_member);
        }

        PreparedSurfaceProjection::Surface(std::sync::Arc::new(ProjectedSurface {
            members,
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            has_index_signature: false,
        }))
    }

    pub(crate) fn project_prepared_requested_member_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        let cache_key = PreparedMemberCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            member_name: member_name.to_string(),
            kind: PreparedMemberCacheKind::Requested,
            substitutions: prepared_substitution_key(substitutions),
        };
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("prepared_member_cache");
        if let Some(cached) = self.prepared_member_cache.borrow().get(&cache_key).cloned() {
            return cached;
        }
        // Step 3 closure: peek ctx-owned PreparedMemberDb.
        {
            let arc_key = arc_prepared_member_cache_key(
                scope_canonical_id,
                symbol_name,
                member_name,
                crate::resolver_core::cache_keys::PreparedMemberCacheKind::Requested,
                substitutions,
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
        if substitutions.is_empty() {
            if let Some(prepared) = self.prepared_type_decl(scope_canonical_id, symbol_name) {
                if let Some(default_substitutions) =
                    build_default_type_param_substitutions(prepared.as_ref(), &[])
                {
                    if !default_substitutions.is_empty() {
                        let result = self.project_prepared_requested_member_from_symbol(
                            scope_canonical_id,
                            symbol_name,
                            member_name,
                            &default_substitutions,
                            active,
                        );
                        self.publish_prepared_member_to_host_db(
                            scope_canonical_id,
                            symbol_name,
                            member_name,
                            crate::resolver_core::cache_keys::PreparedMemberCacheKind::Requested,
                            substitutions,
                            &result,
                        );
                        self.prepared_member_cache
                            .borrow_mut()
                            .insert(cache_key, result.clone());
                        return result;
                    }
                }
            }
        }

        if let Some(cached) = self.cached_prepared_requested_member(
            scope_canonical_id,
            symbol_name,
            member_name,
            substitutions,
        ) {
            self.publish_prepared_member_to_host_db(
                scope_canonical_id,
                symbol_name,
                member_name,
                crate::resolver_core::cache_keys::PreparedMemberCacheKind::Requested,
                substitutions,
                &Some(cached.clone()),
            );
            self.prepared_member_cache
                .borrow_mut()
                .insert(cache_key, Some(cached.clone()));
            return Some(cached);
        }

        let visit_key = (scope_canonical_id.to_string(), symbol_name.to_string());
        if !active.insert(visit_key.clone()) {
            return None;
        }

        let result = self
            .prepared_type_decl(scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member) = prepared.member(member_name) {
                    let projected_member = ProjectedMember {
                        name: member_name.to_string(),
                        ty: apply_type_param_substitutions(&member.ty, substitutions),
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                    };
                    if !type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
                        self.cache_prepared_requested_member(
                            scope_canonical_id,
                            symbol_name,
                            &projected_member,
                            substitutions,
                        );
                    }
                    return Some(projected_member);
                }

                self.project_prepared_requested_member_from_expr(
                    scope_canonical_id,
                    prepared.as_ref(),
                    &prepared.body,
                    member_name,
                    substitutions,
                    active,
                )
            });

        active.remove(&visit_key);
        self.publish_prepared_member_to_host_db(
            scope_canonical_id,
            symbol_name,
            member_name,
            crate::resolver_core::cache_keys::PreparedMemberCacheKind::Requested,
            substitutions,
            &result,
        );
        self.prepared_member_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    /// Step 3 closure helper: write-through to ctx-owned
    /// PreparedMemberDb.
    pub(super) fn publish_prepared_member_to_host_db(
        &self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        kind: crate::resolver_core::cache_keys::PreparedMemberCacheKind,
        substitutions: &FxHashMap<String, TypeExpr>,
        result: &Option<ProjectedMember>,
    ) {
        let arc_key = arc_prepared_member_cache_key(
            scope_canonical_id,
            symbol_name,
            member_name,
            kind,
            substitutions,
        );
        let ctx = self.ctx;
        let host_db = ctx.project_type_store().prepared_member_db();
        let captured_value = result.clone();
        let captured_canonical = scope_canonical_id.to_string();
        let _ = host_db.get_or_compute(&arc_key, ctx, move || {
            let dep_sig = engine_dep_signature_for_canonical(ctx, captured_canonical.as_str());
            Some((captured_value, dep_sig))
        });
    }

    fn project_prepared_requested_member_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        use verter_type_expr::ObjectMember;

        match expr {
            TypeExpr::Parenthesized(inner) => self.project_prepared_requested_member_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                member_name,
                substitutions,
                active,
            ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_prepared_requested_member_from_expr(
                    scope_canonical_id,
                    prepared,
                    part,
                    member_name,
                    substitutions,
                    active,
                )
            }),
            TypeExpr::Object(object) => object.properties.iter().find_map(|member| match member {
                ObjectMember::Property(property) if property.name == member_name => {
                    Some(ProjectedMember {
                        name: property.name.clone(),
                        ty: apply_type_param_substitutions(&property.ty, substitutions),
                        optional: property.optional,
                        readonly: property.readonly,
                        is_method: false,
                    })
                }
                ObjectMember::Method(method) if method.name == member_name => {
                    Some(ProjectedMember {
                        name: method.name.clone(),
                        ty: TypeExpr::Function(std::sync::Arc::new(
                            substitute_function_expr_if_needed(&method.function, substitutions),
                        )),
                        optional: method.optional,
                        readonly: false,
                        is_method: true,
                    })
                }
                _ => None,
            }),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(substituted) =
                    substituted_ref_expr_if_needed(expr, name.as_ref(), substitutions)
                {
                    return self.project_prepared_requested_member_from_expr(
                        scope_canonical_id,
                        prepared,
                        &substituted,
                        member_name,
                        &FxHashMap::default(),
                        active,
                    );
                }
                match (name.as_ref(), type_arguments.as_ref()) {
                    ("Partial", [inner]) => self
                        .project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            inner,
                            member_name,
                            substitutions,
                            active,
                        )
                        .map(|mut member| {
                            member.optional = true;
                            member
                        }),
                    ("Required", [inner]) => self
                        .project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            inner,
                            member_name,
                            substitutions,
                            active,
                        )
                        .map(|mut member| {
                            member.optional = false;
                            member
                        }),
                    ("Readonly", [inner]) => self
                        .project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            inner,
                            member_name,
                            substitutions,
                            active,
                        )
                        .map(|mut member| {
                            member.readonly = true;
                            member
                        }),
                    ("NonNullable", [inner]) => self.project_prepared_requested_member_from_expr(
                        scope_canonical_id,
                        prepared,
                        inner,
                        member_name,
                        substitutions,
                        active,
                    ),
                    ("Pick", [target, keys]) => {
                        let requested = self.prepared_string_literal_keys(
                            scope_canonical_id,
                            prepared,
                            keys,
                            active,
                        )?;
                        if !requested.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            target,
                            member_name,
                            substitutions,
                            active,
                        )
                    }
                    ("Omit", [target, keys]) => {
                        let omitted = self.prepared_string_literal_keys(
                            scope_canonical_id,
                            prepared,
                            keys,
                            active,
                        )?;
                        if omitted.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            target,
                            member_name,
                            substitutions,
                            active,
                        )
                    }
                    _ if matches!(name.as_ref(), "Array" | "ReadonlyArray" | "Promise") => None,
                    _ if is_builtin_name(name.as_ref()) => None,
                    _ => {
                        let (target_canonical_id, target_symbol_name) = self
                            .resolve_prepared_surface_target(
                                scope_canonical_id,
                                prepared,
                                name.as_ref(),
                            )?;
                        let target_prepared =
                            self.prepared_type_decl(&target_canonical_id, &target_symbol_name)?;
                        let target_substitutions = build_default_type_param_substitutions(
                            target_prepared.as_ref(),
                            type_arguments.as_ref(),
                        )?;
                        self.project_prepared_requested_member_from_symbol(
                            &target_canonical_id,
                            &target_symbol_name,
                            member_name,
                            &target_substitutions,
                            active,
                        )
                    }
                }
            }
            _ => None,
        }
    }

    pub(super) fn resolve_prepared_surface_target(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        name: &str,
    ) -> Option<(String, String)> {
        let cache_key = PreparedTargetCacheKey {
            active_scope_canonical_id: scope_canonical_id.to_string(),
            decl_canonical_id: prepared.root_identity.canonical_id.clone(),
            decl_symbol_name: prepared.root_identity.symbol_name.clone(),
            requested_name: name.to_string(),
        };
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("prepared_target_cache");
        if let Some(cached) = self.prepared_target_cache.borrow().get(&cache_key).cloned() {
            return cached;
        }
        // Step 3 closure: peek ctx-owned PreparedTargetDb.
        {
            let arc_key = arc_prepared_target_cache_key(
                scope_canonical_id,
                prepared.root_identity.canonical_id.as_str(),
                prepared.root_identity.symbol_name.as_str(),
                name,
            );
            let host_db = self.ctx.project_type_store().prepared_target_db();
            if let Some(opt_arc_pair) = host_db.peek(&arc_key, self.ctx) {
                let value: Option<(String, String)> =
                    opt_arc_pair.map(|(c, n)| (c.as_ref().to_string(), n.as_ref().to_string()));
                self.prepared_target_cache
                    .borrow_mut()
                    .insert(cache_key, value.clone());
                return value;
            }
        }

        let resolve_prepared_target =
            |this: &mut Self, canonical_source: String, resolved_name: String| {
                let mut canonical_source = if canonical_source.is_empty() {
                    scope_canonical_id.to_string()
                } else {
                    canonical_source
                };
                let mut resolved_name = if resolved_name.is_empty() {
                    name.to_string()
                } else {
                    resolved_name
                };

                if canonical_source != scope_canonical_id {
                    if let Some((routed_source, routed_name)) =
                        this.ctx.resolve_named_type_export_target_shallow(
                            canonical_source.as_str(),
                            resolved_name.as_str(),
                        )
                    {
                        if this
                            .prepared_type_decl(routed_source.as_str(), routed_name.as_str())
                            .is_some()
                        {
                            canonical_source = routed_source;
                            resolved_name = routed_name;
                        }
                    }
                }

                this.prepared_type_decl(&canonical_source, &resolved_name)
                    .map(|_| (canonical_source, resolved_name))
            };

        let resolved = prepared
            .name_resolution
            .get(name)
            .and_then(|resolved| {
                resolve_prepared_target(
                    self,
                    resolved.canonical_id.clone(),
                    resolved.symbol_name.clone(),
                )
            })
            .or_else(|| {
                let declaration = self.resolve_type_declaration(scope_canonical_id, name);
                resolve_prepared_target(
                    self,
                    declaration.canonical_source,
                    declaration.resolved_name,
                )
            });
        // Step 3 closure: write-through to ctx-owned PreparedTargetDb.
        {
            let arc_key = arc_prepared_target_cache_key(
                scope_canonical_id,
                prepared.root_identity.canonical_id.as_str(),
                prepared.root_identity.symbol_name.as_str(),
                name,
            );
            let ctx = self.ctx;
            let host_db = ctx.project_type_store().prepared_target_db();
            let captured_value: Option<(std::sync::Arc<str>, std::sync::Arc<str>)> =
                resolved.as_ref().map(|(c, n)| {
                    (
                        std::sync::Arc::<str>::from(c.as_str()),
                        std::sync::Arc::<str>::from(n.as_str()),
                    )
                });
            let captured_canonical = scope_canonical_id.to_string();
            let _ = host_db.get_or_compute(&arc_key, ctx, move || {
                let dep_sig = engine_dep_signature_for_canonical(ctx, captured_canonical.as_str());
                Some((captured_value, dep_sig))
            });
        }
        self.prepared_target_cache
            .borrow_mut()
            .insert(cache_key, resolved.clone());
        resolved
    }
}
