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

#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::FxHashMap;

#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "session_metrics")]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
#[cfg(feature = "session_metrics")]
use web_time::Instant;

use crate::compile::{assemble_main_module, merge_external_sources};
use crate::hash::compile_profile_hash;
use crate::host_manage::component_meta_trace_custom;
use crate::id::{parse_raw_id, render_ids, render_single_id};
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
type FrontierRequestedRoutes =
    rustc_hash::FxHashMap<(String, String), crate::resolver_core::RouteDemand>;
type RouteShallowStateCache =
    rustc_hash::FxHashMap<String, Arc<crate::resolver_core::ShallowFileState>>;
type FrontierCompanionPlanCache = rustc_hash::FxHashMap<
    (String, String, crate::resolver_core::RouteDemand),
    Arc<[PlannedFrontierCompanion]>,
>;

struct DirectComponentMetaDeclarationResolver<'a> {
    host: &'a VerterHost,
}

impl crate::resolver_core::DeclarationMetadataResolver
    for DirectComponentMetaDeclarationResolver<'_>
{
    fn resolve_export_target(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<crate::resolver_core::ResolvedExportTarget> {
        None
    }

    fn get_export_span_follow_reexports(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<verter_span::Span> {
        None
    }

    fn read_source(&self, canonical_source: &str) -> Option<String> {
        self.host
            .read_analysis_source(canonical_source)
            .map(|source| source.to_string())
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        self.host
            .local_type_declaration_id(canonical_source, resolved_name)
    }

    fn resolve_type_dependency_canonical(
        &self,
        _from_canonical: &str,
        _import_source: &str,
    ) -> Option<String> {
        None
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<crate::resolver_core::ResolvedLocalTypeSymbolMetadata> {
        let analysis = self.host.external_type_analysis(canonical_source)?;
        let symbol = analysis.local_type_symbol(resolved_name)?;
        let kind = match symbol.kind {
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::TypeAlias => {
                crate::resolver_core::ResolvedDeclarationKind::TypeAlias
            }
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Interface => {
                crate::resolver_core::ResolvedDeclarationKind::Interface
            }
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Class => {
                crate::resolver_core::ResolvedDeclarationKind::Class
            }
        };
        Some(crate::resolver_core::ResolvedLocalTypeSymbolMetadata {
            kind,
            span: symbol.span,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedFrontierCompanion {
    alias: String,
    resolved_canonical: String,
    resolved_exported_name: String,
    route: crate::resolver_core::RouteDemand,
}

#[derive(Default)]
struct FrontierCompanionPlans {
    entries: FrontierCompanionPlanCache,
}

impl FrontierCompanionPlans {
    fn get_or_compute<F>(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
        route: &crate::resolver_core::RouteDemand,
        compute: F,
    ) -> Arc<[PlannedFrontierCompanion]>
    where
        F: FnOnce() -> Vec<PlannedFrontierCompanion>,
    {
        let key = (
            canonical_id.to_string(),
            exported_name.to_string(),
            route.clone(),
        );
        if let Some(cached) = self.entries.get(&key) {
            return Arc::clone(cached);
        }
        let planned: Arc<[PlannedFrontierCompanion]> = Arc::from(compute());
        self.entries.insert(key, Arc::clone(&planned));
        planned
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExternalTypeTraceBaseline {
    tracked_len: usize,
    resolution_len: usize,
    cache_len: usize,
}

impl ExternalTypeTraceBaseline {
    fn capture(
        tracked_deps: &std::collections::BTreeSet<String>,
        resolution_deps: &std::collections::BTreeSet<String>,
        cache: &ExternalTypeCache,
    ) -> Self {
        Self {
            tracked_len: tracked_deps.len(),
            resolution_len: resolution_deps.len(),
            cache_len: cache.len(),
        }
    }
}

fn external_type_trace_success_status(has_result: bool) -> &'static str {
    if has_result {
        "ok:resolved"
    } else {
        "ok:none"
    }
}

fn external_type_trace_error_status(
    error: &crate::types::ExternalTypeResolveError,
) -> &'static str {
    match error {
        crate::types::ExternalTypeResolveError::MissingRootDependency => "err:missing_root",
        crate::types::ExternalTypeResolveError::DepthLimitExceeded { .. } => "err:depth_limit",
        crate::types::ExternalTypeResolveError::StepLimitExceeded { .. } => "err:step_limit",
    }
}

fn external_type_trace_deltas(
    baseline: ExternalTypeTraceBaseline,
    tracked_len: usize,
    resolution_len: usize,
    cache_len: usize,
) -> (usize, usize, usize) {
    (
        tracked_len.saturating_sub(baseline.tracked_len),
        resolution_len.saturating_sub(baseline.resolution_len),
        cache_len.saturating_sub(baseline.cache_len),
    )
}

fn emit_external_type_from_loaded_files_trace_result(
    owner_canonical: &str,
    import_source: &str,
    type_name: &str,
    status: &'static str,
    baseline: ExternalTypeTraceBaseline,
    tracked_len: usize,
    resolution_len: usize,
    cache_len: usize,
    visiting_len: usize,
    store_view_active: bool,
) {
    let (tracked_delta, resolution_delta, cache_delta) =
        external_type_trace_deltas(baseline, tracked_len, resolution_len, cache_len);
    component_meta_trace_custom!(
        "resolve_external_type_from_loaded_files_result",
        format!(
            "owner={} import={} type={} status={} tracked_delta={} resolution_delta={} cache_delta={} visiting={} store_view={}",
            owner_canonical,
            import_source,
            type_name,
            status,
            tracked_delta,
            resolution_delta,
            cache_delta,
            visiting_len,
            store_view_active,
        ),
    );
}

fn external_type_frontier_layer_start_detail(
    source_canonical: &str,
    exported_name: &str,
    layer: usize,
    pending: usize,
    resolved: usize,
) -> String {
    format!(
        "source={} exported={} layer={} pending={} resolved={}",
        source_canonical, exported_name, layer, pending, resolved,
    )
}

fn external_type_frontier_layer_result_detail(
    source_canonical: &str,
    exported_name: &str,
    layer: usize,
    pending_next: usize,
    resolved: usize,
    has_more: bool,
    target_found: bool,
    route_cycle: bool,
) -> String {
    format!(
        "source={} exported={} layer={} pending_next={} resolved={} has_more={} target_found={} route_cycle={}",
        source_canonical,
        exported_name,
        layer,
        pending_next,
        resolved,
        has_more,
        target_found,
        route_cycle,
    )
}

/// Lightweight projection of a [`RouteOwnedShallowEntry`] used by
/// [`crate::resolver_store`] when snapshotting the route-only shallow cache
/// for fact-capture. **Not** a cache entry itself; the project-store-owned
/// [`RouteOwnedShallowDb`](crate::project_type_store::RouteOwnedShallowDb)
/// retains the full entry, while this projection is the thin shape exposed
/// across the in-crate consumer boundary (Phase 6b sub-plan §6b.D2a step 5).
pub(crate) struct RouteOwnedShallowStateSnapshot {
    pub canonical_id: String,
    pub whole_hash: Hash16,
    pub route_hash: Option<Hash16>,
}

impl RouteOwnedShallowStateSnapshot {
    /// Project a [`RouteOwnedShallowEntry`] into a snapshot. The entry's
    /// shallow state is consulted to compute `route_hash` only when the
    /// surface is resolvable — mirrors the pre-migration `From` behaviour
    /// that lived inline in `snapshot_route_owned_shallow_cache_entries`.
    pub(crate) fn from_entry(
        canonical_id: &str,
        entry: &crate::project_type_store::RouteOwnedShallowEntry,
    ) -> Self {
        let route_hash = entry
            .shallow_state
            .has_resolvable_surface()
            .then(|| crate::resolver_store::hash_route_surface(entry.shallow_state.as_ref()));
        Self {
            canonical_id: canonical_id.to_string(),
            whole_hash: entry.whole_hash,
            route_hash,
        }
    }
}

fn wildcard_source_stem_for_matching(path: &str) -> Option<String> {
    let mut segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let mut stem = segments.pop()?;

    for suffix in [
        ".d.ts", ".d.mts", ".d.cts", ".vue", ".tsx", ".ts", ".jsx", ".js", ".mts", ".cts",
    ] {
        if let Some(stripped) = stem.strip_suffix(suffix) {
            stem = stripped;
            break;
        }
    }

    if stem == "index" {
        stem = segments.pop()?;
    }

    let mut normalized = String::new();
    let mut uppercase_next = true;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                normalized.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                normalized.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }

    (!normalized.is_empty()).then_some(normalized)
}

fn wildcard_match_score(
    exported_name: &str,
    wildcard: &crate::resolver_core::WildcardReexport,
) -> usize {
    let candidate = if wildcard.canonical_id.is_empty() {
        wildcard.source_specifier.as_str()
    } else {
        wildcard.canonical_id.as_str()
    };
    let Some(stem) = wildcard_source_stem_for_matching(candidate) else {
        return 0;
    };
    if exported_name.starts_with(stem.as_str()) {
        stem.len()
    } else {
        0
    }
}

fn ordered_wildcard_indices_for_exported_name(
    wildcards: &[crate::resolver_core::WildcardReexport],
    exported_name: &str,
) -> Vec<usize> {
    let mut scored = wildcards
        .iter()
        .enumerate()
        .map(|(index, wildcard)| (index, wildcard_match_score(exported_name, wildcard)))
        .collect::<Vec<_>>();
    scored.sort_by(|(left_index, left_score), (right_index, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_index.cmp(right_index))
    });
    scored.into_iter().map(|(index, _)| index).collect()
}

fn external_type_debug_enabled() -> bool {
    std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
        || std::env::var_os("VERTER_META_DEBUG").is_some()
}

fn external_type_debug(message: impl AsRef<str>) {
    if external_type_debug_enabled() {
        eprintln!("[verter-meta] {}", message.as_ref());
    }
}

#[cfg(test)]
thread_local! {
    static FORBID_ROUTE_FRONTIER_FOR_TESTS: Cell<usize> = const { Cell::new(0) };
    static FORBID_IMPORT_ROUTE_SHADOW_FOR_TESTS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) struct RouteFrontierGuard;
#[cfg(test)]
pub(crate) struct ImportRouteShadowGuard;

#[cfg(test)]
impl Drop for RouteFrontierGuard {
    fn drop(&mut self) {
        FORBID_ROUTE_FRONTIER_FOR_TESTS.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[cfg(test)]
impl Drop for ImportRouteShadowGuard {
    fn drop(&mut self) {
        FORBID_IMPORT_ROUTE_SHADOW_FOR_TESTS.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[cfg(test)]
pub(crate) fn forbid_route_frontier_for_tests() -> RouteFrontierGuard {
    FORBID_ROUTE_FRONTIER_FOR_TESTS.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
    RouteFrontierGuard
}

#[cfg(test)]
pub(crate) fn forbid_import_route_shadow_for_tests() -> ImportRouteShadowGuard {
    FORBID_IMPORT_ROUTE_SHADOW_FOR_TESTS.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
    ImportRouteShadowGuard
}

#[cfg(test)]
fn assert_route_frontier_allowed() {
    assert!(
        !route_frontier_forbidden_for_current_thread(),
        "route/root production path should not fall back through the external-type frontier",
    );
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn assert_import_route_shadow_allowed() {
    assert!(
        !import_route_shadow_forbidden_for_current_thread(),
        "route/root/component-meta production path should not read legacy import-route shadow maps",
    );
}

#[cfg(test)]
pub(crate) fn route_frontier_forbidden_for_current_thread() -> bool {
    FORBID_ROUTE_FRONTIER_FOR_TESTS.with(|depth| depth.get() > 0)
}

#[cfg(test)]
pub(crate) fn import_route_shadow_forbidden_for_current_thread() -> bool {
    FORBID_IMPORT_ROUTE_SHADOW_FOR_TESTS.with(|depth| depth.get() > 0)
}

#[cfg(not(test))]
fn assert_route_frontier_allowed() {}

#[cfg(not(test))]
#[allow(dead_code)]
pub(crate) fn assert_import_route_shadow_allowed() {}

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
    /// Phase 6b.D2a step 3 — invalidate the route-only shallow entry for a
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

    /// Phase 6b.D2a step 3 — snapshot the route-only shallow entries from the
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

    fn cache_import_route_result(
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

        {
            if let Some(mut entry) = self.compile_cache.get_mut(owner_canonical) {
                entry
                    .import_routes
                    .insert(import_source.to_string(), resolution.clone());
                entry.dependencies.insert(resolved_canonical_id.to_string());
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
        self.resolved_type_cache.lock().get(&key).cloned()
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

    #[allow(clippy::type_complexity)]
    fn run_external_type_frontier_closure(
        &self,
        dep_canonical: &str,
        type_name: &str,
        requested_routes: &mut FrontierRequestedRoutes,
        companion_plans: &mut FrontierCompanionPlans,
    ) -> Result<
        (
            crate::resolver_core::ExternalTypeFrontier,
            Option<(String, String)>,
            bool,
        ),
        crate::types::ExternalTypeResolveError,
    > {
        assert_route_frontier_allowed();
        let adapter = HostFrontierAdapter {
            host: self,
            materialize_symbols: false,
            // Frontier discovery stays route-only. Materialization resolves only
            // the demanded companion targets after the route is known.
            route_exports_only: true,
            route_shallow_cache: RefCell::new(RouteShallowStateCache::default()),
        };
        let mut frontier = crate::resolver_core::ExternalTypeFrontier::new();
        let mut inspected_symbols = rustc_hash::FxHashSet::default();
        frontier.seed(std::iter::once(
            crate::resolver_core::PendingExternalSymbol {
                canonical_id: dep_canonical.to_string(),
                exported_name: type_name.to_string(),
                route: Some(
                    requested_routes
                        .get(&(dep_canonical.to_string(), type_name.to_string()))
                        .cloned()
                        .unwrap_or_default(),
                ),
            },
        ));

        let mut frontier_layer = 0usize;
        loop {
            let (target, had_route_cycle) = loop {
                frontier_layer += 1;
                component_meta_trace_custom!(
                    "external_type_frontier_layer_start",
                    external_type_frontier_layer_start_detail(
                        dep_canonical,
                        type_name,
                        frontier_layer,
                        frontier.pending_count(),
                        frontier.resolved_count(),
                    ),
                );
                let has_more = frontier.run_one_level(&adapter).map_err(|failure| {
                    crate::types::ExternalTypeResolveError::StepLimitExceeded {
                        limit: failure.limit,
                        type_name: type_name.to_string(),
                        last_dep: failure.context,
                    }
                })?;
                let (target, had_route_cycle) =
                    frontier.final_target_for_with_cycle(&adapter, dep_canonical, type_name);
                component_meta_trace_custom!(
                    "external_type_frontier_layer_result",
                    external_type_frontier_layer_result_detail(
                        dep_canonical,
                        type_name,
                        frontier_layer,
                        frontier.pending_count(),
                        frontier.resolved_count(),
                        has_more,
                        target.is_some(),
                        had_route_cycle,
                    ),
                );
                if target.is_some() || !has_more {
                    break (target, had_route_cycle);
                }
            };
            if target.is_none() {
                return Ok((frontier, None, had_route_cycle));
            }

            frontier.clear_pending();

            let companion_seeds = self.collect_frontier_companion_seeds(
                &frontier,
                &adapter,
                &mut inspected_symbols,
                requested_routes,
                companion_plans,
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
                let seed_route = seed.route.clone().unwrap_or_default();
                requested_routes
                    .entry((seed.canonical_id.clone(), seed.exported_name.clone()))
                    .and_modify(|existing| {
                        *existing =
                            crate::resolver_core::merge_route_demands(existing, &seed_route);
                    })
                    .or_insert(seed_route);
            }
            frontier.seed(companion_seeds);
        }
    }

    fn collect_frontier_companion_seeds(
        &self,
        frontier: &crate::resolver_core::ExternalTypeFrontier,
        adapter: &HostFrontierAdapter<'_>,
        inspected_symbols: &mut rustc_hash::FxHashSet<(String, String)>,
        requested_routes: &mut FrontierRequestedRoutes,
        companion_plans: &mut FrontierCompanionPlans,
    ) -> Vec<crate::resolver_core::PendingExternalSymbol> {
        let mut seeds = Vec::new();
        let requested_symbols: Vec<_> = requested_routes
            .iter()
            .map(|((canonical_id, exported_name), route)| {
                (canonical_id.clone(), exported_name.clone(), route.clone())
            })
            .collect();

        for (requested_canonical_id, requested_exported_name, requested_route) in requested_symbols
        {
            let Some((canonical_id, exported_name)) = frontier.final_target_for(
                adapter,
                &requested_canonical_id,
                &requested_exported_name,
            ) else {
                continue;
            };
            requested_routes
                .entry((canonical_id.clone(), exported_name.clone()))
                .and_modify(|existing| {
                    *existing =
                        crate::resolver_core::merge_route_demands(existing, &requested_route);
                })
                .or_insert_with(|| requested_route.clone());
            if !inspected_symbols.insert((canonical_id.clone(), exported_name.clone())) {
                continue;
            }

            let planned_companions = self.planned_frontier_companions(
                &canonical_id,
                &exported_name,
                &requested_route,
                companion_plans,
            );
            for companion in planned_companions.iter() {
                let (target_canonical, target_name) = frontier
                    .final_target_for(
                        adapter,
                        &companion.resolved_canonical,
                        &companion.resolved_exported_name,
                    )
                    .unwrap_or((
                        companion.resolved_canonical.clone(),
                        companion.resolved_exported_name.clone(),
                    ));
                seeds.push(crate::resolver_core::PendingExternalSymbol {
                    canonical_id: target_canonical,
                    exported_name: target_name,
                    route: Some(companion.route.clone()),
                });
            }
        }

        seeds
    }

    fn materialize_frontier_resolved_type(
        &self,
        frontier: &crate::resolver_core::ExternalTypeFrontier,
        requested_routes: &FrontierRequestedRoutes,
        companion_plans: &mut FrontierCompanionPlans,
        canonical_id: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        let adapter = HostFrontierAdapter {
            host: self,
            // Frontier routing is already complete before materialization starts.
            // Keep final-target checks on the same shallow/export-owned path so
            // package declaration files do not reopen full imported-state
            // materialization while companion targets are selected.
            materialize_symbols: false,
            route_exports_only: true,
            route_shallow_cache: RefCell::new(RouteShallowStateCache::default()),
        };
        let mut memo = rustc_hash::FxHashMap::default();
        let mut active = rustc_hash::FxHashSet::default();
        self.materialize_frontier_resolved_type_with_memo(
            frontier,
            requested_routes,
            companion_plans,
            &adapter,
            canonical_id,
            exported_name,
            tracked_deps,
            resolution_deps,
            &mut memo,
            &mut active,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn materialize_frontier_resolved_type_with_memo(
        &self,
        frontier: &crate::resolver_core::ExternalTypeFrontier,
        requested_routes: &FrontierRequestedRoutes,
        companion_plans: &mut FrontierCompanionPlans,
        adapter: &HostFrontierAdapter<'_>,
        canonical_id: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
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

        let resolved = {
            let route = requested_routes
                .get(&(canonical_id.to_string(), exported_name.to_string()))
                .cloned()
                .unwrap_or_default();
            let planned_companions = self.planned_frontier_companions(
                canonical_id,
                exported_name,
                &route,
                companion_plans,
            );
            let mut companion_types = ResolvedExternalTypes::default();
            for companion in planned_companions.iter() {
                let (target_canonical, target_name) = frontier
                    .final_target_for(
                        adapter,
                        &companion.resolved_canonical,
                        &companion.resolved_exported_name,
                    )
                    .unwrap_or((
                        companion.resolved_canonical.clone(),
                        companion.resolved_exported_name.clone(),
                    ));
                if frontier
                    .get_resolved(&target_canonical, &target_name)
                    .is_none()
                {
                    continue;
                }
                if let Some(resolved_companion) = self.materialize_frontier_resolved_type_with_memo(
                    frontier,
                    requested_routes,
                    companion_plans,
                    adapter,
                    &target_canonical,
                    &target_name,
                    tracked_deps,
                    resolution_deps,
                    memo,
                    active,
                ) {
                    tracked_deps.insert(target_canonical.clone());
                    resolution_deps.insert(target_canonical.clone());
                    if external_type_debug_enabled() {
                        external_type_debug(format!(
                            "frontier_materialize companion owner={} exported={} alias={} target={}:{} cached_member_count={}",
                            canonical_id,
                            exported_name,
                            companion.alias,
                            target_canonical,
                            target_name,
                            resolved_companion.props.len(),
                        ));
                    }
                    companion_types
                        .entry(companion.alias.clone())
                        .or_insert(resolved_companion);
                }
            }

            self.resolve_external_type_from_indexed_ready(
                canonical_id,
                exported_name,
                &companion_types,
            )
        };

        active.remove(&cache_key);
        memo.insert(cache_key, resolved.clone());
        resolved
    }

    fn planned_frontier_companions(
        &self,
        canonical_id: &str,
        exported_name: &str,
        route: &crate::resolver_core::RouteDemand,
        companion_plans: &mut FrontierCompanionPlans,
    ) -> Arc<[PlannedFrontierCompanion]> {
        companion_plans.get_or_compute(canonical_id, exported_name, route, || {
            let Some(analysis) = self.external_type_analysis(canonical_id) else {
                return Vec::new();
            };
            let required_import_routes =
                self.required_import_routes_for_exported_route(canonical_id, exported_name, route);
            let required_import_names = required_import_routes
                .keys()
                .cloned()
                .collect::<rustc_hash::FxHashSet<_>>();
            let mut attempted_requests = rustc_hash::FxHashSet::default();
            let mut planned = Vec::new();

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

                    let Some(dep_canonical) =
                        self.resolve_type_dependency_canonical(canonical_id, &binding.source)
                    else {
                        continue;
                    };
                    let (resolved_canonical, resolved_name) = self
                        .resolve_imported_type_root(dep_canonical.as_str(), imported_name.as_str());
                    planned.push(PlannedFrontierCompanion {
                        alias: required_alias.clone(),
                        resolved_canonical,
                        resolved_exported_name: resolved_name,
                        route: required_import_routes
                            .get(&required_alias)
                            .cloned()
                            .unwrap_or_default(),
                    });
                }
            }

            planned
        })
    }

    fn append_route_participant_fact_versions(
        &self,
        canonical: &str,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
        route_shallow_cache: Option<&RouteShallowStateCache>,
    ) {
        if let Some(hash) = self.current_or_read_whole_hash(canonical) {
            let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_string(),
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }

        let route_hash = {
            let normalized_canonical = self
                .resolve_eval_dependency_canonical(canonical)
                .unwrap_or_else(|| canonical.to_string());
            route_shallow_cache
                .and_then(|cache| cache.get(normalized_canonical.as_str()))
                .filter(|state| state.as_ref().has_resolvable_surface())
                .map(|state| crate::resolver_store::hash_route_surface(state.as_ref()))
                .or_else(|| {
                    self.shallow_file_state(canonical)
                        .filter(|state| state.has_resolvable_surface())
                        .map(|state| crate::resolver_store::hash_route_surface(&state))
                })
        };
        if let Some(hash) = route_hash {
            let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical.to_string(),
                kind: crate::resolver_core::DerivedFactKind::Route,
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }
    }

    pub(crate) fn resolve_route_type_edge(
        &self,
        owner_canonical: &str,
        source_specifier: &str,
    ) -> Option<String> {
        let resolved = self
            .ws()
            .resolve_import(
                owner_canonical,
                source_specifier,
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind: verter_workspace::ResolveRequestKind::TypeImport,
                },
            )
            .map(|resolution| {
                self.normalize_live_type_dependency_target(
                    owner_canonical,
                    source_specifier,
                    resolution.source_id.as_str(),
                )
            })
            .or_else(|| self.fallback_relative_type_companion(owner_canonical, source_specifier))
            .or_else(|| {
                self.ws()
                    .resolve_import(
                        owner_canonical,
                        source_specifier,
                        verter_workspace::ResolutionContext {
                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                            kind: verter_workspace::ResolveRequestKind::EsmImport,
                        },
                    )
                    .map(|resolution| {
                        self.normalize_live_type_dependency_target(
                            owner_canonical,
                            source_specifier,
                            resolution.source_id.as_str(),
                        )
                    })
            })?;

        if resolved.ends_with(".vue") {
            let known_hash = self
                .current_or_read_whole_hash(resolved.as_str())
                .or_else(|| self.cached_route_owned_shallow_whole_hash(resolved.as_str()));
            if let Some(hash) = known_hash {
                if !self.store_view_allows_current_whole_hash(resolved.as_str(), hash) {
                    return None;
                }
            }
        } else if self.current_or_read_whole_hash(resolved.as_str()).is_none() {
            // Canonical resolver-edge ensure_loaded: when a cross-file type
            // import resolves to a workspace `.ts`/`.d.ts` file the host
            // hasn't seen yet, load it once so subsequent probes hit the
            // cache.
            if !self.is_evalable(resolved.as_str()) && !self.ensure_loaded(resolved.as_str()) {
                return None;
            }
        }

        Some(resolved)
    }

    fn resolve_named_type_export_route_from_target(
        &self,
        provider_canonical: &str,
        target: &crate::resolver_core::ExportTarget,
        active: &mut rustc_hash::FxHashSet<(String, String)>,
        participants: &mut rustc_hash::FxHashSet<String>,
        route_shallow_cache: &mut RouteShallowStateCache,
    ) -> Option<crate::resolver_core::RouteResult> {
        match target {
            crate::resolver_core::ExportTarget::Local { symbol_name } => {
                let state = self.route_shallow_state(provider_canonical, route_shallow_cache)?;
                if state.is_import_local(symbol_name) {
                    let import_target = state.import_target(symbol_name)?;
                    let target_canonical = if import_target.canonical_id.is_empty() {
                        self.resolve_route_type_edge(
                            provider_canonical,
                            import_target.source_specifier.as_str(),
                        )?
                    } else {
                        import_target.canonical_id.clone()
                    };
                    return self.resolve_named_type_export_route_uncached(
                        target_canonical.as_str(),
                        import_target.imported_name.as_str(),
                        active,
                        participants,
                        route_shallow_cache,
                    );
                }

                Some(crate::resolver_core::RouteResult::Resolved {
                    defining_canonical: provider_canonical.to_string(),
                    defining_symbol: symbol_name.clone(),
                })
            }
            crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                original_name,
                canonical_id,
                ..
            } => {
                let target_canonical = if canonical_id.is_empty() {
                    self.resolve_route_type_edge(provider_canonical, source_specifier.as_str())?
                } else {
                    canonical_id.clone()
                };
                self.resolve_named_type_export_route_uncached(
                    target_canonical.as_str(),
                    original_name.as_str(),
                    active,
                    participants,
                    route_shallow_cache,
                )
            }
        }
    }

    /// Phase 6b.D2a step 3 — query the cached `whole_hash` for a canonical
    /// without forcing a cold materialisation. Used by warm-path callers
    /// that need a content-hash for `store_view_allows_current_whole_hash`
    /// without consuming the full route-only artifact. Reads the
    /// project-store DB directly via `get_any` (no tiered staleness gate
    /// here — callers reapply their own staleness check via
    /// `store_view_allows_current_whole_hash`).
    pub(crate) fn cached_route_owned_shallow_whole_hash(
        &self,
        canonical_id: &str,
    ) -> Option<Hash16> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());
        self.project_type_store
            .route_owned_shallow()
            .get_any(normalized_canonical.as_str())
            .map(|entry| entry.whole_hash)
    }

    /// Phase 6b.D2a step 3 — return the cached eval-state tuple for a
    /// canonical via the materialiser. On cache miss, materialises through
    /// [`Self::ensure_route_owned_shallow_entry`].
    pub(crate) fn cached_route_owned_eval_state(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
        Hash16,
    )> {
        let entry = self.ensure_route_owned_shallow_entry(canonical_id)?;
        Some((
            Arc::clone(&entry.raw_source),
            entry.cached_parse.clone(),
            entry.whole_hash,
        ))
    }

    /// Phase 6b.D2a step 3 — return the cached `FileAnalysisSnapshot` for a
    /// canonical via the materialiser. On cache miss, materialises through
    /// [`Self::ensure_route_owned_shallow_entry`].
    pub(crate) fn cached_route_owned_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::types::FileAnalysisSnapshot>> {
        let entry = self.ensure_route_owned_shallow_entry(canonical_id)?;
        Some(Arc::clone(&entry.snapshot))
    }

    /// Phase 6b.D2a step 2 — shared materialiser for the route-only
    /// shallow artifact. Three-layer pattern matching the verified
    /// `ensure_indexed_ready` template at `host_manage.rs:3417`:
    ///
    /// 1. **Pre-flight fast path** — `get_any()` + tiered staleness gate
    ///    (warm callers exit zero-I/O).
    /// 2. **Singleflight on miss** — collapses concurrent cold callers to
    ///    one leader via
    ///    [`UnifiedResolverRuntime::route_owned_shallow_singleflight`](crate::resolver_core::resolver_runtime::UnifiedResolverRuntime::route_owned_shallow_singleflight).
    /// 3. **Inside flight**: re-check `get_any()` + tiered gate, capture
    ///    BOTH generations BEFORE the read, hash-validated re-check after
    ///    hashing, parse + analysis, then a **pre-publish fence** that
    ///    re-reads both generations to detect mid-flight mutations.
    ///
    /// The materialiser publishes once per content generation; subsequent
    /// callers within that generation see the published `Arc`.
    /// `Arc::ptr_eq` over the returned entry holds for two concurrent cold
    /// callers (singleflight collapse).
    pub(crate) fn ensure_route_owned_shallow_entry(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::project_type_store::RouteOwnedShallowEntry>> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());
        let canonical_id = normalized_canonical.as_str();

        // STEP 1 — pre-flight fast path (warm callers, ZERO I/O).
        if let Some(entry) = self
            .project_type_store
            .route_owned_shallow()
            .get_any(canonical_id)
        {
            if self.route_owned_entry_is_fresh(canonical_id, entry.as_ref()) {
                return Some(entry);
            }
            self.project_type_store
                .route_owned_shallow()
                .remove(canonical_id);
        }

        if canonical_id.is_empty() || crate::host_manage::is_raw_import_specifier_id(canonical_id) {
            return None;
        }

        // STEP 2 — singleflight on miss. Mirrors `indexed_singleflight`
        // pattern at host_manage.rs:3725–3743 — uses `()` error type, returns
        // `Option` at the outer fn.
        let canonical_arc: Arc<str> = Arc::from(canonical_id);
        let token = crate::resolver_core::StoreViewCompatToken {
            epoch: 0,
            session: None,
        };
        let materialize =
            || -> Result<Arc<crate::project_type_store::RouteOwnedShallowEntry>, ()> {
                // STEP 3 — re-check inside flight (apply the tiered gate again).
                if let Some(entry) = self
                    .project_type_store
                    .route_owned_shallow()
                    .get_any(canonical_id)
                {
                    if self.route_owned_entry_is_fresh(canonical_id, entry.as_ref()) {
                        return Ok(entry);
                    }
                    self.project_type_store
                        .route_owned_shallow()
                        .remove(canonical_id);
                }
                // STEP 4 — capture BOTH generations BEFORE read+parse, so any
                // mutation that lands during materialisation produces a generation
                // mismatch on the pre-publish fence (STEP 7).
                let workspace_generation = self.ws().content_generation();
                let project_generation = self.project_type_store.current_project_generation();

                let raw_source = self.read_analysis_source(canonical_id).ok_or(())?;
                let whole_hash = crate::hash::hash_16(raw_source.as_bytes());

                // STEP 5 — hash-validated re-check WITH the tiered gate.
                // A by-hash hit must still pass the full freshness gate (per
                // hard-stop constraint #13): tier-3 may reject the entry even
                // though tier-1 (whole_hash) matches.
                if let Some(entry) = self
                    .project_type_store
                    .route_owned_shallow()
                    .get(canonical_id, whole_hash)
                {
                    if self.route_owned_entry_is_fresh(canonical_id, entry.as_ref()) {
                        return Ok(entry);
                    }
                    self.project_type_store
                        .route_owned_shallow()
                        .remove(canonical_id);
                }

                // Honour the request-scoped store-view gate exactly like the
                // pre-migration body did (`store_view_allows_current_whole_hash`).
                if !self.store_view_allows_current_whole_hash(canonical_id, whole_hash) {
                    return Err(());
                }

                // If a parallel materialisation populated `IndexedReadyDb` while
                // we were reading, prefer that authoritative shape — same
                // shortcut the pre-migration body had at host_resolve.rs:2257.
                if let Some(facts) = self.project_type_store.indexed().get_any(canonical_id) {
                    if facts.whole_hash == whole_hash {
                        // The IndexedReady authority is preferred; do NOT publish
                        // a route-only shadow because the IndexedReady fast path
                        // is the canonical reader.
                        return Err(());
                    }
                }

                // STEP 6 — cold parse + analysis.
                let cached_parse = canonical_id.ends_with(".vue").then(|| {
                    Arc::new(verter_compiler::compile::parse_sfc(&raw_source, None, None))
                });
                let eval_source = Arc::<str>::from(Self::build_eval_script_source(
                    raw_source.as_ref(),
                    cached_parse.as_deref(),
                ));
                let snapshot = Arc::new(self.build_route_owned_snapshot_from_source_state(
                    canonical_id,
                    &raw_source,
                    cached_parse.as_deref(),
                    whole_hash,
                ));
                let (eval_env, external_type_analysis) = self
                    .build_eval_env_and_external_type_analysis(
                        canonical_id,
                        whole_hash,
                        raw_source.as_ref(),
                        cached_parse.as_deref(),
                        &eval_source,
                    );
                let shallow_state =
                    Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
                        whole_hash,
                        Arc::clone(&external_type_analysis),
                        Some(eval_env.as_ref()),
                    ));

                // STEP 7 — PRE-PUBLISH FENCE.
                // Re-read both generations. If either has bumped since STEP 4,
                // a route-resolution mutation or content mutation landed during
                // read+parse; the entry we just built is already stale. Abort
                // the publish so the next caller re-cold-materialises against
                // the new state.
                let workspace_generation_post = self.ws().content_generation();
                let project_generation_post = self.project_type_store.current_project_generation();
                if workspace_generation_post != workspace_generation
                    || project_generation_post != project_generation
                {
                    return Err(());
                }

                let entry = Arc::new(crate::project_type_store::RouteOwnedShallowEntry {
                    whole_hash,
                    workspace_generation,
                    project_generation,
                    raw_source: Arc::clone(&raw_source),
                    eval_source,
                    cached_parse,
                    snapshot,
                    external_type_analysis,
                    shallow_state,
                });
                self.project_type_store
                    .route_owned_shallow()
                    .publish(canonical_arc.clone(), Arc::clone(&entry));
                Ok(entry)
            };
        let singleflight = &self.resolver.runtime.route_owned_shallow_singleflight;
        match singleflight.run(canonical_arc.clone(), token, materialize) {
            Ok(run_result) => Some((*run_result.value).clone()),
            Err(()) => None,
        }
    }

    /// Phase 6b.D2a step 2 — tiered staleness gate for route-only entries.
    /// Mirrors pre-migration `cached_route_owned_shallow_state_entry` body
    /// (host_resolve.rs:2128–2147) extended with tier-3 `project_generation`
    /// per tenth-pass Codex P0:
    ///
    /// - **Tier 3** — `entry.project_generation` must match
    ///   [`ProjectTypeStore::current_project_generation`]. Covers
    ///   `configure_projects` / `set_exact_resolutions` /
    ///   `configure_resolver` route-resolution mutations that DO NOT bump
    ///   `content_generation`.
    /// - **Tier 1** — when `get_whole_hash` returns `Some`, the scheduler-
    ///   backed authoritative content hash is the truth.
    /// - **Tier 2** — fallback for route-only files the scheduler hasn't
    ///   seen: `entry.workspace_generation == ws().content_generation()`
    ///   AND `ws().file_exists(canonical_id)`.
    fn route_owned_entry_is_fresh(
        &self,
        canonical_id: &str,
        entry: &crate::project_type_store::RouteOwnedShallowEntry,
    ) -> bool {
        // Tier 3 — project graph / route resolution.
        if entry.project_generation != self.project_type_store.current_project_generation() {
            return false;
        }
        // Tier 1 — scheduler-backed authoritative content hash.
        if let Some(auth_hash) = self.get_whole_hash(canonical_id) {
            return auth_hash == entry.whole_hash;
        }
        // Tier 2 — workspace_generation + file_exists.
        entry.workspace_generation == self.ws().content_generation()
            && self.ws().file_exists(canonical_id)
    }

    /// Phase 6b.D2a — test-only accessor for the route-only freshness gate.
    /// Used by `phase_6b_characterization_tests` to discriminate tier-2
    /// behaviour without depending on the public materialiser path (which
    /// always populates `compile_cache` and would otherwise put tier-1 in
    /// charge).
    #[cfg(test)]
    pub(crate) fn route_owned_entry_is_fresh_for_test(
        &self,
        canonical_id: &str,
        entry: &crate::project_type_store::RouteOwnedShallowEntry,
    ) -> bool {
        self.route_owned_entry_is_fresh(canonical_id, entry)
    }

    /// Phase 6b.D2a step 1 (writers) — `route_shallow_state` is the
    /// route-only frontier reader. Body now delegates to the shared
    /// materialiser ([`Self::ensure_route_owned_shallow_entry`]) and
    /// returns the entry's `shallow_state`. The request-scoped
    /// `route_shallow_cache` (frontier-engine memo, kept per-request to
    /// avoid repeated `Arc` clones) is still populated for in-flight
    /// frontier traversal.
    fn route_shallow_state(
        &self,
        canonical_id: &str,
        route_shallow_cache: &mut RouteShallowStateCache,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());

        // Authoritative `IndexedReady` fast path — preserved from the
        // pre-migration body so scheduler-materialised entries take
        // precedence over route-only shadow entries.
        if let Some(facts) = self
            .project_type_store
            .indexed()
            .get_any(normalized_canonical.as_str())
        {
            return Some(Arc::clone(&facts.shallow_state));
        }

        // Request-scoped memo (frontier engine de-dupe). NOT a host-side
        // mirror — see `HostFrontierAdapter::route_shallow_cache` doc-comment
        // (Phase 6b §6b.2.F9 `scratch` classification).
        if let Some(cached) = route_shallow_cache.get(normalized_canonical.as_str()) {
            return Some(Arc::clone(cached));
        }

        let entry = self.ensure_route_owned_shallow_entry(normalized_canonical.as_str())?;
        let shallow_state = Arc::clone(&entry.shallow_state);
        route_shallow_cache.insert(normalized_canonical.clone(), Arc::clone(&shallow_state));
        Some(shallow_state)
    }

    pub(crate) fn route_owned_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let mut route_shallow_cache = RouteShallowStateCache::default();
        self.route_shallow_state(canonical_id, &mut route_shallow_cache)
    }

    fn resolve_named_type_export_route_uncached(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        active: &mut rustc_hash::FxHashSet<(String, String)>,
        participants: &mut rustc_hash::FxHashSet<String>,
        route_shallow_cache: &mut RouteShallowStateCache,
    ) -> Option<crate::resolver_core::RouteResult> {
        let key = (provider_canonical.to_string(), exported_name.to_string());
        if !active.insert(key.clone()) {
            return Some(crate::resolver_core::RouteResult::Miss);
        }
        participants.insert(provider_canonical.to_string());

        let result = (|| {
            let state = self.route_shallow_state(provider_canonical, route_shallow_cache)?;

            if let Some(target) = state.export_target(exported_name) {
                return self.resolve_named_type_export_route_from_target(
                    provider_canonical,
                    target,
                    active,
                    participants,
                    route_shallow_cache,
                );
            }

            let wildcard_indices = ordered_wildcard_indices_for_exported_name(
                &state.wildcard_reexports,
                exported_name,
            );
            for wildcard_index in wildcard_indices {
                let wildcard = &state.wildcard_reexports[wildcard_index];
                let target_canonical = if wildcard.canonical_id.is_empty() {
                    self.resolve_route_type_edge(
                        provider_canonical,
                        wildcard.source_specifier.as_str(),
                    )
                } else {
                    Some(wildcard.canonical_id.clone())
                };
                let Some(target_canonical) = target_canonical else {
                    continue;
                };
                let child = self.resolve_named_type_export_route_uncached(
                    target_canonical.as_str(),
                    exported_name,
                    active,
                    participants,
                    route_shallow_cache,
                )?;
                if !child.is_miss() {
                    return Some(child);
                }
            }

            Some(crate::resolver_core::RouteResult::Miss)
        })();

        active.remove(&key);
        result
    }

    pub(crate) fn build_named_type_export_route_entry(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(
        crate::resolver_core::RouteResult,
        Vec<crate::resolver_core::FactVersionRef>,
    )> {
        let mut active = rustc_hash::FxHashSet::default();
        let mut touched_canonical_ids = rustc_hash::FxHashSet::default();
        let mut route_shallow_cache = RouteShallowStateCache::default();
        let route_result = self.resolve_named_type_export_route_uncached(
            dep_canonical,
            requested_name,
            &mut active,
            &mut touched_canonical_ids,
            &mut route_shallow_cache,
        )?;

        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut participants: Vec<String> = touched_canonical_ids.into_iter().collect();
        participants.sort();
        participants.dedup();
        for canonical in participants {
            self.append_route_participant_fact_versions(
                canonical.as_str(),
                &mut facts,
                &mut seen,
                None,
            );
        }

        Some((route_result, facts))
    }

    fn resolve_named_type_export_target_uncached(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(dep_canonical)
            .unwrap_or_else(|| dep_canonical.to_string());
        let live_view = self.resolver_store_view();

        let cached_route = self
            .resolver
            .runtime
            .routes
            .get_or_resolve_route_with_facts(
                normalized_canonical.as_str(),
                requested_name,
                &live_view,
                || {
                    self.build_named_type_export_route_entry(
                        normalized_canonical.as_str(),
                        requested_name,
                    )
                },
            )?;
        cached_route
            .resolved()
            .map(|(defining_canonical, defining_symbol)| {
                (defining_canonical.to_owned(), defining_symbol.to_owned())
            })
    }

    pub(crate) fn resolve_named_type_export_target_shallow(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        let result =
            self.resolve_named_type_export_target_uncached(dep_canonical, requested_name)?;
        component_meta_trace_custom!(
            "resolve_named_type_export_target_result",
            format!(
                "owner={} requested={} source=route_db target={} exported={} materialized=false",
                dep_canonical, requested_name, result.0, result.1
            ),
        );
        Some(result)
    }

    pub(crate) fn resolve_named_type_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        let result =
            self.resolve_named_type_export_target_uncached(dep_canonical, requested_name)?;
        let _ = self.ensure_indexed_ready(result.0.as_str());
        component_meta_trace_custom!(
            "resolve_named_type_export_target_result",
            format!(
                "owner={} requested={} source=route_db target={} exported={} materialized=true",
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
    pub(crate) fn read_dep_source_for_type_resolution(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String> {
        component_meta_trace_custom!(
            "read_dep_source_for_type_resolution",
            format!("owner={} store_view={}", dep_canonical, false),
        );
        if let Some(_profile_hash) = profile_hash {
            if let Some(state) = self.effective_file_state(dep_canonical, None) {
                if self.store_view_allows_current_whole_hash(dep_canonical, state.whole_hash) {
                    let eval_source = Arc::<str>::from(Self::build_eval_script_source(
                        state.source.as_ref(),
                        state.cached_parse.as_deref(),
                    ));
                    component_meta_trace_custom!(
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
        let facts = self.ensure_indexed_ready(dep_canonical)?;
        let eval_source = Arc::clone(&facts.eval_source);
        component_meta_trace_custom!(
            "read_dep_source_for_type_resolution_result",
            format!(
                "owner={} source=module-facts bytes={} has_cached_parse={} whole_hash={:?}",
                dep_canonical,
                eval_source.len(),
                facts.cached_parse.is_some(),
                facts.whole_hash,
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
                    self.cache_import_route_result(
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
                    self.cache_import_route_result(
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
                            self.cache_import_route_result(
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
        let sched_snapshot_at_start = self.scheduler.try_get_source(&canonical_id);

        let cache_miss = {
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

        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return None;
            }
        }

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
            {
                if let Some(mut cc) = self.compile_cache.get_mut(&canonical) {
                    cc.cached_tsc_extract = Some((whole_hash, Arc::clone(&arc)));
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
        if let Some(mut cc) = self.compile_cache.get_mut(canonical_id) {
            cc.latest_diagnostics.insert(profile_hash, diagnostics);
            cc.diagnostics_generation += 1;
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
            prop_constness_overrides: None, // TODO(Phase 6): populated by cross-file optimizer,
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
/// Consumed by component-meta resolution and frontier integration tests.
pub(crate) struct HostFrontierAdapter<'a> {
    pub host: &'a VerterHost,
    pub materialize_symbols: bool,
    pub route_exports_only: bool,
    /// Request-scoped memoisation of route-only [`ShallowFileState`] entries
    /// for the duration of a single frontier traversal. **NOT a host-side
    /// mirror** of the host's `route_owned_shallow` cache (Phase 6b: the
    /// host cache lives on `ProjectTypeStore.route_owned_shallow`); this
    /// `RefCell<...>` exists only to dedupe repeated reads of the same
    /// canonical within one request, so request-level callers do not
    /// repeatedly clone the host-cached `Arc`. Lifetime bounded to the
    /// adapter (`'a`). Phase 6b classification: `scratch`. See sub-plan
    /// §6b.2.F9.
    pub route_shallow_cache: RefCell<RouteShallowStateCache>,
}

impl crate::resolver_core::FrontierHost for HostFrontierAdapter<'_> {
    fn ensure_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let canonical = self
            .host
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());

        if self.route_exports_only {
            return self.host.route_shallow_state(
                canonical.as_str(),
                &mut self.route_shallow_cache.borrow_mut(),
            );
        }

        // IndexedReadyDb fast path.
        if let Some(facts) = self
            .host
            .project_type_store
            .indexed()
            .get_any(canonical.as_str())
        {
            if facts.shallow_state.has_resolvable_surface() || !self.materialize_symbols {
                if facts.shallow_state.has_wildcard_reexports() {
                    self.host
                        .provenance
                        .resolver_barrel_fact_reuse
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                return Some(facts.shallow_state.clone());
            }
        }

        // Materialize through ensure_indexed_ready.
        let facts = self.host.ensure_indexed_ready(canonical.as_str())?;
        if facts.shallow_state.has_resolvable_surface() || !self.materialize_symbols {
            if facts.shallow_state.has_wildcard_reexports() {
                self.host
                    .provenance
                    .resolver_barrel_fact_reuse
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            return Some(facts.shallow_state.clone());
        }

        None
    }
    fn route_exports_only(&self) -> bool {
        self.route_exports_only
    }

    fn resolve_type_edge_canonical(
        &self,
        owner_canonical: &str,
        source_specifier: &str,
    ) -> Option<String> {
        self.host
            .resolve_type_dependency_canonical(owner_canonical, source_specifier)
    }
}

#[cfg(test)]
#[path = "host_resolve_tests.rs"]
mod host_resolve_tests;

#[cfg(test)]
#[path = "frontier_tests.rs"]
mod frontier_tests;
