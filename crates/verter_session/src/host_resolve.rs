//! `impl VerterHost` â€” resolve and virtual file retrieval methods.
//!
//! Contains [`VerterHost::resolve`], [`VerterHost::get_virtual_file`],
//! [`VerterHost::list_virtual_files`], and the internal [`VerterHost::compile_entry`]
//! helper that drives on-demand compilation.
//!
//! Cross-file component-meta / analysis rule: host-backed consumers share one
//! resolver and one traversal policy.
//! - `Type` mode resolves symbol identity + canonical source location only.
//! - `Expanded` mode uses the same traversal, then materializes expanded shape.
//! - Component-meta must use the shared expanded path for all macro-facing
//!   surfaces, including Options API metadata.
//! - Traversal only follows imports reachable from the requested declaration graph.
//! - Barrel and `export *` hops must be cached once discovered because repeated
//!   wildcard re-export scans are expensive.

use std::sync::Arc;

use rustc_hash::FxHashMap;

#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "session_metrics")]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
#[cfg(feature = "session_metrics")]
use web_time::Instant;

#[cfg(not(feature = "scheduler"))]
use crate::cache::enforce_profile_cap;
use crate::compile::{assemble_main_module, merge_external_sources};
use crate::hash::compile_profile_hash;
use crate::host_manage::{component_meta_trace_event, component_meta_trace_scope};
use crate::id::{parse_raw_id, render_ids, render_single_id};
#[cfg(not(feature = "scheduler"))]
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;
use oxc_allocator::Allocator;
use verter_compiler::compile::CodegenOptions;
use verter_compiler::compile::{
    compile as compile_sfc, compile_from_parsed, format_import_specifier, VerterCompileOptions,
};
use verter_compiler::utils::oxc::vue::resolve_type::{
    imported_member_name_for_required_alias, required_import_alias_names_for_binding,
};

type ResolvedExternalTypes =
    rustc_hash::FxHashMap<String, verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>;

type ExternalTypeCache = crate::resolver_core::ExternalTypeBodyCache;
type FrontierTargetResult = (
    Option<(String, String)>,
    rustc_hash::FxHashSet<String>,
    bool,
);

fn external_type_debug_enabled() -> bool {
    std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
        || std::env::var_os("VERTER_META_DEBUG").is_some()
}

fn external_type_debug(message: impl AsRef<str>) {
    if external_type_debug_enabled() {
        eprintln!("[verter-meta] {}", message.as_ref());
    }
}

struct HostExternalMacroTypeCollector<'a> {
    host: &'a VerterHost,
}

impl crate::resolver_core::ExternalMacroTypeCollectorHost for HostExternalMacroTypeCollector<'_> {
    type Error = crate::types::ExternalTypeResolveError;

    fn resolve_external_macro_type(
        &self,
        owner_canonical: &str,
        dep: &verter_semantic::analysis::MacroTypeDep,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        profile_hash: Option<u64>,
    ) -> Result<Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>, Self::Error>
    {
        self.host.resolve_external_type_from_loaded_files(
            owner_canonical,
            &dep.import_source,
            &dep.type_name,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            profile_hash,
            0,
        )
    }

    fn map_external_macro_type_error(
        &self,
        owner_canonical: &str,
        dep: &verter_semantic::analysis::MacroTypeDep,
        import_span: Option<verter_span::Span>,
        error: &Self::Error,
    ) -> crate::resolver_core::ExternalMacroTypeDiagnostic {
        let (code, message) = match error {
            crate::types::ExternalTypeResolveError::MissingRootDependency => (
                "HOST_MISSING_MACRO_TYPE_DEP".to_string(),
                format!(
                    "missing macro type dependency '{}' for type '{}' in '{}'",
                    dep.import_source, dep.type_name, owner_canonical
                ),
            ),
            crate::types::ExternalTypeResolveError::DepthLimitExceeded {
                limit,
                type_name,
                last_dep,
            } => (
                "HOST_EXTERNAL_TYPE_DEPTH_LIMIT".to_string(),
                format!(
                    "external type resolution depth limit ({}) exceeded for type '{}' (last dep: '{}')",
                    limit, type_name, last_dep
                ),
            ),
            crate::types::ExternalTypeResolveError::StepLimitExceeded {
                limit,
                type_name,
                last_dep,
            } => (
                "HOST_EXTERNAL_TYPE_STEP_LIMIT".to_string(),
                format!(
                    "external type resolution step budget ({}) exceeded for type '{}' (last dep: '{}')",
                    limit, type_name, last_dep
                ),
            ),
        };

        crate::resolver_core::ExternalMacroTypeDiagnostic {
            code,
            message,
            span: import_span,
        }
    }
}

impl VerterHost {
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

