//! `impl VerterHost` — frontier-driven external type resolution and the
//! component-meta macro element/surface entry points.
//!
//! Owns the resolution body for
//! `resolve_external_type_from_loaded_files_with_view` (the BFS frontier
//! driver, depth/step gates, host-cache lookup, route materialisation, and
//! post-publish cache writes) plus the component-meta macro hooks that
//! build on it:
//! - `resolve_component_meta_macro_elements_target_with_view`
//! - `build_imported_macro_declaration_from_target`
//! - `resolve_component_meta_macro_surface_with_view`
//! - `resolve_component_meta_macro_elements_with_view`
//!
//! Each `_with_view` helper has a base wrapper (`#[cfg(test)]`-gated) that
//! passes `view = None`; production paths reach the view-aware variant via
//! `HostExternalMacroTypeCollector` and `HostComponentMetaResolver`.

use super::frontier_helpers::{
    emit_external_type_from_loaded_files_trace_result, external_type_debug,
    external_type_trace_error_status, external_type_trace_success_status,
    DirectComponentMetaDeclarationResolver, ExternalTypeCache, ExternalTypeTraceBaseline,
    FrontierCompanionPlans, FrontierRequestedRoutes,
};
use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

impl VerterHost {
    /// Base wrapper that fixes `view = None`. Only the integration tests
    /// under `#[cfg(test)]` modules (`host_resolve_tests`, `frontier_tests`,
    /// `host_manage_tests`) call this directly; production paths flow
    /// through [`Self::resolve_external_type_from_loaded_files_with_view`]
    /// via [`super::external_macro_collector::HostExternalMacroTypeCollector`].
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn resolve_external_type_from_loaded_files(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        required_root_dep: bool,
        kind: verter_workspace::ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
    ) -> Result<
        Option<verter_parser::utils::oxc::script::type_surface::ResolvedElements>,
        crate::types::ExternalTypeResolveError,
    > {
        // Test-only wrapper: construct a bare-host ctx so tightened tests
        // do not need to assemble the request boundary inline.
        crate::resolver_core::with_bare_host_ctx_for_test(self, |ctx| {
            self.resolve_external_type_from_loaded_files_with_view(
                ctx,
                owner_canonical,
                import_source,
                type_name,
                tracked_deps,
                resolution_deps,
                cache,
                visiting,
                required_root_dep,
                kind,
                use_host_cache,
                profile_hash,
                depth,
                None,
            )
        })
    }

