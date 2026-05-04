//! `impl VerterHost` — import-route + dependency canonical resolution.
//!
//! Owns the helpers that drive route resolution / canonical-ID lookup
//! before any external type traversal happens:
//! - Wrappers around the project-store-owned `RouteOwnedShallowDb`
//!   (`invalidate_route_owned_shallow_cache`,
//!   `snapshot_route_owned_shallow_cache_entries`).
//! - `expand_relative_candidates` — pre-snapshot blocker hydration probe.
//! - `authoritative_import_route` + `import_route_target` /
//!   `import_route_is_known_miss` predicates.
//! - The runtime/declaration target classifiers
//!   (`runtime_like_dependency_target`,
//!   `declaration_like_dependency_target`,
//!   `runtime_dependency_target`).
//! - `shallow_route_dependency_target` for shallow re-export consultation.
//! - `prefer_type_dependency_target_from_resolution` /
//!   `normalize_live_type_dependency_target` /
//!   `fallback_relative_type_companion`.
//! - The cache-write helpers `cache_import_route_result` and
//!   `resolve_workspace_dependency_and_cache`.
//! - The public `resolve_loaded_dependency_canonical`,
//!   `resolve_type_dependency_canonical`, and
//!   `resolve_type_dependency_canonical_shallow` entry points.

use super::frontier_helpers::RouteOwnedShallowStateSnapshot;
use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

impl VerterHost {
    /// invalidate the route-only shallow entry for a
    /// canonical via the project-store-owned
    /// [`RouteOwnedShallowDb`](crate::project_type_store::RouteOwnedShallowDb).
    /// The pre-migration host-mutex (`route_owned_shallow_cache`) is gone;
    /// this thin wrapper keeps the existing call site
    /// (`host_manage::set_import_dependencies`) working through one indirection
    /// while the body delegates to the store DB. Future cleanup may inline
    /// the call.
    pub(crate) fn invalidate_route_owned_shallow_cache(&self, canonical_id: &str) {
        self.project_type_store
            .route_owned_shallow()
            .remove(canonical_id);
    }

    /// snapshot the route-only shallow entries from the
    /// project-store DB for fact-capture (`resolver_store::derived_hashes`).
    /// Iteration is across `DashMap<Arc<str>, Arc<RouteOwnedShallowEntry>>`;
    /// the projection ([`RouteOwnedShallowStateSnapshot`]) carries the
    /// minimal `(canonical_id, whole_hash, optional route_hash)` shape
    /// consumed by `resolver_store.rs:137`.
    pub(crate) fn snapshot_route_owned_shallow_cache_entries(
        &self,
    ) -> Vec<RouteOwnedShallowStateSnapshot> {
        self.project_type_store
            .route_owned_shallow()
            .for_each_entry(|canonical_id, entry| {
                RouteOwnedShallowStateSnapshot::from_entry(canonical_id, entry)
            })
    }

    /// Expand a relative import specifier into all candidate canonical IDs.
    ///
    /// Given an owner file and a relative specifier (e.g. `./types`), returns
    /// a list of candidates: the direct path, then with each resolve extension,
    /// then `/index` variants. Used by pre-snapshot blocker hydration to probe
    /// the filesystem without a full resolver.
    pub fn expand_relative_candidates(
        &self,
        owner_canonical: &str,
        specifier: &str,
    ) -> Vec<String> {
        let direct = crate::id::resolve_external(owner_canonical, specifier);
        let mut candidates = vec![direct.clone()];
        for ext in &self.config.resolve_extensions {
            candidates.push(format!("{direct}{ext}"));
        }
        for ext in &self.config.resolve_extensions {
            candidates.push(format!("{direct}/index{ext}"));
        }
        candidates
    }