    fn cached_dependency_resolution_in_view(
        &self,
        owner_canonical: &str,
        import_source: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<crate::types::DependencyResolution> {
        if let Some(view) = store_view {
            if let Some(resolution) = view.dependency_resolution(owner_canonical, import_source) {
                component_meta_trace_event!(
                    "cached_dependency_resolution_in_view_result",
                    format!(
                        "owner={} import={} source=store_view target={}",
                        owner_canonical,
                        import_source,
                        Self::dependency_resolution_target(resolution)
                            .as_deref()
                            .unwrap_or("<none>"),
                    ),
                );
                return Some(resolution.clone());
            }
            if view.tracks_whole_hash(owner_canonical) {
                return None;
            }
        }

        if let Some(resolution) = self
            .clone_current_imported_dependency_entry(owner_canonical, store_view)
            .and_then(|entry| entry.dependency_resolutions.get(import_source).cloned())
        {
            component_meta_trace_event!(
                "cached_dependency_resolution_in_view_result",
                format!(
                    "owner={} import={} source=imported_dependency_cache target={}",
                    owner_canonical,
                    import_source,
                    Self::dependency_resolution_target(&resolution)
                        .as_deref()
                        .unwrap_or("<none>"),
                ),
            );
            return Some(resolution);
        }

        self.current_eval_state_in_view(owner_canonical, store_view)?;

        #[cfg(feature = "scheduler")]
        {
            if let Some(resolution) = self
                .compile_cache
                .get(owner_canonical)
                .and_then(|entry| entry.dependency_resolutions.get(import_source).cloned())
            {
                component_meta_trace_event!(
                    "cached_dependency_resolution_in_view_result",
                    format!(
                        "owner={} import={} source=compile_cache target={}",
                        owner_canonical,
                        import_source,
                        Self::dependency_resolution_target(&resolution)
                            .as_deref()
                            .unwrap_or("<none>"),
                    ),
                );
                return Some(resolution);
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let resolution = {
                let files = crate::shared::read_lock(&self.files);
                files
                    .get(owner_canonical)
                    .and_then(|entry| entry.dependency_resolutions.get(import_source).cloned())
            };
            if resolution.is_some() {
                component_meta_trace_event!(
                    "cached_dependency_resolution_in_view_result",
                    format!(
                        "owner={} import={} source=host_files target={}",
                        owner_canonical,
                        import_source,
                        resolution
                            .as_ref()
                            .and_then(Self::dependency_resolution_target)
                            .as_deref()
                            .unwrap_or("<none>"),
                    ),
                );
                return resolution;
            }
        }

        component_meta_trace_event!(
            "cached_dependency_resolution_in_view_result",
            format!(
                "owner={} import={} source={} target={}",
                owner_canonical, import_source, "miss", "<none>",
            ),
        );
        None
    }

    fn dependency_resolution_target(
        resolution: &crate::types::DependencyResolution,
    ) -> Option<String> {
        resolution
            .resolved_canonical_id
            .clone()
            .or_else(|| resolution.effective_target().map(str::to_string))
    }

    fn runtime_like_dependency_target(path: &str) -> bool {
        path.ends_with(".js")
            || path.ends_with(".jsx")
            || path.ends_with(".mjs")
            || path.ends_with(".cjs")
    }

    fn prefer_type_dependency_target_from_resolution_in_view(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolution: &crate::types::DependencyResolution,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        if let Some(candidate) = resolution
            .possible_canonical_ids
            .iter()
            .min_by_key(|candidate| crate::types::extension_priority(candidate))
        {
            return Some(candidate.clone());
        }

        let resolved = Self::dependency_resolution_target(resolution)?;
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

        let _ = store_view;
        Some(resolved.to_string())
    }

    fn normalize_live_type_dependency_target_in_view(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolved: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> String {
        if let Some(fallback) = self.resolve_eval_dependency_canonical_in_view(resolved, store_view)
        {
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

    fn fallback_relative_type_companion_in_view(
        &self,
        owner_canonical: &str,
        import_source: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        if !import_source.starts_with('.') || !Self::runtime_like_dependency_target(import_source) {
            return None;
        }
        let direct = crate::id::resolve_external(owner_canonical, import_source);
        self.resolve_eval_dependency_canonical_in_view(direct.as_str(), store_view)
    }

    fn cache_dependency_resolution_result(
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

        #[cfg(feature = "scheduler")]
        {
            if let Some(mut entry) = self.compile_cache.get_mut(owner_canonical) {
                entry
                    .dependency_resolutions
                    .insert(import_source.to_string(), resolution.clone());
                entry.dependencies.insert(resolved_canonical_id.to_string());
                return;
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(owner_canonical) {
                entry
                    .dependency_resolutions
                    .insert(import_source.to_string(), resolution.clone());
                entry.dependencies.insert(resolved_canonical_id.to_string());
                return;
            }
        }

        let workspace_generation = self.ws().content_generation();
        if let Some(entry) = self
            .imported_dependency_cache
            .lock()
            .get_mut(owner_canonical)
        {
            if entry.workspace_generation == workspace_generation {
                std::sync::Arc::make_mut(entry)
                    .dependency_resolutions
                    .insert(import_source.to_string(), resolution);
            }
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
        self.cache_dependency_resolution_result(owner_canonical, import_source, &resolved);
        Some(resolved)
    }

    pub(crate) fn resolve_loaded_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> Option<String> {
        if let Some(resolved) = self
            .cached_dependency_resolution_in_view(owner_canonical, import_source, None)
            .and_then(|resolution| Self::dependency_resolution_target(&resolution))
        {
            return Some(resolved);
        }

        self.resolve_workspace_dependency_and_cache(owner_canonical, import_source, kind)
    }

    pub(crate) fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        if let Some(resolved) = self
            .cached_dependency_resolution_in_view(owner_canonical, import_source, None)
            .and_then(|resolution| {
                self.prefer_type_dependency_target_from_resolution_in_view(
                    owner_canonical,
                    import_source,
                    &resolution,
                    None,
                )
            })
        {
            return Some(resolved);
        }

        let type_resolved = self
            .resolve_loaded_dependency_canonical(
                owner_canonical,
                import_source,
                verter_workspace::ResolveRequestKind::TypeImport,
            )
            .map(|resolved| {
                self.normalize_live_type_dependency_target_in_view(
                    owner_canonical,
                    import_source,
                    resolved.as_str(),
                    None,
                )
            })
            .or_else(|| {
                self.fallback_relative_type_companion_in_view(owner_canonical, import_source, None)
            });
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

    pub(crate) fn resolve_type_dependency_canonical_shallow_in_view(
        &self,
        owner_canonical: &str,
        import_source: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        if let Some(resolved) = self
            .cached_dependency_resolution_in_view(owner_canonical, import_source, store_view)
            .and_then(|resolution| {
                self.prefer_type_dependency_target_from_resolution_in_view(
                    owner_canonical,
                    import_source,
                    &resolution,
                    store_view,
                )
            })
        {
            return Some(resolved);
        }

        if import_source.starts_with('.') {
            let direct = crate::id::resolve_external(owner_canonical, import_source);
            if let Some(resolved) =
                self.resolve_eval_dependency_canonical_in_view(direct.as_str(), store_view)
            {
                self.cache_dependency_resolution_result(owner_canonical, import_source, &resolved);
                return Some(resolved);
            }
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
            .prefer_type_dependency_target_from_resolution_in_view(
                owner_canonical,
                import_source,
                &resolution,
                store_view,
            )
            .unwrap_or(resolved);

        if store_view.is_some()
            && !self.store_view_allows_current_whole_hash(
                &preferred,
                self.get_whole_hash(&preferred).unwrap_or_default(),
                store_view,
            )
        {
            return None;
        }

        self.cache_dependency_resolution_result(owner_canonical, import_source, &preferred);
        Some(preferred)
    }

    pub(crate) fn resolve_type_dependency_canonical_in_view(
        &self,
        owner_canonical: &str,
        import_source: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        if let Some(view) = store_view {
            if let Some(resolved) = self
                .cached_dependency_resolution_in_view(owner_canonical, import_source, Some(view))
                .and_then(|resolution| {
                    self.prefer_type_dependency_target_from_resolution_in_view(
                        owner_canonical,
                        import_source,
                        &resolution,
                        Some(view),
                    )
                })
            {
                return Some(resolved);
            }
            if import_source.starts_with('.') {
                if let Some(resolved) = self
                    .ws()
                    .resolve_import(
                        owner_canonical,
                        import_source,
                        verter_workspace::ResolutionContext {
                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                            kind: verter_workspace::ResolveRequestKind::TypeImport,
                        },
                    )
                    .map(|resolution| {
                        self.normalize_live_type_dependency_target_in_view(
                            owner_canonical,
                            import_source,
                            resolution.source_id.as_str(),
                            Some(view),
                        )
                    })
                    .filter(|resolved| {
                        self.store_view_allows_current_whole_hash(
                            resolved,
                            self.get_whole_hash(resolved).unwrap_or_default(),
                            Some(view),
                        )
                    })
                {
                    return Some(resolved);
                }
                if let Some(resolved) = self.fallback_relative_type_companion_in_view(
                    owner_canonical,
                    import_source,
                    Some(view),
                ) {
                    return Some(resolved);
                }
                return self
                    .ws()
                    .resolve_import(
                        owner_canonical,
                        import_source,
                        verter_workspace::ResolutionContext {
                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                            kind: verter_workspace::ResolveRequestKind::EsmImport,
                        },
                    )
                    .map(|resolution| {
                        self.normalize_live_type_dependency_target_in_view(
                            owner_canonical,
                            import_source,
                            resolution.source_id.as_str(),
                            Some(view),
                        )
                    })
                    .filter(|resolved| {
                        self.store_view_allows_current_whole_hash(
                            resolved,
                            self.get_whole_hash(resolved).unwrap_or_default(),
                            Some(view),
                        )
                    });
            }
            if let Some(resolved) = self
                .ws()
                .resolve_import(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .map(|resolution| {
                    self.normalize_live_type_dependency_target_in_view(
                        owner_canonical,
                        import_source,
                        resolution.source_id.as_str(),
                        Some(view),
                    )
                })
            {
                return Some(resolved);
            }
            return self
                .ws()
                .resolve_import(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::EsmImport,
                    },
                )
                .map(|resolution| {
                    self.normalize_live_type_dependency_target_in_view(
                        owner_canonical,
                        import_source,
                        resolution.source_id.as_str(),
                        Some(view),
                    )
                });
        }
        self.resolve_type_dependency_canonical(owner_canonical, import_source)
    }

    pub(crate) fn resolve_loaded_dependency_canonical_in_view(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_workspace::ResolveRequestKind,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        if kind == verter_workspace::ResolveRequestKind::TypeImport {
            return self.resolve_type_dependency_canonical_in_view(
                owner_canonical,
                import_source,
                store_view,
            );
        }

        if let Some(view) = store_view {
            if let Some(resolved) = self
                .cached_dependency_resolution_in_view(owner_canonical, import_source, Some(view))
                .and_then(|resolution| Self::dependency_resolution_target(&resolution))
            {
                return Some(resolved);
            }
            return self
                .ws()
                .resolve_import(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind,
                    },
                )
                .map(|resolution| resolution.source_id);
        }

        self.resolve_loaded_dependency_canonical(owner_canonical, import_source, kind)
    }

    #[allow(clippy::too_many_arguments)]
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
        self.resolve_external_type_from_loaded_files_in_view(
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
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn resolve_external_type_from_loaded_files_in_view(
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
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Result<
        Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
        crate::types::ExternalTypeResolveError,
    > {
        let _trace = component_meta_trace_scope!(
            "resolve_external_type_from_loaded_files",
            format!(
                "owner={} import={} type={} depth={} required_root_dep={} kind={kind:?} store_view={} cache_entries={} visiting={} use_host_cache={}",
                owner_canonical,
                import_source,
                type_name,
                depth,
                required_root_dep,
                store_view.is_some(),
                cache.len(),
                visiting.len(),
                use_host_cache,
            ),
        );

        if depth >= crate::types::MAX_RESOLVE_DEPTH {
            return Err(crate::types::ExternalTypeResolveError::DepthLimitExceeded {
                limit: crate::types::MAX_RESOLVE_DEPTH,
                type_name: type_name.to_string(),
                last_dep: owner_canonical.to_string(),
            });
        }

        let Some(dep_canonical) = self.resolve_loaded_dependency_canonical_in_view(
            owner_canonical,
            import_source,
            kind,
            store_view,
        ) else {
            return if required_root_dep {
                Err(crate::types::ExternalTypeResolveError::MissingRootDependency)
            } else {
                Ok(None)
            };
        };

        tracked_deps.insert(dep_canonical.clone());
        resolution_deps.insert(dep_canonical.clone());

        let cache_key = (dep_canonical.clone(), type_name.to_string());
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        let mut effective_target = None;
        let mut frontier_for_materialization = None;
        if use_host_cache && profile_hash.is_none() && store_view.is_none() {
            if let Some(route_entry) =
                self.lookup_import_type_route_cache(owner_canonical, import_source, type_name, kind)
            {
                self.provenance
                    .resolver_route_fact_reuse
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                for dep in &route_entry.tracked_deps {
                    tracked_deps.insert(dep.clone());
                    resolution_deps.insert(dep.clone());
                }

                if let Some(target) = route_entry.target {
                    if let Some(entry) = self.lookup_resolved_external_type_cache_in_view(
                        target.final_canonical_id.as_str(),
                        target.exported_name.as_str(),
                        kind,
                        store_view,
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
                            (target.final_canonical_id, target.exported_name),
                            resolved.clone(),
                        );
                        return Ok(resolved);
                    }

                    self.provenance
                        .resolved_external_type_cache_misses
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    effective_target = Some((target.final_canonical_id, target.exported_name));
                } else {
                    cache.insert(cache_key.clone(), None);
                    return Ok(None);
                }
            }
        }

        if effective_target.is_none() {
            let (frontier, target, had_route_cycle) = self
                .run_external_type_frontier_closure_in_view(
                    dep_canonical.as_str(),
                    type_name,
                    store_view,
                )?;
            let touched_ids = frontier.touched_canonical_ids();

            for touched_id in touched_ids {
                tracked_deps.insert(touched_id.clone());
                resolution_deps.insert(touched_id);
            }

            let Some(target) = target else {
                if self
                    .current_eval_state_in_view(dep_canonical.as_str(), store_view)
                    .is_none()
                {
                    return if required_root_dep {
                        Err(crate::types::ExternalTypeResolveError::MissingRootDependency)
                    } else {
                        Ok(None)
                    };
                }

                if had_route_cycle {
                    self.provenance
                        .resolver_cycle_detections
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }

                cache.insert(cache_key.clone(), None);
                if depth == 0 && use_host_cache && profile_hash.is_none() && store_view.is_none() {
                    self.store_import_type_route_cache(
                        owner_canonical,
                        import_source,
                        type_name,
                        kind,
                        None,
                        resolution_deps,
                    );
                }
                return Ok(None);
            };
            frontier_for_materialization = Some(frontier);
            effective_target = Some(target);
        }

        let (effective_dep_canonical, effective_type_name) =
            effective_target.expect("effective target should exist before body resolution");
        tracked_deps.insert(effective_dep_canonical.clone());
        resolution_deps.insert(effective_dep_canonical.clone());

        if use_host_cache {
            if let Some(entry) = self.lookup_resolved_external_type_cache_in_view(
                effective_dep_canonical.as_str(),
                effective_type_name.as_str(),
                kind,
                store_view,
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
            return Ok(None);
        }

        let resolved = (|| {
            let (frontier, touched_ids, final_target) =
                if let Some(frontier) = frontier_for_materialization.take() {
                    (
                        frontier,
                        rustc_hash::FxHashSet::default(),
                        (effective_dep_canonical.clone(), effective_type_name.clone()),
                    )
                } else {
                    let (frontier, target, _had_route_cycle) = self
                        .run_external_type_frontier_closure_in_view(
                            effective_dep_canonical.as_str(),
                            effective_type_name.as_str(),
                            store_view,
                        )?;
                    let Some(target) = target else {
                        return Ok(None);
                    };
                    let touched_ids = frontier.touched_canonical_ids();
                    (frontier, touched_ids, target)
                };

            for touched_id in touched_ids {
                tracked_deps.insert(touched_id.clone());
                resolution_deps.insert(touched_id);
            }

            let (final_canonical, final_exported) = final_target;
            Ok(self
                .materialize_frontier_resolved_type_in_view(
                    &frontier,
                    final_canonical.as_str(),
                    final_exported.as_str(),
                    tracked_deps,
                    resolution_deps,
                    store_view,
                )
                .or_else(|| {
                    self.resolve_external_type_from_cached_dependency_state_in_view(
                        effective_dep_canonical.as_str(),
                        effective_type_name.as_str(),
                        &ResolvedExternalTypes::default(),
                        store_view,
                    )
                }))
        })();
        visiting.remove(&final_target_key);
        let resolved = resolved?;

        if use_host_cache && profile_hash.is_none() {
            self.store_resolved_external_type_cache_in_view(
                effective_dep_canonical.as_str(),
                effective_type_name.as_str(),
                kind,
                resolved.clone(),
                resolution_deps.iter().cloned().collect(),
                store_view,
            );
        }

        cache.insert(cache_key.clone(), resolved.clone());
        cache.insert(
            (effective_dep_canonical.clone(), effective_type_name.clone()),
            resolved.clone(),
        );

        if depth == 0 && use_host_cache && profile_hash.is_none() && store_view.is_none() {
            self.store_import_type_route_cache(
                owner_canonical,
                import_source,
                type_name,
                kind,
                resolved
                    .as_ref()
                    .map(|_| crate::types::NormalizedTypeTarget {
                        final_canonical_id: effective_dep_canonical.clone(),
                        exported_name: effective_type_name.clone(),
                    }),
                resolution_deps,
            );
        }

        let tracked_before = tracked_deps.len();
        let resolution_before = resolution_deps.len();
        let cache_before = cache.len();
        let result = Ok(resolved);
        component_meta_trace_event!(
            "resolve_external_type_from_loaded_files_result",
            format!(
                "owner={} import={} type={} status={} tracked_delta={} resolution_delta={} cache_delta={} visiting={} store_view={}",
                owner_canonical,
                import_source,
                type_name,
                match &result {
                    Ok(Some(_)) => "ok:resolved",
                    Ok(None) => "ok:none",
                    Err(crate::types::ExternalTypeResolveError::MissingRootDependency) => {
                        "err:missing_root"
                    }
                    Err(crate::types::ExternalTypeResolveError::DepthLimitExceeded { .. }) => {
                        "err:depth_limit"
                    }
                    Err(crate::types::ExternalTypeResolveError::StepLimitExceeded { .. }) => {
                        "err:step_limit"
                    }
                },
                tracked_deps.len().saturating_sub(tracked_before),
                resolution_deps.len().saturating_sub(resolution_before),
                cache.len().saturating_sub(cache_before),
                visiting.len(),
                store_view.is_some(),
            ),
        );
        result
    }

    fn current_type_resolution_hash_in_view(
        &self,
        canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<crate::resolver_core::ResolverHash16> {
        store_view
            .and_then(|view| view.whole_hash(canonical))
            .or_else(|| self.get_whole_hash(canonical))
            .or_else(|| {
                self.read_dep_source_for_type_resolution_in_view(canonical, None, store_view)
                    .map(|source| crate::hash::hash_16(source.as_bytes()))
            })
    }

    fn lookup_resolved_external_type_cache_in_view(
        &self,
        dep_canonical: &str,
        type_name: &str,
        kind: verter_workspace::ResolveRequestKind,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<crate::types::ResolvedTypeCacheEntry> {
        let dep_source_hash =
            self.current_type_resolution_hash_in_view(dep_canonical, store_view)?;
        let key = crate::types::ResolvedTypeCacheKey {
            dep_canonical_id: dep_canonical.to_string(),
            dep_source_hash,
            type_name: type_name.to_string(),
            resolve_kind: kind,
        };
        self.resolved_type_cache.lock().get(&key).cloned()
    }

    fn store_resolved_external_type_cache_in_view(
        &self,
        dep_canonical: &str,
        type_name: &str,
        kind: verter_workspace::ResolveRequestKind,
        resolved: Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
        tracked_deps: Vec<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        let Some(dep_source_hash) =
            self.current_type_resolution_hash_in_view(dep_canonical, store_view)
        else {
            return;
        };

        let key = crate::types::ResolvedTypeCacheKey {
            dep_canonical_id: dep_canonical.to_string(),
            dep_source_hash,
            type_name: type_name.to_string(),
            resolve_kind: kind,
        };
        let mut host_cache = self.resolved_type_cache.lock();
        if host_cache.len() >= crate::types::RESOLVED_TYPE_CACHE_CAP {
            host_cache.clear();
        }
        host_cache.insert(
            key,
            crate::types::ResolvedTypeCacheEntry {
                resolved,
                tracked_deps,
            },
        );
    }

    #[cfg(feature = "scheduler")]
    fn lookup_import_type_route_cache(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> Option<crate::types::ImportTypeRouteEntry> {
        let route_key = (import_source.to_string(), type_name.to_string(), kind);
        let route_entry = self
            .compile_cache
            .get(owner_canonical)
            .and_then(|cc| cc.import_route_cache.get(&route_key).cloned())?;

        let owner_hash = self.get_whole_hash(owner_canonical).unwrap_or_default();
        let fresh = route_entry.owner_hash == owner_hash
            && route_entry.route_hashes.iter().all(|(canonical, hash)| {
                self.get_whole_hash(canonical).unwrap_or_default() == *hash
            });
        if !fresh {
            return None;
        }

        let neg_fresh = route_entry
            .negative_barrel_gen
            .as_ref()
            .map(|(barrel_canonical, generation)| {
                self.compile_cache
                    .get(barrel_canonical)
                    .and_then(|cc| {
                        cc.barrel_export_surface
                            .as_ref()
                            .map(|surface| surface.generation == *generation)
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(true);
        if route_entry.target.is_none() && !neg_fresh {
            return None;
        }

        Some(route_entry)
    }

    #[cfg(not(feature = "scheduler"))]
    fn lookup_import_type_route_cache(
        &self,
        _owner_canonical: &str,
        _import_source: &str,
        _type_name: &str,
        _kind: verter_workspace::ResolveRequestKind,
    ) -> Option<crate::types::ImportTypeRouteEntry> {
        None
    }

    fn store_import_type_route_cache(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        kind: verter_workspace::ResolveRequestKind,
        target: Option<crate::types::NormalizedTypeTarget>,
        resolution_deps: &std::collections::BTreeSet<String>,
    ) {
        #[cfg(feature = "scheduler")]
        {
            let route_key = (import_source.to_string(), type_name.to_string(), kind);
            let route_entry = crate::types::ImportTypeRouteEntry {
                owner_hash: self.get_whole_hash(owner_canonical).unwrap_or_default(),
                target,
                tracked_deps: resolution_deps.iter().cloned().collect(),
                route_hashes: resolution_deps
                    .iter()
                    .filter_map(|dep| self.get_whole_hash(dep).map(|hash| (dep.clone(), hash)))
                    .collect(),
                negative_barrel_gen: None,
            };
            if let Some(mut cc) = self.compile_cache.get_mut(owner_canonical) {
                cc.import_route_cache.insert(route_key, route_entry);
            }
        }
        #[cfg(not(feature = "scheduler"))]
        {
            let _ = (
                owner_canonical,
                import_source,
                type_name,
                kind,
                target,
                resolution_deps,
            );
        }
    }

    fn resolve_external_type_target_via_frontier_in_view(
        &self,
        dep_canonical: &str,
        type_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Result<FrontierTargetResult, crate::types::ExternalTypeResolveError> {
        let adapter = HostFrontierAdapter {
            host: self,
            store_view,
            materialize_symbols: false,
            route_exports_only: true,
        };
        let mut frontier = crate::resolver_core::ExternalTypeFrontier::new();
        frontier.seed(std::iter::once(
            crate::resolver_core::PendingExternalSymbol {
                canonical_id: dep_canonical.to_string(),
                exported_name: type_name.to_string(),
            },
        ));
        if let Err(failure) = frontier.run(&adapter) {
            return Err(crate::types::ExternalTypeResolveError::StepLimitExceeded {
                limit: failure.limit,
                type_name: type_name.to_string(),
                last_dep: failure.context,
            });
        }
        let target = frontier.final_target_for(&adapter, dep_canonical, type_name);
        let had_route_cycle = target.is_none()
            && frontier
                .get_resolved(dep_canonical, type_name)
                .and_then(|resolved| resolved.route_provenance.as_ref())
                .is_some();
        Ok((target, frontier.touched_canonical_ids(), had_route_cycle))
    }

    #[allow(clippy::type_complexity)]
    fn run_external_type_frontier_closure_in_view(
        &self,
        dep_canonical: &str,
        type_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Result<
        (
            crate::resolver_core::ExternalTypeFrontier,
            Option<(String, String)>,
            bool,
        ),
        crate::types::ExternalTypeResolveError,
    > {
        let adapter = HostFrontierAdapter {
            host: self,
            store_view,
            materialize_symbols: false,
            // Frontier discovery stays route-only. Materialization resolves only
            // the demanded companion targets after the route is known.
            route_exports_only: true,
        };
        let mut frontier = crate::resolver_core::ExternalTypeFrontier::new();
        let mut inspected_symbols = rustc_hash::FxHashSet::default();
        let mut requested_symbols = rustc_hash::FxHashSet::default();
        frontier.seed(std::iter::once(
            crate::resolver_core::PendingExternalSymbol {
                canonical_id: dep_canonical.to_string(),
                exported_name: type_name.to_string(),
            },
        ));
        requested_symbols.insert((dep_canonical.to_string(), type_name.to_string()));

        loop {
            if let Err(failure) = frontier.run(&adapter) {
                return Err(crate::types::ExternalTypeResolveError::StepLimitExceeded {
                    limit: failure.limit,
                    type_name: type_name.to_string(),
                    last_dep: failure.context,
                });
            }

            let target = frontier.final_target_for(&adapter, dep_canonical, type_name);
            let had_route_cycle = target.is_none()
                && frontier
                    .get_resolved(dep_canonical, type_name)
                    .and_then(|resolved| resolved.route_provenance.as_ref())
                    .is_some();
            if target.is_none() {
                return Ok((frontier, None, had_route_cycle));
            }

            let companion_seeds = self.collect_frontier_companion_seeds_in_view(
                &frontier,
                &adapter,
                store_view,
                &mut inspected_symbols,
                &requested_symbols,
            );
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "frontier_closure source={} exported={} resolved={} new_companions={}",
                    dep_canonical,
                    type_name,
                    frontier.resolved_count(),
                    companion_seeds.len(),
                ));
            }
            if companion_seeds.is_empty() {
                return Ok((frontier, target, had_route_cycle));
            }

            for seed in &companion_seeds {
                requested_symbols.insert((seed.canonical_id.clone(), seed.exported_name.clone()));
            }
            frontier.seed(companion_seeds);
        }
    }

    fn collect_frontier_companion_seeds_in_view(
        &self,
        frontier: &crate::resolver_core::ExternalTypeFrontier,
        adapter: &HostFrontierAdapter<'_>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
        inspected_symbols: &mut rustc_hash::FxHashSet<(String, String)>,
        requested_symbols: &rustc_hash::FxHashSet<(String, String)>,
    ) -> Vec<crate::resolver_core::PendingExternalSymbol> {
        let mut seeds = Vec::new();

        for (requested_canonical_id, requested_exported_name) in requested_symbols.iter() {
            let Some((canonical_id, exported_name)) =
                frontier.final_target_for(adapter, requested_canonical_id, requested_exported_name)
            else {
                continue;
            };
            if !inspected_symbols.insert((canonical_id.clone(), exported_name.clone())) {
                continue;
            }

            let Some(analysis) = self.external_type_analysis_in_view(&canonical_id, store_view)
            else {
                continue;
            };
            let required_import_names = self.required_import_names_for_exported_type_in_view(
                &canonical_id,
                &exported_name,
                store_view,
            );
            let mut attempted_requests = rustc_hash::FxHashSet::default();
            for binding in &analysis.extracted.bindings {
                let required_aliases =
                    required_import_alias_names_for_binding(binding, &required_import_names);
                for required_alias in required_aliases {
                    let Some(imported_name) =
                        imported_member_name_for_required_alias(binding, &required_alias)
                    else {
                        continue;
                    };
                    let request_key = (
                        required_alias.clone(),
                        binding.source.clone(),
                        imported_name.clone(),
                    );
                    if !attempted_requests.insert(request_key) {
                        continue;
                    }

                    let Some(dep_canonical) = self.resolve_type_dependency_canonical_in_view(
                        &canonical_id,
                        &binding.source,
                        store_view,
                    ) else {
                        continue;
                    };
                    let (resolved_canonical, resolved_name) = self
                        .resolve_imported_type_root_in_view(
                            dep_canonical.as_str(),
                            imported_name.as_str(),
                            store_view,
                        );
                    let (target_canonical, target_name) = frontier
                        .final_target_for(adapter, &resolved_canonical, &resolved_name)
                        .unwrap_or((resolved_canonical, resolved_name));
                    seeds.push(crate::resolver_core::PendingExternalSymbol {
                        canonical_id: target_canonical,
                        exported_name: target_name,
                    });
                }
            }
        }

        seeds
    }

    fn materialize_frontier_resolved_type_in_view(
        &self,
        frontier: &crate::resolver_core::ExternalTypeFrontier,
        canonical_id: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        let adapter = HostFrontierAdapter {
            host: self,
            store_view,
            // Frontier routing is already complete before materialization starts.
            // Keep final-target checks on the same shallow/export-owned path so
            // package declaration files do not reopen full imported-state
            // materialization while companion targets are selected.
            materialize_symbols: false,
            route_exports_only: false,
        };
        let mut memo = rustc_hash::FxHashMap::default();
        let mut active = rustc_hash::FxHashSet::default();
        self.materialize_frontier_resolved_type_with_memo_in_view(
            frontier,
            &adapter,
            canonical_id,
            exported_name,
            tracked_deps,
            resolution_deps,
            store_view,
            &mut memo,
            &mut active,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn materialize_frontier_resolved_type_with_memo_in_view(
        &self,
        frontier: &crate::resolver_core::ExternalTypeFrontier,
        adapter: &HostFrontierAdapter<'_>,
        canonical_id: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
        memo: &mut rustc_hash::FxHashMap<
            (String, String),
            Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
        >,
        active: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        let cache_key = (canonical_id.to_string(), exported_name.to_string());
        if let Some(cached) = memo.get(&cache_key) {
            return cached.clone();
        }
        if !active.insert(cache_key.clone()) {
            self.provenance
                .resolver_cycle_detections
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return None;
        }

        tracked_deps.insert(canonical_id.to_string());
        resolution_deps.insert(canonical_id.to_string());

        let resolved = (|| {
            let analysis = self.external_type_analysis_in_view(canonical_id, store_view)?;
            let required_import_names = self.required_import_names_for_exported_type_in_view(
                canonical_id,
                exported_name,
                store_view,
            );
            let mut companion_types = ResolvedExternalTypes::default();
            let mut attempted_requests = rustc_hash::FxHashSet::default();

            for binding in &analysis.extracted.bindings {
                let required_aliases =
                    required_import_alias_names_for_binding(binding, &required_import_names);
                for required_alias in required_aliases {
                    let Some(imported_name) =
                        imported_member_name_for_required_alias(binding, &required_alias)
                    else {
                        continue;
                    };
                    let request_key = (
                        required_alias.clone(),
                        binding.source.clone(),
                        imported_name.clone(),
                    );
                    if !attempted_requests.insert(request_key) {
                        continue;
                    }

                    let Some(dep_canonical) = self.resolve_type_dependency_canonical_in_view(
                        canonical_id,
                        &binding.source,
                        store_view,
                    ) else {
                        continue;
                    };
                    let (resolved_canonical, resolved_name) = self
                        .resolve_imported_type_root_in_view(
                            dep_canonical.as_str(),
                            imported_name.as_str(),
                            store_view,
                        );
                    let (target_canonical, target_name) = frontier
                        .final_target_for(adapter, &resolved_canonical, &resolved_name)
                        .unwrap_or((resolved_canonical, resolved_name));
                    if frontier
                        .get_resolved(&target_canonical, &target_name)
                        .is_none()
                    {
                        continue;
                    }
                    if let Some(companion) = self
                        .materialize_frontier_resolved_type_with_memo_in_view(
                            frontier,
                            adapter,
                            &target_canonical,
                            &target_name,
                            tracked_deps,
                            resolution_deps,
                            store_view,
                            memo,
                            active,
                        )
                    {
                        tracked_deps.insert(target_canonical.clone());
                        resolution_deps.insert(target_canonical.clone());
                        companion_types.entry(required_alias).or_insert(companion);
                    }
                }
            }

            self.resolve_external_type_from_cached_dependency_state_in_view(
                canonical_id,
                exported_name,
                &companion_types,
                store_view,
            )
        })();

        active.remove(&cache_key);
        memo.insert(cache_key, resolved.clone());
        resolved
    }

    /// Ensure the export registry is populated for a file.
    ///
    /// For files in compile_cache with scheduler analysis data, builds the
    /// registry from `HostAnalysisData.export_signatures` (no OXC parsing).
    /// For files only on disk, reads the file and extracts export signatures
    /// (one parse, cached for all future lookups).
    #[cfg(test)]
    #[cfg(not(feature = "scheduler"))]
    pub(crate) fn ensure_export_registry_in_view(
        &self,
        canonical: &str,
        _store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<crate::types::FileExportRegistry> {
        if let Some(registry) = read_lock(&self.files).get(canonical).and_then(|entry| {
            entry
                .export_registry
                .as_ref()
                .filter(|registry| registry.source_hash == entry.whole_hash)
                .cloned()
        }) {
            return Some(registry);
        }

        let registry = {
            let entry = self.ensure_shallow_imported_dependency_state_in_view(canonical, None)?;
            entry.external_type_analysis.as_ref().map(|analysis| {
                Self::build_export_registry_from_external_type_analysis(
                    analysis.as_ref(),
                    entry.whole_hash,
                )
            })
        };

        if let Some(ref reg) = registry {
            if let Some(file) = write_lock(&self.files).get_mut(canonical) {
                file.export_registry = Some(reg.clone());
            }
        }

        registry
    }

    /// Ensure the export registry is populated for a file.
    ///
    /// For files in compile_cache with scheduler analysis data, builds the
    /// registry from `HostAnalysisData.export_signatures` (no OXC parsing).
    /// For files only on disk, reads the file and extracts export signatures
    /// (one parse, cached for all future lookups).
    #[cfg(test)]
    #[cfg(feature = "scheduler")]
    pub(crate) fn ensure_export_registry_in_view(
        &self,
        canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<crate::types::FileExportRegistry> {
        // Check if already populated
        if let Some(cc) = self.compile_cache.get(canonical) {
            if let Some(ref registry) = cc.export_registry {
                let current_hash = store_view
                    .and_then(|view| view.whole_hash(canonical))
                    .or_else(|| self.get_whole_hash(canonical))
                    .unwrap_or_default();
                if registry.source_hash == current_hash
                    && self.store_view_allows_current_whole_hash(
                        canonical,
                        registry.source_hash,
                        store_view,
                    )
                {
                    return Some(registry.clone());
                }
            }
        }

        // Build from scheduler analysis data if available
        let registry = {
            use crate::host_executor::HostAnalysisData;

            let analysis_snap = self.scheduler.try_get_analysis(canonical);
            let scheduler_export_sigs = analysis_snap
                .as_ref()
                .and_then(|snap| snap.downcast_data::<HostAnalysisData>())
                .map(|ad| ad.export_signatures.clone());
            let imported_entry =
                self.clone_current_imported_dependency_entry(canonical, store_view);
            let imported_registry = imported_entry.as_ref().and_then(|entry| {
                entry.external_type_analysis.as_ref().map(|analysis| {
                    Self::build_export_registry_from_external_type_analysis(
                        analysis.as_ref(),
                        entry.whole_hash,
                    )
                })
            });
            let imported_whole_hash = imported_entry.as_ref().map(|entry| entry.whole_hash);

            let source_hash = self
                .get_whole_hash(canonical)
                .or(imported_whole_hash)
                .unwrap_or_default();

            if let Some(export_sigs) = scheduler_export_sigs {
                Some(Self::build_export_registry(&export_sigs, source_hash))
            } else if let Some(registry) = imported_registry {
                Some(registry)
            } else {
                let entry =
                    self.ensure_shallow_imported_dependency_state_in_view(canonical, store_view)?;
                entry.external_type_analysis.as_ref().map(|analysis| {
                    Self::build_export_registry_from_external_type_analysis(
                        analysis.as_ref(),
                        entry.whole_hash,
                    )
                })
            }
        };

        // Cache on compile_cache entry
        if let Some(ref reg) = registry {
            let mut cc = self.compile_cache.entry(canonical.to_string()).or_default();
            cc.export_registry = Some(reg.clone());
        }

        registry
    }

    /// Build a `FileExportRegistry` from export signatures.
    #[cfg(test)]
    fn build_export_registry(
        sigs: &[verter_semantic::analysis::ExportSignature],
        source_hash: Hash16,
    ) -> crate::types::FileExportRegistry {
        let mut named = rustc_hash::FxHashMap::default();
        let mut wildcard_edges = Vec::new();

        for sig in sigs {
            if sig.name == "*" {
                if let Some(source) = &sig.reexport_source {
                    wildcard_edges.push(source.clone());
                }
            } else if let Some(source) = &sig.reexport_source {
                named.insert(
                    sig.name.clone(),
                    crate::types::ExportEntry::Alias {
                        source_specifier: source.clone(),
                        original_name: sig
                            .reexport_local
                            .as_deref()
                            .unwrap_or(&sig.name)
                            .to_string(),
                    },
                );
            } else {
                named.insert(sig.name.clone(), crate::types::ExportEntry::Defined);
            }
        }

        crate::types::FileExportRegistry {
            source_hash,
            named,
            wildcard_edges,
        }
    }

    #[cfg(test)]
    fn build_export_registry_from_external_type_analysis(
        analysis: &verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource,
        source_hash: Hash16,
    ) -> crate::types::FileExportRegistry {
        let mut named = rustc_hash::FxHashMap::default();
        for name in analysis.exported_local_type_names() {
            named.insert(name.to_string(), crate::types::ExportEntry::Defined);
        }
        for name in analysis.exported_local_symbol_names() {
            named.insert(name.to_string(), crate::types::ExportEntry::Defined);
        }
        for (name, source_specifier, original_name) in analysis.direct_reexport_entries() {
            named.insert(
                name.to_string(),
                crate::types::ExportEntry::Alias {
                    source_specifier: source_specifier.to_string(),
                    original_name: original_name.to_string(),
                },
            );
        }

        crate::types::FileExportRegistry {
            source_hash,
            named,
            wildcard_edges: analysis.wildcard_reexport_sources().to_vec(),
        }
    }

    pub(crate) fn resolve_named_type_export_target_in_view(
        &self,
        dep_canonical: &str,
        requested_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(String, String)> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical_in_view(dep_canonical, store_view)
            .unwrap_or_else(|| dep_canonical.to_string());
        let _ = self.ensure_shallow_imported_dependency_state_in_view(
            normalized_canonical.as_str(),
            store_view,
        );

        let (result, _, _) = self
            .resolve_external_type_target_via_frontier_in_view(
                normalized_canonical.as_str(),
                requested_name,
                store_view,
            )
            .ok()?;
        let result = result.map(|(canonical, exported_name)| {
            let canonical = self
                .resolve_eval_dependency_canonical_in_view(canonical.as_str(), store_view)
                .unwrap_or(canonical);
            (canonical, exported_name)
        })?;
        let _ =
            self.ensure_shallow_imported_dependency_state_in_view(result.0.as_str(), store_view);
        #[cfg(test)]
        let _ = self.ensure_export_registry_in_view(result.0.as_str(), store_view);
        component_meta_trace_event!(
            "resolve_named_type_export_target_in_view_result",
            format!(
                "owner={} requested={} source=frontier target={} exported={}",
                dep_canonical, requested_name, result.0, result.1
            ),
        );
        Some(result)
    }

    /// Read the effective source for a dependency file for type resolution.
    ///
    /// On the scheduler path, tries the scheduler's source snapshot first.
    /// On the WASM path, tries `self.files` first.
    /// Both fall back to reading from the VFS workspace.
    /// For Vue SFCs, extracts only `<script>` / `<script setup>` content.
    #[cfg(test)]
    pub(crate) fn read_dep_source_for_type_resolution(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String> {
        self.read_dep_source_for_type_resolution_in_view(dep_canonical, profile_hash, None)
    }

    pub(crate) fn read_dep_source_for_type_resolution_in_view(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        let _trace = component_meta_trace_scope!(
            "read_dep_source_for_type_resolution",
            format!(
                "owner={} store_view={}",
                dep_canonical,
                store_view.is_some()
            ),
        );
        if let Some(profile_hash) = profile_hash {
            if let Some(state) = self.effective_file_state(dep_canonical, Some(profile_hash)) {
                if self.store_view_allows_current_whole_hash(
                    dep_canonical,
                    state.whole_hash,
                    store_view,
                ) {
                    let eval_source = Arc::<str>::from(Self::build_eval_script_source(
                        state.source.as_ref(),
                        state.cached_parse.as_deref(),
                    ));
                    component_meta_trace_event!(
                        "read_dep_source_for_type_resolution_result",
                        format!(
                            "owner={} source=effective-file-state bytes={} has_cached_parse={} whole_hash={:?}",
                            dep_canonical,
                            eval_source.len(),
                            state.cached_parse.is_some(),
                            state.whole_hash,
                        ),
                    );
                    return Some(eval_source.to_string());
                }
            }
        }
        let entry =
            self.ensure_shallow_imported_dependency_state_in_view(dep_canonical, store_view)?;
        let eval_source = entry.eval_source.clone().unwrap_or_else(|| {
            Arc::<str>::from(
                extract_vue_script_content(
                    entry.raw_source.as_ref(),
                    entry.cached_parse.as_deref(),
                )
                .unwrap_or_else(|| entry.raw_source.to_string()),
            )
        });
        component_meta_trace_event!(
            "read_dep_source_for_type_resolution_result",
            format!(
                "owner={} source=host-materialized-cache bytes={} has_cached_parse={} whole_hash={:?}",
                dep_canonical,
                eval_source.len(),
                entry.cached_parse.is_some(),
                entry.whole_hash,
            )
        );
        Some(eval_source.to_string())
    }

    fn collect_external_types_from_loaded_files(
        &self,
        owner_canonical: &str,
        macro_type_deps: &[verter_semantic::analysis::MacroTypeDep],
        script_imports: &[verter_semantic::analysis::AnalyzedImport],
        profile_hash: Option<u64>,
    ) -> (
        Option<ResolvedExternalTypes>,
        Vec<HostDiagnostic>,
        std::collections::BTreeSet<String>,
    ) {
        let collected = crate::resolver_core::collect_external_macro_types(
            &HostExternalMacroTypeCollector { host: self },
            owner_canonical,
            macro_type_deps,
            script_imports,
            profile_hash,
        );

        (
            collected.resolved,
            collected
                .diagnostics
                .into_iter()
                .map(|diag| HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: diag.code,
                    message: diag.message,
                    span: diag.span,
                })
                .collect(),
            collected.tracked_dependencies,
        )
    }
}

impl VerterHost {
    /// Resolve a raw import identifier (bundler query string or LSP `._VERTER_.` format)
    /// to its canonical ID, virtual node kind, and rendered bundler/LSP IDs.
    ///
    /// Returns `None` if the raw ID cannot be parsed.
    pub fn resolve(&self, raw_id: &str) -> Option<ResolvedId> {
        #[cfg(feature = "session_metrics")]
        self.metrics
            .resolves
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let parsed = parse_raw_id(raw_id)?;
        let canonical = self.resolve_alias_or_canonical(&parsed.canonical_id);
        let (exists, bundler_id, lsp_id) = {
            #[cfg(feature = "scheduler")]
            {
                use crate::host_executor::HostSourceData;
                let meta = self.scheduler.try_get_source(&canonical).and_then(|s| {
                    s.downcast_data::<HostSourceData>()
                        .map(|h| h.parse.meta.clone())
                });
                match meta {
                    Some(m) => {
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &m);
                        (true, b, l)
                    }
                    None => {
                        let default_meta = FileMeta::default();
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &default_meta);
                        (false, b, l)
                    }
                }
            }
            #[cfg(not(feature = "scheduler"))]
            {
                let files = read_lock(&self.files);
                match files.get(&canonical) {
                    Some(f) => {
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &f.meta);
                        (true, b, l)
                    }
                    None => {
                        let default_meta = FileMeta::default();
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &default_meta);
                        (false, b, l)
                    }
                }
            }
        };
        Some(ResolvedId {
            canonical_id: canonical,
            node_kind: parsed.node_kind,
            exists_in_host: exists,
            bundler_id,
            lsp_id,
        })
    }

    /// Ensure a file is compiled and cached for the given profile.
    ///
    /// Unlike [`get_virtual_file`](Self::get_virtual_file), this does not require
    /// specifying a `VirtualNodeKind`. It simply ensures the compilation cache is
    /// populated so that subsequent `get_ide()`, `get_analysis()`, or
    /// `get_virtual_file()` calls hit the cache.
    ///
    /// Returns `Ok(())` on success (cache hit or successful compilation).
    /// Returns `Err(HostError)` if the file is missing or compilation fails.
    fn hydrate_compile_blockers(&self, canonical_id: &str) {
        let Some(blockers) = self.get_compile_blockers(canonical_id) else {
            return;
        };

        let workspace = self.workspace();
        let mut blocker_ids = std::collections::BTreeSet::new();

        for request in blockers.external_source_requests {
            let resolved = workspace
                .resolve_import(
                    canonical_id,
                    &request.specifier,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::SfcSrcAttr,
                    },
                )
                .map(|resolution| {
                    self.cache_dependency_resolution_result(
                        canonical_id,
                        &request.specifier,
                        &resolution.source_id,
                    );
                    resolution.source_id
                })
                .unwrap_or(request.resolved_canonical_id);
            if resolved != canonical_id {
                blocker_ids.insert(resolved);
            }
        }

        for dep in blockers.macro_type_deps.iter() {
            let resolved = workspace
                .resolve_import(
                    canonical_id,
                    &dep.import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .inspect(|resolution| {
                    self.cache_dependency_resolution_result(
                        canonical_id,
                        &dep.import_source,
                        &resolution.source_id,
                    );
                })
                .or_else(|| {
                    workspace
                        .resolve_import(
                            canonical_id,
                            &dep.import_source,
                            verter_workspace::ResolutionContext {
                                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                kind: verter_workspace::ResolveRequestKind::EsmImport,
                            },
                        )
                        .inspect(|resolution| {
                            self.cache_dependency_resolution_result(
                                canonical_id,
                                &dep.import_source,
                                &resolution.source_id,
                            );
                        })
                })
                .map(|resolution| resolution.source_id);
            if let Some(resolved) = resolved.filter(|resolved| resolved != canonical_id) {
                blocker_ids.insert(resolved);
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        for blocker_id in blocker_ids {
            let _ = self.ensure_loaded(&blocker_id);
        }
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn ensure_compiled(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Result<(), HostError> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);

        // Check cache
        {
            #[cfg(feature = "scheduler")]
            {
                use crate::host_executor::HostSourceData;
                let snap = self.scheduler.try_get_source(&canonical).ok_or_else(|| {
                    HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    }
                })?;
                let hd = snap.downcast_data::<HostSourceData>().ok_or_else(|| {
                    HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    }
                })?;
                if hd.file_kind == FileKind::NonSfc {
                    return Ok(());
                }
                if let Some(cc) = self.compile_cache.get(&canonical) {
                    let soh = cc
                        .style_overrides
                        .get(&profile_hash)
                        .map(|o| o.hash)
                        .unwrap_or(0);
                    if let Some(slot) = cc.compile_slots.get(&profile_hash) {
                        if slot.semantic_hash == hd.parse.semantic_hash
                            && slot.style_override_hash == soh
                        {
                            return Ok(());
                        }
                    }
                }
            }
            #[cfg(not(feature = "scheduler"))]
            {
                let files = read_lock(&self.files);
                let entry = files
                    .get(&canonical)
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    })?;
                if entry.file_kind == FileKind::NonSfc {
                    return Ok(());
                }
                let soh = entry
                    .style_overrides
                    .get(&profile_hash)
                    .map(|o| o.hash)
                    .unwrap_or(0);
                if let Some(slot) = entry.compile_slots.get(&profile_hash) {
                    if slot.semantic_hash == entry.semantic_hash && slot.style_override_hash == soh
                    {
                        return Ok(());
                    }
                }
            }
        }

        self.hydrate_compile_blockers(&canonical);

        // Cache miss â€” compile by requesting the Main virtual file.
        // This populates ALL cached outputs (script, template, styles, TSX, etc.)
        // for the given profile.
        let _ = self.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })?;
        Ok(())
    }

    /// Retrieve a compiled virtual file (script, template, style, or main bundle).
    ///
    /// On cache hit, returns immediately. On cache miss, compiles the file using
    /// `verter_compiler::compile`, caches the result, and returns the requested node.
    /// In dev mode with [`CompileErrorPolicy::DevServeLastKnownGood`], falls back
    /// to the last successful compilation when the current source has errors.
    pub fn get_virtual_file(&self, query: VirtualQuery) -> Result<VirtualFileResponse, HostError> {
        #[cfg(feature = "session_metrics")]
        self.metrics
            .virtual_loads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let (canonical_id, node_kind, raw_was_lsp) = if let Some(raw) = query.raw_id.clone() {
            let parsed = parse_raw_id(&raw).ok_or(HostError::InvalidQuery)?;
            (
                self.resolve_alias_or_canonical(&parsed.canonical_id),
                parsed.node_kind,
                parsed.was_lsp_like,
            )
        } else if let (Some(canonical), Some(node_kind)) =
            (query.canonical_id.clone(), query.node_kind.clone())
        {
            (
                self.resolve_alias_or_canonical(&canonical),
                node_kind,
                false,
            )
        } else {
            return Err(HostError::InvalidQuery);
        };

        let profile_hash = compile_profile_hash(&query.compile_profile);

        // Cache hit check and compile input extraction under a single read lock.
        // This avoids cloning the full FileEntry (with all compile_slots, style_overrides, etc.)
        // on the hot path.
        struct CacheMiss {
            compile_input: CompileInput,
            fallback_last_good: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
            meta: FileMeta,
            /// Captured under read lock so the compile slot is stored with the
            /// semantic_hash that was current when we decided to compile.
            semantic_hash: Hash16,
        }

        // Capture scheduler source state at compile START for artifact commit.
        #[cfg(feature = "scheduler")]
        let sched_snapshot_at_start = self.scheduler.try_get_source(&canonical_id);

        let cache_miss = {
            #[cfg(feature = "scheduler")]
            {
                use crate::host_executor::{HostAnalysisData, HostSourceData};

                let source_snap =
                    self.scheduler
                        .try_get_source(&canonical_id)
                        .ok_or_else(|| HostError::MissingSource {
                            canonical_id: canonical_id.clone(),
                        })?;
                let hd = source_snap
                    .downcast_data::<HostSourceData>()
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;
                let parse = &hd.parse;

                let cc_ref = self.compile_cache.get(&canonical_id);

                // Cache hit check from compile_cache
                let soh = cc_ref
                    .as_ref()
                    .and_then(|cc| cc.style_overrides.get(&profile_hash).map(|o| o.hash))
                    .unwrap_or(0);
                let coh = cc_ref
                    .as_ref()
                    .and_then(|cc| {
                        cc.content_overrides
                            .get(&profile_hash)
                            .map(|o| o.layer.hash)
                    })
                    .unwrap_or(0);

                if let Some(ref cc) = cc_ref {
                    if let Some(slot) = cc.compile_slots.get(&profile_hash) {
                        if slot.semantic_hash == parse.semantic_hash
                            && slot.style_override_hash == soh
                            && slot.content_override_hash == coh
                        {
                            #[cfg(feature = "session_metrics")]
                            self.metrics
                                .compile_cache_hits
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                            // Build effective meta for cache-hit render_ids
                            let mut hit_meta = parse.meta.clone();
                            if let Some(so) = cc.style_overrides.get(&profile_hash) {
                                for (idx, lang) in so.lang_overrides.iter().enumerate() {
                                    if let Some(ref l) = lang {
                                        if idx < hit_meta.style_langs.len() {
                                            hit_meta.style_langs[idx] = Some(l.clone());
                                        }
                                    }
                                }
                            }

                            if let Some(found) = slot.outputs.get(&node_kind) {
                                return Ok(VirtualFileResponse {
                                    id: render_single_id(
                                        &canonical_id,
                                        &node_kind,
                                        &hit_meta,
                                        raw_was_lsp,
                                    ),
                                    code: found.code.clone(),
                                    source_map: found.source_map.clone(),
                                    lang: found.lang.clone(),
                                    stale: false,
                                    diagnostics: slot.diagnostics.clone(),
                                    meta: found.meta.clone(),
                                });
                            }
                        }
                    }
                }

                // Cache miss â€” use effective_* helpers for override-aware state
                let efs = self
                    .effective_file_state(&canonical_id, Some(profile_hash))
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;
                let effective_meta = self
                    .effective_meta(&canonical_id, Some(profile_hash))
                    .unwrap_or_else(|| parse.meta.clone());

                let style_override_layer = cc_ref.as_ref().and_then(|cc| {
                    cc.style_overrides
                        .get(&profile_hash)
                        .map(|o| o.layer.clone())
                });
                let content_override_layer = cc_ref.as_ref().and_then(|cc| {
                    cc.content_overrides
                        .get(&profile_hash)
                        .map(|o| o.layer.clone())
                });
                let fallback_last_good = cc_ref.as_ref().and_then(|cc| {
                    cc.compile_slots
                        .get(&profile_hash)
                        .and_then(|slot| slot.last_good_outputs.clone())
                });

                // Style v-bind vars from raw analysis (override-independent)
                let analysis_snap = self.scheduler.try_get_analysis(&canonical_id);
                let style_analyses: Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>> =
                    analysis_snap
                        .as_ref()
                        .and_then(|a| a.downcast_data::<HostAnalysisData>())
                        .map(|ad| Arc::clone(&ad.style_analyses))
                        .unwrap_or_default();

                drop(cc_ref);

                CacheMiss {
                    compile_input: CompileInput {
                        canonical_id: canonical_id.clone(),
                        source: efs.source,
                        meta: effective_meta.clone(),
                        parse_diagnostics: parse.parse_diagnostics.clone(),
                        src_blocks: parse.src_blocks.clone(),
                        external_requests: parse.external_requests.clone(),
                        style_override_layer,
                        content_override_layer,
                        macro_type_deps: efs.script_analysis.macro_type_deps.clone(),
                        script_imports: efs.script_analysis.imports.clone(),
                        script_macros: efs.script_analysis.macros.clone(),
                        script_bindings: efs.script_analysis.bindings.clone(),
                        cached_parse: efs.cached_parse,
                        style_v_bind_vars: style_analyses
                            .iter()
                            .flat_map(|sa| {
                                sa.v_binds.iter().map(|vb| {
                                    vb.expression
                                        .split('.')
                                        .next()
                                        .unwrap_or(&vb.expression)
                                        .to_string()
                                })
                            })
                            .collect(),
                    },
                    fallback_last_good,
                    meta: effective_meta,
                    semantic_hash: parse.semantic_hash,
                }
            }

            #[cfg(not(feature = "scheduler"))]
            {
                let files = read_lock(&self.files);
                let entry = files
                    .get(&canonical_id)
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;

                let soh = entry
                    .style_overrides
                    .get(&profile_hash)
                    .map(|o| o.hash)
                    .unwrap_or(0);
                let coh = entry
                    .content_overrides
                    .get(&profile_hash)
                    .map(|o| o.layer.hash)
                    .unwrap_or(0);

                if let Some(slot) = entry.compile_slots.get(&profile_hash) {
                    if slot.semantic_hash == entry.semantic_hash
                        && slot.style_override_hash == soh
                        && slot.content_override_hash == coh
                    {
                        #[cfg(feature = "session_metrics")]
                        self.metrics
                            .compile_cache_hits
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                        if let Some(found) = slot.outputs.get(&node_kind) {
                            return Ok(VirtualFileResponse {
                                id: render_single_id(
                                    &canonical_id,
                                    &node_kind,
                                    &entry.meta,
                                    raw_was_lsp,
                                ),
                                code: found.code.clone(),
                                source_map: found.source_map.clone(),
                                lang: found.lang.clone(),
                                stale: false,
                                diagnostics: slot.diagnostics.clone(),
                                meta: found.meta.clone(),
                            });
                        }
                    }
                }

                let fallback_last_good = entry
                    .compile_slots
                    .get(&profile_hash)
                    .and_then(|slot| slot.last_good_outputs.clone());
                let efs = self
                    .effective_file_state(&canonical_id, Some(profile_hash))
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;

                CacheMiss {
                    compile_input: CompileInput {
                        canonical_id: entry.canonical_id.clone(),
                        source: efs.source,
                        meta: efs.meta.clone(),
                        parse_diagnostics: entry.parse_diagnostics.clone(),
                        src_blocks: entry.src_blocks.clone(),
                        external_requests: entry.external_requests.clone(),
                        style_override_layer: entry.style_overrides.get(&profile_hash).cloned(),
                        content_override_layer: entry
                            .content_overrides
                            .get(&profile_hash)
                            .map(|o| o.layer.clone()),
                        macro_type_deps: efs.script_analysis.macro_type_deps.clone(),
                        script_imports: efs.script_analysis.imports.clone(),
                        script_macros: efs.script_analysis.macros.clone(),
                        script_bindings: efs.script_analysis.bindings.clone(),
                        cached_parse: efs.cached_parse,
                        style_v_bind_vars: entry
                            .style_analyses
                            .iter()
                            .flat_map(|sa| {
                                sa.v_binds.iter().map(|vb| {
                                    vb.expression
                                        .split('.')
                                        .next()
                                        .unwrap_or(&vb.expression)
                                        .to_string()
                                })
                            })
                            .collect(),
                    },
                    fallback_last_good,
                    meta: efs.meta,
                    semantic_hash: entry.semantic_hash,
                }
            }
        };

        let CacheMiss {
            compile_input,
            fallback_last_good,
            meta,
            semantic_hash: captured_semantic_hash,
        } = cache_miss;

        #[cfg(feature = "session_metrics")]
        self.metrics
            .compile_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        #[cfg(feature = "session_metrics")]
        let compile_start = Instant::now();

        let style_override_hash = compile_input
            .style_override_layer
            .as_ref()
            .map(|o| o.hash)
            .unwrap_or(0);
        let content_override_hash = compile_input
            .content_override_layer
            .as_ref()
            .map(|o| o.hash)
            .unwrap_or(0);

        let (compiled_outputs, diagnostics, stale, compiled_tsx, compiled_template_analysis) =
            match self.compile_entry(&compile_input, &query.compile_profile) {
                Ok((outputs, diagnostics, tsx, tpl)) => (outputs, diagnostics, false, tsx, tpl),
                Err(diagnostics) => {
                    self.store_latest_diagnostics(&canonical_id, profile_hash, diagnostics.clone());
                    let policy = self.config.compile_error_policy;
                    if self.config.dev_mode && policy == CompileErrorPolicy::DevServeLastKnownGood {
                        if let Some(last_good) = fallback_last_good.clone() {
                            (last_good, diagnostics, true, None, None)
                        } else {
                            return Err(HostError::CompileError { diagnostics });
                        }
                    } else {
                        return Err(HostError::CompileError { diagnostics });
                    }
                }
            };

        #[cfg(feature = "session_metrics")]
        {
            let compile_elapsed_us = compile_start.elapsed().as_micros() as u64;
            self.metrics
                .compile_time_us_total
                .fetch_add(compile_elapsed_us, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut per_profile) = self.metrics.compile_time_us_total_by_profile.lock() {
                let entry = per_profile.entry(profile_hash).or_insert(0);
                *entry = entry.saturating_add(compile_elapsed_us);
            }
            if let Ok(mut per_profile_count) = self.metrics.compile_count_by_profile.lock() {
                let entry = per_profile_count.entry(profile_hash).or_insert(0);
                *entry = entry.saturating_add(1);
            }
        }

        let last_tick = self.tick.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Store compile results.
        // compile_cache is the authority for profile state.
        #[cfg(feature = "scheduler")]
        {
            if let Some(mut cc) = self.compile_cache.get_mut(&canonical_id) {
                cc.compile_slots.insert(
                    profile_hash,
                    CompileSlot {
                        semantic_hash: captured_semantic_hash,
                        style_override_hash,
                        content_override_hash,
                        outputs: compiled_outputs.clone(),
                        diagnostics: diagnostics.clone(),
                        last_good_outputs: if stale {
                            fallback_last_good.clone()
                        } else {
                            Some(compiled_outputs.clone())
                        },
                        last_access_tick: last_tick,
                        tsx: compiled_tsx.clone(),
                        template_analysis: compiled_template_analysis.clone(),
                    },
                );
                cc.latest_diagnostics
                    .insert(profile_hash, diagnostics.clone());
                cc.diagnostics_generation += 1;
            }
        }

        // Commit to scheduler artifact snapshot (scheduler path only).
        #[cfg(feature = "scheduler")]
        {
            // Persist raw template analysis to compile_cache for profileless consumers
            // (e.g. cross_file, get_analysis). Only for non-override compiles.
            if compiled_template_analysis.is_some()
                && compile_input.content_override_layer.is_none()
            {
                if let Some(mut cc) = self.compile_cache.get_mut(&canonical_id) {
                    cc.raw_template_analysis = compiled_template_analysis.clone().map(Arc::new);
                }
            }

            if let Some(ref snap) = sched_snapshot_at_start {
                self.scheduler.commit_artifact(
                    &canonical_id,
                    profile_hash,
                    verter_scheduler::node::ArtifactSnapshot {
                        generation: snap.generation,
                        profile_hash,
                        data: Arc::new(crate::host_executor::HostArtifactData {
                            outputs: compiled_outputs.clone(),
                            diagnostics: diagnostics.clone(),
                        }),
                    },
                );
            }
        }

        // Write per-profile state to files (WASM path only).
        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical_id) {
                let last_good_outputs = if stale {
                    fallback_last_good.clone()
                } else {
                    Some(compiled_outputs.clone())
                };
                if compiled_template_analysis.is_some() {
                    entry.template_analysis = compiled_template_analysis.clone().map(Arc::new);
                }
                entry.compile_slots.insert(
                    profile_hash,
                    CompileSlot {
                        semantic_hash: captured_semantic_hash,
                        style_override_hash,
                        content_override_hash,
                        outputs: compiled_outputs.clone(),
                        diagnostics: diagnostics.clone(),
                        last_good_outputs,
                        last_access_tick: last_tick,
                        tsx: compiled_tsx,
                        template_analysis: compiled_template_analysis,
                    },
                );
                entry
                    .latest_diagnostics
                    .insert(profile_hash, diagnostics.clone());
                entry.diagnostics_generation += 1;
                enforce_profile_cap(entry, self.config.max_profiles_per_file.max(1));
            }
        }

        let found =
            compiled_outputs
                .get(&node_kind)
                .ok_or_else(|| HostError::MissingVirtualNode {
                    canonical_id: canonical_id.clone(),
                })?;

        Ok(VirtualFileResponse {
            id: render_single_id(&canonical_id, &node_kind, &meta, raw_was_lsp),
            code: found.code.clone(),
            source_map: found.source_map.clone(),
            lang: found.lang.clone(),
            stale,
            diagnostics,
            meta: found.meta.clone(),
        })
    }

    /// List all virtual node kinds for a file (Main, Script, Template, Style, Custom).
    pub fn list_virtual_files(&self, canonical_id: &str) -> Vec<VirtualNodeKind> {
        self.list_virtual_nodes(canonical_id)
    }

    /// Retrieve the combined TSX output for LSP type checking.
    ///
    /// Returns the IDE code (TSX or JSX) and optional source map for the given file and profile.
    /// This is a dedicated API separate from the virtual file system, since IDE
    /// output is only consumed by the LSP and playground, never by bundlers.
    pub fn get_ide(&self, canonical_id: &str, profile: &CompileProfile) -> Option<IdeResponse> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);

        #[cfg(feature = "scheduler")]
        {
            let cc = self.compile_cache.get(&canonical)?;
            if cc.evicted {
                return None;
            }
            let slot = cc.compile_slots.get(&profile_hash)?;
            let tsx = slot.tsx.as_ref()?;
            Some(IdeResponse {
                code: tsx.code.clone(),
                source_map: tsx.source_map.clone(),
                is_jsx: tsx.is_jsx,
                destructured_block: tsx.destructured_block.clone(),
            })
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            let slot = entry.compile_slots.get(&profile_hash)?;
            let tsx = slot.tsx.as_ref()?;
            Some(IdeResponse {
                code: tsx.code.clone(),
                source_map: tsx.source_map.clone(),
                is_jsx: tsx.is_jsx,
                destructured_block: tsx.destructured_block.clone(),
            })
        }
    }

    /// Generate public API output for a Vue SFC â€” minimal TypeScript declarations.
    ///
    /// Unlike [`get_ide`](Self::get_ide), this does NOT require a prior
    /// [`get_virtual_file`](Self::get_virtual_file) call. It performs
    /// macro-only extraction (OXC parse â†’ defineProps/Emits/Model/Options)
    /// and generates a `ComponentPublicInstance`-based declaration.
    ///
    /// Returns `None` if the file is not in the host or not a Vue SFC.
    pub fn get_public_api(&self, canonical_id: &str) -> Option<TscResponse> {
        self.get_public_api_with_mode(canonical_id, PublicApiMode::Public, None)
    }

    /// Generate public API output for a Vue SFC using the requested surface mode.
    ///
    /// `PublicApiMode::Public` matches the default application-facing instance shape.
    /// `PublicApiMode::Testing` exposes internal `<script setup>` bindings in a
    /// Vue Test Utils-like debug surface.
    ///
    /// When `profile` is provided, script/content overrides for that compile
    /// profile are reflected in the generated API surface.
    pub fn get_public_api_with_mode(
        &self,
        canonical_id: &str,
        mode: PublicApiMode,
        profile: Option<&CompileProfile>,
    ) -> Option<TscResponse> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = profile.map(compile_profile_hash);

        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return None;
            }
        }

        #[cfg(feature = "scheduler")]
        let (source, file_kind, macro_type_deps, script_imports, cached_extract, whole_hash) = {
            let efs = self.effective_file_state(&canonical, profile_hash)?;
            let file_kind = self.scheduler.try_get_source(&canonical).and_then(|snap| {
                snap.downcast_data::<crate::host_executor::HostSourceData>()
                    .map(|hd| hd.file_kind)
            })?;
            if file_kind != FileKind::VueSfc {
                return None;
            }
            let cached = self.compile_cache.get(&canonical).and_then(|cc| {
                cc.cached_tsc_extract.as_ref().and_then(|(hash, extract)| {
                    if *hash == efs.whole_hash {
                        Some(Arc::clone(extract))
                    } else {
                        None
                    }
                })
            });
            (
                efs.source,
                file_kind,
                efs.script_analysis.macro_type_deps.clone(),
                efs.script_analysis.imports.clone(),
                cached,
                efs.whole_hash,
            )
        };

        #[cfg(not(feature = "scheduler"))]
        let (source, file_kind, macro_type_deps, script_imports, cached_extract, whole_hash) = {
            let (file_kind, cached_extract) = {
                let files = read_lock(&self.files);
                let entry = files.get(&canonical)?;
                (entry.file_kind, entry.cached_tsc_extract.clone())
            };
            let efs = self.effective_file_state(&canonical, profile_hash)?;
            let cached_extract = cached_extract.and_then(|(hash, extract)| {
                if hash == efs.whole_hash {
                    Some(extract)
                } else {
                    None
                }
            });
            (
                efs.source,
                file_kind,
                efs.script_analysis.macro_type_deps.clone(),
                efs.script_analysis.imports.clone(),
                cached_extract,
                efs.whole_hash,
            )
        };
        if file_kind != FileKind::VueSfc {
            return None;
        }
        // Derive component name from canonical_id: last path segment, strip .vue extension.
        let component_name = canonical
            .rsplit('/')
            .next()
            .unwrap_or(&canonical)
            .trim_end_matches(".vue")
            .to_string();
        let (external_types, _, transitive_macro_type_deps) = self
            .collect_external_types_from_loaded_files(
                &canonical,
                &macro_type_deps,
                &script_imports,
                profile_hash,
            );
        self.sync_transitive_macro_type_dependencies(&canonical, &transitive_macro_type_deps);
        let tsc_mode = match mode {
            PublicApiMode::Public => verter_compiler::tsc::TscMode::Public,
            PublicApiMode::Testing => verter_compiler::tsc::TscMode::Testing,
        };

        // Try cached extract path: avoids re-parsing SFC + OXC on cache hit.
        let extract = if let Some(cached) = cached_extract {
            cached
        } else if let Some(fresh) = verter_compiler::tsc::extract_tsc_state(
            &source,
            &component_name,
            &verter_compiler::tsc::TscExtractOptions {
                filename: Some(canonical.clone()),
            },
        ) {
            let arc = Arc::new(fresh);
            #[cfg(feature = "scheduler")]
            {
                if let Some(mut cc) = self.compile_cache.get_mut(&canonical) {
                    cc.cached_tsc_extract = Some((whole_hash, Arc::clone(&arc)));
                }
            }

            #[cfg(not(feature = "scheduler"))]
            {
                let mut files = write_lock(&self.files);
                if let Some(entry) = files.get_mut(&canonical) {
                    entry.cached_tsc_extract = Some((whole_hash, Arc::clone(&arc)));
                }
            }
            arc
        } else {
            // No <script setup> â€” fall through to direct path for empty stub
            let tsc_out = verter_compiler::tsc::generate_tsc_output_with_options(
                &source,
                &component_name,
                &verter_compiler::tsc::TscGenOptions {
                    conditional_root_narrowing: false,
                    filename: Some(canonical.clone()),
                    external_types,
                    mode: tsc_mode,
                },
            );
            return Some(TscResponse {
                code: Arc::from(tsc_out.code),
                source_map: if tsc_out.source_map.is_empty() {
                    None
                } else {
                    Some(Arc::from(tsc_out.source_map))
                },
            });
        };

        let tsc_out = verter_compiler::tsc::generate_tsc_from_state(
            &extract,
            &source,
            &component_name,
            tsc_mode,
            external_types.as_ref(),
        );
        Some(TscResponse {
            code: Arc::from(tsc_out.code),
            source_map: if tsc_out.source_map.is_empty() {
                None
            } else {
                Some(Arc::from(tsc_out.source_map))
            },
        })
    }

    /// Store diagnostics from a failed compile without triggering recompilation.
    pub(crate) fn store_latest_diagnostics(
        &self,
        canonical_id: &str,
        profile_hash: u64,
        diagnostics: DiagnosticsSnapshot,
    ) {
        #[cfg(feature = "scheduler")]
        if let Some(mut cc) = self.compile_cache.get_mut(canonical_id) {
            cc.latest_diagnostics.insert(profile_hash, diagnostics);
            cc.diagnostics_generation += 1;
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(canonical_id) {
                entry.latest_diagnostics.insert(profile_hash, diagnostics);
                entry.diagnostics_generation += 1;
            }
        }
    }

    #[allow(clippy::type_complexity)]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn compile_entry(
        &self,
        snapshot: &CompileInput,
        profile: &CompileProfile,
    ) -> Result<
        (
            FxHashMap<VirtualNodeKind, CachedVirtualFile>,
            DiagnosticsSnapshot,
            Option<CachedTsx>,
            Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
        ),
        DiagnosticsSnapshot,
    > {
        let mut diagnostics = snapshot.parse_diagnostics.clone();

        let mut merged_source = snapshot.source.to_string();
        if !snapshot.src_blocks.is_empty() {
            let ext_sources = {
                let mut map = FxHashMap::default();
                for req in &snapshot.external_requests {
                    if let Some(dep_source) = self.resolve_dep_source(
                        &snapshot.canonical_id,
                        &req.resolved_canonical_id,
                        &req.specifier,
                    ) {
                        map.insert(req.resolved_canonical_id.clone(), dep_source);
                    }
                }
                map
            };

            for (idx, req) in snapshot.external_requests.iter().enumerate() {
                if !ext_sources.contains_key(&req.resolved_canonical_id) {
                    let span = snapshot.src_blocks.get(idx).map(|block| {
                        verter_span::Span::new(block.tag_open_start, block.tag_open_end)
                    });
                    diagnostics =
                        diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                            severity: HostSeverity::Error,
                            code: "HOST_MISSING_EXTERNAL_SOURCE".to_string(),
                            message: format!(
                                "missing external source '{}' for '{}'",
                                req.specifier, snapshot.canonical_id
                            ),
                            span,
                        }]));
                }
            }

            if diagnostics.has_errors {
                return Err(diagnostics);
            }

            merged_source =
                merge_external_sources(&merged_source, &snapshot.src_blocks, &ext_sources);
        }