    /// View-aware variant of [`Self::resolve_external_type_from_loaded_files`].
    ///
    /// When `view` is `Some`, every nested step — dependency resolution, the
    /// route-frontier closure, dep-source reads, and the resolved-type cache —
    /// observes the session overlay. Base callers (`view = None`) get the
    /// historical base-only behaviour.
    #[allow(clippy::too_many_arguments)]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn resolve_external_type_from_loaded_files_with_view(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        required_root_dep: bool,
        kind: verter_workspace::ResolveRequestKind,
        use_host_cache: bool,
        // Threaded by the `ExternalMacroTypeCollectorHost` contract; this
        // resolution body does not key on the profile (request-local dedupe
        // only, no profile-scoped warm admission).
        _profile_hash: Option<u64>,
        depth: usize,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Result<
        Option<verter_parser::utils::oxc::script::type_surface::ResolvedElements>,
        crate::types::ExternalTypeResolveError,
    > {
        component_meta_trace_custom!(
            "resolve_external_type_from_loaded_files",
            format!(
                "owner={} import={} type={} depth={} required_root_dep={} kind={kind:?} store_view={} cache_entries={} visiting={} use_host_cache={}",
                owner_canonical,
                import_source,
                type_name,
                depth,
                required_root_dep,
                false,
                cache.len(),
                visiting.len(),
                use_host_cache,
            ),
        );
        let trace_baseline =
            ExternalTypeTraceBaseline::capture(tracked_deps, resolution_deps, cache);
        let emit_trace_result = |status: &'static str,
                                 tracked_len: usize,
                                 resolution_len: usize,
                                 cache_len: usize,
                                 visiting_len: usize| {
            emit_external_type_from_loaded_files_trace_result(
                owner_canonical,
                import_source,
                type_name,
                status,
                trace_baseline,
                tracked_len,
                resolution_len,
                cache_len,
                visiting_len,
                false,
            );
        };

        if depth >= crate::types::MAX_RESOLVE_DEPTH {
            let err = crate::types::ExternalTypeResolveError::DepthLimitExceeded {
                limit: crate::types::MAX_RESOLVE_DEPTH,
                type_name: type_name.to_string(),
                last_dep: owner_canonical.to_string(),
            };
            emit_trace_result(
                external_type_trace_error_status(&err),
                tracked_deps.len(),
                resolution_deps.len(),
                cache.len(),
                visiting.len(),
            );
            return Err(err);
        }

        let Some(dep_canonical) =
            self.resolve_loaded_dependency_canonical(owner_canonical, import_source, kind)
        else {
            if required_root_dep {
                let err = crate::types::ExternalTypeResolveError::MissingRootDependency;
                emit_trace_result(
                    external_type_trace_error_status(&err),
                    tracked_deps.len(),
                    resolution_deps.len(),
                    cache.len(),
                    visiting.len(),
                );
                return Err(err);
            }
            emit_trace_result(
                external_type_trace_success_status(false),
                tracked_deps.len(),
                resolution_deps.len(),
                cache.len(),
                visiting.len(),
            );
            return Ok(None);
        };

        tracked_deps.insert(dep_canonical.clone());
        resolution_deps.insert(dep_canonical.clone());
        let mut requested_routes = FrontierRequestedRoutes::default();
        requested_routes.insert(
            (dep_canonical.clone(), type_name.to_string()),
            crate::resolver_core::RouteDemand::Whole,
        );

        let cache_key = (dep_canonical.clone(), type_name.to_string());
        if let Some(cached) = cache.get(&cache_key) {
            emit_trace_result(
                external_type_trace_success_status(cached.is_some()),
                tracked_deps.len(),
                resolution_deps.len(),
                cache.len(),
                visiting.len(),
            );
            return Ok(cached.clone());
        }

        let mut companion_plans = FrontierCompanionPlans::default();
        let (frontier, target, had_route_cycle) = match self
            .run_external_type_frontier_closure_with_view(
                ctx,
                dep_canonical.as_str(),
                type_name,
                &mut requested_routes,
                &mut companion_plans,
                view,
            ) {
            Ok(result) => result,
            Err(err) => {
                emit_trace_result(
                    external_type_trace_error_status(&err),
                    tracked_deps.len(),
                    resolution_deps.len(),
                    cache.len(),
                    visiting.len(),
                );
                return Err(err);
            }
        };
        let touched_ids = frontier.touched_canonical_ids();

        for touched_id in touched_ids {
            tracked_deps.insert(touched_id.clone());
            resolution_deps.insert(touched_id);
        }

        let Some(target) = target else {
            // Per-request audit attribution. The cross-file frontier
            // closure resolved this `(owner, type_name)` pair to
            // `target = None` — broken import chain. The session
            // helper bumps `frontier_closure_invocations_target_none`
            // always, and bumps
            // `frontier_closure_redundant_target_none_pairs` when the
            // same pair was already observed earlier in this request
            // — the dominant signal for the "cross-request
            // negative-resolution caching defect" hypothesis. The
            // host-owned resolved-type cache stores positive entries
            // only and is consulted only AFTER `target = Some`, so a
            // `None` outcome here ALWAYS proxies "no negative cache
            // entry short-circuited the closure" — emit
            // `ResolvedExternalTypeCacheNegativeMiss` to surface the
            // cost separately from the redundant-pair count.
            if let Some(req_ctx) = crate::request_context::current_request_context() {
                req_ctx.observe_frontier_target_none_for_pair(owner_canonical, type_name);
            }
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::ResolvedExternalTypeCacheNegativeMiss);
            }
            if self
                .ensure_indexed_ready_serve(dep_canonical.as_str())
                .is_none()
            {
                if required_root_dep {
                    let err = crate::types::ExternalTypeResolveError::MissingRootDependency;
                    emit_trace_result(
                        external_type_trace_error_status(&err),
                        tracked_deps.len(),
                        resolution_deps.len(),
                        cache.len(),
                        visiting.len(),
                    );
                    return Err(err);
                }
                emit_trace_result(
                    external_type_trace_success_status(false),
                    tracked_deps.len(),
                    resolution_deps.len(),
                    cache.len(),
                    visiting.len(),
                );
                return Ok(None);
            }

            if had_route_cycle {
                self.provenance
                    .resolver_cycle_detections
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }

            cache.insert(cache_key.clone(), None);
            emit_trace_result(
                external_type_trace_success_status(false),
                tracked_deps.len(),
                resolution_deps.len(),
                cache.len(),
                visiting.len(),
            );
            return Ok(None);
        };

        let (effective_dep_canonical, effective_type_name) = target;
        tracked_deps.insert(effective_dep_canonical.clone());
        resolution_deps.insert(effective_dep_canonical.clone());

        let final_target_key = (effective_dep_canonical.clone(), effective_type_name.clone());
        if let Some(cached) = cache.get(&final_target_key).cloned() {
            cache.insert(cache_key.clone(), cached.clone());
            emit_trace_result(
                external_type_trace_success_status(cached.is_some()),
                tracked_deps.len(),
                resolution_deps.len(),
                cache.len(),
                visiting.len(),
            );
            return Ok(cached);
        }

        if !visiting.insert(final_target_key.clone()) {
            self.provenance
                .resolver_cycle_detections
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            external_type_debug(format!(
                "resolve_external_type cycle dep={} type={}",
                effective_dep_canonical, effective_type_name
            ));
            cache.insert(cache_key.clone(), None);
            emit_trace_result(
                external_type_trace_success_status(false),
                tracked_deps.len(),
                resolution_deps.len(),
                cache.len(),
                visiting.len(),
            );
            return Ok(None);
        }

        // The indexed-ready fall-through that used to follow a frontier miss
        // (the parser's structural element expander) is severed on THIS
        // compile-facing loaded-files rail: the element payload authority for
        // compile positions is the collector's kind-aware dispatch-backed
        // normalizers (`imported_emits_resolved_elements` /
        // `imported_props_resolved_elements` / the per-name companion fold).
        // The frontier walk below still records companion routes, dependency
        // facts, and cycle detections.
        let resolved = self.materialize_frontier_resolved_type_with_view(
            ctx,
            &frontier,
            &requested_routes,
            &mut companion_plans,
            effective_dep_canonical.as_str(),
            effective_type_name.as_str(),
            tracked_deps,
            resolution_deps,
            view,
        );
        visiting.remove(&final_target_key);

        cache.insert(cache_key.clone(), resolved.clone());
        cache.insert(
            (effective_dep_canonical.clone(), effective_type_name.clone()),
            resolved.clone(),
        );
        emit_trace_result(
            external_type_trace_success_status(resolved.is_some()),
            tracked_deps.len(),
            resolution_deps.len(),
            cache.len(),
            visiting.len(),
        );
        Ok(resolved)
    }

    /// View-aware macro-elements-target resolver.
    ///
    /// Propagates `view` into the frontier closure, the materialiser, the
    /// indexed-ready fall-through, and the eventual call out to
    /// `resolve_external_type_from_loaded_files_with_view` so the
    /// resolved-type cache slot and the dep-source reads observe the active
    /// session overlay.
    ///
    /// The resolved payload is the [`crate::resolver_core::ResolvedMacroElements`]
    /// bundle — the legacy elements DTO PLUS the keep-all `native_props`
    /// rows projected from the SAME dispatch surface resolution — memoized
    /// per request in the cache's DEDICATED macro-elements slot family.
    #[allow(clippy::too_many_arguments)]
    fn resolve_component_meta_macro_elements_target_with_view(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<(
        String,
        String,
        String,
        crate::resolver_core::ResolvedMacroElements,
    )> {
        let dep_canonical = self.resolve_loaded_dependency_canonical(
            owner_canonical,
            import_source,
            verter_workspace::ResolveRequestKind::TypeImport,
        )?;

        tracked_deps.insert(dep_canonical.clone());
        resolution_deps.insert(dep_canonical.clone());

        let cache_key = (dep_canonical.clone(), type_name.to_string());
        if let Some(cached) = cache.macro_elements(&cache_key).cloned() {
            let resolution = cached?;
            // Re-query the project-global `ImportedRootDb` for the target
            // identity. It collapses concurrent cold requests internally, so
            // repeated calls are cheap warm hits — there is no need for a
            // second per-request memo layer above it. Route through `ctx`
            // so request-bound callers exercise the overlay-aware view.
            let (target_canonical, target_name) =
                ctx.resolve_imported_type_root(dep_canonical.as_str(), type_name);
            tracked_deps.insert(target_canonical.clone());
            resolution_deps.insert(target_canonical.clone());
            return Some((dep_canonical, target_canonical, target_name, resolution));
        }

        let (seed_canonical, seed_type_name) =
            ctx.resolve_imported_type_root(dep_canonical.as_str(), type_name);
        tracked_deps.insert(seed_canonical.clone());
        resolution_deps.insert(seed_canonical.clone());

        let seed_target_key = (seed_canonical.clone(), seed_type_name.clone());
        if let Some(cached) = cache.macro_elements(&seed_target_key).cloned() {
            cache.insert_macro_elements(cache_key, cached.clone());
            let resolution = cached?;
            return Some((dep_canonical, seed_canonical, seed_type_name, resolution));
        }

        let mut requested_routes = FrontierRequestedRoutes::default();
        requested_routes.insert(
            (seed_canonical.clone(), seed_type_name.clone()),
            crate::resolver_core::RouteDemand::Whole,
        );

        let mut companion_plans = FrontierCompanionPlans::default();
        let (frontier, target, had_route_cycle) = self
            .run_external_type_frontier_closure_with_view(
                ctx,
                seed_canonical.as_str(),
                seed_type_name.as_str(),
                &mut requested_routes,
                &mut companion_plans,
                view,
            )
            .ok()?;

        for touched_id in frontier.touched_canonical_ids() {
            tracked_deps.insert(touched_id.clone());
            resolution_deps.insert(touched_id);
        }

        let Some((effective_dep_canonical, effective_type_name)) = target else {
            if had_route_cycle {
                self.provenance
                    .resolver_cycle_detections
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            cache.insert_macro_elements(cache_key, None);
            return None;
        };

        tracked_deps.insert(effective_dep_canonical.clone());
        resolution_deps.insert(effective_dep_canonical.clone());

        let final_target_key = (effective_dep_canonical.clone(), effective_type_name.clone());
        if let Some(cached) = cache.macro_elements(&final_target_key).cloned() {
            cache.insert_macro_elements(cache_key, cached.clone());
            let resolution = cached?;
            return Some((
                dep_canonical,
                effective_dep_canonical,
                effective_type_name,
                resolution,
            ));
        }

        // The owner's DIRECT imports are part of its macro type environment:
        // an ambient side-effect import (`import './aug'`) can contribute
        // `declare module` augmentations to the routed target's declaration.
        // Ensure them BEFORE the dispatch terminal — the `MergedDecl`
        // augmentation stitch enumerates augmenters over INDEXED files, so
        // resolving the target before its augmenter is indexed would memoize
        // an un-stitched surface for the whole request.
        self.ensure_owner_direct_imports_indexed(ctx, owner_canonical);

        // Dispatch-owned terminal (same rail as the loaded-files path above):
        // resolve the routed target's declaration carrier through the ONE
        // shared dispatch and request its EMPTY-PATH one-level `Shallow`
        // surface; the thin combined normalize preserves member values as
        // semantic carriers and projects the keep-all `native_props` rows
        // from the SAME surface. The component-meta REGISTRY entry is
        // published SEPARATELY as the original symbolic Ref (the cold
        // resolver's shallow seed) — this legacy elements DTO is never the
        // registry/type authority, so resolving here cannot fold surfaces
        // the registry must keep shallow (the `keeps_*_symbolic` meta
        // trackers pin that). `None` means a genuine unresolved
        // route/declaration; a transient recursion back-edge returns
        // un-cached.
        let outcome = crate::typeinfo::framework_surface::vue_exec::named_type_elements_outcome(
            ctx,
            effective_dep_canonical.as_str(),
            effective_type_name.as_str(),
            // This rail's consumer (the component-meta `ResolvedMacroMeta`)
            // publishes the keep-all `native_props` rows.
            crate::typeinfo::framework_surface::vue_exec::NativeProjection::Include,
            // The component-meta rail surfaces unresolved arms through the
            // expansion-diagnostics projection, not this elements DTO.
            &mut Vec::new(),
        );
        use crate::typeinfo::framework_surface::vue_exec::NamedTypeElementsOutcome;
        let resolved = match outcome {
            NamedTypeElementsOutcome::Resolved(resolution) => Some(resolution),
            // Transient back-edge — never a cacheable negative.
            NamedTypeElementsOutcome::Recursive => return None,
            NamedTypeElementsOutcome::Miss => None,
        };

        cache.insert_macro_elements(cache_key, resolved.clone());
        cache.insert_macro_elements(final_target_key, resolved.clone());
        resolved.map(|resolution| {
            (
                dep_canonical,
                effective_dep_canonical,
                effective_type_name,
                resolution,
            )
        })
    }

    /// Ensure every DIRECT import target of `owner_canonical` is indexed —
    /// including bindingless side-effect imports (`import './aug'`), the
    /// carriers of ambient `declare module` augmentations. One bounded hop
    /// over the owner's own import list (never a transitive walk); each
    /// ensure is the canonical once-per-generation materialization.
    fn ensure_owner_direct_imports_indexed(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
    ) {
        let Some(serve) = ctx.ensure_indexed_ready_serve(owner_canonical) else {
            return;
        };
        let specifiers: Vec<String> = serve
            .indexed
            .snapshot
            .imports
            .iter()
            .map(|import| import.source.clone())
            .collect();
        for specifier in specifiers {
            if let Some(dep_canonical) =
                self.resolve_type_dependency_canonical(owner_canonical, specifier.as_str())
            {
                let _ = ctx.ensure_indexed_ready_serve(dep_canonical.as_str());
            }
        }
    }

    fn build_imported_macro_declaration_from_target(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        dep_canonical: &str,
        requested_name: &str,
        target_canonical: &str,
        target_name: &str,
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
        self.provenance
            .imported_macro_declaration_builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resolver = DirectComponentMetaDeclarationResolver { host: self };
        let mut declaration = crate::resolver_core::resolve_direct_local_type_declaration(
            &resolver,
            target_canonical,
            target_name,
        )
        .unwrap_or_else(|| {
            crate::meta_resolve::resolve_type_declaration_with_context(
                self,
                ctx,
                dep_canonical,
                requested_name,
            )
        });
        declaration.requested_name = requested_name.to_string();
        if declaration.resolved_name.is_empty() {
            declaration.resolved_name = target_name.to_string();
        }
        if declaration.canonical_source.is_empty() {
            declaration.canonical_source = target_canonical.to_string();
        }
        declaration
    }

    /// Base wrapper that fixes `view = None`. Test-only.
    #[cfg(test)]
    pub(crate) fn resolve_component_meta_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
    ) -> Option<crate::resolver_core::ResolvedImportedMacroSurface> {
        crate::resolver_core::with_bare_host_ctx_for_test(self, |ctx| {
            self.resolve_component_meta_macro_surface_with_view(
                ctx,
                owner_canonical,
                import_source,
                type_name,
                tracked_deps,
                resolution_deps,
                cache,
                None,
            )
        })
    }

    /// View-aware variant of [`Self::resolve_component_meta_macro_surface`].
    ///
    /// Threads `view` into the macro-target resolver so the resolved-type
    /// cache slot and dep-source reads honour the active session overlay.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_component_meta_macro_surface_with_view(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<crate::resolver_core::ResolvedImportedMacroSurface> {
        component_meta_trace_custom!(
            "resolve_component_meta_macro_elements",
            format!(
                "owner={} import={} type={} store_view={} cache_entries={}",
                owner_canonical,
                import_source,
                type_name,
                false,
                cache.macro_elements_len(),
            ),
        );

        let (dep_canonical, effective_dep_canonical, effective_type_name, resolution) = self
            .resolve_component_meta_macro_elements_target_with_view(
                ctx,
                owner_canonical,
                import_source,
                type_name,
                tracked_deps,
                resolution_deps,
                cache,
                view,
            )?;
        Some(crate::resolver_core::ResolvedImportedMacroSurface {
            declaration: self.build_imported_macro_declaration_from_target(
                ctx,
                dep_canonical.as_str(),
                type_name,
                effective_dep_canonical.as_str(),
                effective_type_name.as_str(),
            ),
            resolution,
        })
    }

    /// Base wrapper that fixes `view = None`. Test-only.
    #[cfg(test)]
    pub(crate) fn resolve_component_meta_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
    ) -> Option<crate::resolver_core::ResolvedMacroElements> {
        crate::resolver_core::with_bare_host_ctx_for_test(self, |ctx| {
            self.resolve_component_meta_macro_elements_with_view(
                ctx,
                owner_canonical,
                import_source,
                type_name,
                tracked_deps,
                resolution_deps,
                cache,
                None,
            )
        })
    }

    /// View-aware variant of [`Self::resolve_component_meta_macro_elements`].
    ///
    /// Threads `view` into the macro-target resolver so cross-file macro type
    /// resolution observes the session overlay.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_component_meta_macro_elements_with_view(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<crate::resolver_core::ResolvedMacroElements> {
        self.resolve_component_meta_macro_elements_target_with_view(
            ctx,
            owner_canonical,
            import_source,
            type_name,
            tracked_deps,
            resolution_deps,
            cache,
            view,
        )
        .map(|(_, _, _, resolution)| resolution)
    }
}
