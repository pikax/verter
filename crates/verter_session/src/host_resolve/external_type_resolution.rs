//! `impl VerterHost` — frontier-driven external type resolution and the
//! component-meta macro element/surface entry points.
//!
//! Owns the resolution body for `resolve_external_type_from_loaded_files`
//! (the BFS frontier driver, depth/step gates, host-cache lookup, route
//! materialisation, and post-publish cache writes) plus the component-meta
//! macro hooks that build on it:
//! - `resolve_component_meta_macro_elements_target`
//! - `build_imported_macro_declaration_from_target`
//! - `resolve_component_meta_macro_surface`
//! - `resolve_component_meta_macro_elements`
//!
//! The `current_type_resolution_hash` /
//! `lookup_resolved_external_type_cache` /
//! `store_resolved_external_type_cache` helpers are kept here too so the
//! cache delegation to `ProjectTypeStore::resolved_type_cache()`
//! (Tier 1C-α invariant) stays adjacent to its only callers.

use super::frontier_helpers::{
    emit_external_type_from_loaded_files_trace_result, external_type_debug,
    external_type_trace_error_status, external_type_trace_success_status,
    DirectComponentMetaDeclarationResolver, ExternalTypeCache, ExternalTypeTraceBaseline,
    FrontierCompanionPlans, FrontierRequestedRoutes, ResolvedExternalTypes,
};
use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