        let alloc = Allocator::new();
        let core_opts = CodegenOptions {
            filename: profile
                .filename
                .clone()
                .or_else(|| Some(snapshot.canonical_id.clone())),
            is_production: profile.is_production,
            // Host always assembles a standalone `function render()` via
            // assemble_main_module, so inline mode must be off â€” otherwise the
            // template emits bare identifiers (missing `$setup.` prefix).
            inline: Some(false),
            component_id: profile.component_id.clone(),
            delimiters: profile.delimiters.clone(),
            custom_elements: profile.custom_elements.clone(),
            comments: profile.comments,
            runtime_module_name: profile.runtime_module_name.clone(),
            types_module_name: profile.types_module_name.clone(),
            target: profile.target,
            embed_ambient_types: profile.embed_ambient_types,
            conditional_root_narrowing: profile.conditional_root_narrowing,
            strict_slots: profile.strict_slots,
            ..CodegenOptions::default()
        };

        let mut unresolved_macro_type_diags = Vec::new();
        let profile_hash = compile_profile_hash(profile);

        let (external_types, missing_macro_type_diags, transitive_macro_type_deps) = self
            .collect_external_types_from_loaded_files(
                &snapshot.canonical_id,
                &snapshot.macro_type_deps,
                &snapshot.script_imports,
                Some(profile_hash),
            );
        self.sync_transitive_macro_type_dependencies(
            &snapshot.canonical_id,
            &transitive_macro_type_deps,
        );
        unresolved_macro_type_diags.extend(missing_macro_type_diags);

