//! `impl VerterHost` — resolve and virtual file retrieval methods.
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
#[cfg(feature = "host_metrics")]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
#[cfg(feature = "host_metrics")]
use web_time::Instant;

use oxc_allocator::Allocator;
use verter_core::compile::CodegenOptions;
use verter_core::compile::{
    compile as compile_sfc, compile_from_parsed, format_import_specifier, VerterCompileOptions,
};
use verter_resolver::{
    resolve_external_type_request,
    resolve_type_through_barrel as resolve_type_through_barrel_via_resolver,
    BarrelResolutionResolver, BarrelResolutionState, ExportRegistryView, ExternalTypeBodyResolver,
    ExternalTypeRequestResolver, ExternalTypeResolvedCacheEntry, ExternalTypeRouteEntry,
    RegistryExportEntry, RegistryResolvedTarget, RegistryRouteResolver,
};

#[cfg(not(feature = "scheduler"))]
use crate::cache::enforce_profile_cap;
use crate::compile::{assemble_main_module, merge_external_sources};
use crate::hash::compile_profile_hash;
use crate::id::{parse_raw_id, render_ids, render_single_id};
#[cfg(not(feature = "scheduler"))]
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;

type ResolvedExternalTypes =
    rustc_hash::FxHashMap<String, verter_core::utils::oxc::vue::resolve_type::ResolvedElements>;

type ExternalTypeCache = rustc_hash::FxHashMap<
    (String, String),
    Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
>;

fn external_type_debug_enabled() -> bool {
    std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
        || std::env::var_os("VERTER_META_DEBUG").is_some()
}

fn external_type_debug(message: impl AsRef<str>) {
    if external_type_debug_enabled() {
        eprintln!("[verter-meta] {}", message.as_ref());
    }
}

struct ViewExternalTypeResolver<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a crate::resolver_store::HostStoreView>,
}

struct ViewExternalTypeGraphResolver<'a> {
    host: &'a VerterHost,
    store_view: &'a crate::resolver_store::HostStoreView,
}

struct HostExternalMacroTypeCollector<'a> {
    host: &'a VerterHost,
}

struct HostLiveExternalTypeRequestResolver<'a> {
    host: &'a VerterHost,
}

#[cfg(feature = "scheduler")]
struct HostRegistryRouteResolver<'a> {
    host: &'a VerterHost,
}

impl verter_resolver::ExternalMacroTypeCollectorHost for HostExternalMacroTypeCollector<'_> {
    type Error = crate::types::ExternalTypeResolveError;

    fn resolve_external_macro_type(
        &self,
        owner_canonical: &str,
        dep: &verter_analysis::MacroTypeDep,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut rustc_hash::FxHashMap<
            (String, String),
            Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
        >,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        profile_hash: Option<u64>,
    ) -> Result<Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>, Self::Error>
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
            verter_vfs::ResolveRequestKind::TypeImport,
            true,
            profile_hash,
            0,
        )
    }

    fn map_external_macro_type_error(
        &self,
        owner_canonical: &str,
        dep: &verter_analysis::MacroTypeDep,
        import_span: Option<verter_span::Span>,
        error: &Self::Error,
    ) -> verter_resolver::ExternalMacroTypeDiagnostic {
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

        verter_resolver::ExternalMacroTypeDiagnostic {
            code,
            message,
            span: import_span,
        }
    }
}