    pub(crate) fn authoritative_import_route(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<crate::types::DependencyResolution> {
        let (resolution, source_kind) = if let Some(resolution) =
            self.cached_import_route_resolution(owner_canonical, import_source)
        {
            (Some(resolution), "host-cache")
        } else {
            (
                self.ensure_indexed_ready(owner_canonical)?
                    .import_routes
                    .get(import_source)
                    .cloned(),
                "indexed_ready",
            )
        };

        component_meta_trace_custom!(
            "authoritative_import_route_result",
            format!(
                "owner={} import={} source={} target={}",
                owner_canonical,
                import_source,
                source_kind,
                resolution
                    .as_ref()
                    .and_then(Self::import_route_target)
                    .as_deref()
                    .unwrap_or("<none>"),
            ),
        );
        resolution
    }

    pub(crate) fn import_route_target(
        resolution: &crate::types::DependencyResolution,
    ) -> Option<String> {
        resolution
            .resolved_canonical_id
            .clone()
            .or_else(|| resolution.effective_target().map(str::to_string))
    }

    pub(crate) fn import_route_is_known_miss(
        resolution: &crate::types::DependencyResolution,
    ) -> bool {
        resolution.resolved_canonical_id.is_none()
            && resolution.effective_target().is_none()
            && resolution.possible_canonical_ids.is_empty()
    }

    fn runtime_like_dependency_target(path: &str) -> bool {
        path.ends_with(".js")
            || path.ends_with(".jsx")
            || path.ends_with(".mjs")
            || path.ends_with(".cjs")
    }

    fn declaration_like_dependency_target(path: &str) -> bool {
        path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts")
    }

    fn runtime_dependency_target(
        resolution: &crate::types::DependencyResolution,
    ) -> Option<String> {
        let resolved = Self::import_route_target(resolution)?;
        (!Self::declaration_like_dependency_target(&resolved)).then_some(resolved)
    }

    fn shallow_route_dependency_target(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        let state = &self.ensure_indexed_ready(owner_canonical)?.shallow_state;
        state
            .exports
            .values()
            .find_map(|target| match target {
                crate::resolver_core::ExportTarget::Reexport {
                    source_specifier,
                    canonical_id,
                    ..
                } if source_specifier == import_source && !canonical_id.is_empty() => {
                    Some(canonical_id.clone())
                }
                _ => None,
            })
            .or_else(|| {
                state
                    .wildcard_reexports
                    .iter()
                    .find(|target| {
                        target.source_specifier == import_source && !target.canonical_id.is_empty()
                    })
                    .map(|target| target.canonical_id.clone())
            })
    }

    pub(crate) fn prefer_type_dependency_target_from_resolution(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolution: &crate::types::DependencyResolution,
    ) -> Option<String> {
        if let Some(candidate) = resolution
            .possible_canonical_ids
            .iter()
            .min_by_key(|candidate| crate::types::extension_priority(candidate))
        {
            return Some(candidate.clone());
        }

        let resolved = Self::import_route_target(resolution)?;
        if !import_source.starts_with('.') && Self::runtime_like_dependency_target(&resolved) {
            if let Some(resolved_type) = self
                .ws()
                .resolve_import(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .map(|resolution| resolution.source_id)
            {
                return Some(resolved_type);
            }
        }

        Some(resolved.to_string())
    }

    pub(crate) fn normalize_live_type_dependency_target(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolved: &str,
    ) -> String {
        if let Some(fallback) = self.resolve_eval_dependency_canonical(resolved) {
            if fallback != resolved {
                return fallback;
            }
        }

        if !import_source.starts_with('.') && Self::runtime_like_dependency_target(resolved) {
            if let Some(resolved_type) = self
                .ws()
                .resolve_import(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .map(|resolution| resolution.source_id)
            {
                return resolved_type;
            }
        }

        resolved.to_string()
    }

    pub(crate) fn fallback_relative_type_companion(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        if !import_source.starts_with('.') || !Self::runtime_like_dependency_target(import_source) {
            return None;
        }
        let direct = crate::id::resolve_external(owner_canonical, import_source);
        self.resolve_eval_dependency_canonical(direct.as_str())
    }

    pub(super) fn cache_import_route_result(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolved_canonical_id: &str,
    ) {
        let resolution = crate::types::DependencyResolution {
            specifier: import_source.to_string(),
            resolved_canonical_id: Some(resolved_canonical_id.to_string()),
            possible_canonical_ids: vec![resolved_canonical_id.to_string()],
        };

        // import_routes is on DerivedRawState; dependencies is on
        // DependencyState (D48 split).
        {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(owner_canonical.to_string())
                .or_default();
            derived_ref
                .value_mut()
                .import_routes
                .insert(import_source.to_string(), resolution.clone());
        }
        {
            let mut dep_ref = self
                .dependency_cache()
                .entry(owner_canonical.to_string())
                .or_default();
            dep_ref
                .value_mut()
                .dependencies
                .insert(resolved_canonical_id.to_string());
        }
    }

    fn resolve_workspace_dependency_and_cache(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> Option<String> {
        let resolved = self
            .ws()
            .resolve_import(
                owner_canonical,
                import_source,
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind,
                },
            )?
            .source_id;
        self.cache_import_route_result(owner_canonical, import_source, &resolved);
        Some(resolved)
    }

    pub(crate) fn resolve_loaded_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> Option<String> {
        if let Some(existing) = self.authoritative_import_route(owner_canonical, import_source) {
            let cached = if kind == verter_workspace::ResolveRequestKind::TypeImport {
                Self::import_route_target(&existing)
            } else {
                Self::runtime_dependency_target(&existing)
            };
            if let Some(resolved) = cached {
                // For type imports, prefer declaration companion (.d.ts) over
                // runtime files (.js) when both exist.
                if kind == verter_workspace::ResolveRequestKind::TypeImport
                    && Self::runtime_like_dependency_target(&resolved)
                {
                    return Some(self.normalize_live_type_dependency_target(
                        owner_canonical,
                        import_source,
                        &resolved,
                    ));
                }
                return Some(resolved);
            }
            if Self::import_route_is_known_miss(&existing) {
                return None;
            }
        }

        let resolved =
            self.resolve_workspace_dependency_and_cache(owner_canonical, import_source, kind)?;
        // For type imports, normalize through declaration companion preference
        // (.d.ts over .js) when both exist.
        if kind == verter_workspace::ResolveRequestKind::TypeImport
            && Self::runtime_like_dependency_target(&resolved)
        {
            return Some(self.normalize_live_type_dependency_target(
                owner_canonical,
                import_source,
                &resolved,
            ));
        }
        Some(resolved)
    }

    pub(crate) fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        if let Some(resolved) = self
            .authoritative_import_route(owner_canonical, import_source)
            .and_then(|resolution| {
                self.prefer_type_dependency_target_from_resolution(
                    owner_canonical,
                    import_source,
                    &resolution,
                )
            })
        {
            return Some(resolved);
        }
        if self
            .authoritative_import_route(owner_canonical, import_source)
            .is_some_and(|resolution| Self::import_route_is_known_miss(&resolution))
        {
            return None;
        }

        let type_resolved = self
            .resolve_loaded_dependency_canonical(
                owner_canonical,
                import_source,
                verter_workspace::ResolveRequestKind::TypeImport,
            )
            .map(|resolved| {
                self.normalize_live_type_dependency_target(
                    owner_canonical,
                    import_source,
                    resolved.as_str(),
                )
            })
            .or_else(|| self.fallback_relative_type_companion(owner_canonical, import_source));
        let esm_resolved = type_resolved.as_ref().is_none().then(|| {
            self.resolve_loaded_dependency_canonical(
                owner_canonical,
                import_source,
                verter_workspace::ResolveRequestKind::EsmImport,
            )
        });
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "resolve_type_dependency owner={} import={} type={:?} esm={:?}",
                owner_canonical, import_source, type_resolved, esm_resolved,
            ));
        }
        type_resolved.or(esm_resolved.flatten())
    }

    #[allow(dead_code)]
    pub(crate) fn resolve_type_dependency_canonical_shallow(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        if let Some(existing) = self.authoritative_import_route(owner_canonical, import_source) {
            if let Some(resolved) = self.prefer_type_dependency_target_from_resolution(
                owner_canonical,
                import_source,
                &existing,
            ) {
                return Some(resolved);
            }
            if Self::import_route_is_known_miss(&existing) {
                return None;
            }
        }

        if let Some(resolved) = self.shallow_route_dependency_target(owner_canonical, import_source)
        {
            return Some(resolved);
        }

        let resolved = self
            .ws()
            .resolve_import(
                owner_canonical,
                import_source,
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind: verter_workspace::ResolveRequestKind::TypeImport,
                },
            )?
            .source_id;
        let resolution = crate::types::DependencyResolution {
            specifier: import_source.to_string(),
            resolved_canonical_id: Some(resolved.clone()),
            possible_canonical_ids: vec![resolved.clone()],
        };
        let preferred = self
            .prefer_type_dependency_target_from_resolution(
                owner_canonical,
                import_source,
                &resolution,
            )
            .unwrap_or(resolved);

        self.cache_import_route_result(owner_canonical, import_source, &preferred);
        Some(preferred)
    }
}