        if !unresolved_macro_type_diags.is_empty() {
            diagnostics =
                diagnostics.merge(DiagnosticsSnapshot::from_vec(unresolved_macro_type_diags));
            return Err(diagnostics);
        }

        let scope = self.config.effective_scope();
        let verter_opts = VerterCompileOptions {
            force_vapor: profile.force_vapor,
            force_js: profile.force_js,
            source_map: profile.source_map,
            ssr: profile.ssr,
            external_types,
            extract_template_data: scope.needs_template_analysis(),
            prop_constness_overrides: None, // TODO(Phase 6): populated by cross-file optimizer
            style_v_bind_vars: snapshot.style_v_bind_vars.clone(),
        };

        // Reuse cached parse when source wasn't modified by external src= merging
        // and no custom delimiters/elements that would change parse behavior.
        let can_use_cache = snapshot.src_blocks.is_empty()
            && profile.delimiters.is_none()
            && profile.custom_elements.is_none();

        let compiled = if can_use_cache {
            if let Some(ref cached) = snapshot.cached_parse {
                compile_from_parsed(&merged_source, cached, &core_opts, &verter_opts, &alloc)
            } else {
                compile_sfc(&merged_source, &core_opts, &verter_opts, &alloc)
            }
        } else {
            compile_sfc(&merged_source, &core_opts, &verter_opts, &alloc)
        };

