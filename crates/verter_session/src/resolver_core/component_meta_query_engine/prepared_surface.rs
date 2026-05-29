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
    engine_fact_signature_for_canonical_member, engine_fact_signature_for_exported_type,
    ComponentMetaQueryEngine, PreparedMemberCacheKey, PreparedMemberCacheKind,
    PreparedSurfaceCacheKey, PreparedTargetCacheKey,
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
    /// without embedding a separate resolver).
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

    #[allow(dead_code)]
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

    /// Test-only escape hatch exposing the `from_root_body` parameter.
    ///
    /// Drives `project_prepared_surface_from_symbol` with an explicit
    /// `from_root_body` flag so the discriminating cache-identity
    /// tests in `crate::r21_c4_cache_identity_tests` can exercise BOTH
    /// entry contexts (`true` = body position;
    /// `false` = heritage descent). The public
    /// `cached_prepared_root_surface` only enters at body position.
    #[cfg(test)]
    pub(crate) fn r21_c4_project_prepared_surface_from_symbol_with_flag(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        from_root_body: bool,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        let previous_root = self
            .current_prepared_request_root
            .replace(scope_canonical_id.to_string());
        let mut active = FxHashSet::default();
        let result = match self.project_prepared_surface_from_symbol(
            scope_canonical_id,
            symbol_name,
            &FxHashMap::default(),
            from_root_body,
            &mut active,
        ) {
            PreparedSurfaceProjection::Surface(surface)
                if !projected_surface_is_empty(&surface) =>
            {
                Some(surface)
            }
            _ => None,
        };
        self.current_prepared_request_root = previous_root;
        result
    }

    #[allow(dead_code)]
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
        // Top-level entry through `cached_prepared_root_surface`
        // enters the requested symbol AT THE BODY POSITION of the macro
        // T argument — `from_root_body = true`. Recursive descents
        // inside the walker thread their own arm-specific flag.
        match self.project_prepared_surface_from_symbol(
            scope_canonical_id,
            symbol_name,
            &FxHashMap::default(),
            true,
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

    #[allow(dead_code)]
    fn project_prepared_surface_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        from_root_body: bool,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        // `from_root_body` is the caller's macro-T own-body flag and
        // is part of the cache identity: the cached
        // `ProjectedSurface`'s per-member `declared_in_macro_type_arg`
        // reflects whether the symbol was entered at a body position
        // vs. a heritage descent, so two distinct entry contexts must
        // publish two distinct cache slots.
        let cache_key = PreparedSurfaceCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            substitutions: prepared_substitution_key(substitutions),
            from_root_body,
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
            let arc_key = arc_prepared_surface_cache_key(
                scope_canonical_id,
                symbol_name,
                substitutions,
                from_root_body,
            );
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
        // Observe the keyed canonical's content version ONCE here,
        // before any surface value is computed, through the view-aware
        // `authoritative_current_content_hash` oracle — under a
        // `SessionResolverContext` it resolves the overlay content hash
        // for an overlay-bearing session, so an overlay-derived entry
        // roots on the overlay version rather than the base hash. The
        // observed hash is threaded into the provenance-pure signature
        // builder by the publish helper so the published surface and
        // its signature root on one content version.
        let observed_keyed_hash = self
            .ctx
            .authoritative_current_content_hash(scope_canonical_id);
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
                            from_root_body,
                            active,
                        );
                        self.publish_prepared_surface_to_host_db(
                            scope_canonical_id,
                            symbol_name,
                            substitutions,
                            from_root_body,
                            &result,
                            observed_keyed_hash,
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
                from_root_body,
                &cached,
                observed_keyed_hash,
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
                    from_root_body,
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
            from_root_body,
            &result,
            observed_keyed_hash,
        );
        self.prepared_surface_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    /// Write-through to ctx-owned `PreparedSurfaceDb`. Called after
    /// compute publishes a result so the next request (or a concurrent
    /// reader) gets the warm hit.
    ///
    /// `observed_keyed_hash` is the keyed canonical
    /// (`scope_canonical_id`)'s content version observed at the value
    /// source — threaded into the provenance-pure signature builder so
    /// the entry's self-root and parse facts root on that one observed
    /// version rather than a current-content re-read. `?` on a `None`
    /// observation or builder result refuses shared-cache admission.
    #[allow(dead_code)]
    fn publish_prepared_surface_to_host_db(
        &self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        from_root_body: bool,
        result: &PreparedSurfaceProjection,
        observed_keyed_hash: Option<crate::resolver_core::ResolverHash16>,
    ) {
        let arc_key = arc_prepared_surface_cache_key(
            scope_canonical_id,
            symbol_name,
            substitutions,
            from_root_body,
        );
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
        let captured_symbol = symbol_name.to_string();
        let _ = host_db.get_or_compute(&arc_key, ctx, move || {
            let fact_sig = engine_fact_signature_for_exported_type(
                ctx,
                captured_canonical.as_str(),
                captured_symbol.as_str(),
                observed_keyed_hash?,
            )?;
            Some((payload, fact_sig))
        });
    }

    /// Surface projection with `from_root_body` threading.
    ///
    /// `from_root_body` is the caller's macro-T own-body flag. The walker
    /// mirrors the parser's `resolve_type_elements_inner_with_ctx_ref_guarded`
    /// semantics:
    ///
    /// - `TypeExpr::Object` (a TS `TSTypeLiteral`) → an own-body literal
    ///   that stamps its members with the caller's `from_root_body`.
    /// - `TypeExpr::Parenthesized` → transparent, propagates the flag.
    /// - `TypeExpr::Union` arms → propagate the flag (unions enumerate
    ///   options, not heritage).
    /// - `TypeExpr::Intersection` arms → an arm is own-body iff it is
    ///   itself a `TypeExpr::Object` AND the caller is at body position
    ///   (`from_root_body=true`); every other arm shape (Ref, utility
    ///   types, etc.) is a heritage-like descent and recurses with
    ///   `from_root_body=false`.
    /// - `TypeExpr::Ref` → delegated to `project_prepared_surface_from_ref`,
    ///   which routes utility types (`Pick`/`Omit`/`Partial`/`Required`/
    ///   `Readonly`/`NonNullable`) at `from_root_body=false` (their first
    ///   type-argument is a heritage descent) and named-type references
    ///   at the caller's flag (`MyProps` at body position propagates
    ///   `from_root_body=true` into MyProps's own body).
    #[allow(dead_code)]
    fn project_prepared_surface_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        substitutions: &FxHashMap<String, TypeExpr>,
        from_root_body: bool,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        match expr {
            TypeExpr::Parenthesized(inner) => self.project_prepared_surface_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                substitutions,
                from_root_body,
                active,
            ),
            TypeExpr::Object(object) => PreparedSurfaceProjection::Surface(std::sync::Arc::new(
                projected_surface_from_object_expr_with_substitutions(
                    object,
                    &prepared.type_parameters,
                    substitutions,
                    from_root_body,
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
                let arms: Vec<PreparedSurfaceProjection> = parts
                    .iter()
                    .map(|part| {
                        // Mirror parser semantic (decl.rs:1712):
                        // `arm_from_root_body = matches!(ty,
                        // TSType::TSTypeLiteral(_)) && from_root_body`.
                        // A literal arm of the intersection is own-body
                        // iff the carrier is at body position; every
                        // other arm shape is heritage-like and descends
                        // with `from_root_body=false`. We unwrap
                        // `Parenthesized` first so `(X & Y) & { foo }`
                        // shapes still detect the literal arm.
                        let arm_from_root_body = arm_is_own_body_literal(part) && from_root_body;
                        self.project_prepared_surface_from_expr(
                            scope_canonical_id,
                            prepared,
                            part,
                            substitutions,
                            arm_from_root_body,
                            active,
                        )
                    })
                    .collect();
                merge_prepared_intersection_arms(arms)
            }
            TypeExpr::Union(parts) => {
                let mut surfaces = Vec::with_capacity(parts.len());
                for part in parts.iter() {
                    match self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        part,
                        substitutions,
                        from_root_body,
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
                        from_root_body,
                        active,
                    );
                }
                self.project_prepared_surface_from_ref(
                    scope_canonical_id,
                    prepared,
                    name.as_ref(),
                    type_arguments.as_ref(),
                    from_root_body,
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
            // Synthetic carriers contribute no own-body surface — they
            // are intrinsic terminal leaves, equivalent to `Empty` here.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Infer { .. } => PreparedSurfaceProjection::Empty,
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::TypeOf(_) => PreparedSurfaceProjection::Unsupported,
        }
    }

    /// Ref-dispatch `from_root_body` threading.
    ///
    /// Mirrors the parser's utility-type handling (decl.rs +
    /// `try_resolve_heritage_utility_type`): the FIRST type-argument of
    /// `Pick`/`Omit`/`Partial`/`Required`/`Readonly`/`NonNullable` is a
    /// heritage-like descent that descends with `from_root_body=false`.
    /// For a non-utility, non-builtin named reference (the `_ => ` arm
    /// below), the walker recurses into the target symbol AT THE
    /// CALLER's `from_root_body` — `defineProps<MyProps>()` where
    /// `MyProps` is a named interface enters `MyProps`'s body at body
    /// position; `extends MyProps` enters at heritage position.
    #[allow(dead_code)]
    fn project_prepared_surface_from_ref(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        name: &str,
        type_arguments: &[TypeExpr],
        from_root_body: bool,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        // Utility types descend into their first argument as a
        // heritage-like step — `from_root_body = false` matches the
        // parser semantic in `try_resolve_heritage_utility_type`.
        // For a non-utility named reference the `_ =>` arm propagates
        // `from_root_body` into the target symbol's body resolution.
        match (name, type_arguments) {
            ("Partial", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    false,
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
                    false,
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
                    false,
                    active,
                ),
                |member| member.readonly = true,
            ),
            ("NonNullable", [inner]) => self.project_prepared_surface_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                &FxHashMap::default(),
                false,
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
                        false,
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
                // Named-type reference: the parser's
                // `resolve_named_local_type_with_ctx_ref` propagates
                // the caller's `from_root_body` into the target's
                // own-body resolution (`MyProps` at body position
                // ⇒ MyProps's own body members get
                // `declared_in_macro_type_arg=true`).
                self.project_prepared_surface_from_symbol(
                    &target_canonical_id,
                    &target_symbol_name,
                    &target_substitutions,
                    from_root_body,
                    active,
                )
            }
        }
    }

    #[allow(dead_code)]
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
            // Pick selects members FROM the target — the resulting
            // members did NOT appear in the consuming macro T's own
            // body literal, so they enter member-projection at
            // `from_root_body=false`.
            let Some(projected_member) = self.project_prepared_requested_member_from_expr(
                scope_canonical_id,
                prepared,
                expr,
                member_name,
                substitutions,
                false,
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

    /// `from_root_body` is part of the cache identity for
    /// `PreparedMemberCacheKey`: different entry contexts (body
    /// position vs. heritage descent) publish distinct
    /// `ProjectedMember` values whose `declared_in_macro_type_arg`
    /// reflects the entry context, so the per-member cache MUST be
    /// keyed on `from_root_body`.
    pub(crate) fn project_prepared_requested_member_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        from_root_body: bool,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        let cache_key = PreparedMemberCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            member_name: member_name.to_string(),
            kind: PreparedMemberCacheKind::Requested,
            substitutions: prepared_substitution_key(substitutions),
            from_root_body,
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
                from_root_body,
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
        // Observe the keyed canonical's content version ONCE here,
        // before any member value is computed, through the view-aware
        // `authoritative_current_content_hash` oracle (overlay-correct
        // under a `SessionResolverContext`). Every
        // `publish_prepared_member_to_host_db` call below threads this
        // single observation into the provenance-pure signature
        // builder so the published `ProjectedMember` and its fact
        // signature root on exactly one `scope_canonical_id` content
        // version. `None` (the canonical has no authoritative current
        // content) refuses shared-cache admission inside the publish
        // helper.
        let observed_keyed_hash = self
            .ctx
            .authoritative_current_content_hash(scope_canonical_id);
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
                            from_root_body,
                            active,
                        );
                        self.publish_prepared_member_to_host_db(
                            scope_canonical_id,
                            symbol_name,
                            member_name,
                            crate::resolver_core::cache_keys::PreparedMemberCacheKind::Requested,
                            substitutions,
                            from_root_body,
                            &result,
                            observed_keyed_hash,
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
                from_root_body,
                &Some(cached.clone()),
                observed_keyed_hash,
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
                    // SAFETY: `PreparedMember` flattens own-body and
                    // heritage-reached members into a single
                    // `member_index` — single-member projection lookup
                    // CANNOT discriminate them at this leaf. The
                    // caller's `from_root_body` is the best evidence
                    // available: at body position the caller wants to
                    // treat the member as own-body unless the member
                    // is structurally heritage-reached (which the
                    // recursive `from_expr` path classifies precisely
                    // for object literals + heritage-utility cases).
                    // The leaf path retains the conservative default
                    // of `from_root_body && true_when_provable` —
                    // simplified here to the caller's flag because
                    // the member-index does not provide the discriminator.
                    let projected_member = ProjectedMember {
                        name: member_name.to_string(),
                        ty: apply_type_param_substitutions(&member.ty, substitutions),
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                        declared_in_macro_type_arg: from_root_body,
                        // `PreparedMember` carries the real OXC declaration-site
                        // spans; substitution rewrites the value type, not the
                        // member's own declaration site.
                        spans: member.spans,
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
                    from_root_body,
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
            from_root_body,
            &result,
            observed_keyed_hash,
        );
        self.prepared_member_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    /// Write-through to ctx-owned `PreparedMemberDb`.
    ///
    /// `observed_keyed_hash` is the keyed canonical
    /// (`scope_canonical_id`)'s content version observed at the value
    /// source by the caller — threaded in here, NOT re-acquired. The
    /// signature builder is provenance-pure: the entry's self-root
    /// `FileWholeHash` and its `MemberPresence` / `Member` parse facts
    /// all root on this one observed version, so the published
    /// `ProjectedMember` value and its fact signature agree on exactly
    /// one content identity. A current-content re-read here would open
    /// a publish race — an `upsert` landing between the value-compute
    /// and this write-through would root a stale member by a
    /// fresh-looking current hash, and `revalidate_after_compute`
    /// (fresh-vs-fresh) could not catch it.
    ///
    /// On a `None` `observed_keyed_hash` (the keyed canonical has no
    /// current shallow state) or a `None` from the signature builder
    /// (the observed version's parse-fact registry is unrecoverable),
    /// the closure `?`-returns `None`, so the entry is NOT admitted to
    /// the shared `PreparedMemberDb`. The freshly-computed value is
    /// still returned to the caller by `project_prepared_requested_member_from_symbol`;
    /// only the shared-cache admission is refused.
    pub(super) fn publish_prepared_member_to_host_db(
        &self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        kind: crate::resolver_core::cache_keys::PreparedMemberCacheKind,
        substitutions: &FxHashMap<String, TypeExpr>,
        from_root_body: bool,
        result: &Option<ProjectedMember>,
        observed_keyed_hash: Option<crate::resolver_core::ResolverHash16>,
    ) {
        let arc_key = arc_prepared_member_cache_key(
            scope_canonical_id,
            symbol_name,
            member_name,
            kind,
            substitutions,
            from_root_body,
        );
        let ctx = self.ctx;
        let host_db = ctx.project_type_store().prepared_member_db();
        let captured_value = result.clone();
        let captured_canonical = scope_canonical_id.to_string();
        let captured_symbol = symbol_name.to_string();
        let captured_member = member_name.to_string();
        let _ = host_db.get_or_compute(&arc_key, ctx, move || {
            // R28 path-precise: observe the exporter+member pair so
            // sibling-member edits in the same exporter keep the
            // consumer warm, and a body edit on the specific member
            // invalidates it. The signature roots on the
            // caller-observed keyed-canonical content version — never
            // a current-content re-read. `?` on a `None` observation
            // or builder result refuses shared-cache admission.
            let fact_sig = engine_fact_signature_for_canonical_member(
                ctx,
                captured_canonical.as_str(),
                captured_symbol.as_str(),
                captured_member.as_str(),
                observed_keyed_hash?,
            )?;
            Some((captured_value, fact_sig))
        });
    }

    /// Single-member projection with `from_root_body` threading.
    ///
    /// Mirrors `project_prepared_surface_from_expr`'s rules: intersection
    /// arms classify per their shape, utility-type inner arg descends
    /// at `from_root_body=false`, named Ref propagates the caller's
    /// flag into the target symbol's body resolution.
    fn project_prepared_requested_member_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        from_root_body: bool,
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
                from_root_body,
                active,
            ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                // Mirror the surface-walker classification: an
                // intersection arm is own-body iff it is a literal
                // object AND the carrier is at body position.
                let arm_from_root_body = arm_is_own_body_literal(part) && from_root_body;
                self.project_prepared_requested_member_from_expr(
                    scope_canonical_id,
                    prepared,
                    part,
                    member_name,
                    substitutions,
                    arm_from_root_body,
                    active,
                )
            }),
            // A `TypeExpr::Object` literal at this position is an
            // own-body literal — the member it contributes declares
            // its name in the consumer's macro T argument iff the
            // caller's `from_root_body` is `true`.
            TypeExpr::Object(object) => object.properties.iter().find_map(|member| match member {
                ObjectMember::Property(property) if property.name == member_name => {
                    Some(ProjectedMember {
                        name: property.name.clone(),
                        ty: apply_type_param_substitutions(&property.ty, substitutions),
                        optional: property.optional,
                        readonly: property.readonly,
                        is_method: false,
                        declared_in_macro_type_arg: from_root_body,
                        // IR property carries its real OXC spans verbatim;
                        // substitution rewrites the value type, not the
                        // member's own declaration site.
                        spans: property.spans,
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
                        declared_in_macro_type_arg: from_root_body,
                        // IR method carries its real OXC spans verbatim;
                        // substitution rewrites the value type, not the
                        // member's own declaration site.
                        spans: method.spans,
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
                        from_root_body,
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
                            false,
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
                            false,
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
                            false,
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
                        false,
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
                            false,
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
                            false,
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
                        // Named-type ref: propagates the caller's
                        // `from_root_body` into the target symbol's
                        // body resolution.
                        self.project_prepared_requested_member_from_symbol(
                            &target_canonical_id,
                            &target_symbol_name,
                            member_name,
                            &target_substitutions,
                            from_root_body,
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

        // Observe BOTH keyed canonicals' content versions ONCE here,
        // before the value is resolved, through the view-aware
        // `authoritative_current_content_hash` oracle (overlay-correct
        // under a `SessionResolverContext`). `PreparedTargetCacheKey`
        // is keyed on the active scope AND the declaring canonical, so
        // both are self-roots. The provenance-pure signature builder
        // roots each self-root on its own observed hash threaded in
        // below — never a current-content re-read inside the closure.
        // A `None` for either (the canonical has no authoritative
        // current content) refuses shared-cache admission; the
        // resolved value is still returned to the caller from
        // `resolved` below.
        let observed_scope_hash = self
            .ctx
            .authoritative_current_content_hash(scope_canonical_id);
        let observed_decl_hash = self
            .ctx
            .authoritative_current_content_hash(prepared.root_identity.canonical_id.as_str());

        // `resolve_prepared_target` returns the resolved
        // `(canonical_source, resolved_name)` AND the observed content
        // version of the FINAL routed declaring file — read from the
        // `PreparedDeclBundle::owner_whole_hash` of the bundle ACTUALLY
        // USED to confirm the resolution. That hash is an observed
        // identity baked into the bundle at materialisation; it is NOT
        // a current-content re-read, so it cannot race a concurrent
        // `upsert` to the routed file the way a post-resolution
        // `authoritative_current_content_hash` call would.
        let resolve_prepared_target =
            |this: &mut Self,
             canonical_source: String,
             resolved_name: String|
             -> Option<(String, String, crate::resolver_core::ResolverHash16)> {
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

                // Fetch the prepared-decl bundle of the FINAL routed
                // declaring file — the same bundle the value resolves
                // through — and capture its observed content version.
                let bundle = this.ctx.prepared_decl_bundle(canonical_source.as_str())?;
                bundle.prepared_type_decls.get(resolved_name.as_str())?;
                Some((canonical_source, resolved_name, bundle.owner_whole_hash))
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
        // The caller-facing result drops the observed-hash component.
        let resolved_pair: Option<(String, String)> =
            resolved.as_ref().map(|(c, n, _)| (c.clone(), n.clone()));
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
                resolved.as_ref().map(|(c, n, _)| {
                    (
                        std::sync::Arc::<str>::from(c.as_str()),
                        std::sync::Arc::<str>::from(n.as_str()),
                    )
                });
            // The FINAL routed declaring canonical + symbol + its
            // observed content version, captured at the value source.
            let captured_routed: Option<(String, String, crate::resolver_core::ResolverHash16)> =
                resolved
                    .as_ref()
                    .map(|(c, n, h)| (c.clone(), n.clone(), *h));
            let captured_canonical = scope_canonical_id.to_string();
            let captured_target = name.to_string();
            let captured_decl_canonical = prepared.root_identity.canonical_id.clone();
            let captured_decl_symbol = prepared.root_identity.symbol_name.clone();
            let _ = host_db.get_or_compute(&arc_key, ctx, move || {
                // PreparedTarget caches a (scope, target-name) →
                // resolved (canonical, symbol) mapping. The entry has
                // up to THREE self-roots: the active scope, the
                // original declaring canonical
                // (`root_identity.canonical_id`), AND the FINAL routed
                // declaring canonical. The signature builder is
                // provenance-pure: each self-root roots on the content
                // version observed for that file at the value source
                // (`observed_scope_hash` / `observed_decl_hash` /
                // `captured_routed`'s observed hash from the
                // prepared-decl bundle), threaded in here — never a
                // current-content re-read. `?` on a `None` observation
                // (a canonical has no current shallow state) or builder
                // result refuses shared-cache admission; the resolved
                // value is still returned to the caller from
                // `resolved_pair`.
                //
                // Re-route boundary: `resolve_prepared_target` (the
                // closure above) re-routes `canonical_source` to a
                // THIRD declaring file via
                // `resolve_named_type_export_target_shallow` when the
                // requested name re-exports through an intermediate
                // module. `PreparedTargetCacheKey` only encodes the
                // active scope + original declaring canonical, so the
                // entry carries the routed canonical explicitly in its
                // `self_root_canonicals` and the signature roots it —
                // a content edit to that third file rejects the entry.
                let observed_scope_hash = observed_scope_hash?;
                let observed_decl_hash = observed_decl_hash?;
                let routed_decl_ref: Option<(&str, &str, crate::resolver_core::ResolverHash16)> =
                    captured_routed
                        .as_ref()
                        .map(|(c, n, h)| (c.as_str(), n.as_str(), *h));
                let fact_sig = super::engine_fact_signature_for_prepared_target(
                    ctx,
                    captured_canonical.as_str(),
                    captured_target.as_str(),
                    observed_scope_hash,
                    captured_decl_canonical.as_str(),
                    captured_decl_symbol.as_str(),
                    observed_decl_hash,
                    routed_decl_ref,
                )?;
                // Self-root canonical set: active scope + original
                // declaring canonical + final routed declaring
                // canonical (deduped).
                let mut self_root_canonicals: Vec<std::sync::Arc<str>> = vec![
                    std::sync::Arc::<str>::from(captured_canonical.as_str()),
                    std::sync::Arc::<str>::from(captured_decl_canonical.as_str()),
                ];
                if let Some((routed_canonical, _, _)) = captured_routed.as_ref() {
                    let routed_arc = std::sync::Arc::<str>::from(routed_canonical.as_str());
                    if !self_root_canonicals.contains(&routed_arc) {
                        self_root_canonicals.push(routed_arc);
                    }
                }
                Some((
                    captured_value,
                    fact_sig,
                    std::sync::Arc::from(self_root_canonicals),
                ))
            });
        }
        self.prepared_target_cache
            .borrow_mut()
            .insert(cache_key, resolved_pair.clone());
        resolved_pair
    }
}

pub(super) fn arm_is_own_body_literal(expr: &TypeExpr) -> bool {
    let mut cursor = expr;
    loop {
        match cursor {
            TypeExpr::Object(_) => return true,
            TypeExpr::Parenthesized(inner) => cursor = inner,
            _ => return false,
        }
    }
}

/// Merge the projected surfaces of an intersection's arms under the
/// non-fatal-unsupported rule.
///
/// **TypeScript semantics**: `A & B` publishes the union of A's and
/// B's members. A non-fatal `Unsupported` arm contributes nothing but
/// MUST NOT invalidate the contributions of resolvable sibling arms.
/// The intersection itself is `Unsupported` only when EVERY arm
/// fails to resolve (the `saw_resolved_arm == false` case).
///
/// The inverse short-circuit — returning `Unsupported` on the first
/// `Unsupported` arm — drops body members of `AuthForm.vue` /
/// `Form.vue` / `Table.vue` on the nuxt-ui bench corpus. The
/// `// Skip` branch below is the load-bearing invariant; the
/// discriminating unit test
/// `merge_prepared_intersection_arms_skips_unsupported_arm_when_sibling_resolves`
/// guards against re-introduction.
///
/// Extracted into a pure helper so the intersection-merge rule is
/// unit-testable in isolation — see
/// `intersection_merge_tests::merge_prepared_intersection_arms_*`.
/// The unit tests discriminate directly against the
/// `saw_resolved_arm` logic without relying on the higher-level
/// component-meta pipeline (which has multiple rescue paths a
/// synthetic single-component fixture inadvertently exercises).
pub(super) fn merge_prepared_intersection_arms(
    arms: Vec<PreparedSurfaceProjection>,
) -> PreparedSurfaceProjection {
    let mut surfaces = Vec::with_capacity(arms.len());
    let mut saw_resolved_arm = false;
    for arm in arms {
        match arm {
            PreparedSurfaceProjection::Surface(surface) => {
                saw_resolved_arm = true;
                surfaces.push(surface);
            }
            PreparedSurfaceProjection::Empty => {
                saw_resolved_arm = true;
            }
            PreparedSurfaceProjection::Unsupported => {
                // Skip — contributes nothing to the intersection's
                // surface but does not invalidate sibling arms.
            }
        }
    }
    if !saw_resolved_arm {
        return PreparedSurfaceProjection::Unsupported;
    }
    projected_surface_from_parts_intersection(surfaces)
}

#[cfg(test)]
#[path = "prepared_surface_tests.rs"]
mod intersection_merge_tests;