impl verter_resolver::ExternalTypeGraphResolver for ViewExternalTypeGraphResolver<'_> {
    type Error = crate::types::ExternalTypeResolveError;

    fn max_external_type_resolve_steps(&self) -> usize {
        crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS
    }

    fn max_external_type_resolve_depth(&self) -> usize {
        crate::types::MAX_RESOLVE_DEPTH
    }

    fn missing_root_dependency(&self) -> Self::Error {
        crate::types::ExternalTypeResolveError::MissingRootDependency
    }

    fn depth_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
        crate::types::ExternalTypeResolveError::DepthLimitExceeded {
            limit: crate::types::MAX_RESOLVE_DEPTH,
            type_name: type_name.to_string(),
            last_dep: last_dep.to_string(),
        }
    }

    fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
        crate::types::ExternalTypeResolveError::StepLimitExceeded {
            limit: crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS,
            type_name: type_name.to_string(),
            last_dep: last_dep.to_string(),
        }
    }

    fn resolve_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Option<String> {
        self.host.resolve_loaded_dependency_canonical_in_view(
            owner_canonical,
            import_source,
            kind,
            Some(self.store_view),
        )
    }

    fn read_source_for_type_resolution(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String> {
        self.host.read_dep_source_for_type_resolution_in_view(
            dep_canonical,
            profile_hash,
            Some(self.store_view),
        )
    }

    fn debug_enabled(&self) -> bool {
        external_type_debug_enabled()
    }

    fn debug_log(&self, message: String) {
        external_type_debug(message);
    }

    fn lookup_resolved_type_cache(
        &self,
        dep_canonical: &str,
        type_name: &str,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Option<verter_resolver::ExternalTypeResolvedCacheEntry> {
        let effective_canonical = self
            .host
            .load_eval_dependency_canonical_with_fallback_in_view(
                dep_canonical,
                Some(self.store_view),
            )
            .unwrap_or_else(|| dep_canonical.to_string());
        let source_hash = self
            .store_view
            .whole_hash(&effective_canonical)
            .or_else(|| self.host.get_whole_hash(&effective_canonical))
            .or_else(|| {
                self.host
                    .read_dep_source_for_type_resolution_in_view(
                        &effective_canonical,
                        None,
                        Some(self.store_view),
                    )
                    .map(|source| crate::hash::hash_16(source.as_bytes()))
            })?;
        let key = crate::types::ResolvedTypeCacheKey {
            dep_canonical_id: effective_canonical,
            dep_source_hash: source_hash,
            type_name: type_name.to_string(),
            resolve_kind: kind,
        };
        let host_cache = self.host.resolved_type_cache.lock();
        let entry = host_cache.get(&key)?;
        Some(verter_resolver::ExternalTypeResolvedCacheEntry {
            resolved: entry.resolved.clone(),
            tracked_deps: entry.tracked_deps.clone(),
        })
    }

    fn note_resolved_type_cache_hit(&self) {
        self.host
            .provenance
            .resolved_external_type_cache_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn note_resolved_type(
        &self,
        dep_canonical: &str,
        type_name: &str,
        resolved: Option<&verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
        tracked_deps: &[String],
    ) {
        let effective_canonical = self
            .host
            .load_eval_dependency_canonical_with_fallback_in_view(
                dep_canonical,
                Some(self.store_view),
            )
            .unwrap_or_else(|| dep_canonical.to_string());
        let source_hash = self
            .store_view
            .whole_hash(&effective_canonical)
            .or_else(|| self.host.get_whole_hash(&effective_canonical))
            .or_else(|| {
                self.host
                    .read_dep_source_for_type_resolution_in_view(
                        &effective_canonical,
                        None,
                        Some(self.store_view),
                    )
                    .map(|source| crate::hash::hash_16(source.as_bytes()))
            })
            .unwrap_or_default();
        let key = crate::types::ResolvedTypeCacheKey {
            dep_canonical_id: effective_canonical,
            dep_source_hash: source_hash,
            type_name: type_name.to_string(),
            resolve_kind: verter_vfs::ResolveRequestKind::TypeImport,
        };
        let mut host_cache = self.host.resolved_type_cache.lock();
        if host_cache.len() >= crate::types::RESOLVED_TYPE_CACHE_CAP {
            host_cache.clear();
        }
        self.host
            .provenance
            .resolved_external_type_cache_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        host_cache.insert(
            key,
            crate::types::ResolvedTypeCacheEntry {
                resolved: resolved.cloned(),
                tracked_deps: tracked_deps.to_vec(),
            },
        );
    }
}

#[cfg(feature = "scheduler")]
impl RegistryRouteResolver for HostRegistryRouteResolver<'_> {
    fn ensure_export_registry(&self, canonical: &str) -> Option<ExportRegistryView> {
        let registry = self.host.ensure_export_registry(canonical)?;
        Some(ExportRegistryView {
            source_hash: registry.source_hash,
            named: registry
                .named
                .into_iter()
                .map(|(name, entry)| {
                    let entry = match entry {
                        crate::types::ExportEntry::Defined => RegistryExportEntry::Defined,
                        crate::types::ExportEntry::Alias {
                            source_specifier,
                            original_name,
                        } => RegistryExportEntry::Alias {
                            source_specifier,
                            original_name,
                        },
                    };
                    (name, entry)
                })
                .collect(),
            wildcard_edges: registry.wildcard_edges,
        })
    }

    fn resolve_loaded_dependency_canonical(
        &self,
        canonical: &str,
        source_specifier: &str,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Option<String> {
        self.host
            .resolve_loaded_dependency_canonical(canonical, source_specifier, kind)
    }
}

impl ExternalTypeBodyResolver for ViewExternalTypeResolver<'_> {
    type Error = crate::types::ExternalTypeResolveError;

    fn max_external_type_resolve_steps(&self) -> usize {
        crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS
    }

    fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
        crate::types::ExternalTypeResolveError::StepLimitExceeded {
            limit: crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS,
            type_name: type_name.to_string(),
            last_dep: last_dep.to_string(),
        }
    }

    fn debug_enabled(&self) -> bool {
        external_type_debug_enabled()
    }

    fn debug_log(&self, message: String) {
        external_type_debug(message);
    }

    fn note_cycle_detected(&self) {
        self.host
            .provenance
            .resolver_cycle_detections
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn resolve_external_type_recursive(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut verter_resolver::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        required_root_dep: bool,
        kind: verter_vfs::ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
    ) -> Result<Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>, Self::Error>
    {
        self.host.resolve_external_type_from_loaded_files_in_view(
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
            self.store_view,
        )
    }

    fn resolve_type_through_barrel(
        &self,
        barrel_canonical: &str,
        type_name: &str,
        wildcard_sources: &[String],
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut verter_resolver::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        kind: verter_vfs::ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
        debug_enabled: bool,
    ) -> Result<Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>, Self::Error>
    {
        let _ = debug_enabled;
        resolve_type_through_barrel_via_resolver(
            self,
            barrel_canonical,
            type_name,
            wildcard_sources,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
            kind,
            use_host_cache,
            profile_hash,
            depth,
        )
    }
}

impl BarrelResolutionResolver for ViewExternalTypeResolver<'_> {
    type Error = crate::types::ExternalTypeResolveError;

    fn cached_barrel_state(&self, barrel_canonical: &str) -> Option<BarrelResolutionState> {
        if self.store_view.is_some() {
            return None;
        }

        #[cfg(feature = "scheduler")]
        {
            self.host
                .compile_cache
                .get(barrel_canonical)
                .and_then(|cc| cc.barrel_export_surface.clone())
                .map(|state| BarrelResolutionState {
                    export_map: state.export_map,
                    source_hash: state.source_hash,
                    wildcard_sources: state.wildcard_sources,
                    scanned_sources: state.scanned_sources,
                    tracked_deps: state.tracked_deps,
                    fully_resolved: state.fully_resolved,
                    generation: state.generation,
                })
        }
        #[cfg(not(feature = "scheduler"))]
        {
            let _ = barrel_canonical;
            None
        }
    }

    fn source_hash(&self, canonical: &str) -> verter_resolver::ResolverHash16 {
        self.store_view
            .and_then(|view| view.whole_hash(canonical))
            .unwrap_or_else(|| self.host.get_whole_hash(canonical).unwrap_or_default())
    }

    fn resolve_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Option<String> {
        self.host.resolve_loaded_dependency_canonical_in_view(
            owner_canonical,
            import_source,
            kind,
            self.store_view,
        )
    }

    fn read_source_for_type_resolution(
        &self,
        canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String> {
        self.host.read_dep_source_for_type_resolution_in_view(
            canonical,
            profile_hash,
            self.store_view,
        )
    }

    fn resolve_external_type_recursive(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut verter_resolver::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        required_root_dep: bool,
        kind: verter_vfs::ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
    ) -> Result<Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>, Self::Error>
    {
        self.host.resolve_external_type_from_loaded_files_in_view(
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
            self.store_view,
        )
    }

    fn persist_barrel_state(
        &self,
        barrel_canonical: &str,
        state: &BarrelResolutionState,
        replace_existing: bool,
    ) {
        if self.store_view.is_some() {
            return;
        }

        self.host.persist_barrel_state(
            barrel_canonical,
            &crate::types::BarrelResolutionState {
                export_map: state.export_map.clone(),
                source_hash: state.source_hash,
                wildcard_sources: state.wildcard_sources.clone(),
                scanned_sources: state.scanned_sources.clone(),
                tracked_deps: state.tracked_deps.clone(),
                fully_resolved: state.fully_resolved,
                generation: state.generation,
            },
            replace_existing,
        );
    }

    fn note_barrel_fact_reuse(&self) {
        self.host
            .provenance
            .resolver_barrel_fact_reuse
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn debug_enabled(&self) -> bool {
        external_type_debug_enabled()
    }

    fn debug_log(&self, message: String) {
        external_type_debug(message);
    }
}

impl ExternalTypeBodyResolver for HostLiveExternalTypeRequestResolver<'_> {
    type Error = crate::types::ExternalTypeResolveError;

    fn max_external_type_resolve_steps(&self) -> usize {
        crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS
    }

    fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
        crate::types::ExternalTypeResolveError::StepLimitExceeded {
            limit: crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS,
            type_name: type_name.to_string(),
            last_dep: last_dep.to_string(),
        }
    }

    fn debug_enabled(&self) -> bool {
        external_type_debug_enabled()
    }

    fn debug_log(&self, message: String) {
        external_type_debug(message);
    }

    fn note_cycle_detected(&self) {
        self.host
            .provenance
            .resolver_cycle_detections
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn resolve_external_type_recursive(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut verter_resolver::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        required_root_dep: bool,
        kind: verter_vfs::ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
    ) -> Result<Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>, Self::Error>
    {
        resolve_external_type_request(
            self,
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
        )
    }

    fn resolve_type_through_barrel(
        &self,
        barrel_canonical: &str,
        type_name: &str,
        wildcard_sources: &[String],
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut verter_resolver::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        kind: verter_vfs::ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
        debug_enabled: bool,
    ) -> Result<Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>, Self::Error>
    {
        let _ = debug_enabled;
        let resolver = ViewExternalTypeResolver {
            host: self.host,
            store_view: None,
        };
        resolve_type_through_barrel_via_resolver(
            &resolver,
            barrel_canonical,
            type_name,
            wildcard_sources,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
            kind,
            use_host_cache,
            profile_hash,
            depth,
        )
    }
}

impl ExternalTypeRequestResolver for HostLiveExternalTypeRequestResolver<'_> {
    fn max_external_type_resolve_depth(&self) -> usize {
        crate::types::MAX_RESOLVE_DEPTH
    }

    fn missing_root_dependency(&self) -> Self::Error {
        crate::types::ExternalTypeResolveError::MissingRootDependency
    }

    fn depth_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
        crate::types::ExternalTypeResolveError::DepthLimitExceeded {
            limit: crate::types::MAX_RESOLVE_DEPTH,
            type_name: type_name.to_string(),
            last_dep: last_dep.to_string(),
        }
    }

    fn resolve_loaded_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Option<String> {
        self.host.resolve_loaded_dependency_canonical_in_view(
            owner_canonical,
            import_source,
            kind,
            None,
        )
    }

    fn lookup_route_cache(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Option<ExternalTypeRouteEntry> {
        #[cfg(feature = "scheduler")]
        {
            let route_key = (import_source.to_string(), type_name.to_string(), kind);
            let route_entry = self
                .host
                .compile_cache
                .get(owner_canonical)
                .and_then(|cc| cc.import_route_cache.get(&route_key).cloned())?;

            let owner_hash = self
                .host
                .get_whole_hash(owner_canonical)
                .unwrap_or_default();
            let fresh = route_entry.owner_hash == owner_hash
                && route_entry.route_hashes.iter().all(|(canonical, hash)| {
                    self.host.get_whole_hash(canonical).unwrap_or_default() == *hash
                });
            if !fresh {
                return None;
            }

            let neg_fresh = route_entry
                .negative_barrel_gen
                .as_ref()
                .map(|(barrel_canonical, generation)| {
                    self.host
                        .compile_cache
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

            Some(ExternalTypeRouteEntry {
                owner_hash: route_entry.owner_hash,
                target: route_entry.target.map(|target| RegistryResolvedTarget {
                    final_canonical_id: target.final_canonical_id,
                    exported_name: target.exported_name,
                }),
                tracked_deps: route_entry.tracked_deps,
                route_hashes: route_entry.route_hashes,
                negative_barrel_generation: route_entry.negative_barrel_gen,
            })
        }
        #[cfg(not(feature = "scheduler"))]
        {
            let _ = (owner_canonical, import_source, type_name, kind);
            None
        }
    }

    fn whole_hash(&self, canonical: &str) -> verter_resolver::ResolverHash16 {
        self.host.get_whole_hash(canonical).unwrap_or_default()
    }

    fn compute_source_hash(&self, source: &str) -> verter_resolver::ResolverHash16 {
        crate::hash::hash_16(source.as_bytes())
    }

    fn lookup_resolved_type_cache(
        &self,
        dep_canonical: &str,
        dep_source_hash: verter_resolver::ResolverHash16,
        type_name: &str,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Option<ExternalTypeResolvedCacheEntry> {
        let key = crate::types::ResolvedTypeCacheKey {
            dep_canonical_id: dep_canonical.to_string(),
            dep_source_hash,
            type_name: type_name.to_string(),
            resolve_kind: kind,
        };
        self.host
            .resolved_type_cache
            .lock()
            .get(&key)
            .cloned()
            .map(|entry| ExternalTypeResolvedCacheEntry {
                resolved: entry.resolved,
                tracked_deps: entry.tracked_deps,
            })
    }

    fn note_resolved_type_cache_hit(&self) {
        self.host
            .provenance
            .resolved_external_type_cache_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn note_resolved_type_cache_miss(&self) {
        self.host
            .provenance
            .resolved_external_type_cache_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn note_route_fact_reuse(&self) {
        self.host
            .provenance
            .resolver_route_fact_reuse
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn resolve_type_via_registry(
        &self,
        canonical: &str,
        type_name: &str,
        kind: verter_vfs::ResolveRequestKind,
        visited: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<verter_resolver::RegistryRoute> {
        #[cfg(feature = "scheduler")]
        {
            let route = self
                .host
                .resolve_type_via_registry(canonical, type_name, kind, visited);
            Some(verter_resolver::RegistryRoute {
                target: route.target.map(|target| RegistryResolvedTarget {
                    final_canonical_id: target.final_canonical_id,
                    exported_name: target.exported_name,
                }),
                tracked_deps: route.tracked_deps,
                route_hashes: route.route_hashes,
            })
        }
        #[cfg(not(feature = "scheduler"))]
        {
            let _ = (canonical, type_name, kind, visited);
            None
        }
    }

    fn read_source_for_type_resolution(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String> {
        self.host
            .read_dep_source_for_type_resolution(dep_canonical, profile_hash)
    }

    fn store_resolved_type_cache(
        &self,
        dep_canonical: &str,
        dep_source_hash: verter_resolver::ResolverHash16,
        type_name: &str,
        kind: verter_vfs::ResolveRequestKind,
        resolved: Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
        tracked_deps: Vec<String>,
    ) {
        let key = crate::types::ResolvedTypeCacheKey {
            dep_canonical_id: dep_canonical.to_string(),
            dep_source_hash,
            type_name: type_name.to_string(),
            resolve_kind: kind,
        };
        let mut host_cache = self.host.resolved_type_cache.lock();
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

    fn store_route_cache(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        kind: verter_vfs::ResolveRequestKind,
        entry: ExternalTypeRouteEntry,
    ) {
        #[cfg(feature = "scheduler")]
        {
            let route_key = (import_source.to_string(), type_name.to_string(), kind);
            let route_entry = crate::types::ImportTypeRouteEntry {
                owner_hash: entry.owner_hash,
                target: entry
                    .target
                    .map(|target| crate::types::NormalizedTypeTarget {
                        final_canonical_id: target.final_canonical_id,
                        exported_name: target.exported_name,
                    }),
                tracked_deps: entry.tracked_deps,
                route_hashes: entry.route_hashes,
                negative_barrel_gen: entry.negative_barrel_generation,
            };
            if let Some(mut cc) = self.host.compile_cache.get_mut(owner_canonical) {
                cc.import_route_cache.insert(route_key, route_entry);
            }
        }
        #[cfg(not(feature = "scheduler"))]
        {
            let _ = (owner_canonical, import_source, type_name, kind, entry);
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

    pub(crate) fn resolve_loaded_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Option<String> {
        self.ws()
            .resolve_import(
                owner_canonical,
                import_source,
                verter_vfs::ResolutionContext {
                    phase: verter_vfs::ResolvePhase::CodegenBlocker,
                    kind,
                },
            )
            .map(|r| r.source_id)
    }

    pub(crate) fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.resolve_loaded_dependency_canonical(
            owner_canonical,
            import_source,
            verter_vfs::ResolveRequestKind::TypeImport,
        )
        .or_else(|| {
            self.resolve_loaded_dependency_canonical(
                owner_canonical,
                import_source,
                verter_vfs::ResolveRequestKind::EsmImport,
            )
        })
    }

    pub(crate) fn resolve_type_dependency_canonical_in_view(
        &self,
        owner_canonical: &str,
        import_source: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        if let Some(view) = store_view {
            self.current_eval_state_in_view(owner_canonical, Some(view))?;
            if let Some(resolved) = view
                .dependency_resolution(owner_canonical, import_source)
                .and_then(|resolution| {
                    resolution
                        .resolved_canonical_id
                        .clone()
                        .or_else(|| resolution.effective_target().map(str::to_string))
                })
            {
                return Some(resolved);
            }

            return self.resolve_type_dependency_canonical(owner_canonical, import_source);
        }
        self.resolve_type_dependency_canonical(owner_canonical, import_source)
    }

    pub(crate) fn resolve_loaded_dependency_canonical_in_view(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_vfs::ResolveRequestKind,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        if kind == verter_vfs::ResolveRequestKind::TypeImport {
            return self.resolve_type_dependency_canonical_in_view(
                owner_canonical,
                import_source,
                store_view,
            );
        }

        if let Some(view) = store_view {
            self.current_eval_state_in_view(owner_canonical, Some(view))?;
            if let Some(resolved) = view
                .dependency_resolution(owner_canonical, import_source)
                .and_then(|resolution| {
                    resolution
                        .resolved_canonical_id
                        .clone()
                        .or_else(|| resolution.effective_target().map(str::to_string))
                })
            {
                return Some(resolved);
            }

            return self.resolve_loaded_dependency_canonical(owner_canonical, import_source, kind);
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
        kind: verter_vfs::ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
    ) -> Result<
        Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
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
        kind: verter_vfs::ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Result<
        Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
        crate::types::ExternalTypeResolveError,
    > {
        if let Some(view) = store_view {
            let resolver = ViewExternalTypeGraphResolver {
                host: self,
                store_view: view,
            };
            return verter_resolver::resolve_external_type_from_graph(
                &resolver,
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
            );
        }

        let resolver = HostLiveExternalTypeRequestResolver { host: self };
        resolve_external_type_request(
            &resolver,
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
        )
    }

    #[cfg(feature = "scheduler")]
    fn persist_barrel_state(
        &self,
        barrel_canonical: &str,
        state: &crate::types::BarrelResolutionState,
        replace_existing: bool,
    ) {
        if let Some(mut cc) = self.compile_cache.get_mut(barrel_canonical) {
            match cc.barrel_export_surface.as_mut() {
                Some(existing)
                    if !replace_existing && existing.source_hash == state.source_hash =>
                {
                    let prior_exports = existing.export_map.len();
                    let prior_scanned = existing.scanned_sources.len();
                    let prior_fully_resolved = existing.fully_resolved;

                    for (name, value) in &state.export_map {
                        existing
                            .export_map
                            .entry(name.clone())
                            .or_insert_with(|| value.clone());
                    }
                    for (child, hash) in &state.scanned_sources {
                        existing
                            .scanned_sources
                            .entry(child.clone())
                            .or_insert(*hash);
                    }
                    existing
                        .tracked_deps
                        .extend(state.tracked_deps.iter().cloned());
                    existing.fully_resolved |= state.fully_resolved;

                    if existing.export_map.len() != prior_exports
                        || existing.scanned_sources.len() != prior_scanned
                        || existing.fully_resolved != prior_fully_resolved
                    {
                        existing.generation = existing.generation.saturating_add(1);
                    }
                }
                Some(existing) => {
                    let mut next = state.clone();
                    next.generation = existing.generation.saturating_add(1);
                    cc.barrel_export_surface = Some(next);
                }
                None => {
                    cc.barrel_export_surface = Some(state.clone());
                }
            }
        }
    }

    #[cfg(not(feature = "scheduler"))]
    fn persist_barrel_state(
        &self,
        _barrel_canonical: &str,
        _state: &crate::types::BarrelResolutionState,
        _replace_existing: bool,
    ) {
    }

    /// Ensure the export registry is populated for a file.
    ///
    /// For files in compile_cache with scheduler analysis data, builds the
    /// registry from `HostAnalysisData.export_signatures` (no OXC parsing).
    /// For files only on disk, reads the file and extracts export signatures
    /// (one parse, cached for all future lookups).
    #[cfg(feature = "scheduler")]
    fn ensure_export_registry(&self, canonical: &str) -> Option<crate::types::FileExportRegistry> {
        // Check if already populated
        if let Some(cc) = self.compile_cache.get(canonical) {
            if let Some(ref registry) = cc.export_registry {
                // Validate freshness: source hash must match
                let current_hash = self.get_whole_hash(canonical).unwrap_or_default();
                if registry.source_hash == current_hash {
                    return Some(registry.clone());
                }
            }
        }

        // Build from scheduler analysis data if available
        let registry = {
            use crate::host_executor::HostAnalysisData;

            let analysis_snap = self.scheduler.try_get_analysis(canonical);
            let export_sigs: Vec<verter_analysis::ExportSignature> = analysis_snap
                .as_ref()
                .and_then(|snap| snap.downcast_data::<HostAnalysisData>())
                .map(|ad| ad.export_signatures.clone())
                .unwrap_or_default();

            let source_hash = self.get_whole_hash(canonical).unwrap_or_default();

            // If no analysis data and no whole_hash, try reading from disk
            if export_sigs.is_empty() && source_hash == [0u8; 16] {
                // File not in scheduler — try disk read + export signature extraction
                let source = self.read_dep_source_for_type_resolution(canonical, None)?;
                let disk_hash = crate::hash::hash_16(source.as_bytes());
                let alloc = oxc_allocator::Allocator::new();
                let sigs = verter_analysis::extract_export_signatures(
                    &source,
                    oxc_span::SourceType::ts(),
                    &alloc,
                );
                Some(Self::build_export_registry(&sigs, disk_hash))
            } else {
                Some(Self::build_export_registry(&export_sigs, source_hash))
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
    fn build_export_registry(
        sigs: &[verter_analysis::ExportSignature],
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

    /// Resolve a type name through the export registry using BFS.
    ///
    /// Named exports are checked first (authoritative). If not found, BFS
    /// through wildcard edges in source declaration order. Returns a
    /// `RegistryRoute` with the target and traversal metadata.
    #[cfg(feature = "scheduler")]
    fn resolve_type_via_registry(
        &self,
        canonical: &str,
        type_name: &str,
        kind: verter_vfs::ResolveRequestKind,
        visited: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> crate::types::RegistryRoute {
        let resolver = HostRegistryRouteResolver { host: self };
        let route = verter_resolver::resolve_type_via_registry(
            &resolver, canonical, type_name, kind, visited,
        );

        crate::types::RegistryRoute {
            target: route
                .target
                .map(|target| crate::types::NormalizedTypeTarget {
                    final_canonical_id: target.final_canonical_id,
                    exported_name: target.exported_name,
                }),
            tracked_deps: route.tracked_deps,
            route_hashes: route.route_hashes,
        }
    }

    /// Read the effective source for a dependency file for type resolution.
    ///
    /// On the scheduler path, tries the scheduler's source snapshot first.
    /// On the WASM path, tries `self.files` first.
    /// Both fall back to reading from the VFS workspace.
    /// For Vue SFCs, extracts only `<script>` / `<script setup>` content.
    pub(crate) fn read_dep_source_for_type_resolution(
        &self,
        dep_canonical: &str,
        _profile_hash: Option<u64>,
    ) -> Option<String> {
        // Try in-memory source first (scheduler or files map).
        #[cfg(feature = "scheduler")]
        {
            if let Some(efs) = self.effective_file_state(dep_canonical, _profile_hash) {
                let source = efs.source.as_ref();
                if dep_canonical.ends_with(".vue") {
                    return extract_vue_script_content(source, efs.cached_parse.as_deref());
                } else {
                    return Some(source.to_string());
                }
            }
        }
        #[cfg(not(feature = "scheduler"))]
        {
            let file_kind = {
                let files = crate::shared::read_lock(&self.files);
                files.get(dep_canonical).map(|entry| entry.file_kind)
            };
            if let (Some(file_kind), Some(efs)) = (
                file_kind,
                self.effective_file_state(dep_canonical, _profile_hash),
            ) {
                if file_kind == FileKind::VueSfc {
                    return extract_vue_script_content(
                        efs.source.as_ref(),
                        efs.cached_parse.as_deref(),
                    );
                } else {
                    return Some(efs.source.to_string());
                }
            }
        }

        // Workspace-read fallback: read from disk via VFS.
        let ws = self.ws();
        let source = ws.read_file(dep_canonical)?;
        if dep_canonical.ends_with(".vue") {
            extract_vue_script_content(&source, None)
        } else {
            Some(source.to_string())
        }
    }

    pub(crate) fn read_dep_source_for_type_resolution_in_view(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        let Some(view) = store_view else {
            return self.read_dep_source_for_type_resolution(dep_canonical, profile_hash);
        };

        #[cfg(feature = "scheduler")]
        {
            if let Some((source, cached_parse, _)) =
                self.current_eval_state_in_view(dep_canonical, Some(view))
            {
                if dep_canonical.ends_with(".vue") {
                    return extract_vue_script_content(source.as_ref(), cached_parse.as_deref());
                }
                return Some(source.to_string());
            }
        }
        #[cfg(not(feature = "scheduler"))]
        {
            let file_kind = {
                let files = crate::shared::read_lock(&self.files);
                files.get(dep_canonical).map(|entry| entry.file_kind)
            };
            if let (Some(file_kind), Some((source, cached_parse, _))) = (
                file_kind,
                self.current_eval_state_in_view(dep_canonical, Some(view)),
            ) {
                if file_kind == FileKind::VueSfc {
                    return extract_vue_script_content(source.as_ref(), cached_parse.as_deref());
                }
                return Some(source.to_string());
            }
        }

        let source = self.ws().read_file(dep_canonical)?;
        let whole_hash = crate::hash::hash_16(source.as_bytes());
        if !self.store_view_allows_current_whole_hash(dep_canonical, whole_hash, Some(view)) {
            return None;
        }
        if dep_canonical.ends_with(".vue") {
            extract_vue_script_content(&source, None)
        } else {
            Some(source.to_string())
        }
    }

    fn collect_external_types_from_loaded_files(
        &self,
        owner_canonical: &str,
        macro_type_deps: &[verter_analysis::MacroTypeDep],
        script_imports: &[verter_analysis::AnalyzedImport],
        profile_hash: Option<u64>,
    ) -> (
        Option<ResolvedExternalTypes>,
        Vec<HostDiagnostic>,
        std::collections::BTreeSet<String>,
    ) {
        let collected = verter_resolver::collect_external_macro_types(
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
        #[cfg(feature = "host_metrics")]
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

        // Cache miss — compile by requesting the Main virtual file.
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
    /// `verter_core::compile`, caches the result, and returns the requested node.
    /// In dev mode with [`CompileErrorPolicy::DevServeLastKnownGood`], falls back
    /// to the last successful compilation when the current source has errors.
    pub fn get_virtual_file(&self, query: VirtualQuery) -> Result<VirtualFileResponse, HostError> {
        #[cfg(feature = "host_metrics")]
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
                            #[cfg(feature = "host_metrics")]
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

                // Cache miss — use effective_* helpers for override-aware state
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
                let style_analyses: Arc<Vec<verter_analysis::StyleBlockAnalysis>> = analysis_snap
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
                        #[cfg(feature = "host_metrics")]
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

        #[cfg(feature = "host_metrics")]
        self.metrics
            .compile_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        #[cfg(feature = "host_metrics")]
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

        #[cfg(feature = "host_metrics")]
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

    /// Generate public API output for a Vue SFC — minimal TypeScript declarations.
    ///
    /// Unlike [`get_ide`](Self::get_ide), this does NOT require a prior
    /// [`get_virtual_file`](Self::get_virtual_file) call. It performs
    /// macro-only extraction (OXC parse → defineProps/Emits/Model/Options)
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
            PublicApiMode::Public => verter_core::tsc::TscMode::Public,
            PublicApiMode::Testing => verter_core::tsc::TscMode::Testing,
        };

        // Try cached extract path: avoids re-parsing SFC + OXC on cache hit.
        let extract = if let Some(cached) = cached_extract {
            cached
        } else if let Some(fresh) = verter_core::tsc::extract_tsc_state(
            &source,
            &component_name,
            &verter_core::tsc::TscExtractOptions {
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
            // No <script setup> — fall through to direct path for empty stub
            let tsc_out = verter_core::tsc::generate_tsc_output_with_options(
                &source,
                &component_name,
                &verter_core::tsc::TscGenOptions {
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

        let tsc_out = verter_core::tsc::generate_tsc_from_state(
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
            Option<verter_analysis::template::TemplateAnalysisSnapshot>,
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
            // assemble_main_module, so inline mode must be off — otherwise the
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
                            verter_core::compile::CompileDiagnosticSeverity::Error => {
                                HostSeverity::Error
                            }
                            verter_core::compile::CompileDiagnosticSeverity::Warning => {
                                HostSeverity::Warning
                            }
                            verter_core::compile::CompileDiagnosticSeverity::Info => {
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

        // Combined IDE output (TSX/JSX) for LSP type checking — stored separately, not as virtual file
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
            // Build script import pairs for component → source resolution
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
    imports: &[verter_analysis::AnalyzedImport],
    macros: &[verter_analysis::AnalyzedMacro],
    bindings: &[verter_analysis::AnalyzedBinding],
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
        .find(|mac| mac.kind == verter_analysis::AnalyzedMacroKind::DefineProps);
    if let Some(dp) = define_props {
        for field in &dp.prop_fields {
            if let Some(type_ann) = &field.type_annotation {
                let classes = verter_analysis::parse_string_literal_union(type_ann);
                if !classes.is_empty() {
                    unions.push((field.name.clone(), classes));
                }
            }
        }
    }

    for binding in bindings {
        if let Some(type_ann) = &binding.type_annotation {
            let effective_type =
                verter_analysis::unwrap_reactive_type(type_ann).unwrap_or(type_ann);
            let classes = verter_analysis::parse_string_literal_union(effective_type);
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
    cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
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
    parsed: &verter_core::parser::types::ParsedSfc,
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

#[cfg(test)]
#[path = "host_resolve_tests.rs"]
mod host_resolve_tests;