        let mut compile_diags = diagnostics.clone();
        if !compiled.errors.is_empty() {
            compile_diags = compile_diags.merge(DiagnosticsSnapshot::from_vec(
                compiled
                    .errors
                    .iter()
                    .map(|d| HostDiagnostic {
                        severity: match d.severity {
                            verter_compiler::compile::CompileDiagnosticSeverity::Error => {
                                HostSeverity::Error
                            }
                            verter_compiler::compile::CompileDiagnosticSeverity::Warning => {
                                HostSeverity::Warning
                            }
                            verter_compiler::compile::CompileDiagnosticSeverity::Info => {
                                HostSeverity::Info
                            }
                        },
                        code: d.code.clone(),
                        message: d.message.clone(),
                        span: d.span,
                    })
                    .collect(),
            ));
        }

        if compile_diags.has_errors {
            return Err(compile_diags);
        }

        let mut outputs = FxHashMap::default();

        let main_code =
            assemble_main_module(&snapshot.canonical_id, &compiled, &snapshot.meta, profile);
        outputs.insert(
            VirtualNodeKind::Main,
            CachedVirtualFile {
                code: Arc::from(main_code),
                source_map: None,
                lang: Some(if profile.force_js {
                    "js".to_string()
                } else {
                    snapshot
                        .meta
                        .script_lang
                        .as_deref()
                        .unwrap_or("js")
                        .to_string()
                }),
                meta: VirtualMeta {
                    scope_id: if compiled.scope_id.is_empty() {
                        None
                    } else {
                        Some(compiled.scope_id.clone())
                    },
                    ..VirtualMeta::default()
                },
            },
        );

