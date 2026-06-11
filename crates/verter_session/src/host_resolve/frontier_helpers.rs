//! Top-level types, traces, and helpers shared across the host_resolve
//! sub-modules.
//!
//! Owns:
//! - Type aliases (`ResolvedExternalTypes`, `ExternalTypeCache`,
//!   `FrontierRequestedRoutes`, `RouteShallowStateCache`,
//!   `FrontierCompanionPlanCache`).
//! - The `DirectComponentMetaDeclarationResolver` adapter used when
//!   building imported macro declarations.
//! - The frontier companion-plan caches (`PlannedFrontierCompanion`,
//!   `FrontierCompanionPlans`).
//! - External-type trace baseline + emit helpers
//!   (`ExternalTypeTraceBaseline`, `external_type_trace_*`,
//!   `emit_external_type_from_loaded_files_trace_result`,
//!   `external_type_frontier_layer_*_detail`).
//! - The route-only shallow snapshot projection
//!   (`RouteOwnedShallowStateSnapshot`).
//! - Wildcard re-export ranking helpers.
//! - Compile-time-toggled `external_type_debug` helper.

use std::sync::Arc;

use crate::host_manage::component_meta_trace_custom;
use crate::types::*;
use crate::VerterHost;

pub(crate) type ResolvedExternalTypes = rustc_hash::FxHashMap<
    String,
    verter_compiler::utils::oxc::script::type_surface::ResolvedElements,
>;

pub(crate) type ExternalTypeCache = crate::resolver_core::ExternalTypeBodyCache;
pub(crate) type FrontierRequestedRoutes =
    rustc_hash::FxHashMap<(String, String), crate::resolver_core::RouteDemand>;
pub(crate) type RouteShallowStateCache =
    rustc_hash::FxHashMap<String, Arc<crate::resolver_core::ShallowFileState>>;
pub(crate) type FrontierCompanionPlanCache = rustc_hash::FxHashMap<
    (String, String, crate::resolver_core::RouteDemand),
    Arc<[PlannedFrontierCompanion]>,
>;

pub(crate) struct DirectComponentMetaDeclarationResolver<'a> {
    pub host: &'a VerterHost,
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
            verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSymbolKind::TypeAlias => {
                crate::resolver_core::ResolvedDeclarationKind::TypeAlias
            }
            verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSymbolKind::Interface => {
                crate::resolver_core::ResolvedDeclarationKind::Interface
            }
            verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSymbolKind::Class => {
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
pub(crate) struct PlannedFrontierCompanion {
    pub alias: String,
    pub resolved_canonical: String,
    pub resolved_exported_name: String,
    pub route: crate::resolver_core::RouteDemand,
}

#[derive(Default)]
pub(crate) struct FrontierCompanionPlans {
    entries: FrontierCompanionPlanCache,
}

impl FrontierCompanionPlans {
    pub(super) fn get_or_compute<F>(
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
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExternalTypeTraceBaseline {
    pub tracked_len: usize,
    pub resolution_len: usize,
    pub cache_len: usize,
}

impl ExternalTypeTraceBaseline {
    pub(super) fn capture(
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

pub(crate) fn external_type_trace_success_status(has_result: bool) -> &'static str {
    if has_result {
        "ok:resolved"
    } else {
        "ok:none"
    }
}

pub(crate) fn external_type_trace_error_status(
    error: &crate::types::ExternalTypeResolveError,
) -> &'static str {
    match error {
        crate::types::ExternalTypeResolveError::MissingRootDependency => "err:missing_root",
        crate::types::ExternalTypeResolveError::DepthLimitExceeded { .. } => "err:depth_limit",
        crate::types::ExternalTypeResolveError::StepLimitExceeded { .. } => "err:step_limit",
    }
}

pub(crate) fn external_type_trace_deltas(
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_external_type_from_loaded_files_trace_result(
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

pub(crate) fn external_type_frontier_layer_start_detail(
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn external_type_frontier_layer_result_detail(
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
/// across the in-crate consumer boundary.
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

pub(crate) fn wildcard_source_stem_for_matching(path: &str) -> Option<String> {
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

pub(crate) fn wildcard_match_score(
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

pub(crate) fn ordered_wildcard_indices_for_exported_name(
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

pub(crate) fn external_type_debug_enabled() -> bool {
    std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
        || std::env::var_os("VERTER_META_DEBUG").is_some()
}

pub(crate) fn external_type_debug(message: impl AsRef<str>) {
    if external_type_debug_enabled() {
        eprintln!("[verter-meta] {}", message.as_ref());
    }
}