impl VerterHost {
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
        Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
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
        let (frontier, target, had_route_cycle) = match self.run_external_type_frontier_closure(
            dep_canonical.as_str(),
            type_name,
            &mut requested_routes,
            &mut companion_plans,
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
            if self.ensure_indexed_ready(dep_canonical.as_str()).is_none() {
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

        if use_host_cache {
            if let Some(entry) = self.lookup_resolved_external_type_cache(
                effective_dep_canonical.as_str(),
                effective_type_name.as_str(),
                kind,
            ) {
                self.provenance
                    .resolved_external_type_cache_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                for dep in &entry.tracked_deps {
                    tracked_deps.insert(dep.clone());
                    resolution_deps.insert(dep.clone());
                }
                let resolved = entry.resolved.clone();
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
                return Ok(resolved);
            }

            if profile_hash.is_none() {
                self.provenance
                    .resolved_external_type_cache_misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

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

        let resolved = self
            .materialize_frontier_resolved_type(
                &frontier,
                &requested_routes,
                &mut companion_plans,
                effective_dep_canonical.as_str(),
                effective_type_name.as_str(),
                tracked_deps,
                resolution_deps,
            )
            .or_else(|| {
                self.resolve_external_type_from_indexed_ready(
                    effective_dep_canonical.as_str(),
                    effective_type_name.as_str(),
                    &ResolvedExternalTypes::default(),
                )
            });
        visiting.remove(&final_target_key);

        if use_host_cache && profile_hash.is_none() {
            self.store_resolved_external_type_cache(
                effective_dep_canonical.as_str(),
                effective_type_name.as_str(),
                kind,
                resolved.clone(),
                resolution_deps.iter().cloned().collect(),
            );
        }

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

    fn resolve_component_meta_macro_elements_target(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
    ) -> Option<(
        String,
        String,
        String,
        verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements,
    )> {
        let dep_canonical = self.resolve_loaded_dependency_canonical(
            owner_canonical,
            import_source,
            verter_workspace::ResolveRequestKind::TypeImport,
        )?;

        tracked_deps.insert(dep_canonical.clone());
        resolution_deps.insert(dep_canonical.clone());

        let cache_key = (dep_canonical.clone(), type_name.to_string());
        if let Some(cached) = cache.get(&cache_key).cloned() {
            let elements = cached?;
            // Re-query the project-global `ImportedRootDb` for the target
            // identity. It collapses concurrent cold requests internally, so
            // repeated calls are cheap warm hits — there is no need for a
            // second per-request memo layer above it.
            let (target_canonical, target_name) =
                self.resolve_imported_type_root(dep_canonical.as_str(), type_name);
            tracked_deps.insert(target_canonical.clone());
            resolution_deps.insert(target_canonical.clone());
            return Some((dep_canonical, target_canonical, target_name, elements));
        }

        let (seed_canonical, seed_type_name) =
            self.resolve_imported_type_root(dep_canonical.as_str(), type_name);
        tracked_deps.insert(seed_canonical.clone());
        resolution_deps.insert(seed_canonical.clone());

        let seed_target_key = (seed_canonical.clone(), seed_type_name.clone());
        if let Some(cached) = cache.get(&seed_target_key).cloned() {
            cache.insert(cache_key, cached.clone());
            let elements = cached?;
            return Some((dep_canonical, seed_canonical, seed_type_name, elements));
        }

        let mut requested_routes = FrontierRequestedRoutes::default();
        requested_routes.insert(
            (seed_canonical.clone(), seed_type_name.clone()),
            crate::resolver_core::RouteDemand::Whole,
        );

        let mut companion_plans = FrontierCompanionPlans::default();
        let (frontier, target, had_route_cycle) = self
            .run_external_type_frontier_closure(
                seed_canonical.as_str(),
                seed_type_name.as_str(),
                &mut requested_routes,
                &mut companion_plans,
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
            cache.insert(cache_key, None);
            return None;
        };

        tracked_deps.insert(effective_dep_canonical.clone());
        resolution_deps.insert(effective_dep_canonical.clone());

        let final_target_key = (effective_dep_canonical.clone(), effective_type_name.clone());
        if let Some(cached) = cache.get(&final_target_key).cloned() {
            cache.insert(cache_key, cached.clone());
            let elements = cached?;
            return Some((
                dep_canonical,
                effective_dep_canonical,
                effective_type_name,
                elements,
            ));
        }

        let resolved = self
            .materialize_frontier_resolved_type(
                &frontier,
                &requested_routes,
                &mut companion_plans,
                effective_dep_canonical.as_str(),
                effective_type_name.as_str(),
                tracked_deps,
                resolution_deps,
            )
            .or_else(|| {
                self.resolve_external_type_from_indexed_ready(
                    effective_dep_canonical.as_str(),
                    effective_type_name.as_str(),
                    &ResolvedExternalTypes::default(),
                )
            });

        cache.insert(cache_key, resolved.clone());
        cache.insert(final_target_key, resolved.clone());
        resolved.map(|elements| {
            (
                dep_canonical,
                effective_dep_canonical,
                effective_type_name,
                elements,
            )
        })
    }

    fn build_imported_macro_declaration_from_target(
        &self,
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
            crate::meta_resolve::resolve_type_declaration(self, dep_canonical, requested_name)
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

    pub(crate) fn resolve_component_meta_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
    ) -> Option<crate::resolver_core::ResolvedImportedMacroSurface> {
        component_meta_trace_custom!(
            "resolve_component_meta_macro_elements",
            format!(
                "owner={} import={} type={} store_view={} cache_entries={}",
                owner_canonical,
                import_source,
                type_name,
                false,
                cache.len(),
            ),
        );

        let (dep_canonical, effective_dep_canonical, effective_type_name, elements) = self
            .resolve_component_meta_macro_elements_target(
                owner_canonical,
                import_source,
                type_name,
                tracked_deps,
                resolution_deps,
                cache,
            )?;
        Some(crate::resolver_core::ResolvedImportedMacroSurface {
            declaration: self.build_imported_macro_declaration_from_target(
                dep_canonical.as_str(),
                type_name,
                effective_dep_canonical.as_str(),
                effective_type_name.as_str(),
            ),
            elements,
        })
    }

    pub(crate) fn resolve_component_meta_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        self.resolve_component_meta_macro_elements_target(
            owner_canonical,
            import_source,
            type_name,
            tracked_deps,
            resolution_deps,
            cache,
        )
        .map(|(_, _, _, elements)| elements)
    }

    fn current_type_resolution_hash(
        &self,
        canonical: &str,
    ) -> Option<crate::resolver_core::ResolverHash16> {
        self.current_or_read_whole_hash(canonical).or_else(|| {
            self.read_dep_source_for_type_resolution(canonical, None)
                .map(|source| crate::hash::hash_16(source.as_bytes()))
        })
    }

    fn lookup_resolved_external_type_cache(
        &self,
        dep_canonical: &str,
        type_name: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> Option<crate::types::ResolvedTypeCacheEntry> {
        let dep_source_hash = self.current_type_resolution_hash(dep_canonical)?;
        let key = crate::types::ResolvedTypeCacheKey {
            dep_canonical_id: dep_canonical.to_string(),
            dep_source_hash,
            type_name: type_name.to_string(),
            resolve_kind: kind,
        };
        // Tier 1C-α — delegate to the rehomed `ResolvedTypeCacheDb`.
        // The DB owns the bounded clear-all-at-cap policy internally.
        self.resolved_type_cache().lookup(&key)
    }

    fn store_resolved_external_type_cache(
        &self,
        dep_canonical: &str,
        type_name: &str,
        kind: verter_workspace::ResolveRequestKind,
        resolved: Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
        tracked_deps: Vec<String>,
    ) {
        let Some(dep_source_hash) = self.current_type_resolution_hash(dep_canonical) else {
            return;
        };

        let key = crate::types::ResolvedTypeCacheKey {
            dep_canonical_id: dep_canonical.to_string(),
            dep_source_hash,
            type_name: type_name.to_string(),
            resolve_kind: kind,
        };
        // Tier 1C-α — `ResolvedTypeCacheDb::insert` honours the
        // bounded clear-all-at-`RESOLVED_TYPE_CACHE_CAP` policy
        // internally. The off-store `parking_lot::Mutex` is gone.
        self.resolved_type_cache().insert(
            key,
            crate::types::ResolvedTypeCacheEntry {
                resolved,
                tracked_deps,
            },
        );
    }
}