        if let Some(script) = compiled.script {
            outputs.insert(
                VirtualNodeKind::Script,
                CachedVirtualFile {
                    code: Arc::from(script.code),
                    source_map: if script.source_map.is_empty() {
                        None
                    } else {
                        Some(Arc::from(script.source_map))
                    },
                    lang: Some("ts".to_string()),
                    meta: VirtualMeta::default(),
                },
            );
        }

        if let Some(template) = compiled.template {
            let code = if template.imports.is_empty() {
                template.code
            } else {
                let runtime = profile.runtime_module_name.as_deref().unwrap_or("vue");
                let specifiers: Vec<String> = template
                    .imports
                    .iter()
                    .map(|name| format_import_specifier(name))
                    .collect();
                format!(
                    "import {{ {} }} from \"{}\"\n{}",
                    specifiers.join(", "),
                    runtime,
                    template.code,
                )
            };
            outputs.insert(
                VirtualNodeKind::Template,
                CachedVirtualFile {
                    code: Arc::from(code),
                    source_map: if template.source_map.is_empty() {
                        None
                    } else {
                        Some(Arc::from(template.source_map))
                    },
                    lang: Some("tsx".to_string()),
                    meta: VirtualMeta::default(),
                },
            );
        }

        let style_layer = snapshot.style_override_layer.as_ref();

        for (i, style) in compiled.styles.into_iter().enumerate() {
            let override_entry = style_layer.and_then(|layer| layer.by_index.get(&i));
            outputs.insert(
                VirtualNodeKind::Style { index: i },
                CachedVirtualFile {
                    code: override_entry
                        .map(|e| e.code.clone())
                        .unwrap_or_else(|| Arc::from(style.code)),
                    source_map: override_entry.and_then(|e| e.source_map.clone()),
                    lang: Some(style.lang.unwrap_or_else(|| "css".to_string())),
                    meta: VirtualMeta {
                        style_index: Some(i),
                        ..VirtualMeta::default()
                    },
                },
            );
        }

        for (i, block) in compiled.custom_blocks.into_iter().enumerate() {
            outputs.insert(
                VirtualNodeKind::Custom { index: i },
                CachedVirtualFile {
                    code: Arc::from(block.content),
                    source_map: None,
                    lang: snapshot.meta.custom_langs.get(i).cloned().flatten(),
                    meta: VirtualMeta {
                        custom_index: Some(i),
                        block_type: Some(block.block_type),
                        ..VirtualMeta::default()
                    },
                },
            );
        }

        // Combined IDE output (TSX/JSX) for LSP type checking â€” stored separately, not as virtual file
        let cached_tsx = compiled.tsx.map(|tsx| CachedTsx {
            code: Arc::from(tsx.code),
            source_map: if tsx.source_map.is_empty() {
                None
            } else {
                Some(Arc::from(tsx.source_map))
            },
            is_jsx: tsx.is_jsx,
            destructured_block: tsx.destructured_block,
        });

        // Convert raw template data into analysis types when available
        let template_analysis = compiled.template_data.as_ref().map(|raw| {
            // Build script import pairs for component â†’ source resolution
            let (all_imports, binding_class_unions, props_binding_name) = template_converter_inputs(
                &snapshot.script_imports,
                &snapshot.script_macros,
                &snapshot.script_bindings,
            );
            crate::template_convert::convert_raw_to_analysis(
                raw,
                &all_imports,
                &binding_class_unions,
                props_binding_name.as_deref(),
            )
        });

        Ok((outputs, compile_diags, cached_tsx, template_analysis))
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn template_converter_inputs(
    imports: &[verter_semantic::analysis::AnalyzedImport],
    macros: &[verter_semantic::analysis::AnalyzedMacro],
    bindings: &[verter_semantic::analysis::AnalyzedBinding],
) -> (
    Vec<(String, String)>,
    Vec<(String, Vec<String>)>,
    Option<String>,
) {
    let all_imports: Vec<(String, String)> = imports
        .iter()
        .flat_map(|imp| {
            imp.bindings
                .iter()
                .map(|binding| (binding.name.clone(), imp.source.clone()))
        })
        .collect();

    let mut unions = Vec::new();
    let define_props = macros
        .iter()
        .find(|mac| mac.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps);
    if let Some(dp) = define_props {
        for field in &dp.prop_fields {
            if let Some(type_ann) = &field.type_annotation {
                let classes = verter_semantic::analysis::parse_string_literal_union(type_ann);
                if !classes.is_empty() {
                    unions.push((field.name.clone(), classes));
                }
            }
        }
    }

    for binding in bindings {
        if let Some(type_ann) = &binding.type_annotation {
            let effective_type =
                verter_semantic::analysis::unwrap_reactive_type(type_ann).unwrap_or(type_ann);
            let classes = verter_semantic::analysis::parse_string_literal_union(effective_type);
            if !classes.is_empty() {
                unions.push((binding.name.clone(), classes));
            }
        }
    }

    let props_binding_name = define_props.and_then(|dp| dp.binding_name.clone());

    (all_imports, unions, props_binding_name)
}

/// Extract concatenated script content from a Vue SFC source string.
///
/// Cached parse spans are used when they agree with a raw-source scan. If the
/// parser produced lossy spans for forgiving SFC input, fall back to the raw
/// scan so type resolution still sees the original script text.
pub(crate) fn extract_vue_script_content(
    source: &str,
    cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
) -> Option<String> {
    let scanned = extract_vue_script_content_from_source(source);
    let parsed =
        cached_parse.and_then(|parsed| extract_vue_script_content_from_parsed(source, parsed));

    match (parsed, scanned) {
        (Some(parsed), Some(scanned)) if parsed == scanned => Some(parsed),
        (_, Some(scanned)) => Some(scanned),
        (Some(parsed), None) => Some(parsed),
        (None, None) => None,
    }
}

fn extract_vue_script_content_from_parsed(
    source: &str,
    parsed: &verter_compiler::parser::types::ParsedSfc,
) -> Option<String> {
    let mut script_blocks: Vec<(u32, u32)> = [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .filter_map(|script| script.content.map(|span| (span.start, span.end)))
        .collect();
    script_blocks.sort_by_key(|(start, _)| *start);

    let mut combined = String::new();
    for (start, end) in script_blocks {
        let Some(content) = source.get(start as usize..end as usize) else {
            continue;
        };
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(content);
    }

    (!combined.is_empty()).then_some(combined)
}

fn extract_vue_script_content_from_source(source: &str) -> Option<String> {
    const SCRIPT_OPEN: &[u8] = b"<script";
    const SCRIPT_CLOSE: &[u8] = b"</script>";

    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut combined = String::new();

    while let Some(open_start) = find_ascii_tag(bytes, SCRIPT_OPEN, cursor) {
        let Some(tag_end) = find_tag_end(bytes, open_start) else {
            break;
        };
        if is_self_closing_tag(bytes, tag_end) {
            cursor = tag_end.saturating_add(1);
            continue;
        }

        let content_start = tag_end.saturating_add(1);
        let boundary = find_next_known_root_block(bytes, content_start).unwrap_or(bytes.len());
        let Some(close_start) = find_last_ascii_tag(bytes, SCRIPT_CLOSE, content_start, boundary)
        else {
            cursor = content_start;
            continue;
        };

        let Some(content) = source.get(content_start..close_start) else {
            cursor = close_start.saturating_add(SCRIPT_CLOSE.len());
            continue;
        };
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(content);
        cursor = close_start.saturating_add(SCRIPT_CLOSE.len());
    }

    (!combined.is_empty()).then_some(combined)
}

fn find_ascii_tag(bytes: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || bytes.len() < needle.len() || from >= bytes.len() {
        return None;
    }

    let last_start = bytes.len() - needle.len();
    let mut idx = from;
    while idx <= last_start {
        if bytes[idx..idx + needle.len()].eq_ignore_ascii_case(needle)
            && matches!(
                bytes.get(idx + needle.len()),
                None | Some(b'>')
                    | Some(b'/')
                    | Some(b' ')
                    | Some(b'\t')
                    | Some(b'\n')
                    | Some(b'\r')
            )
        {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

fn find_last_ascii_tag(bytes: &[u8], needle: &[u8], from: usize, to: usize) -> Option<usize> {
    if needle.is_empty() || from >= to || bytes.len() < needle.len() {
        return None;
    }

    let search_end = to.min(bytes.len());
    let mut last = None;
    let mut cursor = from;
    while let Some(idx) = find_ascii_tag(bytes, needle, cursor) {
        if idx >= search_end {
            break;
        }
        last = Some(idx);
        cursor = idx.saturating_add(needle.len());
    }
    last
}

fn find_tag_end(bytes: &[u8], open_start: usize) -> Option<usize> {
    let mut idx = open_start.saturating_add(1);
    let mut quote = None;

    while idx < bytes.len() {
        let ch = bytes[idx];
        match quote {
            Some(active) if ch == active => quote = None,
            Some(_) => {}
            None if ch == b'\'' || ch == b'"' => quote = Some(ch),
            None if ch == b'>' => return Some(idx),
            None => {}
        }
        idx += 1;
    }

    None
}

fn is_self_closing_tag(bytes: &[u8], tag_end: usize) -> bool {
    if tag_end == 0 {
        return false;
    }

    let mut idx = tag_end;
    while idx > 0 {
        idx -= 1;
        match bytes[idx] {
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            b'/' => return true,
            _ => return false,
        }
    }

    false
}

fn find_next_known_root_block(bytes: &[u8], from: usize) -> Option<usize> {
    [
        b"<script".as_slice(),
        b"<template".as_slice(),
        b"<style".as_slice(),
    ]
    .into_iter()
    .filter_map(|needle| find_ascii_tag(bytes, needle, from))
    .min()
}

// ---------------------------------------------------------------------------
// FrontierHost implementation for VerterHost
// ---------------------------------------------------------------------------

/// Adapter connecting the frontier engine to the real host.
///
/// Wraps a `VerterHost` reference with an optional `HostStoreView` for
/// snapshot-consistent resolution.
///
/// Consumed by component-meta resolution (Phase 4+) and frontier integration tests.
pub(crate) struct HostFrontierAdapter<'a> {
    pub host: &'a VerterHost,
    pub store_view: Option<&'a crate::resolver_store::HostStoreView>,
    pub materialize_symbols: bool,
    pub route_exports_only: bool,
}

impl crate::resolver_core::FrontierHost for HostFrontierAdapter<'_> {
    fn ensure_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let canonical = self
            .host
            .resolve_eval_dependency_canonical_in_view(canonical_id, self.store_view)
            .unwrap_or_else(|| canonical_id.to_string());

        if !self.materialize_symbols {
            if let Some(entry) = self
                .host
                .clone_current_imported_dependency_entry(canonical.as_str(), self.store_view)
            {
                if let Some(ref state) = entry.shallow_file_state {
                    if state.has_wildcard_reexports() {
                        self.host
                            .provenance
                            .resolver_barrel_fact_reuse
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    return Some(Arc::clone(state));
                }
            }

            return self
                .host
                .ensure_shallow_imported_dependency_state_in_view(
                    canonical.as_str(),
                    self.store_view,
                )
                .and_then(|entry| entry.shallow_file_state.clone());
        }

        if let Some(entry) = self
            .host
            .clone_current_imported_dependency_entry(canonical.as_str(), self.store_view)
        {
            if let Some(ref state) = entry.shallow_file_state {
                if !state.symbols.is_empty() {
                    if state.has_wildcard_reexports() {
                        self.host
                            .provenance
                            .resolver_barrel_fact_reuse
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    return Some(Arc::clone(state));
                }
            }
        }

        self.host
            .symbol_shallow_file_state_in_view(canonical.as_str(), self.store_view)
    }

    fn resolve_import_canonical(&self, from_canonical: &str, specifier: &str) -> Option<String> {
        self.host.resolve_type_dependency_canonical_shallow_in_view(
            from_canonical,
            specifier,
            self.store_view,
        )
    }

    fn route_exports_only(&self) -> bool {
        self.route_exports_only
    }
}

#[cfg(test)]
#[path = "host_resolve_tests.rs"]
mod host_resolve_tests;

#[cfg(test)]
#[path = "frontier_tests.rs"]
mod frontier_tests;
